//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ResolverMode::Type` vs `ResolverMode::Expanded`)
//! - materialized resolved outputs (`ResolvedComponentMetaState`)
//! - mode-aware caching
//! - JSDoc attachment and typed-tag resolution
//!
//! It calls into `host_resolve.rs` for declaration traversal â€” it does NOT
//! replace or duplicate the shared traversal substrate.
//!
//! # Architecture
//!
//! ```text
//! caller â†’ resolve_component_meta(canonical, mode)
//!            â†“
//!        meta_resolve.rs  (orchestration, materialization, caching)
//!            â†“
//!        host_resolve.rs  (declaration graph traversal, shared cache)
//! ```

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_event,
    component_meta_trace_scope,
};
use crate::resolver_core::{
    run_component_meta_request, ComponentMetaEvalOutputs, ComponentMetaRequestHost, RequestSource,
    SingleflightRole,
};
use crate::resolver_store::HostStoreView;
use crate::types::{FileAnalysisSnapshot, Hash16, ResolverMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

fn trace_request_source(source: RequestSource) -> &'static str {
    match source {
        RequestSource::Cache => "cache",
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } => "flight:leader",
        RequestSource::Flight {
            role: SingleflightRole::Follower,
            ..
        } => "flight:follower",
        RequestSource::Fallback => "fallback",
    }
}

#[derive(Debug, Clone)]
pub struct CapturedComponentMetaInputs {
    whole_hash: Hash16,
    snapshot: FileAnalysisSnapshot,
    owner_eval_source: Option<String>,
    dep_resolutions: rustc_hash::FxHashMap<String, crate::types::DependencyResolution>,
}

impl ComponentMetaRequestHost for VerterHost {
    type View = crate::resolver_store::HostStoreView;
    type Mode = ResolverMode;
    type Resolution = ResolvedComponentMetaState;
    type CapturedInputs = CapturedComponentMetaInputs;

    fn cache_key(
        &self,
        canonical: &str,
        mode: Self::Mode,
    ) -> crate::resolver_core::ResolutionNodeKey {
        resolved_meta_cache_key(canonical, mode)
    }

    fn snapshot_store_view(&self) -> Self::View {
        self.resolver_store_view()
    }

    fn view_mutation_epoch(&self, store_view: &Self::View) -> u64 {
        store_view.mutation_epoch()
    }

    fn current_store_view_epoch(&self) -> u64 {
        VerterHost::current_store_view_epoch(self)
    }

    fn capture_component_meta_inputs(
        &self,
        canonical: &str,
        view: &Self::View,
    ) -> Option<Self::CapturedInputs> {
        let _trace = component_meta_trace_scope!(
            "capture_component_meta_inputs",
            format!("owner={} store_view=true", canonical),
        );
        let snapshot = self.get_raw_analysis_snapshot_in_view(canonical, Some(view))?;
        component_meta_trace_event!(
            "capture_component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        let (source, cached_parse, whole_hash) =
            self.current_eval_state_in_view(canonical, Some(view))?;
        component_meta_trace_event!(
            "capture_component_meta_eval_state",
            format!(
                "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                canonical,
                source.len(),
                cached_parse.is_some(),
            ),
        );
        let owner_eval_source =
            VerterHost::build_eval_script_source(&source, cached_parse.as_deref());
        let dep_resolutions =
            self.dependency_resolutions_for_eval_in_view(canonical, Some(view))?;
        component_meta_trace_event!(
            "capture_component_meta_inputs_result",
            format!(
                "owner={} owner_eval_source_len={} dep_resolutions={}",
                canonical,
                owner_eval_source.len(),
                dep_resolutions.len(),
            ),
        );
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            dep_resolutions,
        })
    }

    fn try_get_cached_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        store_view: &Self::View,
    ) -> Option<Self::Resolution> {
        let _trace = component_meta_trace_scope!(
            "try_get_cached_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = self.try_get_cached_resolved_meta(canonical, mode, store_view);
        component_meta_trace_event!(
            "try_get_cached_component_meta_result",
            format!("owner={} mode={mode:?} hit={}", canonical, result.is_some()),
        );
        result
    }

    fn compute_component_meta(
        &self,
        canonical: &str,
        mode: Self::Mode,
        captured: Option<&Self::CapturedInputs>,
        store_view: Option<&Self::View>,
    ) -> Option<Self::Resolution> {
        if let Some(captured) = captured {
            return self
                .compute_component_meta_state_from_captured(canonical, mode, captured, store_view);
        }

        let whole_hash = store_view
            .and_then(|view| view.whole_hash(canonical))
            .or_else(|| self.get_whole_hash(canonical))
            .unwrap_or_default();
        self.compute_component_meta_state(canonical, mode, whole_hash, store_view)
    }

    fn store_component_meta_result(
        &self,
        canonical: &str,
        mode: Self::Mode,
        result: &Self::Resolution,
    ) {
        self.store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }
}

/// Native declaration kind for the resolved pre-expansion type.
pub type ResolvedDeclarationKind = crate::resolver_core::ResolvedDeclarationKind;

/// Native pre-expansion declaration metadata retained by the shared resolver.
pub type ResolvedTypeDeclaration = crate::resolver_core::ResolvedTypeDeclaration;
pub type ResolvedTypeRegistryMeta = crate::resolver_core::ResolvedTypeRegistryMeta;
pub type ResolvedMacroMeta = crate::resolver_core::ResolvedMacroMeta;
pub type ResolvedNativeProp = crate::resolver_core::ResolvedNativeProp;
pub type ResolvedJsdocBlock = crate::resolver_core::ResolvedJsdocBlock;
pub type ResolvedJsdocTag = crate::resolver_core::ResolvedJsdocTag;

/// Host-owned sidecar result for component-meta / analysis enrichment.
///
/// Raw snapshot remains raw â€” resolved imported metadata lives in this sidecar.
/// `Expanded` mode carries materialized surfaces; `Type` mode carries
/// identity/location only.
#[derive(Debug, Clone)]
pub struct ResolvedComponentMetaState {
    /// The raw analysis snapshot (never mutated for enrichment).
    pub snapshot: FileAnalysisSnapshot,
    /// Which mode was used to produce this state.
    pub mode: ResolverMode,
    /// Content hash of the owner file at resolution time.
    pub whole_hash: Hash16,
    /// Resolved macro metadata from cross-file traversal.
    pub resolved_macros: Vec<ResolvedMacroMeta>,
    /// Resolved type registry entries (populated in `Expanded` mode).
    pub resolved_type_registry:
        Vec<verter_semantic::analysis::component_meta::ResolvedTypeAnalysis>,
    /// Native declaration metadata for each resolved type-registry entry.
    pub resolved_type_registry_meta: Vec<ResolvedTypeRegistryMeta>,
    /// Expanded types (populated in `Expanded` mode only).
    pub evaluated_types: Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
    /// Semantic fact versions consumed while producing this resolved state.
    pub fact_versions: Vec<crate::resolver_core::FactVersionRef>,
}

fn collect_expanded_slot_binding_param_types<'a>(
    ty: &'a verter_semantic::analysis::type_expr::TypeExpr,
    out: &mut Vec<&'a verter_semantic::analysis::type_expr::TypeExpr>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_binding_param_types(inner, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_binding_param_types(inner, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Function(func) => {
            if let Some(first) = func.parameters.first() {
                out.push(&first.ty);
            }
        }
        _ => {}
    }
}

fn collect_expanded_slot_bindings_from_object_type(
    ty: &verter_semantic::analysis::type_expr::TypeExpr,
    seen: &mut rustc_hash::FxHashSet<String>,
    out: &mut Vec<(String, verter_semantic::analysis::type_expr::TypeExpr, bool)>,
) {
    match ty {
        verter_semantic::analysis::type_expr::TypeExpr::Parenthesized(inner) => {
            collect_expanded_slot_bindings_from_object_type(inner, seen, out);
        }
        verter_semantic::analysis::type_expr::TypeExpr::Intersection(types)
        | verter_semantic::analysis::type_expr::TypeExpr::Union(types) => {
            for inner in types.iter() {
                collect_expanded_slot_bindings_from_object_type(inner, seen, out);
            }
        }
        verter_semantic::analysis::type_expr::TypeExpr::Object(obj) => {
            for member in &obj.properties {
                let verter_semantic::analysis::type_expr::ObjectMember::Property(prop) = member
                else {
                    continue;
                };
                if !seen.insert(prop.name.clone()) {
                    continue;
                }
                out.push((prop.name.clone(), prop.ty.clone(), prop.optional));
            }
        }
        _ => {}
    }
}

fn enrich_missing_slot_bindings(
    resolved_macros: &[ResolvedMacroMeta],
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) {
    let mut seen_names: rustc_hash::FxHashSet<String> = evaluated_types
        .slot_bindings
        .iter()
        .map(|binding| binding.name.clone())
        .collect();

    for entry in &evaluated_types.define_slots {
        for slot in &entry.result.value.properties {
            let mut binding_param_types = Vec::new();
            collect_expanded_slot_binding_param_types(&slot.ty, &mut binding_param_types);
            if binding_param_types.is_empty() {
                continue;
            }

            let mut seen_bindings = rustc_hash::FxHashSet::default();
            let mut bindings = Vec::new();
            for binding_param_ty in binding_param_types {
                collect_expanded_slot_bindings_from_object_type(
                    binding_param_ty,
                    &mut seen_bindings,
                    &mut bindings,
                );
            }

            for (binding_name, binding_type, optional) in bindings {
                let field_name = format!("{}.{}", slot.name, binding_name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                evaluated_types.slot_bindings.push(
                    verter_semantic::analysis::type_expand::ExpandedField {
                        name: field_name,
                        r#type: binding_type,
                        raw_type: None,
                        optional,
                        exactness: entry.result.exactness,
                        execution_status: entry.result.execution_status,
                        diagnostics: Vec::new(),
                    },
                );
            }
        }
    }

    for resolved in resolved_macros.iter().filter(|resolved| {
        resolved.macro_kind == verter_semantic::analysis::AnalyzedMacroKind::DefineSlots
    }) {
        for slot in &resolved.slots {
            for binding in &slot.bindings {
                let field_name = format!("{}.{}", slot.name, binding.name);
                if !seen_names.insert(field_name.clone()) {
                    continue;
                }
                let raw_type = binding.type_annotation.clone();
                let parsed_type = raw_type
                    .as_deref()
                    .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation)
                    .unwrap_or_else(|| verter_semantic::analysis::type_expr::TypeExpr::Unknown {
                        raw: "unknown".to_string(),
                    });
                evaluated_types
                    .slot_bindings
                    .push(verter_semantic::analysis::type_expand::ExpandedField {
                    name: field_name,
                    r#type: parsed_type,
                    raw_type,
                    optional: false,
                    exactness:
                        verter_semantic::analysis::type_expand::ExpansionExactness::ExactConcrete,
                    execution_status:
                        verter_semantic::analysis::type_expand::ExpansionExecutionStatus::Completed,
                    diagnostics: Vec::new(),
                });
            }
        }
    }
}

impl VerterHost {
    fn clone_cached_raw_analysis_snapshot(
        &self,
        canonical: &str,
        whole_hash: Hash16,
    ) -> Option<Arc<FileAnalysisSnapshot>> {
        self.raw_analysis_snapshot_cache
            .lock()
            .get(canonical)
            .and_then(|entry| (entry.whole_hash == whole_hash).then(|| Arc::clone(&entry.snapshot)))
    }

    fn cache_raw_analysis_snapshot(
        &self,
        canonical: &str,
        whole_hash: Hash16,
        snapshot: FileAnalysisSnapshot,
    ) -> Arc<FileAnalysisSnapshot> {
        let mut cache = self.raw_analysis_snapshot_cache.lock();
        if let Some(entry) = cache.get(canonical) {
            if entry.whole_hash == whole_hash {
                return Arc::clone(&entry.snapshot);
            }
        }
        let snapshot = Arc::new(snapshot);
        cache.insert(
            canonical.to_string(),
            crate::RawAnalysisSnapshotCacheEntry {
                whole_hash,
                snapshot: Arc::clone(&snapshot),
            },
        );
        snapshot
    }

    #[cfg(test)]
    pub(crate) fn raw_analysis_snapshot_cache_entry(
        &self,
        canonical: &str,
    ) -> Option<Arc<FileAnalysisSnapshot>> {
        self.raw_analysis_snapshot_cache
            .lock()
            .get(canonical)
            .map(|entry| Arc::clone(&entry.snapshot))
    }

    /// Single host-backed resolver API for cross-file component-meta enrichment.
    ///
    /// This is the ONLY entry point for cross-file component-meta resolution.
    /// Mode is chosen explicitly by callers â€” never inferred.
    ///
    /// - `Type`: resolves symbol identity, canonical location, and attached JSDoc
    ///   without materializing expanded shapes.
    /// - `Expanded`: resolves the same way, then materializes props/emits/slots,
    ///   populates the type registry, and computes evaluated types.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.resolve_component_meta_with_view(canonical_or_alias, mode, None)
    }

    pub(crate) fn resolve_component_meta_in_view(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
        store_view: &HostStoreView,
    ) -> Option<ResolvedComponentMetaState> {
        self.resolve_component_meta_with_view(canonical_or_alias, mode, Some(store_view))
    }

    fn resolve_component_meta_with_view(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
        store_view: Option<&HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let _trace = component_meta_trace_scope!(
            "resolve_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = run_component_meta_request(
            self,
            self.resolver_runtime().component_meta.singleflight(),
            &canonical,
            mode,
            store_view,
            STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

        if matches!(result.source, RequestSource::Cache) {
            self.provenance
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            self.provenance
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                self.provenance
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                self.provenance
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        if let Some(started) = started {
            match result.source {
                RequestSource::Cache => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} cached attempt={} took {:?}",
                    canonical,
                    mode,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Flight { role, .. } => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} role={:?} stable attempt={} total took {:?}",
                    canonical,
                    mode,
                    role,
                    result.attempts.saturating_sub(1),
                    started.elapsed(),
                )),
                RequestSource::Fallback => component_meta_debug(format!(
                    "resolve_component_meta owner={} mode={:?} retries_exhausted total took {:?}",
                    canonical,
                    mode,
                    started.elapsed(),
                )),
            }
        }

        if let Some(resolved) = result.value.as_ref() {
            component_meta_trace_event!(
                "resolve_component_meta_result",
                format!(
                    "owner={} mode={mode:?} source={} attempts={} macros={} resolved_types={} has_evaluated_types={} fact_versions={}",
                    canonical,
                    trace_request_source(result.source),
                    result.attempts,
                    resolved.resolved_macros.len(),
                    resolved.resolved_type_registry.len(),
                    resolved.evaluated_types.is_some(),
                    resolved.fact_versions.len(),
                ),
            );
        }

        result.value
    }

    pub(crate) fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(canonical, mode, whole_hash, None, store_view)
    }

    fn compute_component_meta_state_from_captured(
        &self,
        canonical: &str,
        mode: ResolverMode,
        captured: &CapturedComponentMetaInputs,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            store_view,
        )
    }

    fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ResolverMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<ResolvedComponentMetaState> {
        let _trace = component_meta_trace_scope!(
            "compute_component_meta_state",
            format!(
                "owner={} mode={mode:?} captured={} store_view={} whole_hash={whole_hash:?}",
                canonical,
                captured.is_some(),
                store_view.is_some(),
            ),
        );
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot_in_view(canonical, store_view))?;
        component_meta_trace_event!(
            "component_meta_snapshot",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} script_flags={}",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
                snapshot.script_flags,
            ),
        );
        let parts = crate::resolver_core::resolve_component_meta_parts(
            &HostComponentMetaResolver {
                host: self,
                store_view,
            },
            canonical,
            &snapshot,
            mode == ResolverMode::Expanded,
            captured,
        );
        let mut parts = parts;
        if let Some(evaluated_types) = parts.evaluated_types.as_mut() {
            enrich_missing_slot_bindings(&parts.resolved_macros, evaluated_types);
        }
        let registry_before = parts.resolved_type_registry.len();
        let append_start = std::time::Instant::now();
        self.append_component_meta_registry_entries(
            canonical,
            &snapshot,
            parts.evaluated_types.as_ref(),
            &mut parts.resolved_type_registry,
            &mut parts.resolved_type_registry_meta,
            &mut parts.tracked_dependencies,
            store_view,
        );
        let final_store_view = self.resolver_store_view();
        parts.fact_versions = self.current_dependency_fact_versions_in_view(
            canonical,
            &parts.tracked_dependencies,
            Some(&final_store_view),
        );
        let append_elapsed = append_start.elapsed();
        let registry_after = parts.resolved_type_registry.len();
        if crate::host_manage::component_meta_debug_enabled() {
            let dep_cache_size = self.imported_dependency_cache.lock().len();
            crate::host_manage::component_meta_debug(format!(
                "PROFILE owner={} registry_before={} registry_after={} registry_added={} dep_cache_entries={} append_ms={:.1}",
                canonical,
                registry_before,
                registry_after,
                registry_after - registry_before,
                dep_cache_size,
                append_elapsed.as_secs_f64() * 1000.0,
            ));
        }
        component_meta_trace_event!(
            "component_meta_parts",
            format!(
                "owner={} resolved_macros={} resolved_type_registry={} has_evaluated_types={} fact_versions={}",
                canonical,
                parts.resolved_macros.len(),
                parts.resolved_type_registry.len(),
                parts.evaluated_types.is_some(),
                parts.fact_versions.len(),
            ),
        );
        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros: parts.resolved_macros,
            resolved_type_registry: parts.resolved_type_registry,
            resolved_type_registry_meta: parts.resolved_type_registry_meta,
            evaluated_types: parts.evaluated_types,
            fact_versions: parts.fact_versions,
        };
        Some(state)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    fn append_component_meta_registry_entries(
        &self,
        owner_canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
        resolved_type_registry: &mut Vec<
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
        >,
        resolved_type_registry_meta: &mut Vec<ResolvedTypeRegistryMeta>,
        tracked_dependencies: &mut BTreeSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        fn track_component_meta_dependency(
            tracked_dependencies: &mut BTreeSet<String>,
            owner_canonical: &str,
            canonical_id: &str,
        ) {
            if !canonical_id.is_empty() && canonical_id != owner_canonical {
                tracked_dependencies.insert(canonical_id.to_string());
            }
        }

        let mut append_cache = ComponentMetaRegistryAppendCache::default();
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let Some(meta) = resolved_type_registry_meta.get_mut(index) else {
                continue;
            };
            let declaration_source = meta.declaration.canonical_source.as_str();
            if declaration_source.is_empty() || declaration_source == owner_canonical {
                continue;
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_source,
            );
            let requested_exported_name = if meta.declaration.resolved_name.is_empty() {
                entry.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let Some((resolved_canonical_id, resolved_exported_name, prepared)) =
                cached_resolve_prepared_symbol_dependency_alias_in_view(
                    &mut append_cache,
                    self,
                    declaration_source,
                    requested_exported_name,
                    store_view,
                )
            else {
                continue;
            };
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                resolved_canonical_id.as_str(),
            );
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                resolved_canonical_id.as_str(),
            );
            for dependency in &prepared.canonical_dependencies {
                track_component_meta_dependency(
                    tracked_dependencies,
                    owner_canonical,
                    dependency.as_str(),
                );
            }
            meta.declaration.canonical_source = resolved_canonical_id.clone();
            let materialized = cached_solve_component_meta_registry_decl_in_view(
                &mut append_cache,
                self,
                resolved_canonical_id.as_str(),
                resolved_exported_name.as_str(),
                store_view,
            )
            .unwrap_or_else(|| prepared.decl.body.clone());
            entry.type_expr = choose_preferred_component_meta_registry_candidate(
                Some(entry.type_expr.clone()),
                Some(materialized),
            )
            .unwrap_or_else(|| entry.type_expr.clone());
        }

        let mut referenced_names: VecDeque<PendingComponentMetaRegistryRef> = VecDeque::new();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut published_names: rustc_hash::FxHashSet<String> = resolved_type_registry
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        if let Some(evaluated_types) = evaluated_types {
            for field in &evaluated_types.props {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    store_view,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            for field in &evaluated_types.emits {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    store_view,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            for field in &evaluated_types.slot_bindings {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    store_view,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
        }
        for (index, entry) in resolved_type_registry.iter().enumerate() {
            let source_hint = resolved_type_registry_meta
                .get(index)
                .map(|meta| meta.declaration.canonical_source.as_str());
            if should_collect_component_meta_registry_nested_refs(owner_canonical, source_hint) {
                let source_expr = cached_owner_component_meta_registry_collection_expr(
                    &mut append_cache,
                    self,
                    owner_canonical,
                    entry.name.as_str(),
                    store_view,
                );
                collect_component_meta_registry_refs(
                    source_expr.as_ref().unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                    false,
                );
            }
        }

        let mut _loop_iterations: usize = 0;
        let mut _loop_materializations: usize = 0;
        let _loop_start = std::time::Instant::now();
        while let Some(pending) = referenced_names.pop_front() {
            _loop_iterations += 1;
            let type_name = pending.name;
            let imported_owner_route =
                owner_component_meta_registry_import_binding(snapshot, type_name.as_str()).filter(
                    |_| {
                        pending
                            .source_hint
                            .as_deref()
                            .is_none_or(|source| source.is_empty() || source == owner_canonical)
                    },
                );
            let pending_source_hint = imported_owner_route
                .as_ref()
                .map(|(canonical_id, _)| canonical_id.as_str())
                .or(pending.source_hint.as_deref());
            let pending_exported_name = imported_owner_route
                .as_ref()
                .map(|(_, exported_name)| exported_name.as_str())
                .or(pending.exported_name.as_deref());
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING owner={} name={} source_hint={:?} exported={:?}",
                    owner_canonical, type_name, pending_source_hint, pending_exported_name,
                ));
            }
            if published_names.contains(&type_name) {
                continue;
            }
            if !cached_component_meta_registry_ref_can_resolve(
                &mut append_cache,
                self,
                owner_canonical,
                pending_exported_name.unwrap_or(type_name.as_str()),
                pending_source_hint,
                store_view,
            ) {
                continue;
            }
            let requested_exported_name = pending_exported_name.unwrap_or(type_name.as_str());
            if let Some(source_hint) = pending_source_hint
                .filter(|source| !source.is_empty() && *source != owner_canonical)
            {
                track_component_meta_dependency(tracked_dependencies, owner_canonical, source_hint);
                if let Some((resolved_canonical_id, resolved_exported_name, prepared)) =
                    cached_resolve_prepared_symbol_dependency_alias_in_view(
                        &mut append_cache,
                        self,
                        source_hint,
                        requested_exported_name,
                        store_view,
                    )
                {
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        resolved_canonical_id.as_str(),
                    );
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        resolved_canonical_id.as_str(),
                    );
                    for dependency in &prepared.canonical_dependencies {
                        track_component_meta_dependency(
                            tracked_dependencies,
                            owner_canonical,
                            dependency.as_str(),
                        );
                    }
                    let mut declaration = cached_resolve_type_declaration_in_view(
                        &mut append_cache,
                        self,
                        resolved_canonical_id.as_str(),
                        resolved_exported_name.as_str(),
                        store_view,
                    );
                    if declaration.canonical_source.is_empty() {
                        declaration.canonical_source = resolved_canonical_id.clone();
                    }
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        declaration.canonical_source.as_str(),
                    );
                    let type_expr = cached_solve_component_meta_registry_decl_in_view(
                        &mut append_cache,
                        self,
                        resolved_canonical_id.as_str(),
                        resolved_exported_name.as_str(),
                        store_view,
                    )
                    .unwrap_or_else(|| prepared.decl.body.clone());
                    upsert_component_meta_registry_entry(
                        owner_canonical,
                        resolved_type_registry,
                        resolved_type_registry_meta,
                        &mut published_names,
                        &mut queued_names,
                        &mut referenced_names,
                        type_name.clone(),
                        type_expr,
                        declaration,
                        None,
                    );
                    continue;
                }
            }

            let declaration_owner = pending_source_hint
                .filter(|source| !source.is_empty())
                .unwrap_or(owner_canonical);
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_owner,
            );
            let mut declaration = cached_resolve_type_declaration_in_view(
                &mut append_cache,
                self,
                declaration_owner,
                type_name.as_str(),
                store_view,
            );
            if declaration.canonical_source.is_empty() && declaration_owner != owner_canonical {
                declaration = cached_resolve_type_declaration_in_view(
                    &mut append_cache,
                    self,
                    owner_canonical,
                    type_name.as_str(),
                    store_view,
                );
            }
            let mut materialized = if declaration_owner != owner_canonical {
                cached_solve_component_meta_registry_decl_in_view(
                    &mut append_cache,
                    self,
                    declaration_owner,
                    type_name.as_str(),
                    store_view,
                )
            } else {
                None
            };
            let owner_collection_expr = cached_owner_component_meta_registry_collection_expr(
                &mut append_cache,
                self,
                owner_canonical,
                type_name.as_str(),
                store_view,
            );
            materialized = materialized.or_else(|| {
                cached_solve_component_meta_registry_decl_in_view(
                    &mut append_cache,
                    self,
                    owner_canonical,
                    type_name.as_str(),
                    store_view,
                )
            });
            if materialized.is_some() && declaration.canonical_source.is_empty() {
                if let Some(import) = snapshot
                    .imports
                    .iter()
                    .find(|imp| imp.bindings.iter().any(|b| b.name == type_name))
                {
                    if let Some(canonical_id) = import.resolved_canonical_id.as_deref() {
                        if let Some(binding) = import.bindings.iter().find(|b| b.name == type_name)
                        {
                            declaration.canonical_source = canonical_id.to_string();
                            declaration.resolved_name = binding
                                .imported_name
                                .as_deref()
                                .unwrap_or("default")
                                .to_string();
                        }
                    }
                }
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration.canonical_source.as_str(),
            );
            let Some(materialized) = materialized else {
                continue;
            };
            _loop_materializations += 1;
            upsert_component_meta_registry_entry(
                owner_canonical,
                resolved_type_registry,
                resolved_type_registry_meta,
                &mut published_names,
                &mut queued_names,
                &mut referenced_names,
                type_name.clone(),
                materialized,
                declaration,
                owner_collection_expr.as_ref(),
            );
        }
        if crate::host_manage::component_meta_debug_enabled()
            && (_loop_materializations > 0 || _loop_iterations > 0)
        {
            crate::host_manage::component_meta_debug(format!(
                "REGISTRY_LOOP owner={} iterations={} materializations={} published={} loop_ms={:.1}",
                owner_canonical,
                _loop_iterations,
                _loop_materializations,
                published_names.len(),
                _loop_start.elapsed().as_secs_f64() * 1000.0,
            ));
        }
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// This bypasses any legacy `get_analysis()` enrichment path, returning only the base snapshot
    /// with resolved imports and destructured bindings.
    pub(crate) fn get_raw_analysis_snapshot_in_view(
        &self,
        canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<FileAnalysisSnapshot> {
        let _trace = component_meta_trace_scope!(
            "get_raw_analysis_snapshot",
            format!("owner={} store_view={}", canonical, store_view.is_some()),
        );
        // Eviction gate (scheduler path)
        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }
        }

        #[cfg(feature = "scheduler")]
        {
            let mut fallback_whole_hash = None;
            let mut fallback_source: Option<Arc<str>> = None;
            let mut snapshot = if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical)
            {
                let whole_hash = store_view
                    .and_then(|view| view.whole_hash(canonical))
                    .or_else(|| self.get_whole_hash(canonical))
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash, store_view) {
                    return None;
                }
                component_meta_trace_event!(
                    "get_raw_analysis_snapshot_scheduler_hit",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={}",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                snapshot
            } else {
                if let Some(imported_entry) =
                    self.clone_current_imported_dependency_entry(canonical, store_view)
                {
                    if let Some(snapshot) = imported_entry.snapshot.clone() {
                        if store_view.is_none() {
                            self.provenance
                                .raw_analysis_snapshot_cache_hits
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        component_meta_trace_event!(
                            "get_raw_analysis_snapshot_imported_cache",
                            format!(
                                "owner={} hit=true imports={} macros={} bindings={} has_template={} whole_hash={:?}",
                                canonical,
                                snapshot.imports.len(),
                                snapshot.macros.len(),
                                snapshot.bindings.len(),
                                snapshot.template.is_some(),
                                imported_entry.whole_hash,
                            ),
                        );
                        let mut snapshot = if store_view.is_none() {
                            (*self.cache_raw_analysis_snapshot_arc(
                                canonical,
                                imported_entry.whole_hash,
                                snapshot,
                            ))
                            .clone()
                        } else {
                            (*snapshot).clone()
                        };
                        if self.config.effective_scope().needs_template_analysis() {
                            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                        }
                        component_meta_trace_event!(
                            "get_raw_analysis_snapshot_result",
                            format!(
                                "owner={} imports={} macros={} bindings={} has_template={}",
                                canonical,
                                snapshot.imports.len(),
                                snapshot.macros.len(),
                                snapshot.bindings.len(),
                                snapshot.template.is_some(),
                            ),
                        );
                        return Some(snapshot);
                    }
                }

                let source = self.read_analysis_source_in_view(canonical, store_view)?;
                let whole_hash = crate::hash::hash_16(source.as_bytes());
                fallback_whole_hash = Some(whole_hash);
                fallback_source = Some(Arc::clone(&source));
                if store_view.is_none() {
                    if let Some(snapshot) =
                        self.clone_cached_raw_analysis_snapshot(canonical, whole_hash)
                    {
                        self.provenance
                            .raw_analysis_snapshot_cache_hits
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        component_meta_trace_event!(
                            "get_raw_analysis_snapshot_host_cache",
                            format!(
                                "owner={} hit=true bytes={} whole_hash={whole_hash:?}",
                                canonical,
                                source.len(),
                            ),
                        );
                        return Some((*snapshot).clone());
                    }
                    self.provenance
                        .raw_analysis_snapshot_cache_misses
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    component_meta_trace_event!(
                        "get_raw_analysis_snapshot_host_cache",
                        format!(
                            "owner={} hit=false bytes={} whole_hash={whole_hash:?}",
                            canonical,
                            source.len(),
                        ),
                    );
                }
                component_meta_trace_event!(
                    "get_raw_analysis_snapshot_build_from_source",
                    format!("owner={} source_len={}", canonical, source.len()),
                );
                self.build_snapshot_from_source(canonical, &source)
            };
            self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            if let (Some(whole_hash), Some(source)) = (
                fallback_whole_hash,
                fallback_source.as_ref().map(Arc::clone),
            ) {
                let dependency_resolutions =
                    VerterHost::dependency_resolutions_from_snapshot(&snapshot);
                let _ = self.cache_imported_dependency_state(
                    canonical,
                    whole_hash,
                    source,
                    None,
                    Some(Arc::new(snapshot.clone())),
                    None,
                    None,
                    dependency_resolutions,
                );
            }
            component_meta_trace_event!(
                "get_raw_analysis_snapshot_result",
                format!(
                    "owner={} imports={} macros={} bindings={} has_template={}",
                    canonical,
                    snapshot.imports.len(),
                    snapshot.macros.len(),
                    snapshot.bindings.len(),
                    snapshot.template.is_some(),
                ),
            );
            if let Some(whole_hash) = fallback_whole_hash.filter(|_| store_view.is_none()) {
                let cached = self.cache_raw_analysis_snapshot(canonical, whole_hash, snapshot);
                return Some((*cached).clone());
            }
            Some(snapshot)
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            if !self.store_view_allows_current_whole_hash(canonical, entry.whole_hash, store_view) {
                return None;
            }
            // Use build_snapshot_from_entry for Arc::clone pointer bumps
            // instead of allocating new Arcs.
            let mut snapshot = Self::build_snapshot_from_entry(entry);
            component_meta_trace_event!(
                "get_raw_analysis_snapshot_cache_hit",
                format!(
                    "owner={} imports={} macros={} bindings={} has_template={}",
                    canonical,
                    snapshot.imports.len(),
                    snapshot.macros.len(),
                    snapshot.bindings.len(),
                    snapshot.template.is_some(),
                ),
            );
            drop(files);
            self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
            self.enrich_destructured_bindings(&mut snapshot);
            if self.config.effective_scope().needs_template_analysis() {
                self.compute_template_analysis_if_missing(canonical, &mut snapshot);
            }
            component_meta_trace_event!(
                "get_raw_analysis_snapshot_result",
                format!(
                    "owner={} imports={} macros={} bindings={} has_template={}",
                    canonical,
                    snapshot.imports.len(),
                    snapshot.macros.len(),
                    snapshot.bindings.len(),
                    snapshot.template.is_some(),
                ),
            );
            Some(snapshot)
        }
    }

    pub(crate) fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        store_view: &crate::resolver_store::HostStoreView,
    ) -> Option<ResolvedComponentMetaState> {
        let cache_key = resolved_meta_cache_key(canonical, mode);
        if let Some(cached) = self
            .resolver_runtime()
            .component_meta
            .get_if_valid(&cache_key, store_view)
        {
            self.mirror_cached_resolved_meta_arc(canonical, mode, cached.clone());
            return Some(cached.as_ref().clone());
        }

        #[cfg(feature = "scheduler")]
        {
            let entry = self.compile_cache.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            let invalid_details = store_view.invalid_fact_details(&cached.fact_versions, 6);
            if !invalid_details.is_empty() {
                component_meta_trace_event!(
                    "try_get_cached_component_meta_invalid",
                    format!(
                        "owner={} mode={mode:?} cache=legacy facts={} invalid={} details=[{}]",
                        canonical,
                        cached.fact_versions.len(),
                        invalid_details.len(),
                        invalid_details.join(" | "),
                    ),
                );
                return None;
            }
            self.resolver_runtime().component_meta.insert_arc(
                cache_key,
                cached.state.clone(),
                cached.fact_versions.clone(),
            );
            Some(cached.state.as_ref().clone())
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;

            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            let cached = entry.cached_resolved_meta.get(&mode)?;
            let invalid_details = store_view.invalid_fact_details(&cached.fact_versions, 6);
            if !invalid_details.is_empty() {
                component_meta_trace_event!(
                    "try_get_cached_component_meta_invalid",
                    format!(
                        "owner={} mode={mode:?} cache=legacy facts={} invalid={} details=[{}]",
                        canonical,
                        cached.fact_versions.len(),
                        invalid_details.len(),
                        invalid_details.join(" | "),
                    ),
                );
                return None;
            }
            self.resolver_runtime().component_meta.insert_arc(
                cache_key,
                cached.state.clone(),
                cached.fact_versions.clone(),
            );
            Some(cached.state.as_ref().clone())
        }
    }

    fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) {
        component_meta_trace_event!(
            "store_cached_component_meta_result",
            format!(
                "owner={} mode={mode:?} facts={} macros={} resolved_types={} has_evaluated_types={}",
                canonical,
                fact_versions.len(),
                state.resolved_macros.len(),
                state.resolved_type_registry.len(),
                state.evaluated_types.is_some(),
            ),
        );
        let state = Arc::new(state.clone());
        self.resolver_runtime().component_meta.insert_arc(
            resolved_meta_cache_key(canonical, mode),
            state.clone(),
            fact_versions.to_vec(),
        );
        self.mirror_cached_resolved_meta_arc(canonical, mode, state);
    }

    fn mirror_cached_resolved_meta_arc(
        &self,
        canonical: &str,
        mode: ResolverMode,
        state: Arc<ResolvedComponentMetaState>,
    ) {
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            fact_versions: state.fact_versions.clone(),
            state,
        };

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::write_lock;

            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_resolved_meta.insert(mode, cached);
            }
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // Encoded payload cache (shared by NAPI/WASM)
    // ───────────────────────────────────────────────────────────────────────

    /// Try to return a cached encoded payload for the given meta kind.
    /// Validates fact versions against the captured `store_view`.
    pub(crate) fn try_get_cached_meta_payload(
        &self,
        canonical: &str,
        kind: crate::types::MetaPayloadKind,
        store_view: &crate::resolver_store::HostStoreView,
    ) -> Option<Vec<u8>> {
        #[cfg(feature = "scheduler")]
        {
            let entry = self.compile_cache.get(canonical)?;
            let cached = entry.cached_meta_payloads.get(&kind)?;
            if store_view.validates_all(&cached.fact_versions) {
                return Some(cached.payload.clone());
            }
            None
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::read_lock;
            let files = read_lock(&self.files);
            let entry = files.get(canonical)?;
            let cached = entry.cached_meta_payloads.get(&kind)?;
            if store_view.validates_all(&cached.fact_versions) {
                return Some(cached.payload.clone());
            }
            None
        }
    }

    /// Store an encoded payload in the per-file cache.
    pub(crate) fn store_meta_payload(
        &self,
        canonical: &str,
        kind: crate::types::MetaPayloadKind,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        payload: Vec<u8>,
    ) {
        let cached = crate::types::CachedMetaPayload {
            fact_versions: fact_versions.to_vec(),
            payload,
        };

        #[cfg(feature = "scheduler")]
        {
            if let Some(mut entry) = self.compile_cache.get_mut(canonical) {
                entry.cached_meta_payloads.insert(kind, cached);
            }
        }

        #[cfg(not(feature = "scheduler"))]
        {
            use crate::shared::write_lock;
            let mut files = write_lock(&self.files);
            if let Some(entry) = files.get_mut(canonical) {
                entry.cached_meta_payloads.insert(kind, cached);
            }
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.current_dependency_fact_versions_in_view(canonical, tracked_deps, None)
    }

    pub(crate) fn current_dependency_fact_versions_in_view(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions_in_view(canonical, &mut facts, &mut seen, store_view);
        for dep in tracked_deps {
            self.append_dependency_fact_versions_in_view(
                dep.as_str(),
                &mut facts,
                &mut seen,
                store_view,
            );
        }

        facts
    }

    #[cfg(test)]
    pub(crate) fn fact_versions_match(
        &self,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) -> bool {
        let view = self.resolver_store_view();
        fact_versions
            .iter()
            .all(|fact| crate::resolver_core::StoreView::validates(&view, fact))
    }

    fn append_dependency_fact_versions_in_view(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) {
        if let Some(hash) = store_view
            .and_then(|view| view.whole_hash(canonical))
            .or_else(|| self.get_whole_hash(canonical))
        {
            let file_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
                canonical_id: canonical.to_string(),
                hash,
            };
            if seen.insert(file_fact.clone()) {
                facts.push(file_fact);
            }
        }

        for kind in [
            crate::resolver_core::DerivedFactKind::ExportRegistry,
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::BarrelSurface,
            crate::resolver_core::DerivedFactKind::ExactResolution,
        ] {
            if let Some(hash) = store_view
                .and_then(|view| view.derived_hash(canonical, kind))
                .or_else(|| self.current_derived_fact_hash(canonical, kind))
            {
                let fact = crate::resolver_core::FactVersionRef::DerivedFactHash {
                    canonical_id: canonical.to_string(),
                    kind,
                    hash,
                };
                if seen.insert(fact.clone()) {
                    facts.push(fact);
                }
            }
        }

        if let Some(generation) = store_view
            .and_then(|view| view.barrel_generation(canonical))
            .or_else(|| self.current_barrel_generation(canonical))
        {
            let fact = crate::resolver_core::FactVersionRef::BarrelGeneration {
                canonical_id: canonical.to_string(),
                generation,
            };
            if seen.insert(fact.clone()) {
                facts.push(fact);
            }
        }
    }

    fn current_derived_fact_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        match kind {
            crate::resolver_core::DerivedFactKind::DirectSource => {
                self.get_whole_hash(canonical_id)
            }
            crate::resolver_core::DerivedFactKind::ExportRegistry => {
                #[cfg(feature = "scheduler")]
                {
                    self.compile_cache.get(canonical_id).and_then(|cc| {
                        cc.export_registry
                            .as_ref()
                            .map(|registry| registry.source_hash)
                    })
                }
                #[cfg(not(feature = "scheduler"))]
                {
                    crate::shared::read_lock(&self.files)
                        .get(canonical_id)
                        .and_then(|entry| entry.export_registry.as_ref())
                        .map(|registry| registry.source_hash)
                }
            }
            crate::resolver_core::DerivedFactKind::BarrelSurface => {
                #[cfg(feature = "scheduler")]
                {
                    self.compile_cache.get(canonical_id).and_then(|cc| {
                        cc.barrel_export_surface
                            .as_ref()
                            .map(|surface| surface.source_hash)
                    })
                }
                #[cfg(not(feature = "scheduler"))]
                {
                    crate::shared::read_lock(&self.files)
                        .get(canonical_id)
                        .and_then(|entry| entry.barrel_export_surface.as_ref())
                        .map(|surface| surface.source_hash)
                }
            }
            crate::resolver_core::DerivedFactKind::Route => {
                #[cfg(feature = "scheduler")]
                {
                    self.compile_cache.get(canonical_id).and_then(|cc| {
                        (!cc.import_route_cache.is_empty()).then(|| {
                            crate::resolver_store::hash_import_route_cache(&cc.import_route_cache)
                        })
                    })
                }
                #[cfg(not(feature = "scheduler"))]
                {
                    crate::shared::read_lock(&self.files)
                        .get(canonical_id)
                        .and_then(|entry| {
                            (!entry.import_route_cache.is_empty()).then(|| {
                                crate::resolver_store::hash_import_route_cache(
                                    &entry.import_route_cache,
                                )
                            })
                        })
                }
            }
            crate::resolver_core::DerivedFactKind::ExactResolution => self
                .dependency_resolutions_for_eval_in_view(canonical_id, None)
                .and_then(|resolutions| {
                    (!resolutions.is_empty())
                        .then(|| crate::resolver_store::hash_dependency_resolutions(&resolutions))
                }),
        }
    }

    fn current_barrel_generation(&self, canonical_id: &str) -> Option<u64> {
        #[cfg(feature = "scheduler")]
        {
            self.compile_cache.get(canonical_id).and_then(|cc| {
                cc.barrel_export_surface
                    .as_ref()
                    .map(|surface| surface.generation)
            })
        }

        #[cfg(not(feature = "scheduler"))]
        {
            crate::shared::read_lock(&self.files)
                .get(canonical_id)
                .and_then(|entry| entry.barrel_export_surface.as_ref())
                .map(|surface| surface.generation)
        }
    }
}

#[derive(Debug, Clone)]
struct PendingComponentMetaRegistryRef {
    name: String,
    source_hint: Option<String>,
    exported_name: Option<String>,
}

type PreparedAliasEntry = Option<(
    String,
    String,
    crate::resolver_core::CachedPreparedImportedTypeAlias,
)>;

#[derive(Default)]
struct ComponentMetaRegistryAppendCache {
    owner_collection_exprs:
        rustc_hash::FxHashMap<String, Option<verter_semantic::analysis::type_expr::TypeExpr>>,
    materialized_exprs: rustc_hash::FxHashMap<
        (String, String),
        Option<verter_semantic::analysis::type_expr::TypeExpr>,
    >,
    declarations: rustc_hash::FxHashMap<(String, String), ResolvedTypeDeclaration>,
    prepared_aliases: rustc_hash::FxHashMap<(String, String), PreparedAliasEntry>,
    resolvable_refs: rustc_hash::FxHashMap<(String, String), bool>,
}

fn upsert_component_meta_registry_entry(
    owner_canonical: &str,
    resolved_type_registry: &mut Vec<
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
    >,
    resolved_type_registry_meta: &mut Vec<ResolvedTypeRegistryMeta>,
    published_names: &mut rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: String,
    type_expr: verter_semantic::analysis::type_expr::TypeExpr,
    declaration: ResolvedTypeDeclaration,
    collection_expr: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
) {
    let declaration_source_hint =
        (!declaration.canonical_source.is_empty()).then(|| declaration.canonical_source.clone());
    let collect_nested_refs = should_collect_component_meta_registry_nested_refs(
        owner_canonical,
        declaration_source_hint.as_deref(),
    );
    if let Some(index) = resolved_type_registry
        .iter()
        .position(|entry| entry.name == name)
    {
        let existing = resolved_type_registry[index].type_expr.clone();
        let preferred = choose_preferred_component_meta_registry_candidate(
            Some(existing.clone()),
            Some(type_expr),
        )
        .unwrap_or(existing.clone());
        if preferred != existing {
            resolved_type_registry[index].type_expr = preferred.clone();
            if let Some(meta) = resolved_type_registry_meta.get_mut(index) {
                *meta = ResolvedTypeRegistryMeta {
                    name: name.clone(),
                    declaration,
                };
            }
            if collect_nested_refs {
                collect_component_meta_registry_refs(
                    collection_expr.unwrap_or(&preferred),
                    published_names,
                    queued_names,
                    referenced_names,
                    declaration_source_hint.as_deref(),
                    false,
                );
            }
        }
        return;
    }

    if collect_nested_refs {
        collect_component_meta_registry_refs(
            collection_expr.unwrap_or(&type_expr),
            published_names,
            queued_names,
            referenced_names,
            declaration_source_hint.as_deref(),
            false,
        );
    }
    resolved_type_registry.push(
        verter_semantic::analysis::component_meta::ResolvedTypeAnalysis {
            name: name.clone(),
            type_expr,
            type_expansion: None,
        },
    );
    resolved_type_registry_meta.push(ResolvedTypeRegistryMeta {
        name: name.clone(),
        declaration,
    });
    published_names.insert(name);
}

fn cached_owner_component_meta_registry_collection_expr(
    cache: &mut ComponentMetaRegistryAppendCache,
    host: &VerterHost,
    owner_canonical: &str,
    name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    if let Some(cached) = cache.owner_collection_exprs.get(name) {
        return cached.clone();
    }
    let expr =
        owner_component_meta_registry_collection_expr(host, owner_canonical, name, store_view);
    cache
        .owner_collection_exprs
        .insert(name.to_string(), expr.clone());
    expr
}

fn cached_solve_component_meta_registry_decl_in_view(
    cache: &mut ComponentMetaRegistryAppendCache,
    host: &VerterHost,
    scope_canonical_id: &str,
    requested_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let key = (scope_canonical_id.to_string(), requested_name.to_string());
    if let Some(cached) = cache.materialized_exprs.get(&key) {
        return cached.clone();
    }
    let solved = solve_component_meta_registry_decl_in_view(
        host,
        scope_canonical_id,
        requested_name,
        store_view,
    );
    cache.materialized_exprs.insert(key, solved.clone());
    solved
}

fn cached_resolve_type_declaration_in_view(
    cache: &mut ComponentMetaRegistryAppendCache,
    host: &VerterHost,
    canonical_source: &str,
    requested_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> ResolvedTypeDeclaration {
    let key = (canonical_source.to_string(), requested_name.to_string());
    if let Some(cached) = cache.declarations.get(&key) {
        return cached.clone();
    }
    let declaration =
        resolve_type_declaration_in_view(host, canonical_source, requested_name, store_view);
    cache.declarations.insert(key, declaration.clone());
    declaration
}

fn cached_resolve_prepared_symbol_dependency_alias_in_view(
    cache: &mut ComponentMetaRegistryAppendCache,
    host: &VerterHost,
    canonical_id: &str,
    exported_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<(
    String,
    String,
    crate::resolver_core::CachedPreparedImportedTypeAlias,
)> {
    let key = (canonical_id.to_string(), exported_name.to_string());
    if let Some(cached) = cache.prepared_aliases.get(&key) {
        return cached.clone();
    }
    let resolved = host.resolve_prepared_symbol_dependency_alias_in_view(
        canonical_id,
        exported_name,
        store_view,
    );
    cache.prepared_aliases.insert(key, resolved.clone());
    resolved
}

fn cached_component_meta_registry_ref_can_resolve(
    cache: &mut ComponentMetaRegistryAppendCache,
    host: &VerterHost,
    owner_canonical: &str,
    name: &str,
    source_hint: Option<&str>,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> bool {
    if is_component_meta_registry_builtin_name(name) {
        return false;
    }

    let scope_canonical_id = source_hint
        .filter(|source| !source.is_empty())
        .unwrap_or(owner_canonical);
    let key = (scope_canonical_id.to_string(), name.to_string());
    if let Some(cached) = cache.resolvable_refs.get(&key) {
        return *cached;
    }

    let resolvable = if host
        .prepared_type_decl_in_view(scope_canonical_id, name, store_view)
        .is_some()
    {
        true
    } else {
        let (resolved_canonical_id, resolved_name) =
            host.resolve_imported_type_root_in_view(scope_canonical_id, name, store_view);
        (resolved_canonical_id != scope_canonical_id || resolved_name != name)
            && host
                .prepared_type_decl_in_view(
                    resolved_canonical_id.as_str(),
                    resolved_name.as_str(),
                    store_view,
                )
                .is_some()
    };

    cache.resolvable_refs.insert(key, resolvable);
    resolvable
}

fn is_component_meta_registry_builtin_name(name: &str) -> bool {
    verter_semantic::analysis::type_solver::builtin::BuiltinUtility::from_name(name).is_some()
        || matches!(name, "Array" | "ReadonlyArray" | "Promise")
}

fn is_component_meta_registry_package_source(source_hint: Option<&str>) -> bool {
    source_hint.is_some_and(|source| source.contains("/node_modules/"))
}

fn should_collect_component_meta_registry_nested_refs(
    owner_canonical: &str,
    source_hint: Option<&str>,
) -> bool {
    match source_hint.filter(|source| !source.is_empty()) {
        Some(source) => source == owner_canonical,
        None => true,
    }
}

fn owner_component_meta_registry_collection_expr(
    host: &VerterHost,
    owner_canonical: &str,
    name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    host.prepared_type_decl_in_view(owner_canonical, name, store_view)
        .map(|prepared| prepared.body.clone())
}

fn owner_component_meta_registry_import_binding(
    snapshot: &FileAnalysisSnapshot,
    local_name: &str,
) -> Option<(String, String)> {
    snapshot.imports.iter().find_map(|import| {
        let canonical_id = import.resolved_canonical_id.as_ref()?;
        let binding = import
            .bindings
            .iter()
            .find(|binding| binding.name == local_name)?;
        let exported_name = binding
            .imported_name
            .clone()
            .unwrap_or_else(|| local_name.to_string());
        Some((canonical_id.clone(), exported_name))
    })
}

fn solve_component_meta_registry_decl_in_view(
    host: &VerterHost,
    scope_canonical_id: &str,
    requested_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    if is_component_meta_registry_package_source(Some(scope_canonical_id)) {
        return host
            .prepared_type_decl_in_view(scope_canonical_id, requested_name, store_view)
            .map(|prepared| prepared.body.clone());
    }

    if let Some(prepared) =
        host.prepared_type_decl_in_view(scope_canonical_id, requested_name, store_view)
    {
        let direct_surface = matches!(
            &prepared.body,
            verter_semantic::analysis::type_expr::TypeExpr::Object(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Union(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Intersection(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Array { .. }
                | verter_semantic::analysis::type_expr::TypeExpr::Tuple { .. }
                | verter_semantic::analysis::type_expr::TypeExpr::Function(_)
                | verter_semantic::analysis::type_expr::TypeExpr::Mapped { .. }
        );
        let preserve_prepared_surface = direct_surface
            && prepared.local_deps.is_empty()
            && (prepared.external_deps.is_empty()
                || prepared
                    .external_deps
                    .iter()
                    .all(|dep| dep.canonical_id.contains("/node_modules/")));
        if preserve_prepared_surface {
            return Some(prepared.body.clone());
        }
    }

    host.shallow_file_state_in_view(scope_canonical_id, store_view)?;
    let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
        host,
        store_view,
        scope_canonical_id,
    );
    let type_ref = verter_semantic::analysis::type_expr::TypeExpr::named(requested_name);
    let result = verter_semantic::analysis::type_solver::solve::solve_type(&type_ref, &solver_host);

    match &result.value {
        verter_semantic::analysis::type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        } if name.as_ref() == requested_name && type_arguments.is_empty() => None,
        _ => Some(result.value),
    }
}

fn enqueue_component_meta_registry_ref(
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    referenced_names: &mut VecDeque<PendingComponentMetaRegistryRef>,
    name: &str,
    source_hint: Option<&str>,
    exported_name: Option<&str>,
) {
    if published_names.contains(name) {
        return;
    }
    let source_hint = source_hint
        .filter(|source| !source.is_empty())
        .map(str::to_string);
    let exported_name = exported_name
        .filter(|exported| !exported.is_empty())
        .map(str::to_string);
    if !queued_names.insert(name.to_string()) {
        if let Some(existing) = referenced_names
            .iter_mut()
            .find(|pending| pending.name == name)
        {
            if existing.source_hint.is_none() {
                existing.source_hint = source_hint;
            }
            if existing.exported_name.is_none() {
                existing.exported_name = exported_name;
            }
        }
        return;
    }
    referenced_names.push_back(PendingComponentMetaRegistryRef {
        name: name.to_string(),
        source_hint,
        exported_name,
    });
}

fn choose_preferred_component_meta_registry_candidate(
    left: Option<verter_semantic::analysis::type_expr::TypeExpr>,
    right: Option<verter_semantic::analysis::type_expr::TypeExpr>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    match (left, right) {
        (Some(left), Some(right)) => {
            let left_non_object = component_meta_registry_has_non_object_top_level_surface(&left);
            let right_non_object = component_meta_registry_has_non_object_top_level_surface(&right);
            if left_non_object != right_non_object {
                return Some(if left_non_object { right } else { left });
            }

            if component_meta_registry_indexed_ref_penalty(&left)
                != component_meta_registry_indexed_ref_penalty(&right)
            {
                return Some(
                    if component_meta_registry_indexed_ref_penalty(&left)
                        < component_meta_registry_indexed_ref_penalty(&right)
                    {
                        left
                    } else {
                        right
                    },
                );
            }

            crate::resolver_core::choose_preferred_imported_type_body(Some(left), Some(right))
        }
        (left, right) => crate::resolver_core::choose_preferred_imported_type_body(left, right),
    }
}

fn component_meta_registry_has_non_object_top_level_surface(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> bool {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Parenthesized(inner) => {
            component_meta_registry_has_non_object_top_level_surface(inner)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
            types
                .iter()
                .any(component_meta_registry_has_non_object_top_level_surface)
                || types.iter().any(|ty| !matches!(ty, TypeExpr::Object(_)))
        }
        TypeExpr::Ref { .. }
        | TypeExpr::IndexedAccess { .. }
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. } => true,
        TypeExpr::Object(_) => false,
        _ => false,
    }
}

fn component_meta_registry_indexed_ref_penalty(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
) -> usize {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::IndexedAccess { object, index } => {
            let local_penalty = matches!(object.as_ref(), TypeExpr::Ref { .. }) as usize;
            local_penalty
                + component_meta_registry_indexed_ref_penalty(object)
                + component_meta_registry_indexed_ref_penalty(index)
        }
        TypeExpr::Union(types) | TypeExpr::Intersection(types) => types
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
        TypeExpr::Array { element, .. }
        | TypeExpr::Rest(element)
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element) => component_meta_registry_indexed_ref_penalty(element),
        TypeExpr::Tuple { elements, .. } => elements
            .iter()
            .map(|element| component_meta_registry_indexed_ref_penalty(&element.ty))
            .sum(),
        TypeExpr::Object(obj) => obj
            .properties
            .iter()
            .map(|member| match member {
                ObjectMember::Property(prop) => {
                    component_meta_registry_indexed_ref_penalty(&prop.ty)
                }
                ObjectMember::IndexSignature(sig) => {
                    component_meta_registry_indexed_ref_penalty(&sig.key_type)
                        + component_meta_registry_indexed_ref_penalty(&sig.value_type)
                }
                ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                    func.parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + func
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
                ObjectMember::Method(method) => {
                    method
                        .function
                        .parameters
                        .iter()
                        .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                        .sum::<usize>()
                        + method
                            .function
                            .return_type
                            .as_deref()
                            .map(component_meta_registry_indexed_ref_penalty)
                            .unwrap_or(0)
                }
            })
            .sum(),
        TypeExpr::Function(func) => {
            func.parameters
                .iter()
                .map(|param| component_meta_registry_indexed_ref_penalty(&param.ty))
                .sum::<usize>()
                + func
                    .return_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            component_meta_registry_indexed_ref_penalty(check)
                + component_meta_registry_indexed_ref_penalty(extends)
                + component_meta_registry_indexed_ref_penalty(true_type)
                + component_meta_registry_indexed_ref_penalty(false_type)
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            component_meta_registry_indexed_ref_penalty(source)
                + component_meta_registry_indexed_ref_penalty(value)
                + name_type
                    .as_deref()
                    .map(component_meta_registry_indexed_ref_penalty)
                    .unwrap_or(0)
        }
        TypeExpr::TemplateLiteral { expressions, .. } => expressions
            .iter()
            .map(component_meta_registry_indexed_ref_penalty)
            .sum(),
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::Ref { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => 0,
    }
}

fn collect_component_meta_registry_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_member_refs: bool,
) {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            for ty in types.iter() {
                collect_component_meta_registry_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        // Registry publication stays shallow: object/function member types remain
        // inline on the owning helper instead of spawning separate registry
        // entries for every nested support type. We still need to notice
        // direct member-surface helper refs such as `Button['variants']['color']`
        // or `LocalConfig<string>['slot']`, because compat display/schema output
        // depends on those helpers being present in the registry.
        TypeExpr::Object(obj) => {
            use verter_semantic::analysis::type_expr::ObjectMember;

            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_member_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        TypeExpr::Function(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            if !allow_plain_member_refs {
                return;
            }
            collect_component_meta_registry_refs(
                check,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                extends,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            if !allow_plain_member_refs {
                return;
            }
            collect_component_meta_registry_refs(
                source,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            collect_component_meta_registry_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_member_refs,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_component_meta_registry_refs(
                    name_type,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_member_refs,
                );
            }
        }
        TypeExpr::TypeParameter(_) => {}
        TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. } => {}
    }
}

fn collect_component_meta_registry_function_surface_refs(
    func: &verter_semantic::analysis::type_expr::FunctionExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    for param in &func.parameters {
        collect_component_meta_registry_member_surface_refs(
            &param.ty,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
    if let Some(return_type) = func.return_type.as_deref() {
        collect_component_meta_registry_member_surface_refs(
            return_type,
            published_names,
            queued_names,
            output,
            source_hint,
            false,
        );
    }
}

fn collect_component_meta_registry_public_field_refs(
    host: &VerterHost,
    owner_canonical: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    field: &verter_semantic::analysis::type_expand::ExpandedField,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    let parsed_raw = field
        .raw_type
        .as_deref()
        .map(verter_semantic::analysis::type_expr_lower::parse_type_annotation);
    let expr = parsed_raw.as_ref().unwrap_or(&field.r#type);

    collect_component_meta_registry_public_surface_refs(
        expr,
        published_names,
        queued_names,
        output,
        source_hint,
    );

    collect_component_meta_registry_public_indexed_access_roots(
        host,
        owner_canonical,
        store_view,
        expr,
        published_names,
        queued_names,
        output,
        source_hint,
    );
}

fn collect_component_meta_registry_public_surface_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    use verter_semantic::analysis::type_expr::TypeExpr;

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
            );
        }
        TypeExpr::Parenthesized(element) => {
            collect_component_meta_registry_public_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::IndexedAccess { .. }
        | TypeExpr::Array { .. }
        | TypeExpr::KeyOf(_)
        | TypeExpr::Rest(_)
        | TypeExpr::Tuple { .. }
        | TypeExpr::Union(_)
        | TypeExpr::Intersection(_)
        | TypeExpr::Conditional { .. }
        | TypeExpr::Mapped { .. }
        | TypeExpr::TemplateLiteral { .. } => {}
        TypeExpr::Object(_)
        | TypeExpr::Function(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::TypeParameter(_)
        | TypeExpr::Infer { .. } => {}
    }
}

fn collect_component_meta_registry_public_indexed_access_roots(
    host: &VerterHost,
    owner_canonical: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
) {
    use verter_semantic::analysis::type_eval::TypeDeclKind;
    use verter_semantic::analysis::type_expr::TypeExpr;

    let TypeExpr::IndexedAccess { object, .. } = expr else {
        return;
    };
    let TypeExpr::Ref {
        name,
        type_arguments,
    } = object.as_ref()
    else {
        return;
    };
    if !type_arguments.is_empty() {
        return;
    }
    let Some(prepared) =
        host.prepared_type_decl_in_view(owner_canonical, name.as_ref(), store_view)
    else {
        return;
    };
    if !matches!(prepared.kind, TypeDeclKind::Interface | TypeDeclKind::Class) {
        return;
    }
    enqueue_component_meta_registry_ref(
        published_names,
        queued_names,
        output,
        name.as_ref(),
        source_hint,
        None,
    );
}

fn collect_component_meta_registry_member_surface_refs(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    published_names: &rustc_hash::FxHashSet<String>,
    queued_names: &mut rustc_hash::FxHashSet<String>,
    output: &mut VecDeque<PendingComponentMetaRegistryRef>,
    source_hint: Option<&str>,
    allow_plain_refs: bool,
) {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match expr {
        TypeExpr::Ref {
            name,
            type_arguments: _,
        } if allow_plain_refs => {
            enqueue_component_meta_registry_ref(
                published_names,
                queued_names,
                output,
                name.as_ref(),
                source_hint,
                None,
            );
        }
        TypeExpr::IndexedAccess { object, index } => {
            collect_component_meta_registry_member_surface_refs(
                object,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
            collect_component_meta_registry_member_surface_refs(
                index,
                published_names,
                queued_names,
                output,
                source_hint,
                true,
            );
        }
        TypeExpr::Array { element, .. }
        | TypeExpr::Parenthesized(element)
        | TypeExpr::KeyOf(element)
        | TypeExpr::Rest(element) => {
            collect_component_meta_registry_member_surface_refs(
                element,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Tuple { elements, .. } => {
            for element in elements.iter() {
                collect_component_meta_registry_member_surface_refs(
                    &element.ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Union(types)
        | TypeExpr::Intersection(types)
        | TypeExpr::TemplateLiteral {
            expressions: types, ..
        } => {
            for ty in types.iter() {
                collect_component_meta_registry_member_surface_refs(
                    ty,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => {
            collect_component_meta_registry_member_surface_refs(
                check,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                extends,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                true_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                false_type,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
        }
        TypeExpr::Mapped {
            source,
            value,
            name_type,
            ..
        } => {
            collect_component_meta_registry_member_surface_refs(
                source,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            collect_component_meta_registry_member_surface_refs(
                value,
                published_names,
                queued_names,
                output,
                source_hint,
                allow_plain_refs,
            );
            if let Some(name_type) = name_type.as_deref() {
                collect_component_meta_registry_member_surface_refs(
                    name_type,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                    allow_plain_refs,
                );
            }
        }
        TypeExpr::Function(func) => {
            collect_component_meta_registry_function_surface_refs(
                func,
                published_names,
                queued_names,
                output,
                source_hint,
            );
        }
        TypeExpr::Object(obj) => {
            for member in &obj.properties {
                match member {
                    ObjectMember::Property(prop) => {
                        collect_component_meta_registry_member_surface_refs(
                            &prop.ty,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::IndexSignature(sig) => {
                        collect_component_meta_registry_member_surface_refs(
                            &sig.key_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                        collect_component_meta_registry_member_surface_refs(
                            &sig.value_type,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                            allow_plain_refs,
                        );
                    }
                    ObjectMember::CallSignature(func) | ObjectMember::ConstructSignature(func) => {
                        collect_component_meta_registry_function_surface_refs(
                            func,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                    ObjectMember::Method(method) => {
                        collect_component_meta_registry_function_surface_refs(
                            &method.function,
                            published_names,
                            queued_names,
                            output,
                            source_hint,
                        );
                    }
                }
            }
        }
        TypeExpr::TypeParameter(_)
        | TypeExpr::Primitive(_)
        | TypeExpr::Literal(_)
        | TypeExpr::Unknown { .. }
        | TypeExpr::RecursiveRef { .. }
        | TypeExpr::TypeOf(_)
        | TypeExpr::Infer { .. }
        | TypeExpr::Ref { .. } => {}
    }
}

fn resolved_meta_cache_key(
    canonical: &str,
    mode: ResolverMode,
) -> crate::resolver_core::ResolutionNodeKey {
    crate::resolver_core::ResolutionNodeKey {
        symbol_id: canonical.to_string(),
        node_kind: crate::resolver_core::ResolutionNodeKind::Assemble,
        traversal_lens: crate::resolver_core::TraversalLens::StructuralObject,
        member_path_hash: 0,
        type_args_hash: 0,
        behavior_flags: match mode {
            ResolverMode::Type => 1,
            ResolverMode::Expanded => 2,
        },
    }
}

struct HostComponentMetaResolver<'a> {
    host: &'a VerterHost,
    store_view: Option<&'a crate::resolver_store::HostStoreView>,
}

impl crate::resolver_core::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        self.host
            .resolve_named_type_export_target_in_view(
                dep_canonical,
                requested_name,
                self.store_view,
            )
            .map(
                |(canonical, name)| crate::resolver_core::ResolvedExportTarget {
                    source_canonical_id: (canonical != dep_canonical).then_some(canonical),
                    source_name: name,
                },
            )
    }

    fn get_export_span_follow_reexports(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<verter_span::Span> {
        self.host
            .get_export_span_follow_reexports_in_view(
                dep_canonical,
                requested_name,
                self.store_view,
            )
            .map(|(_, start, end)| verter_span::Span::new(start, end))
    }

    fn read_source(&self, canonical_source: &str) -> Option<String> {
        read_full_source(self.host, canonical_source, self.store_view)
    }

    fn type_declaration_id(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<verter_semantic::analysis::type_eval::DeclarationId> {
        self.host.local_type_declaration_id_in_view(
            canonical_source,
            resolved_name,
            self.store_view,
        )
    }

    fn resolve_type_dependency_canonical(
        &self,
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host.resolve_type_dependency_canonical_in_view(
            from_canonical,
            import_source,
            self.store_view,
        )
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host.resolve_direct_type_reexport_target_in_view(
            dep_canonical,
            requested_name,
            self.store_view,
        )
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        self.host.resolve_local_import_symbol_target_in_view(
            dep_canonical,
            resolved_name,
            self.store_view,
        )
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host.resolve_local_export_symbol_target_in_view(
            canonical_source,
            exported_name,
            self.store_view,
        )
    }

    fn resolve_local_type_symbol_metadata(
        &self,
        canonical_source: &str,
        resolved_name: &str,
    ) -> Option<crate::resolver_core::ResolvedLocalTypeSymbolMetadata> {
        let analysis = self
            .host
            .external_type_analysis_in_view(canonical_source, self.store_view)?;
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

impl crate::resolver_core::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        resolve_type_declaration_in_view(self.host, dep_canonical, requested_name, self.store_view)
    }

    fn snapshot_imports<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedImport] {
        snapshot.imports.as_slice()
    }

    fn snapshot_macros<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::AnalyzedMacro] {
        snapshot.macros.as_slice()
    }

    fn snapshot_macro_type_deps<'a>(
        &self,
        snapshot: &'a Self::Snapshot,
    ) -> &'a [verter_semantic::analysis::types::MacroTypeDep] {
        snapshot.macro_type_deps.as_slice()
    }

    fn build_eval_outputs(
        &self,
        owner_canonical: &str,
        snapshot: &Self::Snapshot,
        eval_context: Option<&Self::EvalContext>,
    ) -> ComponentMetaEvalOutputs {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ResolverMode::Expanded,
                snapshot.imports.len(),
                snapshot.macro_type_deps.len(),
            ));
        }
        let dep_resolutions = eval_context
            .map(|captured| captured.dep_resolutions.clone())
            .unwrap_or_else(|| {
                self.host
                    .dependency_resolutions_for_eval_in_view(owner_canonical, self.store_view)
                    .unwrap_or_default()
            });
        // Tracked dependencies: snapshot-level candidates + solver-discovered deps.
        // The legacy walker is no longer used for dependency tracking.
        let mut tracked_dependencies = std::collections::BTreeSet::new();
        tracked_dependencies.extend(self.host.cache_dependency_candidates_from_snapshot(
            owner_canonical,
            snapshot,
            &dep_resolutions,
        ));
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // Always run the solver-host macro path. The solver resolves cross-file
        // types on demand from the host's prepared-decl cache.
        let computed_eval_types = self
            .host
            .compute_evaluated_types_with_tracking_from_owner_context_in_view(
                owner_canonical,
                snapshot,
                eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                &dep_resolutions,
                self.store_view,
            );
        if let Some(compute_eval_start) = compute_eval_start {
            let elapsed = compute_eval_start.elapsed();
            component_meta_debug(format!(
                "EVAL_TYPES owner={} elapsed_ms={:.1} has_result={}",
                owner_canonical,
                elapsed.as_secs_f64() * 1000.0,
                computed_eval_types.is_some(),
            ));
        }
        if let Some(computed) = computed_eval_types.as_ref() {
            tracked_dependencies.extend(computed.discovered_dependencies.iter().cloned());
        }
        let evaluated_types = computed_eval_types.and_then(|computed| computed.evaluated_types);
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ResolverMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            tracked_dependencies,
        }
    }

    fn resolve_macro_elements(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<verter_compiler::utils::oxc::vue::resolve_type::ResolvedElements> {
        self.host
            .resolve_external_type_from_loaded_files_in_view(
                owner_canonical,
                import_source,
                exported_name,
                tracked_deps,
                resolution_deps,
                cache,
                visiting,
                false,
                verter_workspace::ResolveRequestKind::TypeImport,
                true,
                None,
                0,
                self.store_view,
            )
            .ok()
            .flatten()
    }

    fn resolve_jsdoc_block(
        &self,
        canonical_source: &str,
        span: verter_span::Span,
        expanded: bool,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<ResolvedJsdocBlock> {
        resolve_jsdoc_block(
            self.host,
            canonical_source,
            span,
            if expanded {
                ResolverMode::Expanded
            } else {
                ResolverMode::Type
            },
            tracked_deps,
            cache,
            visiting,
            verter_workspace::ResolveRequestKind::TypeImport,
            self.store_view,
        )
    }

    fn sync_transitive_macro_type_dependencies(
        &self,
        canonical_id: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) {
        self.host
            .sync_transitive_macro_type_dependencies(canonical_id, tracked_deps);
    }

    fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        self.host
            .current_dependency_fact_versions_in_view(canonical, tracked_deps, self.store_view)
    }
}

pub(crate) fn resolve_type_declaration_in_view(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> ResolvedTypeDeclaration {
    let owned_view;
    let current_view = if let Some(view) = store_view {
        view
    } else {
        owned_view = host.resolver_store_view();
        &owned_view
    };
    let resolver = HostComponentMetaResolver {
        host,
        store_view: Some(current_view),
    };
    let key =
        crate::resolver_core::symbol_resolver::declaration_node_key(dep_canonical, requested_name);
    let mut ctx = crate::resolver_core::symbol_resolver::ResolveContext::new();
    let result = host
        .resolver_runtime()
        .symbol
        .resolve_node(key, current_view, &mut ctx, |_| {
            let declaration = crate::resolver_core::resolve_type_declaration(
                &resolver,
                dep_canonical,
                requested_name,
            );
            let mut tracked_deps = std::collections::BTreeSet::new();
            if !declaration.canonical_source.is_empty()
                && declaration.canonical_source != dep_canonical
            {
                tracked_deps.insert(declaration.canonical_source.clone());
            }

            crate::resolver_core::symbol_resolver::SymbolNodeResult {
                value: crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(
                    declaration,
                ),
                facts: host.current_dependency_fact_versions_in_view(
                    dep_canonical,
                    &tracked_deps,
                    Some(current_view),
                ),
                diagnostics: Vec::new(),
            }
        });

    match result.value {
        crate::resolver_core::symbol_resolver::SymbolNodeValue::Declaration(declaration) => {
            declaration
        }
        _ => unreachable!("declaration resolution must return a declaration node result"),
    }
}

fn read_full_source(
    host: &VerterHost,
    canonical_source: &str,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<String> {
    host.read_analysis_source_in_view(canonical_source, store_view)
        .map(|source| source.to_string())
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_workspace::ResolveRequestKind,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source, store_view)?;
    let (description, tags) =
        verter_semantic::analysis::jsdoc::extract_jsdoc_near_offset(&source, span.start);
    if description.is_none() && tags.is_empty() {
        return None;
    }

    Some(ResolvedJsdocBlock {
        description,
        tags: tags
            .into_iter()
            .map(|tag| {
                map_jsdoc_tag(
                    host,
                    canonical_source,
                    mode,
                    tracked_deps,
                    cache,
                    visiting,
                    kind,
                    store_view,
                    tag,
                )
            })
            .collect(),
    })
}

#[allow(clippy::too_many_arguments)]
fn map_jsdoc_tag(
    host: &VerterHost,
    canonical_source: &str,
    mode: ResolverMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    _kind: verter_workspace::ResolveRequestKind,
    store_view: Option<&crate::resolver_store::HostStoreView>,
    tag: verter_semantic::analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ResolverMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(host, canonical_source, raw_type, tracked_deps, store_view)
        })
    } else {
        None
    };
    ResolvedJsdocTag {
        name: tag.name,
        text,
        raw_type,
        subject_name,
        resolved_type,
    }
}

fn parse_jsdoc_tag_payload(
    tag_name: &str,
    text: Option<String>,
) -> (Option<String>, Option<String>, Option<String>) {
    let Some(text) = text else {
        return (None, None, None);
    };
    let trimmed = text.trim();
    let Some(rest) = trimmed.strip_prefix('{') else {
        return (Some(text), None, None);
    };
    // Depth-aware brace matching: find the closing `}` that matches the
    // opening `{`, handling nested braces like `{Record<string, {nested: true}>}`.
    let end = {
        let mut depth = 0u32;
        let mut found = None;
        for (i, ch) in rest.char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    if depth == 0 {
                        found = Some(i);
                        break;
                    }
                    depth = depth.saturating_sub(1);
                }
                _ => {}
            }
        }
        found
    };
    let Some(end) = end else {
        return (Some(text), None, None);
    };

    let raw_type = Some(rest[..end].trim().to_string());
    let trailing = rest[end + 1..].trim();
    if trailing.is_empty() {
        return (None, raw_type, None);
    }

    if matches!(tag_name, "param" | "arg" | "argument") {
        let mut parts = trailing.splitn(2, char::is_whitespace);
        let subject_name = parts.next().map(str::to_string);
        let text = parts
            .next()
            .map(str::trim)
            .filter(|rest| !rest.is_empty())
            .map(str::to_string);
        (text, raw_type, subject_name)
    } else {
        (Some(trailing.to_string()), raw_type, None)
    }
}

fn resolve_jsdoc_tag_type(
    host: &VerterHost,
    canonical_source: &str,
    raw_type: &str,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    store_view: Option<&crate::resolver_store::HostStoreView>,
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_type);
    let parsed = if parsed.is_unknown() {
        verter_semantic::analysis::type_expr::TypeExpr::Unknown {
            raw: raw_type.to_string(),
        }
    } else {
        parsed
    };

    let _snapshot = host.get_raw_analysis_snapshot_in_view(canonical_source, store_view)?;
    tracked_deps.extend(
        host.imported_symbol_dependencies_for_expr_in_view(canonical_source, &parsed, store_view)
            .into_iter()
            .map(|dependency| dependency.canonical_id),
    );
    let solver_host = crate::resolver_core::SessionSolverHost::with_declaration_scope(
        host,
        store_view,
        canonical_source,
    );
    let resolved = verter_semantic::analysis::type_solver::solve::solve_type(&parsed, &solver_host);
    Some(resolved.value)
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
