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

fn next_component_meta_audit_request_id() -> u64 {
    static NEXT_REQUEST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT_REQUEST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

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

fn request_source_performed_compute(source: RequestSource) -> bool {
    matches!(
        source,
        RequestSource::Flight {
            role: SingleflightRole::Leader,
            ..
        } | RequestSource::Fallback
    )
}

#[derive(Debug, Clone)]
pub struct CapturedComponentMetaInputs {
    whole_hash: Hash16,
    snapshot: FileAnalysisSnapshot,
    owner_eval_source: Option<String>,
    direct_dependency_candidates: std::collections::BTreeSet<String>,
    audit_capture_inputs_ms: f64,
    audit_store_read_ms: f64,
    audit_direct_import_proof_ms: f64,
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
        let audit_enabled = self.config.audit_enabled;
        let capture_started = audit_enabled.then(Instant::now);
        let store_read_started = audit_enabled.then(Instant::now);
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
        let facts = self.ensure_module_facts_in_view(canonical, Some(view))?;
        let whole_hash = facts.whole_hash;
        let store_read_ms = store_read_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_event!(
            "capture_component_meta_eval_state",
            format!(
                "owner={} source_len={} has_cached_parse={} whole_hash={whole_hash:?}",
                canonical,
                facts.raw_source.len(),
                facts.cached_parse.is_some(),
            ),
        );
        let owner_eval_source =
            VerterHost::build_eval_script_source(&facts.raw_source, facts.cached_parse.as_deref());
        let direct_import_started = audit_enabled.then(Instant::now);
        let direct_dependency_candidates =
            self.cache_dependency_candidates_from_snapshot(canonical, &snapshot, Some(view));
        let direct_import_proof_ms = direct_import_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        let capture_inputs_ms = capture_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or(0.0);
        component_meta_trace_event!(
            "capture_component_meta_inputs_result",
            format!(
                "owner={} owner_eval_source_len={} dependency_candidates={}",
                canonical,
                owner_eval_source.len(),
                direct_dependency_candidates.len(),
            ),
        );
        Some(CapturedComponentMetaInputs {
            whole_hash,
            snapshot,
            owner_eval_source: Some(owner_eval_source),
            direct_dependency_candidates,
            audit_capture_inputs_ms: capture_inputs_ms,
            audit_store_read_ms: store_read_ms,
            audit_direct_import_proof_ms: direct_import_proof_ms,
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
pub struct ResolvedComponentMetaComputeAudit {
    pub timings: crate::component_meta_audit::RustTimingAudit,
    pub solver: crate::component_meta_audit::RustSolverAudit,
}

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
    /// Non-semantic compute audit captured only when native audit is enabled.
    pub compute_audit: Option<ResolvedComponentMetaComputeAudit>,
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

fn projected_macro_shape_entry(
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    owner_canonical: &str,
    lowered: &verter_semantic::analysis::type_expr::TypeExpr,
) -> Option<
    verter_semantic::analysis::type_expand::ExpansionResult<
        verter_semantic::analysis::type_expand::ExpandedObjectShape,
    >,
> {
    let projected = query_engine.project_expr_surface_expr(owner_canonical, lowered)?;
    let shape = verter_semantic::analysis::type_expand::type_expr_to_object_shape(&projected);
    if shape.properties.is_empty()
        && shape.index_signatures.is_empty()
        && shape.call_signatures.is_empty()
    {
        return None;
    }
    Some(verter_semantic::analysis::type_expand::ExpansionResult::exact_symbolic(shape))
}

fn macro_shape_surface_weight(
    result: &verter_semantic::analysis::type_expand::ExpansionResult<
        verter_semantic::analysis::type_expand::ExpandedObjectShape,
    >,
) -> usize {
    result.value.properties.len()
        + result.value.index_signatures.len()
        + result.value.call_signatures.len()
}

fn projected_macro_shape_is_preferred(
    existing: Option<
        &verter_semantic::analysis::type_expand::ExpansionResult<
            verter_semantic::analysis::type_expand::ExpandedObjectShape,
        >,
    >,
    projected: &verter_semantic::analysis::type_expand::ExpansionResult<
        verter_semantic::analysis::type_expand::ExpandedObjectShape,
    >,
) -> bool {
    let Some(existing) = existing else {
        return true;
    };
    macro_shape_surface_weight(projected) > macro_shape_surface_weight(existing)
}

fn upsert_projected_define_props_shape(
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    macro_index: usize,
    projected: verter_semantic::analysis::type_expand::ExpansionResult<
        verter_semantic::analysis::type_expand::ExpandedObjectShape,
    >,
) {
    let existing = evaluated_types
        .define_props
        .iter()
        .find(|entry| entry.macro_index == macro_index)
        .map(|entry| &entry.result);
    if !projected_macro_shape_is_preferred(existing, &projected) {
        return;
    }
    if let Some(entry) = evaluated_types
        .define_props
        .iter_mut()
        .find(|entry| entry.macro_index == macro_index)
    {
        entry.result = projected;
    } else {
        evaluated_types.define_props.push(
            verter_semantic::analysis::type_expand::ExpandedMacroProps {
                macro_index,
                result: projected,
            },
        );
    }
}

fn upsert_projected_macro_object_shape(
    entries: &mut Vec<verter_semantic::analysis::type_expand::ExpandedMacroObjectShape>,
    macro_index: usize,
    projected: verter_semantic::analysis::type_expand::ExpansionResult<
        verter_semantic::analysis::type_expand::ExpandedObjectShape,
    >,
) {
    let existing = entries
        .iter()
        .find(|entry| entry.macro_index == macro_index)
        .map(|entry| &entry.result);
    if !projected_macro_shape_is_preferred(existing, &projected) {
        return;
    }
    if let Some(entry) = entries
        .iter_mut()
        .find(|entry| entry.macro_index == macro_index)
    {
        entry.result = projected;
    } else {
        entries.push(
            verter_semantic::analysis::type_expand::ExpandedMacroObjectShape {
                macro_index,
                result: projected,
            },
        );
    }
}

fn enrich_projected_macro_shapes(
    owner_canonical: &str,
    snapshot: &FileAnalysisSnapshot,
    eval_source: &str,
    evaluated_types: &mut verter_semantic::analysis::type_expand::ExpandedComponentTypes,
    query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) {
    let params =
        verter_semantic::analysis::type_eval_build::collect_define_macro_type_params(eval_source);
    let mut define_props_index = 0usize;
    let mut define_emits_index = 0usize;
    let mut define_slots_index = 0usize;

    for (macro_index, mac) in snapshot.macros.iter().enumerate() {
        if !mac.is_type_based {
            continue;
        }

        match mac.kind {
            verter_semantic::analysis::AnalyzedMacroKind::DefineProps => {
                if let Some(lowered) = params.define_props.get(define_props_index) {
                    if let Some(projected) =
                        projected_macro_shape_entry(query_engine, owner_canonical, lowered)
                    {
                        upsert_projected_define_props_shape(
                            evaluated_types,
                            macro_index,
                            projected,
                        );
                    }
                }
                define_props_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                if let Some(lowered) = params.define_emits.get(define_emits_index) {
                    if let Some(projected) =
                        projected_macro_shape_entry(query_engine, owner_canonical, lowered)
                    {
                        upsert_projected_macro_object_shape(
                            &mut evaluated_types.define_emits,
                            macro_index,
                            projected,
                        );
                    }
                }
                define_emits_index += 1;
            }
            verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                if let Some(lowered) = params.define_slots.get(define_slots_index) {
                    if let Some(projected) =
                        projected_macro_shape_entry(query_engine, owner_canonical, lowered)
                    {
                        upsert_projected_macro_object_shape(
                            &mut evaluated_types.define_slots,
                            macro_index,
                            projected,
                        );
                    }
                }
                define_slots_index += 1;
            }
            _ => {}
        }
    }
}

impl VerterHost {
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
        let audit = self.config.audit_enabled.then(|| {
            let request_id = next_component_meta_audit_request_id();
            let (host_cache_before_bytes, workspace_before_bytes) =
                self.component_meta_audit_memory_bytes();
            (
                request_id,
                crate::component_meta_audit::begin_request_audit(request_id),
                crate::component_meta_audit::AuditBuilder::new(request_id, canonical.clone()),
                host_cache_before_bytes,
                workspace_before_bytes,
            )
        });
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

        if let Some((
            _request_id,
            request_audit_guard,
            mut audit_builder,
            host_cache_before_bytes,
            workspace_before_bytes,
        )) = audit
        {
            audit_builder.record_store(self.component_meta_audit_store_snapshot(store_view));
            let (host_cache_after_bytes, workspace_after_bytes) =
                self.component_meta_audit_memory_bytes();
            audit_builder.record_memory_snapshots(
                host_cache_before_bytes,
                host_cache_after_bytes,
                workspace_before_bytes,
                workspace_after_bytes,
            );
            if request_source_performed_compute(result.source) {
                if let Some(compute_audit) = result
                    .value
                    .as_ref()
                    .and_then(|resolved| resolved.compute_audit.as_ref())
                {
                    let mut timings = compute_audit.timings.clone();
                    timings.imported_root_proof_ms =
                        request_audit_guard.snapshot().imported_root_proof_ms;
                    audit_builder.record_timings(timings);
                    audit_builder.record_solver(compute_audit.solver.clone());
                }
            }
            crate::component_meta_audit::emit_audit_trace(&audit_builder.finish());
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
        let audit_enabled = self.config.audit_enabled;
        let mut audit_timings = if audit_enabled {
            captured
                .map(|captured| crate::component_meta_audit::RustTimingAudit {
                    capture_inputs_ms: captured.audit_capture_inputs_ms,
                    store_read_ms: captured.audit_store_read_ms,
                    direct_import_proof_ms: captured.audit_direct_import_proof_ms,
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            crate::component_meta_audit::RustTimingAudit::default()
        };
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
        let owner_solver_host =
            crate::resolver_core::solver_host::SessionSolverHost::with_declaration_scope(
                self, store_view, canonical,
            );
        let mut resolver_host = HostComponentMetaResolver {
            host: self,
            store_view,
            shared_owner_engine: Some(std::cell::RefCell::new(
                verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine::new(
                    &owner_solver_host,
                ),
            )),
        };
        let parts_started = audit_enabled.then(Instant::now);
        let parts = crate::resolver_core::resolve_component_meta_parts(
            &resolver_host,
            canonical,
            &snapshot,
            mode == ResolverMode::Expanded,
            captured,
        );
        if let Some(started) = parts_started {
            audit_timings.solver_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        let mut parts = parts;
        if let Some(evaluated_types) = parts.evaluated_types.as_mut() {
            enrich_missing_slot_bindings(&parts.resolved_macros, evaluated_types);
        }
        let registry_before = parts.resolved_type_registry.len();
        let append_start = std::time::Instant::now();
        let owner_engine = resolver_host
            .shared_owner_engine
            .take()
            .map(std::cell::RefCell::into_inner)
            .unwrap_or_else(|| {
                verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine::new(
                    &owner_solver_host,
                )
            });
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::from_owner_engine(
            self,
            store_view,
            owner_engine,
        );
        self.append_component_meta_registry_entries(
            canonical,
            &snapshot,
            parts.evaluated_types.as_ref(),
            &mut parts.resolved_type_registry,
            &mut parts.resolved_type_registry_meta,
            &mut parts.tracked_dependencies,
            store_view,
            &mut query_engine,
        );
        if mode == ResolverMode::Expanded {
            if let Some(eval_source) = captured
                .and_then(|captured| captured.owner_eval_source.as_deref())
                .map(str::to_string)
                .or_else(|| {
                    self.ensure_module_facts_in_view(canonical, store_view)
                        .map(|facts| {
                            VerterHost::build_eval_script_source(
                                &facts.raw_source,
                                facts.cached_parse.as_deref(),
                            )
                        })
                })
            {
                let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
                enrich_projected_macro_shapes(
                    canonical,
                    &snapshot,
                    &eval_source,
                    &mut evaluated_types,
                    &mut query_engine,
                );
                if !evaluated_types.is_empty() {
                    enrich_missing_slot_bindings(&parts.resolved_macros, &mut evaluated_types);
                    parts.evaluated_types = Some(evaluated_types);
                }
            }
        }
        audit_timings.materialize_ms = append_start.elapsed().as_secs_f64() * 1000.0;
        {
            let ts = query_engine.trace_summary();
            crate::host_manage::component_meta_trace_event!(
                "solver_trace_summary",
                format!(
                    "owner={} steps={} solves={} refs={} host_lookups={} indexed_access={} unions={} intersections={} objects={} conditionals={} mapped={} inst_cache_hits={} inst_cache_misses={} proj_cache_hits={} arena_high_water={} scoped_cache={}",
                    canonical,
                    query_engine.total_steps(),
                    query_engine.solve_count(),
                    ts.resolve_ref_count,
                    ts.resolve_ref_host_lookups,
                    ts.resolve_indexed_access_count,
                    ts.resolve_union_count,
                    ts.resolve_intersection_count,
                    ts.resolve_object_count,
                    ts.resolve_conditional_count,
                    ts.resolve_mapped_count,
                    ts.instantiation_cache_hits,
                    ts.instantiation_cache_misses,
                    ts.projection_cache_hits,
                    ts.arena_high_water,
                    query_engine.scoped_cache_len(),
                ),
            );
        }
        if query_engine.has_fuse_tripped() {
            for trip in query_engine.fuse_trips() {
                crate::host_manage::component_meta_trace_event!(
                    "fuse_tripped",
                    format!(
                        "owner={} fuse={} budget={} actual={}",
                        canonical, trip.fuse_name, trip.budget, trip.actual,
                    ),
                );
            }
        }
        let solver_audit = crate::component_meta_audit::RustSolverAudit {
            total_resolve_steps: query_engine.total_steps(),
            solve_count: query_engine.solve_count(),
        };
        let store_merge_started = audit_enabled.then(Instant::now);
        let final_store_view = self.resolver_store_view();
        parts.fact_versions = self.current_dependency_fact_versions_in_view(
            canonical,
            &parts.tracked_dependencies,
            Some(&final_store_view),
        );
        if let Some(started) = store_merge_started {
            audit_timings.store_merge_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        if audit_enabled {
            audit_timings.imported_root_proof_ms =
                crate::component_meta_audit::current_request_audit_snapshot()
                    .imported_root_proof_ms;
        }
        let append_elapsed = append_start.elapsed();
        let registry_after = parts.resolved_type_registry.len();
        if crate::host_manage::component_meta_debug_enabled() {
            let dep_cache_size = self.resolver.runtime.module_facts.len();
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
            compute_audit: audit_enabled.then_some(ResolvedComponentMetaComputeAudit {
                timings: audit_timings,
                solver: solver_audit,
            }),
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
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
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
        fn materialize_component_meta_registry_candidate(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            query_engine
                .project_type_surface_expr(scope_canonical_id, symbol_name)
                .map(|materialized| {
                    raw_body.map_or_else(
                        || materialized.clone(),
                        |raw| {
                            preserve_package_backed_symbolic_refs(
                                &materialized,
                                raw,
                                scope_canonical_id,
                                query_engine,
                            )
                        },
                    )
                })
                .or_else(|| {
                    raw_body.and_then(|expr| {
                        (!component_meta_registry_has_non_object_top_level_surface(expr))
                            .then(|| expr.clone())
                    })
                })
                .or_else(|| raw_body.cloned())
        }
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
            let Some(resolved) = query_engine
                .resolve_imported_registry_symbol(declaration_source, requested_exported_name)
            else {
                continue;
            };
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                resolved.canonical_id.as_str(),
            );
            for dependency in &resolved.canonical_dependencies {
                track_component_meta_dependency(
                    tracked_dependencies,
                    owner_canonical,
                    dependency.as_str(),
                );
            }
            meta.declaration.canonical_source = resolved.canonical_id.clone();
            let materialized = materialize_component_meta_registry_candidate(
                query_engine,
                resolved.canonical_id.as_str(),
                resolved.exported_name.as_str(),
                Some(&resolved.body),
            )
            .unwrap_or_else(|| resolved.body.clone());
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
            let source_expr =
                query_engine.owner_collection_expr(owner_canonical, entry.name.as_str());
            collect_component_meta_registry_refs(
                source_expr.as_ref().unwrap_or(&entry.type_expr),
                &published_names,
                &mut queued_names,
                &mut referenced_names,
                source_hint,
                false,
            );
        }

        let mut _loop_iterations: usize = 0;
        let mut _loop_materializations: usize = 0;
        let _loop_start = std::time::Instant::now();
        while let Some(pending) = referenced_names.pop_front() {
            _loop_iterations += 1;
            if !query_engine.allow_registry_deepening() {
                break;
            }
            let PendingComponentMetaRegistryRef {
                name: type_name,
                source_hint: pending_source_hint_owned,
                exported_name: pending_exported_name_owned,
                route: pending_route,
            } = pending;
            let imported_owner_route =
                owner_component_meta_registry_import_binding(snapshot, type_name.as_str()).filter(
                    |_| {
                        pending_source_hint_owned
                            .as_deref()
                            .is_none_or(|source| source.is_empty() || source == owner_canonical)
                    },
                );
            let pending_source_hint = imported_owner_route
                .as_ref()
                .map(|(canonical_id, _)| canonical_id.as_str())
                .or(pending_source_hint_owned.as_deref());
            let pending_exported_name = imported_owner_route
                .as_ref()
                .map(|(_, exported_name)| exported_name.as_str())
                .or(pending_exported_name_owned.as_deref());
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING owner={} name={} source_hint={:?} exported={:?} route={:?}",
                    owner_canonical,
                    type_name,
                    pending_source_hint,
                    pending_exported_name,
                    pending_route,
                ));
            }
            if published_names.contains(&type_name) {
                continue;
            }
            if !query_engine.can_resolve_registry_symbol(
                owner_canonical,
                pending_exported_name.unwrap_or(type_name.as_str()),
                pending_source_hint,
            ) {
                continue;
            }
            let requested_exported_name = pending_exported_name.unwrap_or(type_name.as_str());
            if let Some(source_hint) = pending_source_hint
                .filter(|source| !source.is_empty() && *source != owner_canonical)
            {
                if !query_engine.allow_imported_root() {
                    continue;
                }
                track_component_meta_dependency(tracked_dependencies, owner_canonical, source_hint);
                if let Some(resolved) = query_engine
                    .resolve_imported_registry_symbol(source_hint, requested_exported_name)
                {
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        resolved.canonical_id.as_str(),
                    );
                    for dependency in &resolved.canonical_dependencies {
                        track_component_meta_dependency(
                            tracked_dependencies,
                            owner_canonical,
                            dependency.as_str(),
                        );
                    }
                    let mut declaration = query_engine.resolve_type_declaration(
                        resolved.canonical_id.as_str(),
                        resolved.exported_name.as_str(),
                    );
                    if declaration.canonical_source.is_empty() {
                        declaration.canonical_source = resolved.canonical_id.clone();
                    }
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        declaration.canonical_source.as_str(),
                    );
                    let type_expr = materialize_component_meta_registry_candidate(
                        query_engine,
                        resolved.canonical_id.as_str(),
                        resolved.exported_name.as_str(),
                        Some(&resolved.body),
                    )
                    .unwrap_or_else(|| resolved.body.clone());
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
            let mut declaration =
                query_engine.resolve_type_declaration(declaration_owner, type_name.as_str());
            if declaration.canonical_source.is_empty() && declaration_owner != owner_canonical {
                declaration =
                    query_engine.resolve_type_declaration(owner_canonical, type_name.as_str());
            }
            let declaration_body =
                query_engine.named_decl_body(declaration_owner, type_name.as_str());
            let mut materialized = if declaration_owner != owner_canonical {
                materialize_component_meta_registry_candidate(
                    query_engine,
                    declaration_owner,
                    type_name.as_str(),
                    declaration_body.as_ref(),
                )
            } else {
                None
            };
            let owner_collection_expr =
                query_engine.owner_collection_expr(owner_canonical, type_name.as_str());
            materialized = materialized.or_else(|| {
                materialize_component_meta_registry_candidate(
                    query_engine,
                    owner_canonical,
                    type_name.as_str(),
                    owner_collection_expr.as_ref(),
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

        // Registry enrichment: materialize imported type expressions through
        // the shared request-scoped engine so projection/instantiation caches
        // are reused across all registry entries in one request.
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let scope_canonical = if !meta.declaration.canonical_source.is_empty() {
                meta.declaration.canonical_source.as_str()
            } else {
                owner_canonical
            };
            if scope_canonical == owner_canonical {
                continue;
            }
            if !meta.declaration.resolved_name.is_empty()
                && component_meta_registry_expr_references_name(
                    &entry.type_expr,
                    meta.declaration.resolved_name.as_str(),
                )
            {
                continue;
            }
            entry.type_expr = materialize_component_meta_member_surface_expr(
                &entry.type_expr,
                scope_canonical,
                query_engine,
                false,
            );
        }
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// For owner files in the scheduler, reads the scheduler's latest analysis
    /// (which reflects post-recompile state). For imported deps and non-scheduler
    /// files, reads from ModuleFactsDb (materializing on miss). Both paths enrich
    /// the snapshot with resolved imports, destructured bindings, and template
    /// analysis.
    pub(crate) fn get_raw_analysis_snapshot_in_view(
        &self,
        canonical: &str,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<FileAnalysisSnapshot> {
        let _trace = component_meta_trace_scope!(
            "get_raw_analysis_snapshot",
            format!("owner={} store_view={}", canonical, store_view.is_some()),
        );
        let normalized_canonical =
            self.normalized_analysis_canonical_in_view(canonical, store_view);
        let canonical = normalized_canonical.as_ref();

        #[cfg(feature = "scheduler")]
        {
            if let Some(cc) = self.compile_cache.get(canonical) {
                if cc.evicted {
                    return None;
                }
            }

            // Scheduler-first path for owner files: the scheduler has the
            // latest analysis after recompile, including updated import
            // routes for newly-added dependencies. ModuleFactsDb may hold
            // stale import routes for owner files whose deps changed after
            // materialization.
            if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical) {
                let whole_hash = store_view
                    .and_then(|view| view.whole_hash(canonical))
                    .or_else(|| self.get_whole_hash(canonical))
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash, store_view) {
                    return None;
                }
                let mut snapshot = snapshot;
                self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_event!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=scheduler",
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

        // ModuleFactsDb path: covers imported deps and non-scheduler files.
        let facts = self.ensure_module_facts_in_view(canonical, store_view)?;
        let mut snapshot = (*facts.snapshot).clone();
        self.resolve_snapshot_imports_in_view(canonical, &mut snapshot, store_view);
        self.enrich_destructured_bindings(&mut snapshot);
        if self.config.effective_scope().needs_template_analysis() {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
        }
        component_meta_trace_event!(
            "get_raw_analysis_snapshot_result",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} source=module_facts",
                canonical,
                snapshot.imports.len(),
                snapshot.macros.len(),
                snapshot.bindings.len(),
                snapshot.template.is_some(),
            ),
        );
        Some(snapshot)
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
            crate::resolver_core::DerivedFactKind::Route,
            crate::resolver_core::DerivedFactKind::ImportRoute,
        ] {
            if let Some(hash) = store_view
                .and_then(|view| view.derived_hash(canonical, kind))
                .or_else(|| self.current_derived_fact_hash_in_view(canonical, kind, store_view))
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

        // Legacy barrel generation facts removed — provider route cache
        // invalidates via shallow module surface hashes.
    }

    fn current_derived_fact_hash_in_view(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
        store_view: Option<&crate::resolver_store::HostStoreView>,
    ) -> Option<Hash16> {
        match kind {
            crate::resolver_core::DerivedFactKind::DirectSource => {
                self.get_whole_hash(canonical_id)
            }
            crate::resolver_core::DerivedFactKind::Route => {
                // Read-only: only compute Route hash if shallow state already exists.
                // Do NOT call ensure_shallow_* here — fact validation must be side-effect-free.
                let state = self.shallow_file_state_in_view(canonical_id, store_view)?;
                Some(crate::resolver_store::hash_route_surface(&state))
            }
            crate::resolver_core::DerivedFactKind::ImportRoute => self
                .resolver
                .runtime
                .module_facts
                .get_any(canonical_id)
                .and_then(|facts| facts.import_route_hash)
                .or_else(|| {
                    self.ensure_module_facts_in_view(canonical_id, store_view)
                        .and_then(|facts| facts.import_route_hash)
                }),
        }
    }
}

use crate::resolver_core::component_meta_registry::{
    choose_preferred_component_meta_registry_candidate,
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_non_object_top_level_surface,
    owner_component_meta_registry_import_binding, upsert_component_meta_registry_entry,
    PendingComponentMetaRegistryRef,
};

fn materialize_component_meta_member_surface_expr(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested_surface: bool,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    let mut active = rustc_hash::FxHashSet::default();
    materialize_component_meta_member_surface_expr_with_active_stack(
        expr,
        scope_canonical_id,
        engine,
        nested_surface,
        &mut active,
    )
}

fn preserve_package_backed_symbolic_refs(
    materialized: &verter_semantic::analysis::type_expr::TypeExpr,
    raw: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    match (materialized, raw) {
        (TypeExpr::Object(materialized_object), TypeExpr::Object(raw_object)) => {
            let mut object = materialized_object.as_ref().clone();
            for member in &mut object.properties {
                let ObjectMember::Property(property) = member else {
                    continue;
                };
                let raw_property =
                    raw_object
                        .properties
                        .iter()
                        .find_map(|candidate| match candidate {
                            ObjectMember::Property(raw_property)
                                if raw_property.name == property.name =>
                            {
                                Some(raw_property)
                            }
                            _ => None,
                        });
                let Some(raw_property) = raw_property else {
                    continue;
                };
                if let TypeExpr::Ref { name, .. } = &raw_property.ty {
                    if component_meta_ref_resolves_to_package(
                        scope_canonical_id,
                        name.as_ref(),
                        engine,
                    ) {
                        property.ty = raw_property.ty.clone();
                        continue;
                    }
                }
                property.ty = preserve_package_backed_symbolic_refs(
                    &property.ty,
                    &raw_property.ty,
                    scope_canonical_id,
                    engine,
                );
            }
            TypeExpr::Object(Arc::new(object))
        }
        _ => materialized.clone(),
    }
}

fn component_meta_ref_resolves_to_package(
    scope_canonical_id: &str,
    name: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
) -> bool {
    let declaration = engine.resolve_type_declaration(scope_canonical_id, name);
    declaration.canonical_source.contains("/node_modules/")
}

fn materialize_component_meta_member_surface_expr_with_active_stack(
    expr: &verter_semantic::analysis::type_expr::TypeExpr,
    scope_canonical_id: &str,
    engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    nested_surface: bool,
    active: &mut rustc_hash::FxHashSet<verter_semantic::analysis::type_expr::TypeExpr>,
) -> verter_semantic::analysis::type_expr::TypeExpr {
    use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

    if !active.insert(expr.clone()) {
        return expr.clone();
    }

    if nested_surface {
        let projected = match expr {
            TypeExpr::Ref {
                name,
                type_arguments,
            } if type_arguments.is_empty() => {
                if component_meta_ref_resolves_to_package(scope_canonical_id, name.as_ref(), engine)
                {
                    None
                } else {
                    engine.project_type_surface_expr(scope_canonical_id, name.as_ref())
                }
            }
            _ => engine.project_expr_surface_expr(scope_canonical_id, expr),
        };
        if let Some(projected) = projected {
            if projected != *expr {
                let result = materialize_component_meta_member_surface_expr_with_active_stack(
                    &projected,
                    scope_canonical_id,
                    engine,
                    true,
                    active,
                );
                active.remove(expr);
                return result;
            }
        }
    }

    let result = match expr {
        TypeExpr::Function(function) => {
            let mut function = function.as_ref().clone();
            for param in &mut function.parameters {
                param.ty = materialize_component_meta_member_surface_expr_with_active_stack(
                    &param.ty,
                    scope_canonical_id,
                    engine,
                    true,
                    active,
                );
            }
            if let Some(return_type) = function.return_type.as_mut() {
                let materialized = materialize_component_meta_member_surface_expr_with_active_stack(
                    return_type,
                    scope_canonical_id,
                    engine,
                    true,
                    active,
                );
                *return_type = Arc::new(materialized);
            }
            TypeExpr::Function(Arc::new(function))
        }
        TypeExpr::Object(object) => {
            let mut object = object.as_ref().clone();
            for member in &mut object.properties {
                match member {
                    ObjectMember::Property(property) => {
                        let should_materialize = nested_surface
                            || matches!(&property.ty, TypeExpr::Function(_) | TypeExpr::Object(_))
                            || matches!(
                                &property.ty,
                                TypeExpr::Ref {
                                    name,
                                    type_arguments,
                                } if type_arguments.is_empty()
                                    && !component_meta_ref_resolves_to_package(
                                        scope_canonical_id,
                                        name.as_ref(),
                                        engine,
                                    )
                            );
                        if should_materialize {
                            property.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &property.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                    }
                    ObjectMember::IndexSignature(signature) => {
                        signature.key_type =
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &signature.key_type,
                                scope_canonical_id,
                                engine,
                                true,
                                active,
                            );
                        signature.value_type =
                            materialize_component_meta_member_surface_expr_with_active_stack(
                                &signature.value_type,
                                scope_canonical_id,
                                engine,
                                true,
                                active,
                            );
                    }
                    ObjectMember::CallSignature(function)
                    | ObjectMember::ConstructSignature(function) => {
                        for param in &mut function.parameters {
                            param.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &param.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                        if let Some(return_type) = function.return_type.as_mut() {
                            let materialized =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    return_type,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                            *return_type = Arc::new(materialized);
                        }
                    }
                    ObjectMember::Method(method) => {
                        for param in &mut method.function.parameters {
                            param.ty =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    &param.ty,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                        }
                        if let Some(return_type) = method.function.return_type.as_mut() {
                            let materialized =
                                materialize_component_meta_member_surface_expr_with_active_stack(
                                    return_type,
                                    scope_canonical_id,
                                    engine,
                                    true,
                                    active,
                                );
                            *return_type = Arc::new(materialized);
                        }
                    }
                }
            }
            TypeExpr::Object(Arc::new(object))
        }
        TypeExpr::Array { element, readonly } => TypeExpr::Array {
            element: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    element,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            readonly: *readonly,
        },
        TypeExpr::Tuple { elements, readonly } => TypeExpr::Tuple {
            elements: Arc::from(
                elements
                    .iter()
                    .map(
                        |element| verter_semantic::analysis::type_expr::TupleElement {
                            label: element.label.clone(),
                            ty: materialize_component_meta_member_surface_expr_with_active_stack(
                                &element.ty,
                                scope_canonical_id,
                                engine,
                                nested_surface,
                                active,
                            ),
                            optional: element.optional,
                            rest: element.rest,
                        },
                    )
                    .collect::<Vec<_>>(),
            ),
            readonly: *readonly,
        },
        TypeExpr::Union(types) => {
            engine.reset_union_members();
            TypeExpr::Union(Arc::from(
                types
                    .iter()
                    .map(|ty| {
                        if !engine.allow_union_member() {
                            return ty.clone();
                        }
                        materialize_component_meta_member_surface_expr_with_active_stack(
                            ty,
                            scope_canonical_id,
                            engine,
                            nested_surface,
                            active,
                        )
                    })
                    .collect::<Vec<_>>(),
            ))
        }
        TypeExpr::Intersection(types) => TypeExpr::Intersection(Arc::from(
            types
                .iter()
                .map(|ty| {
                    materialize_component_meta_member_surface_expr_with_active_stack(
                        ty,
                        scope_canonical_id,
                        engine,
                        nested_surface,
                        active,
                    )
                })
                .collect::<Vec<_>>(),
        )),
        TypeExpr::Parenthesized(inner) => TypeExpr::Parenthesized(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::Rest(inner) => TypeExpr::Rest(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::KeyOf(inner) => TypeExpr::KeyOf(Arc::new(
            materialize_component_meta_member_surface_expr_with_active_stack(
                inner,
                scope_canonical_id,
                engine,
                nested_surface,
                active,
            ),
        )),
        TypeExpr::Conditional {
            check,
            extends,
            true_type,
            false_type,
        } => TypeExpr::Conditional {
            check: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    check,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            extends: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    extends,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            true_type: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    true_type,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            false_type: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    false_type,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
        },
        TypeExpr::Mapped {
            parameter,
            source,
            optional,
            readonly,
            name_type,
            value,
        } => TypeExpr::Mapped {
            parameter: parameter.clone(),
            source: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    source,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
            optional: *optional,
            readonly: *readonly,
            name_type: name_type.as_deref().map(|name_type| {
                Arc::new(
                    materialize_component_meta_member_surface_expr_with_active_stack(
                        name_type,
                        scope_canonical_id,
                        engine,
                        nested_surface,
                        active,
                    ),
                )
            }),
            value: Arc::new(
                materialize_component_meta_member_surface_expr_with_active_stack(
                    value,
                    scope_canonical_id,
                    engine,
                    nested_surface,
                    active,
                ),
            ),
        },
        TypeExpr::TemplateLiteral {
            quasis,
            expressions,
        } => TypeExpr::TemplateLiteral {
            quasis: quasis.clone(),
            expressions: Arc::from(
                expressions
                    .iter()
                    .map(|expr| {
                        materialize_component_meta_member_surface_expr_with_active_stack(
                            expr,
                            scope_canonical_id,
                            engine,
                            nested_surface,
                            active,
                        )
                    })
                    .collect::<Vec<_>>(),
            ),
        },
        _ => expr.clone(),
    };

    active.remove(expr);
    result
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
    shared_owner_engine: Option<
        std::cell::RefCell<
            verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine<'a>,
        >,
    >,
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
        // Tracked dependencies: snapshot-level candidates + solver-discovered deps.
        // The legacy walker is no longer used for dependency tracking.
        let mut tracked_dependencies = std::collections::BTreeSet::new();
        tracked_dependencies.extend(
            eval_context
                .map(|captured| captured.direct_dependency_candidates.clone())
                .unwrap_or_else(|| {
                    self.host.cache_dependency_candidates_from_snapshot(
                        owner_canonical,
                        snapshot,
                        self.store_view,
                    )
                }),
        );
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // Always run the solver-host macro path. The solver resolves cross-file
        // types on demand from the host's prepared-decl cache.
        let computed_eval_types = if let Some(engine) = &self.shared_owner_engine {
            let eval_source = eval_context
                .and_then(|captured| captured.owner_eval_source.as_deref())
                .map(str::to_string)
                .or_else(|| {
                    self.host
                        .ensure_module_facts_in_view(owner_canonical, self.store_view)
                        .map(|facts| {
                            VerterHost::build_eval_script_source(
                                &facts.raw_source,
                                facts.cached_parse.as_deref(),
                            )
                        })
                });
            eval_source.and_then(|eval_source| {
                let mut engine = engine.borrow_mut();
                self.host
                    .compute_evaluated_types_from_owner_context_in_view(
                        owner_canonical,
                        snapshot,
                        &eval_source,
                        self.store_view,
                        Some(&mut *engine),
                    )
            })
        } else {
            self.host
                .compute_evaluated_types_with_tracking_from_owner_context_in_view(
                    owner_canonical,
                    snapshot,
                    eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                    self.store_view,
                )
        };
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
        let _ = visiting;
        self.host.resolve_component_meta_macro_elements_in_view(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
            self.store_view,
        )
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
        shared_owner_engine: None,
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

    // Ensure module facts are materialized so the solver host can resolve imports.
    let _facts = host.ensure_module_facts_in_view(canonical_source, store_view)?;
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
    let mut engine =
        verter_semantic::analysis::type_solver::query_engine::TypeQueryEngine::new(&solver_host);
    let resolved = engine.solve(&parsed);
    Some(resolved.value)
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
