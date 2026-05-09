//! Host-method surface for component-meta on `VerterHost`.
//!
//! Domain 8 — the inherent `impl VerterHost { ... }` block that
//! lives next to the materialization core. Owns ~18 host methods including
//! `resolve_component_meta`, `compute_component_meta_state`, the
//! `*_inner` audited variants, the registry-publication helpers
//! (`append_component_meta_registry_entries`,
//! `bridge_component_meta_registry_for_imported_macros`, ...), and the
//! request-id / fact-version / fact-key plumbing.
//!
//! Lines 121-2528 of the post-commit-8 `meta_resolve.rs` shell. Rust
//! supports multiple `impl VerterHost { ... }` blocks across files, so
//! this is purely a textual move; no signatures change.
//!
//! All cross-module imports are listed at the top so the body retains its
//! verbatim form. Items that still live in the parent shell
//! (the registry / cycle / origin-graph predicates, the resolver
//! adapter, etc.) are reached via `super::*` until those domains
//! land in their final per-domain siblings

use crate::host_manage::{
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
};
use crate::resolver_core::{run_component_meta_request, RequestSource, SingleflightRole};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

// File moved from `meta_resolve/host_methods.rs` to
// `host_manage/component_meta_methods.rs`. The original `super::X` paths
// resolved through `meta_resolve`'s private siblings; after the move,
// they rewrite to `crate::meta_resolve::X` (the parent module's
// re-exported `pub(crate)` surface).
use crate::meta_resolve::compare_type_expr_improvement;
use crate::meta_resolve::component_meta_registry_prefers_structural_materialization;
use crate::meta_resolve::slot_binding_graph;
use crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS;
use crate::meta_resolve::{
    collect_define_props_root_names, component_meta_owner_local_shallow_substituted_alias_body,
    select_imported_materialization_scope, slot_binding_targets_define_props_root,
    RegistryMaterialization, ResolvedComponentMetaState,
};
use crate::meta_resolve::{
    collect_type_expr_ref_names, lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_field_types, materialize_component_meta_type_expr_until_stable,
    produce_macro_object_shapes_for_purpose,
};
use crate::meta_resolve::{
    drain_dispatch_dep_signature_accumulator, reset_dispatch_dep_signature_accumulator,
};
use crate::meta_resolve::{
    instantiate_local_generic_ref_via_dispatch, pick_via_dispatch_pick_helper,
    project_expr_class_a_via_dispatch, project_expr_class_a_via_dispatch_threaded,
    project_type_surface_expr_via_host_threaded,
};
use crate::meta_resolve::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    resolved_meta_cache_key, should_skip_imported_registry_seed_refresh, trace_request_source,
    CapturedComponentMetaInputs, ResolvedComponentMetaComputeAudit, ResolvedTypeRegistryMeta,
};

// Items that live in the parent shell (`crate::meta_resolve`): the
// registry-structural materialiser, the registry-route preservers, the
// graph-native registry-route + cycle-BFS predicates, and the
// origin-graph builder. The `HostComponentMetaResolver` adapter lives
// in `host_manage/jsdoc_resolve.rs` (host-impl tier).
use crate::host_manage::jsdoc_resolve::HostComponentMetaResolver;
use crate::meta_resolve::build_origin_graph;
use crate::meta_resolve::{
    component_meta_registry_prefers_structural_materialization_node,
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    materialize_component_meta_registry_structural_expr, preserve_nested_symbolic_member_routes,
    preserve_registry_callable_param_member_routes,
    type_expr_needs_nested_symbolic_route_preservation,
};

use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
    component_meta_registry_raw_member_path_surface, enqueue_component_meta_registry_ref,
    merge_component_meta_registry_candidates, owner_component_meta_registry_import_root,
    upsert_component_meta_registry_entry, PendingComponentMetaRegistryRef,
};

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
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.resolve_component_meta_with_view(canonical_or_alias, mode)
    }

    pub(crate) fn resolve_component_meta_with_view(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        let started = component_meta_debug_enabled().then(Instant::now);
        let canonical = self.resolve_alias_or_canonical(canonical_or_alias);
        let audit = self.config.audit_enabled.then(|| {
            // Prefer the request_id stamped by
            // `get_component_meta_with_resolution` (via the installed
            // `RequestContext`). Falls back to the global static only
            // when no context is installed — e.g. direct callers of
            // `resolve_component_meta` outside the audited-request
            // path. Without this link `take_audit_record` would look
            // up the outer id while the record is stored under the
            // inner id, and every `AuditedRequest::resolve` would
            // fail with `AuditRecordMissing`.
            let request_id = crate::request_context::current_request_context()
                .map(|ctx| ctx.request_id)
                .unwrap_or_else(next_component_meta_audit_request_id);
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
        component_meta_trace_custom!(
            "resolve_component_meta",
            format!("owner={} mode={mode:?}", canonical),
        );
        let result = run_component_meta_request(
            self,
            self.resolver_runtime().component_meta.singleflight(),
            &canonical,
            mode,
            None,
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
            component_meta_trace_custom!(
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
            // Joiner-accounting: when the singleflight identified
            // this request as a Follower, the request received its
            // result from the dedup-join (semantically a warm hit on
            // the in-flight computation, not a cold compute). Flip
            // the speculative miss bumped by the warm-cache check
            // into a hit on the active TLS context, and mark
            // `from_cache=true` so the audit record carries the
            // contract-correct attribution. The cold winner stays at
            // the default (`from_cache=false`, miss recorded) and
            // pays for the cold work it actually performed.
            if let RequestSource::Flight {
                role: SingleflightRole::Follower,
                ..
            } = result.source
            {
                audit_builder.mark_joined_inflight();
            }
            let (store_audit, cm_counters) = self.component_meta_audit_store_snapshot(None);
            audit_builder.record_store(store_audit);
            let (host_cache_after_bytes, workspace_after_bytes) =
                self.component_meta_audit_memory_bytes();
            audit_builder.record_memory_snapshots(
                host_cache_before_bytes,
                host_cache_after_bytes,
                workspace_before_bytes,
                workspace_after_bytes,
            );
            // Install the resolver-side solver counters FIRST, then
            // layer materializer counters on top. Order matters
            // because `record_component_meta_payload` replaces the
            // in-flight payload wholesale, while
            // `record_component_meta_store` mutates only the
            // materializer fields.
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
                    audit_builder.record_component_meta_payload(compute_audit.solver.clone());
                }
            }
            audit_builder.record_component_meta_store(
                cm_counters.materialize_structure_calls,
                cm_counters.materialize_structure_cache_hits,
                cm_counters.node_arena_lock_acquisitions,
                cm_counters.family_map_lock_acquisitions,
                cm_counters.dep_signature_merges,
                cm_counters.dep_signature_intern_hits,
            );
            // Project graph-native slot-binding synthesis state onto the
            // audit substrate. `diagnostics` projects via the
            // `host_audit_bridge` so consumers see one canonical
            // `AuditDiagnosticEntry` stream regardless of producer;
            // `should_suppress` mirrors the cache-write gate so audit
            // consumers can see why a request did not warm the cache.
            if let Some(resolved) = result.value.as_ref() {
                let payload = audit_builder.component_meta_payload_mut();
                payload.diagnostics = crate::host_audit_bridge::macro_expansion_to_audit_entries(
                    &resolved.synthesis_diagnostics,
                );
                payload.should_suppress = resolved.synthesis_should_suppress;
            }
            // Mine the semantic footprint when the active request is
            // capturing. Drains the per-request accumulator, builds the
            // per-file attribution vector, then feeds the rest through
            // the deterministic footprint miner before the builder
            // finalises. The file ledger is read off the state BEFORE
            // the miner consumes it.
            if let Some(ctx) = crate::request_context::current_request_context() {
                if ctx.footprint_capture {
                    if let Some(acc) = ctx.audit_accumulator.as_ref() {
                        let state = acc.drain();
                        // Direct imports of the entry: extract from
                        // the entry's shallow surface so the file-
                        // role classifier can distinguish first-level
                        // imports (`DirectImport`) from deeper-closure
                        // files (`TransitiveImport`). When the shallow
                        // surface is not yet available (rare cold
                        // path), the empty set falls back to
                        // `DirectImport` for every non-Entry file.
                        let direct_imports: rustc_hash::FxHashSet<String> = self
                            .shallow_file_state(ctx.canonical_id.as_ref())
                            .map(|sfs| {
                                sfs.import_targets
                                    .values()
                                    .map(|t| t.canonical_id.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        let files = crate::component_meta_audit::build_file_audit_vec(
                            &state,
                            ctx.canonical_id.as_ref(),
                            &direct_imports,
                            self.config.audit_timing_capture && self.config.audit_enabled,
                        );
                        audit_builder.record_files(files);
                        let footprint = crate::component_meta_audit::mine_footprint(
                            self.project_type_store().semantic_graph(),
                            state,
                            &ctx,
                            self.config.max_derivation_edges,
                        );
                        audit_builder.record_footprint(footprint);
                    }
                }
            }
            let record = audit_builder.finish();
            crate::component_meta_audit::emit_audit_trace(&record);
            // Finalise through the `AuditRequestRegistration` planted
            // on the active `RequestContext`. The registration removes
            // the in-flight slot from the host's active-request
            // registry and inserts the record into the records store
            // so `take_audit_record(resolution.request_id)` returns it.
            // When no registration is in scope (synthetic test
            // fixture path), the helper falls back to a direct insert
            // so the host-wide store stays consistent.
            self.finalize_request_audit_record(record);
        }

        result.value
    }

    pub(crate) fn compute_component_meta_state(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
        )
    }

    pub(crate) fn compute_component_meta_state_from_captured(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        captured: &CapturedComponentMetaInputs,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            mode,
            captured.whole_hash,
            Some(captured),
            crate::resolver_core::ComponentMetaResolutionPurpose::Full,
            RegistryMaterialization::Full,
        )
    }

    pub(crate) fn compute_component_meta_state_for_fallthrough(
        &self,
        canonical: &str,
        whole_hash: Hash16,
    ) -> Option<ResolvedComponentMetaState> {
        self.compute_component_meta_state_inner(
            canonical,
            ProjectionMode::Expanded,
            whole_hash,
            None,
            crate::resolver_core::ComponentMetaResolutionPurpose::Fallthrough,
            RegistryMaterialization::SkipAppend,
        )
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn compute_component_meta_state_inner(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        whole_hash: Hash16,
        captured: Option<&CapturedComponentMetaInputs>,
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
        registry_materialization: RegistryMaterialization,
    ) -> Option<ResolvedComponentMetaState> {
        // Step 6.6.A: reset the per-request dep-signature accumulator
        // so each compute call starts fresh. Inner materialize_until_stable
        // calls accumulate dispatch-side facts; we drain + merge them
        // into the published `fact_versions` below.
        reset_dispatch_dep_signature_accumulator();

        let audit_enabled = self.config.audit_enabled;
        let mut audit_timings = if audit_enabled {
            captured
                .map(|captured| crate::component_meta_audit::RequestTimingAudit {
                    capture_inputs_ms: captured.audit_capture_inputs_ms,
                    store_read_ms: captured.audit_store_read_ms,
                    direct_import_proof_ms: captured.audit_direct_import_proof_ms,
                    ..Default::default()
                })
                .unwrap_or_default()
        } else {
            crate::component_meta_audit::RequestTimingAudit::default()
        };
        component_meta_trace_custom!(
            "compute_component_meta_state",
            format!(
                "owner={} mode={mode:?} captured={} store_view={} whole_hash={whole_hash:?}",
                canonical,
                captured.is_some(),
                false,
            ),
        );
        self.provenance
            .component_meta_resolved_state_recomputes
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let snapshot = captured
            .map(|captured| captured.snapshot.clone())
            .or_else(|| self.get_raw_analysis_snapshot(canonical))?;
        component_meta_trace_custom!(
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
        // Retired `shared_owner_engine` /
        // `SessionSolverHost` pair; the resolver host is now a thin
        // wrapper around `VerterHost`.
        let resolver_host = HostComponentMetaResolver { host: self };
        let parts_started = audit_enabled.then(Instant::now);
        let parts = {
            component_meta_trace_custom!(
                "resolve_component_meta_parts",
                format!(
                    "owner={} expanded={} captured={} purpose={:?}",
                    canonical,
                    mode == ProjectionMode::Expanded,
                    captured.is_some(),
                    purpose,
                ),
            );
            crate::resolver_core::resolve_component_meta_parts(
                &resolver_host,
                canonical,
                &snapshot,
                mode == ProjectionMode::Expanded,
                captured,
                purpose,
            )
        };
        if let Some(started) = parts_started {
            audit_timings.solver_ms = started.elapsed().as_secs_f64() * 1000.0;
        }
        let mut parts = parts;
        // Graph-native slot-binding synthesis accumulators. Both
        // call sites OR-fold their `SynthesisResult` into
        // `synthesis_should_suppress` and append diagnostics into
        // `synthesis_diagnostics`; the merged result is propagated
        // through `ResolvedComponentMetaState` so the cache-write
        // gate (`!synthesis_should_suppress`) and audit-payload
        // emission both observe the same state.
        let mut synthesis_diagnostics: Vec<
            verter_semantic::analysis::component_meta::MacroExpansionDiagnostics,
        > = Vec::new();
        let mut synthesis_should_suppress = false;
        if let Some(evaluated_types) = parts.evaluated_types.as_mut() {
            let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
            let result = slot_binding_graph::resolve_slot_bindings_graph_native(
                &mut query_engine,
                canonical,
                &snapshot,
                &parts.resolved_macros,
                evaluated_types,
                &mut synthesis_diagnostics,
            );
            synthesis_should_suppress |= result.should_suppress;
        }
        let registry_before = parts.resolved_type_registry.len();
        let append_start = Instant::now();
        let should_materialize_registry = registry_materialization == RegistryMaterialization::Full;
        let should_produce_macro_object_shapes = mode == ProjectionMode::Expanded;
        let solver_audit = if should_materialize_registry || should_produce_macro_object_shapes {
            // The retired `shared_owner_engine`
            // is gone — dispatch owns all solve-like operations now.
            let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self);
            if should_materialize_registry {
                component_meta_trace_custom!(
                    "append_component_meta_registry_entries",
                    format!(
                        "owner={} evaluated_types={} existing_registry={}",
                        canonical,
                        parts.evaluated_types.is_some(),
                        parts.resolved_type_registry.len(),
                    ),
                );
                self.append_component_meta_registry_entries(
                    canonical,
                    &snapshot,
                    parts.evaluated_types.as_ref(),
                    &mut parts.resolved_type_registry,
                    &mut parts.resolved_type_registry_meta,
                    &mut parts.tracked_dependencies,
                    &mut query_engine,
                );
            }
            if should_produce_macro_object_shapes {
                if let Some(eval_source) = captured
                    .and_then(|captured| captured.owner_eval_source.as_deref())
                    .map(str::to_string)
                    .or_else(|| {
                        self.ensure_indexed_ready(canonical).map(|facts| {
                            VerterHost::build_eval_script_source(
                                &facts.raw_source,
                                facts.cached_parse.as_deref(),
                            )
                        })
                    })
                {
                    let mut evaluated_types = parts.evaluated_types.take().unwrap_or_default();
                    {
                        component_meta_trace_custom!(
                            "produce_macro_object_shapes_for_purpose",
                            format!(
                                "owner={} resolved_macros={} registry={} purpose={:?}",
                                canonical,
                                parts.resolved_macros.len(),
                                parts.resolved_type_registry.len(),
                                purpose,
                            ),
                        );
                        produce_macro_object_shapes_for_purpose(
                            canonical,
                            &snapshot,
                            &parts.resolved_macros,
                            &parts.resolved_type_registry,
                            &parts.resolved_type_registry_meta,
                            &eval_source,
                            &mut evaluated_types,
                            &mut query_engine,
                            purpose,
                        );
                    }
                    {
                        component_meta_trace_custom!(
                            "project_evaluated_types",
                            format!(
                                "owner={} props={} slot_bindings={} define_props={} define_slots={}",
                                canonical,
                                evaluated_types.props.len(),
                                evaluated_types.slot_bindings.len(),
                                evaluated_types.define_props.len(),
                                evaluated_types.define_slots.len(),
                            ),
                        );
                        // §7.1 cutover: per-macro projectors are the
                        // sole component-meta resolution path. Each
                        // projector dispatches `ResolveMacroPayload`
                        // + empty-path Shallow `ProjectPath` and writes
                        // `Vec<ExpandedField>` into `evaluated_types`.
                        // Errors / cycles emit diagnostics into
                        // `synthesis_diagnostics` per §7.5 silent-miss
                        // prevention (treated as macro-expansion
                        // diagnostics on the published analysis).
                        let dispatch =
                            crate::project_semantic_dispatch::ProjectSemanticDispatch::new(
                                query_engine.ctx,
                            );
                        crate::meta_resolve::projectors::project_evaluated_types(
                            &dispatch,
                            query_engine.ctx,
                            canonical,
                            &snapshot,
                            &mut evaluated_types,
                            &mut synthesis_diagnostics,
                        );
                    }
                    if !evaluated_types.is_empty() {
                        let result = slot_binding_graph::resolve_slot_bindings_graph_native(
                            &mut query_engine,
                            canonical,
                            &snapshot,
                            &parts.resolved_macros,
                            &mut evaluated_types,
                            &mut synthesis_diagnostics,
                        );
                        synthesis_should_suppress |= result.should_suppress;
                        {
                            component_meta_trace_custom!(
                                "materialize_component_meta_field_types",
                                format!(
                                    "owner={} props={} events={} slot_bindings={} bindings={}",
                                    canonical,
                                    evaluated_types.props.len(),
                                    evaluated_types.emits.len(),
                                    evaluated_types.slot_bindings.len(),
                                    evaluated_types.bindings.len(),
                                ),
                            );
                            materialize_component_meta_field_types(
                                canonical,
                                &snapshot,
                                &eval_source,
                                &parts.resolved_macros,
                                &mut evaluated_types,
                                &mut query_engine,
                            );
                        }
                        parts.evaluated_types = Some(evaluated_types);
                    }
                }
            }
            {
                crate::host_manage::component_meta_trace_custom!(
                    "semantic_graph_stats",
                    format!("owner={} dispatch_authority=true", canonical),
                );
            }
            if query_engine.has_fuse_tripped() {
                for trip in query_engine.fuse_trips() {
                    crate::host_manage::component_meta_trace_custom!(
                        "fuse_tripped",
                        format!(
                            "owner={} fuse={} budget={} actual={}",
                            canonical, trip.fuse_name, trip.budget, trip.actual,
                        ),
                    );
                }
            }
            crate::component_meta_audit::ComponentMetaPayload {
                total_resolve_steps: 0u64,
                solve_count: 0u32,
                ..Default::default()
            }
        } else {
            crate::host_manage::component_meta_trace_custom!(
                "semantic_graph_stats",
                format!(
                    "owner={} registry_materialization=skipped macro_shapes=skipped",
                    canonical,
                ),
            );
            crate::component_meta_audit::ComponentMetaPayload::default()
        };
        audit_timings.materialize_ms = append_start.elapsed().as_secs_f64() * 1000.0;
        let store_merge_started = audit_enabled.then(Instant::now);
        // Fact versions must reflect the post-resolution state of the host —
        // mid-request `set_import_dependencies` / `ensure_loaded` calls may
        // have updated import_routes and module_facts that the ambient
        // captured view does not see. Build a fresh snapshot here so the
        // stored facts match the live state at store time; a warm follow-up
        // query will then validate against the same post-resolution state.
        parts.fact_versions =
            self.current_dependency_fact_versions(canonical, &parts.tracked_dependencies);
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
            let dep_cache_size = self.project_type_store.indexed().len();
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
        component_meta_trace_custom!(
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
        // Step 6.6.A: drain accumulated dispatch dep_signatures and
        // merge into fact_versions before publish. Each
        // materialize_until_stable_full call inside the compute body
        // pushed the dispatch round-trip's DepSignature into the
        // thread-local accumulator; here we read + merge so warm
        // cache validation captures the dependency graph the
        // dispatch path discovered.
        let mut merged_fact_versions = parts.fact_versions;
        let dispatch_facts = drain_dispatch_dep_signature_accumulator();
        for fact in dispatch_facts {
            if !merged_fact_versions.contains(&fact) {
                merged_fact_versions.push(fact);
            }
        }

        // Step 9.1: SurfaceNodeIdentities sidecar — populated by the
        // audit-gated FieldKind closure inside
        // `compute_evaluated_types`'s
        // `expand_macro_types_impl_with_expander` call. Threaded down
        // through `ComponentMetaEvalOutputs.surface_identities` →
        // `ResolvedComponentMetaParts.surface_identities` → here.
        // `None` when audit is off (the only consumer is the scoped
        // origin export, itself audit-gated).
        let surface_identities = parts.surface_identities;
        let surface_identities_for_export = surface_identities.clone();

        let state = ResolvedComponentMetaState {
            snapshot,
            mode,
            whole_hash,
            resolved_macros: parts.resolved_macros,
            resolved_type_registry: parts.resolved_type_registry,
            resolved_type_registry_meta: parts.resolved_type_registry_meta,
            evaluated_types: parts.evaluated_types,
            fact_versions: merged_fact_versions,
            surface_identities,
            synthesis_diagnostics,
            synthesis_should_suppress,
            compute_audit: audit_enabled.then_some(ResolvedComponentMetaComputeAudit {
                timings: audit_timings,
                solver: solver_audit,
            }),
            // F1 (D3, D34): origin_graph is audit-only. Gate matches LSP's
            // hover-provenance contract at server.rs:6918-6953 — both
            // audit_enabled and footprint_capture must be on.
            // Step 9.2 / F6: surface_identities (when populated) scopes
            // the export to the reachable subgraph rooted at the
            // request's surface nodes; falls back to workspace-total
            // export when None.
            origin_graph: (mode == ProjectionMode::Expanded
                && audit_enabled
                && self.config.footprint_capture)
                .then(|| {
                    build_origin_graph(
                        self.project_type_store.semantic_graph(),
                        surface_identities_for_export.as_ref(),
                    )
                })
                .filter(|dto| !dto.edges.is_empty()),
            request_id: 0,
        };
        Some(state)
    }

    #[cfg_attr(feature = "hotpath", hotpath::measure)]
    pub(crate) fn append_component_meta_registry_entries(
        &self,
        owner_canonical: &str,
        snapshot: &FileAnalysisSnapshot,
        evaluated_types: Option<&verter_semantic::analysis::type_expand::ExpandedComponentTypes>,
        resolved_type_registry: &mut Vec<
            verter_semantic::analysis::component_meta::ResolvedTypeAnalysis,
        >,
        resolved_type_registry_meta: &mut Vec<ResolvedTypeRegistryMeta>,
        tracked_dependencies: &mut BTreeSet<String>,
        query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
    ) {
        let _loop8_timer = crate::loop5_instrumentation::TimerGuard::new(
            &crate::loop5_instrumentation::APPEND_REGISTRY_ENTRIES_CALLS,
            &crate::loop5_instrumentation::APPEND_REGISTRY_ENTRIES_NS,
        );
        fn track_component_meta_dependency(
            tracked_dependencies: &mut BTreeSet<String>,
            owner_canonical: &str,
            canonical_id: &str,
        ) {
            if !canonical_id.is_empty() && canonical_id != owner_canonical {
                tracked_dependencies.insert(canonical_id.to_string());
            }
        }
        fn imported_registry_alias_should_stay_symbolic(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
        ) -> bool {
            use verter_semantic::analysis::type_expr::TypeExpr;

            match expr {
                TypeExpr::Parenthesized(inner) => {
                    imported_registry_alias_should_stay_symbolic(inner)
                }
                TypeExpr::Mapped { .. }
                | TypeExpr::Conditional { .. }
                | TypeExpr::IndexedAccess { .. }
                | TypeExpr::TypeOf(_) => true,
                _ => false,
            }
        }
        fn materialize_component_meta_registry_candidate(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            use verter_semantic::analysis::type_expr::{
                ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
            };

            let imported_generic_alias_scope: Option<String> = raw_body.and_then(|expr| {
                let TypeExpr::Ref {
                    name,
                    type_arguments,
                } = expr
                else {
                    return None;
                };
                if type_arguments.is_empty() {
                    return None;
                }
                let declaration = query_engine.resolve_type_declaration(scope_canonical_id, name);
                if !declaration.canonical_source.is_empty()
                    && declaration.canonical_source != scope_canonical_id
                {
                    Some(declaration.canonical_source.clone())
                } else {
                    None
                }
            });
            let imported_generic_alias_root = imported_generic_alias_scope.is_some();

            // Path C C11-residual-B: owner-local generic Refs preserve
            // helper-Ref structure. When `Button = ComponentConfig<typeof theme>`
            // is declared in the SAME file as `ComponentConfig` (owner-
            // local), the registry should publish Button as the SHALLOW
            // substituted body — `{ variants: ComponentVariants<...>,
            // slots: ComponentSlots<...>, ui: ComponentUI<...> }` —
            // rather than fully materialising every helper. This keeps
            // the registry consumer's Ref-to-helper navigation path
            // queryable rather than collapsing helper identities into
            // their concrete shapes.
            //
            // Distinct from the imported-alias path
            // (`maybe_refine_imported_generic_alias_object` above) which
            // DOES materialise cross-file aliases (because the consumer
            // can't follow Refs to a cross-file helper through the
            // registry directly).
            if !imported_generic_alias_root {
                if let Some(shallow) = component_meta_owner_local_shallow_substituted_alias_body(
                    query_engine,
                    scope_canonical_id,
                    raw_body,
                ) {
                    return Some(shallow);
                }
            }

            let maybe_refine_imported_generic_alias_object =
                |candidate: TypeExpr,
                 query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>| {
                    if !imported_generic_alias_root {
                        return candidate;
                    }
                    let TypeExpr::Object(object) = candidate else {
                        return candidate;
                    };
                    let properties = object
                        .properties
                        .iter()
                        .map(|member| match member {
                            ObjectMember::Property(property) => {
                                let materialized =
                                    query_engine.materialize_member_surface_expr(
                                        scope_canonical_id,
                                        &property.ty,
                                        true,
                                    );
                                let stabilized =
                                    materialize_component_meta_type_expr_until_stable(
                                        &materialized,
                                        scope_canonical_id,
                                        crate::semantic_query::ProjectionMode::Expanded,
                                        query_engine,
                                    );
                                // For generic Ref members (e.g. ComponentVariants<T>),
                                // try expanding and solving in the correct scope so
                                // concrete args produce concrete member shapes.
                                let solved = match &stabilized {
                                    TypeExpr::Ref { type_arguments, .. }
                                        if !type_arguments.is_empty() =>
                                    {
                                        let materialize_scope =
                                            select_imported_materialization_scope(
                                                &stabilized,
                                                scope_canonical_id,
                                                query_engine,
                                            )
                                            .or_else(|| imported_generic_alias_scope.clone())
                                            .unwrap_or_else(|| {
                                                scope_canonical_id.to_string()
                                            });
                                        // Migrate the
                                        // generic-Ref instantiation to dispatch
                                        // (sub- D-T recipe). The
                                        // helper resolves
                                        // `instantiate_local_generic_ref` via
                                        // the dispatch's `Instantiate` arm
                                        // (`SemanticQueryKey::Instantiate`).
                                        let expanded =
                                            instantiate_local_generic_ref_via_dispatch(
                                                query_engine.ctx,
                                                materialize_scope.as_str(),
                                                &stabilized,
                                            )
                                            .unwrap_or_else(|| stabilized.clone());
                                        // Migrate the
                                        // route-loop call to the Class A
                                        // dispatch helper. The helper covers
                                        // the registry-route fast-path AND
                                        // the generic ProjectPath{[],Expanded}
                                        // dispatch; preserving the engine
                                        // thread keeps fuse / scope-payload
                                        // continuity for the route fast-path
                                        // (still on the engine until
                                        // alongside instantiate_local_generic_ref).
                                        project_expr_class_a_via_dispatch_threaded(
                                            query_engine.ctx,
                                            Some(query_engine),
                                            materialize_scope.as_str(),
                                            &expanded,
                                        )
                                        .map(|solved| {
                                            query_engine.materialize_member_surface_expr(
                                                materialize_scope.as_str(),
                                                &solved,
                                                true,
                                            )
                                        })
                                        .unwrap_or_else(|| stabilized.clone())
                                    }
                                    _ => stabilized.clone(),
                                };
                                ObjectMember::Property(ObjectProperty {
                                    name: property.name.clone(),
                                    ty: if compare_type_expr_improvement(
                                        &solved,
                                        &property.ty,
                                    ) {
                                        solved
                                    } else if compare_type_expr_improvement(
                                        &stabilized,
                                        &property.ty,
                                    ) {
                                        stabilized
                                    } else if compare_type_expr_improvement(
                                        &materialized,
                                        &property.ty,
                                    ) {
                                        materialized
                                    } else {
                                        property.ty.clone()
                                    },
                                    optional: property.optional,
                                    readonly: property.readonly,
                                })
                            }
                            other => other.clone(),
                        })
                        .collect();
                    TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties }))
                };

            if prefer_explicit_raw_surface
                && raw_body.is_some_and(component_meta_registry_has_explicit_object_surface)
            {
                return raw_body.cloned().map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                });
            }
            if raw_body.is_some_and(|expr| {
                component_meta_registry_has_non_object_top_level_surface(expr)
                    && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                        expr,
                        scope_canonical_id,
                        query_engine,
                    )
            }) {
                return raw_body.cloned();
            }
            // Migrate the structural-materialisation
            // preference to the graph-native predicate. Lower the raw
            // TypeExpr to a Navigate-mode SemanticNodeId and consult
            // `component_meta_registry_prefers_structural_materialization_node`.
            // Falls back to the legacy TypeExpr predicate when lowering
            // fails (matches conservative "not structural" semantics
            // when no canonical node id exists).
            if let Some(raw) = raw_body.filter(|expr| {
                if !component_meta_registry_has_non_object_top_level_surface(expr) {
                    return false;
                }
                let host = query_engine.ctx;
                let dispatch = crate::project_semantic_dispatch::ProjectSemanticDispatch::new(host);
                if let Some(node) = dispatch.lower_type_expr_in_scope_with_mode(
                    scope_canonical_id,
                    expr,
                    crate::semantic_query::ProjectionMode::Navigate,
                ) {
                    let graph = host.project_type_store().semantic_graph();
                    component_meta_registry_prefers_structural_materialization_node(graph, node, 0)
                } else {
                    // Lowering failure — fall back to the TypeExpr
                    // predicate's classification. Preserves existing
                    // behaviour for shapes the dispatcher cannot lower
                    // (e.g., parser-only TypeExpr arms with no graph
                    // counterpart yet).
                    component_meta_registry_prefers_structural_materialization(expr)
                }
            }) {
                return Some(materialize_component_meta_registry_structural_expr(
                    raw,
                    scope_canonical_id,
                    query_engine,
                ));
            }
            // Bridge via per-engine helper.
            project_type_surface_expr_via_host_threaded(
                query_engine,
                scope_canonical_id,
                symbol_name,
            )
            .map(|materialized| {
                raw_body.map_or_else(
                    || materialized.clone(),
                    |raw| {
                        let preserved_package_refs = lowered_preserve_package_backed_symbolic_refs(
                            &materialized,
                            raw,
                            scope_canonical_id,
                            query_engine,
                        );
                        preserve_registry_callable_param_member_routes(&preserved_package_refs, raw)
                    },
                )
            })
            .map(|candidate| maybe_refine_imported_generic_alias_object(candidate, query_engine))
            .or_else(|| {
                raw_body.and_then(|expr| {
                    (!component_meta_registry_has_non_object_top_level_surface(expr)).then(|| {
                        maybe_refine_imported_generic_alias_object(expr.clone(), query_engine)
                    })
                })
            })
            .or_else(|| {
                raw_body.cloned().map(|candidate| {
                    maybe_refine_imported_generic_alias_object(candidate, query_engine)
                })
            })
        }
        fn build_registry_indexed_access_expr(
            symbol_name: &str,
            path: &[String],
        ) -> verter_semantic::analysis::type_expr::TypeExpr {
            path.iter().fold(
                verter_semantic::analysis::type_expr::TypeExpr::named(symbol_name),
                |object, member| verter_semantic::analysis::type_expr::TypeExpr::IndexedAccess {
                    object: std::sync::Arc::new(object),
                    index: std::sync::Arc::new(
                        verter_semantic::analysis::type_expr::TypeExpr::string_literal(
                            member.clone(),
                        ),
                    ),
                },
            )
        }
        fn wrap_registry_member_path_surface(
            path: &[String],
            leaf: verter_semantic::analysis::type_expr::TypeExpr,
        ) -> verter_semantic::analysis::type_expr::TypeExpr {
            path.iter().rfold(leaf, |child, member| {
                verter_semantic::analysis::type_expr::TypeExpr::Object(std::sync::Arc::new(
                    verter_semantic::analysis::type_expr::ObjectExpr {
                        properties: vec![
                            verter_semantic::analysis::type_expr::ObjectMember::Property(
                                verter_semantic::analysis::type_expr::ObjectProperty {
                                    name: member.clone(),
                                    ty: child,
                                    optional: true,
                                    readonly: false,
                                },
                            ),
                        ],
                    },
                ))
            })
        }
        /// Issue #10 / predicate: does `expr` contain any
        /// callable surface (`TypeExpr::Function` or an Object with a
        /// call/method signature) anywhere reachable from its top-
        /// level structure (Array element, Intersection arm, Union
        /// arm, Object property, Tuple element)?
        ///
        /// Used by the Pick member-route materialiser to detect when
        /// descending into a member's leaf would walk through a
        /// callable parameter type — which, when the param root is
        /// package-backed, must be preserved symbolically rather than
        /// expanded as if it were prop metadata.
        fn type_expr_contains_callable_surface(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
        ) -> bool {
            use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
            match expr {
                TypeExpr::Function(_) => true,
                TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
                    ObjectMember::Property(p) => type_expr_contains_callable_surface(&p.ty),
                    ObjectMember::CallSignature(_)
                    | ObjectMember::ConstructSignature(_)
                    | ObjectMember::Method(_) => true,
                    ObjectMember::IndexSignature(_) => false,
                }),
                TypeExpr::Array { element, .. } => type_expr_contains_callable_surface(element),
                TypeExpr::Tuple { elements, .. } => elements
                    .iter()
                    .any(|el| type_expr_contains_callable_surface(&el.ty)),
                TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                    arms.iter().any(type_expr_contains_callable_surface)
                }
                TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
                    type_expr_contains_callable_surface(inner)
                }
                _ => false,
            }
        }

        /// Issue #10 / extract the raw type of `member`
        /// from `raw_body` when `raw_body` is an Object surface (the
        /// resolved body of the picked alias). Returns `None` when
        /// `raw_body` is not an Object or no property matches.
        fn raw_pick_member_leaf(
            raw_body: &verter_semantic::analysis::type_expr::TypeExpr,
            member: &str,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
            match raw_body {
                TypeExpr::Parenthesized(inner) => raw_pick_member_leaf(inner, member),
                TypeExpr::Object(object) => object.properties.iter().find_map(|m| match m {
                    ObjectMember::Property(p) if p.name == member => Some(p.ty.clone()),
                    _ => None,
                }),
                _ => None,
            }
        }

        /// Issue #10 / predicate: does `param.ty` resolve
        /// to a package-backed declaration?
        ///
        /// Walks every `TypeExpr::Ref { name }` rooted in the
        /// parameter type, lowers each to a `SemanticNodeId` via the
        /// project's dispatch, and returns `true` iff any of those
        /// roots resolves to a package-backed declaration (per the
        /// graph-native `is_package_backed_ref` predicate, which
        /// delegates to `canonical_resolves_to_package`).
        fn callable_param_root_is_package_backed(
            param_ty: &verter_semantic::analysis::type_expr::TypeExpr,
            ctx: &dyn crate::resolver_core::ResolverContext,
            scope_canonical_id: &str,
        ) -> bool {
            use crate::component_meta_materialize::is_package_backed_ref;
            use crate::project_semantic_dispatch::ProjectSemanticDispatch;
            use crate::semantic_query::ProjectionMode;
            use verter_semantic::analysis::type_expr::TypeExpr;
            fn collect_root_refs<'a>(expr: &'a TypeExpr, out: &mut Vec<&'a TypeExpr>) {
                match expr {
                    TypeExpr::Ref { .. } => out.push(expr),
                    TypeExpr::Parenthesized(inner) => collect_root_refs(inner, out),
                    TypeExpr::Array { element, .. } => collect_root_refs(element, out),
                    TypeExpr::Tuple { elements, .. } => {
                        for el in elements.iter() {
                            collect_root_refs(&el.ty, out);
                        }
                    }
                    TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => {
                        for a in arms.iter() {
                            collect_root_refs(a, out);
                        }
                    }
                    _ => {}
                }
            }
            let mut roots: Vec<&TypeExpr> = Vec::new();
            collect_root_refs(param_ty, &mut roots);
            if roots.is_empty() {
                return false;
            }
            let dispatch = ProjectSemanticDispatch::new(ctx);
            roots.iter().any(|r| {
                dispatch
                    .lower_type_expr_in_scope_with_mode(
                        scope_canonical_id,
                        r,
                        ProjectionMode::Navigate,
                    )
                    .is_some_and(|node| is_package_backed_ref(ctx, node))
            })
        }

        /// Issue #10 / predicate: does the picked member's
        /// raw leaf contain a callable surface whose param root is
        /// package-backed? When this fires, the Pick member-route
        /// materialiser MUST bypass the registry indexed-access route
        /// and project the raw leaf directly so the package-backed
        /// callable parameter type stays symbolic.
        fn pick_member_route_should_skip_callable_descent(
            raw_leaf: &verter_semantic::analysis::type_expr::TypeExpr,
            ctx: &dyn crate::resolver_core::ResolverContext,
            scope_canonical_id: &str,
        ) -> bool {
            use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};
            // Visit every Function surface reachable from `raw_leaf`
            // and check ANY parameter root for package-backed-ness.
            fn any_callable_param_is_package_backed(
                expr: &TypeExpr,
                ctx: &dyn crate::resolver_core::ResolverContext,
                scope_canonical_id: &str,
            ) -> bool {
                match expr {
                    TypeExpr::Function(func) => {
                        for p in func.parameters.iter() {
                            if callable_param_root_is_package_backed(&p.ty, ctx, scope_canonical_id)
                            {
                                return true;
                            }
                        }
                        false
                    }
                    TypeExpr::Object(object) => object.properties.iter().any(|m| match m {
                        ObjectMember::Property(p) => {
                            any_callable_param_is_package_backed(&p.ty, ctx, scope_canonical_id)
                        }
                        ObjectMember::CallSignature(func)
                        | ObjectMember::ConstructSignature(func) => {
                            func.parameters.iter().any(|p| {
                                callable_param_root_is_package_backed(
                                    &p.ty,
                                    ctx,
                                    scope_canonical_id,
                                )
                            })
                        }
                        ObjectMember::Method(method) => {
                            method.function.parameters.iter().any(|p| {
                                callable_param_root_is_package_backed(
                                    &p.ty,
                                    ctx,
                                    scope_canonical_id,
                                )
                            })
                        }
                        ObjectMember::IndexSignature(_) => false,
                    }),
                    TypeExpr::Array { element, .. } => {
                        any_callable_param_is_package_backed(element, ctx, scope_canonical_id)
                    }
                    TypeExpr::Tuple { elements, .. } => elements.iter().any(|el| {
                        any_callable_param_is_package_backed(&el.ty, ctx, scope_canonical_id)
                    }),
                    TypeExpr::Union(arms) | TypeExpr::Intersection(arms) => arms
                        .iter()
                        .any(|a| any_callable_param_is_package_backed(a, ctx, scope_canonical_id)),
                    TypeExpr::Parenthesized(inner) | TypeExpr::Rest(inner) => {
                        any_callable_param_is_package_backed(inner, ctx, scope_canonical_id)
                    }
                    _ => false,
                }
            }
            type_expr_contains_callable_surface(raw_leaf)
                && any_callable_param_is_package_backed(raw_leaf, ctx, scope_canonical_id)
        }

        fn materialize_component_meta_registry_candidate_for_route(
            query_engine: &mut crate::resolver_core::ComponentMetaQueryEngine<'_>,
            scope_canonical_id: &str,
            symbol_name: &str,
            route: &crate::resolver_core::RouteDemand,
            raw_body: Option<&verter_semantic::analysis::type_expr::TypeExpr>,
            prefer_explicit_raw_surface: bool,
        ) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
            use verter_semantic::analysis::type_expr::{
                ObjectExpr, ObjectMember, ObjectProperty, TypeExpr,
            };

            match route {
                crate::resolver_core::RouteDemand::Whole => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                    materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )
                }
                crate::resolver_core::RouteDemand::MemberPath(path) => {
                    if let Some(projected) = raw_body.and_then(|expr| {
                        component_meta_registry_raw_member_path_surface(expr, path)
                    }) {
                        return Some(query_engine.materialize_member_surface_expr(
                            scope_canonical_id,
                            &projected,
                            true,
                        ));
                    }
                    let route_expr = build_registry_indexed_access_expr(symbol_name, path);
                    let leaf = project_expr_class_a_via_dispatch(
                        query_engine.ctx,
                        scope_canonical_id,
                        &route_expr,
                    )
                    .unwrap_or(route_expr);
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && component_meta_registry_has_non_object_top_level_surface(&leaf)
                        && matches!(leaf, TypeExpr::IndexedAccess { .. })
                    {
                        return None;
                    }
                    if path.len() > 1
                        && !component_meta_registry_has_explicit_object_surface(&leaf)
                        && !component_meta_registry_has_non_object_top_level_surface(&leaf)
                    {
                        return None;
                    }
                    Some(query_engine.materialize_member_surface_expr(
                        scope_canonical_id,
                        &wrap_registry_member_path_surface(path, leaf),
                        false,
                    ))
                }
                crate::resolver_core::RouteDemand::Pick(members) => {
                    let mut properties = Vec::new();
                    for member in members {
                        // Issue #10 / when the picked
                        // member's raw leaf contains a callable
                        // surface AND any callable parameter root
                        // resolves to a package-backed declaration,
                        // bypass the registry indexed-access route.
                        // Descending into a package-backed callable
                        // parameter (e.g. `(e, message: UIMessage) =>
                        // void` where `UIMessage` lives in `ai`)
                        // expands package internals into the
                        // consumer's prop surface and triggers
                        // unbounded recursion on cyclic external
                        // declarations. The raw leaf carries the
                        // symbolic Ref already; project it directly
                        // through `materialize_member_surface_expr`
                        // so package-backed param roots stay
                        // symbolic.
                        if let Some(raw_leaf) =
                            raw_body.and_then(|body| raw_pick_member_leaf(body, member.as_str()))
                        {
                            if pick_member_route_should_skip_callable_descent(
                                &raw_leaf,
                                query_engine.ctx,
                                scope_canonical_id,
                            ) {
                                let projected_leaf = query_engine.materialize_member_surface_expr(
                                    scope_canonical_id,
                                    &raw_leaf,
                                    true,
                                );
                                properties.push(ObjectMember::Property(ObjectProperty {
                                    name: member.clone(),
                                    ty: projected_leaf,
                                    optional: true,
                                    readonly: false,
                                }));
                                continue;
                            }
                        }
                        let member_route =
                            crate::resolver_core::RouteDemand::MemberPath(vec![member.clone()]);
                        let route_expr = build_registry_indexed_access_expr(
                            symbol_name,
                            std::slice::from_ref(member),
                        );
                        // The alias-body fallback was
                        //; B1's materialiser
                        // branch handles
                        // route shapes natively. The remaining
                        // surface-expr fallback covers non-route
                        // shapes.
                        //
                        // Migrate to dispatch
                        // (sub- D-T recipe: RouteDemand::MemberPath
                        // → Class A with path). The Class A helper handles
                        // the IndexedAccess route_expr through its
                        // registry-route fast-path internally; the previous
                        // `project_route_surface_expr` + fallback chain
                        // collapses to a single dispatch call.
                        let _ = &member_route; // route demand carrier retained for parity
                        let projected = project_expr_class_a_via_dispatch_threaded(
                            query_engine.ctx,
                            Some(query_engine),
                            scope_canonical_id,
                            &route_expr,
                        )
                        .unwrap_or(route_expr);
                        // Issue #10 / record actual descent
                        // into the indexed-access route. The
                        // package-backed suppression branch above bails
                        // before this point; reaching here means we
                        // walked the route-expr through dispatch and
                        // are about to materialise the projected
                        // surface (which, for callable members, walks
                        // through callable parameters).
                        if raw_body
                            .and_then(|body| raw_pick_member_leaf(body, member.as_str()))
                            .as_ref()
                            .is_some_and(type_expr_contains_callable_surface)
                        {
                            crate::capture_token::with_active_capture(|t| {
                                t.record_counter(
                                    crate::meta_resolve::PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER,
                                    1,
                                );
                            });
                        }
                        let member_surface = query_engine.materialize_member_surface_expr(
                            scope_canonical_id,
                            &projected,
                            true,
                        );
                        let stabilized_input = materialize_component_meta_type_expr_until_stable(
                            &member_surface,
                            scope_canonical_id,
                            crate::semantic_query::ProjectionMode::Expanded,
                            query_engine,
                        );
                        let stabilized_surface = query_engine.materialize_member_surface_expr(
                            scope_canonical_id,
                            &stabilized_input,
                            true,
                        );
                        let solved_surface = match &stabilized_surface {
                            TypeExpr::Ref { type_arguments, .. } if !type_arguments.is_empty() => {
                                let materialize_scope_canonical_id =
                                    select_imported_materialization_scope(
                                        &stabilized_surface,
                                        scope_canonical_id,
                                        query_engine,
                                    )
                                    .unwrap_or_else(|| scope_canonical_id.to_string());
                                // Migrate the generic-Ref
                                // instantiation to dispatch.
                                let expanded = instantiate_local_generic_ref_via_dispatch(
                                    query_engine.ctx,
                                    materialize_scope_canonical_id.as_str(),
                                    &stabilized_surface,
                                )
                                .unwrap_or_else(|| stabilized_surface.clone());
                                // Migrate the route-loop
                                // call to the Class A dispatch helper.
                                let solved_opt = project_expr_class_a_via_dispatch_threaded(
                                    query_engine.ctx,
                                    Some(query_engine),
                                    materialize_scope_canonical_id.as_str(),
                                    &expanded,
                                )
                                .or(Some(expanded));
                                solved_opt.map(|solved| {
                                    query_engine.materialize_member_surface_expr(
                                        materialize_scope_canonical_id.as_str(),
                                        &solved,
                                        true,
                                    )
                                })
                            }
                            TypeExpr::Mapped { .. } => {
                                // Migrate the route-loop
                                // call to the Class A dispatch helper.
                                let solved_opt = project_expr_class_a_via_dispatch_threaded(
                                    query_engine.ctx,
                                    Some(query_engine),
                                    scope_canonical_id,
                                    &stabilized_surface,
                                );
                                solved_opt.map(|solved| {
                                    query_engine.materialize_member_surface_expr(
                                        scope_canonical_id,
                                        &solved,
                                        true,
                                    )
                                })
                            }
                            _ => None,
                        };
                        let best_surface = if let Some(solved_surface) = solved_surface {
                            if compare_type_expr_improvement(&solved_surface, &stabilized_surface) {
                                solved_surface
                            } else {
                                stabilized_surface
                            }
                        } else {
                            stabilized_surface
                        };
                        properties.push(ObjectMember::Property(ObjectProperty {
                            name: member.clone(),
                            ty: if compare_type_expr_improvement(&best_surface, &member_surface) {
                                best_surface
                            } else {
                                member_surface
                            },
                            optional: true,
                            readonly: false,
                        }));
                    }
                    (!properties.is_empty())
                        .then(|| TypeExpr::Object(std::sync::Arc::new(ObjectExpr { properties })))
                        .or_else(|| {
                            // Migrate route-target
                            // (RouteDemand::Pick) via D-T recipe: dispatch
                            // through `execute_pick` (sub- D-T).
                            // The pick_via_dispatch_pick_helper resolves the
                            // symbol to a base node via Class A lowering, then
                            // dispatches `Pick<base, key_set>` through the
                            // builtin Pick utility path; falls back to the
                            // raw materialiser candidate for non-Object bases.
                            pick_via_dispatch_pick_helper(
                                query_engine,
                                scope_canonical_id,
                                symbol_name,
                                members.as_slice(),
                            )
                            .map(|projected| {
                                query_engine.materialize_member_surface_expr(
                                    scope_canonical_id,
                                    &projected,
                                    true,
                                )
                            })
                            .or_else(|| {
                                materialize_component_meta_registry_candidate(
                                    query_engine,
                                    scope_canonical_id,
                                    symbol_name,
                                    raw_body,
                                    prefer_explicit_raw_surface,
                                )
                            })
                        })
                }
                crate::resolver_core::RouteDemand::Omit(omitted) => {
                    let materialized = materialize_component_meta_registry_candidate(
                        query_engine,
                        scope_canonical_id,
                        symbol_name,
                        raw_body,
                        prefer_explicit_raw_surface,
                    )?;
                    Some(match materialized {
                        TypeExpr::Object(object) => {
                            let omitted: rustc_hash::FxHashSet<_> =
                                omitted.iter().map(String::as_str).collect();
                            TypeExpr::Object(std::sync::Arc::new(ObjectExpr {
                                properties: object
                                    .properties
                                    .iter()
                                    .filter(|member| match member {
                                        ObjectMember::Property(property) => {
                                            !omitted.contains(property.name.as_str())
                                        }
                                        _ => true,
                                    })
                                    .cloned()
                                    .collect(),
                            }))
                        }
                        other => other,
                    })
                }
            }
        }
        fn collect_imported_component_meta_registry_seed_refs(
            expr: &verter_semantic::analysis::type_expr::TypeExpr,
            published_names: &rustc_hash::FxHashSet<String>,
            queued_names: &mut rustc_hash::FxHashSet<String>,
            output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            source_hint: Option<&str>,
        ) {
            use verter_semantic::analysis::type_expr::{ObjectMember, TypeExpr};

            fn drain_filtered_pending(
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                pending: std::collections::VecDeque<PendingComponentMetaRegistryRef>,
            ) {
                for pending in pending {
                    if matches!(
                        pending.route,
                        crate::resolver_core::RouteDemand::MemberPath(ref path) if path.len() > 1,
                    ) {
                        continue;
                    }
                    enqueue_component_meta_registry_ref(
                        published_names,
                        queued_names,
                        output,
                        pending.name.as_str(),
                        pending.source_hint.as_deref(),
                        pending.exported_name.as_deref(),
                        pending.route,
                    );
                }
            }

            fn collect_one_filtered_expr(
                expr: &verter_semantic::analysis::type_expr::TypeExpr,
                published_names: &rustc_hash::FxHashSet<String>,
                queued_names: &mut rustc_hash::FxHashSet<String>,
                output: &mut std::collections::VecDeque<PendingComponentMetaRegistryRef>,
                source_hint: Option<&str>,
            ) {
                let mut local_queue = std::collections::VecDeque::new();
                let mut local_names = rustc_hash::FxHashSet::default();
                collect_component_meta_registry_refs(
                    expr,
                    published_names,
                    &mut local_names,
                    &mut local_queue,
                    source_hint,
                    false,
                );
                drain_filtered_pending(published_names, queued_names, output, local_queue);
            }

            match expr {
                TypeExpr::Object(obj) => {
                    for member in &obj.properties {
                        match member {
                            ObjectMember::Property(prop) => collect_one_filtered_expr(
                                &prop.ty,
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                            ObjectMember::IndexSignature(sig) => {
                                collect_one_filtered_expr(
                                    &sig.key_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                );
                                collect_one_filtered_expr(
                                    &sig.value_type,
                                    published_names,
                                    queued_names,
                                    output,
                                    source_hint,
                                );
                            }
                            ObjectMember::CallSignature(func)
                            | ObjectMember::ConstructSignature(func) => collect_one_filtered_expr(
                                &TypeExpr::Function(func.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                            ObjectMember::Method(method) => collect_one_filtered_expr(
                                &TypeExpr::Function(method.function.clone().into()),
                                published_names,
                                queued_names,
                                output,
                                source_hint,
                            ),
                        }
                    }
                }
                TypeExpr::Function(func) => collect_one_filtered_expr(
                    &TypeExpr::Function(func.clone()),
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                ),
                _ => collect_one_filtered_expr(
                    expr,
                    published_names,
                    queued_names,
                    output,
                    source_hint,
                ),
            }
        }
        let debug_enabled = crate::host_manage::component_meta_debug_enabled();
        let import_refresh_started = debug_enabled.then(Instant::now);
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started = debug_enabled.then(Instant::now);
            let Some(meta) = resolved_type_registry_meta.get_mut(index) else {
                continue;
            };
            let declaration_source = meta.declaration.canonical_source.clone();
            if declaration_source.is_empty() || declaration_source == owner_canonical {
                continue;
            }
            track_component_meta_dependency(
                tracked_dependencies,
                owner_canonical,
                declaration_source.as_str(),
            );
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
                continue;
            }
            let requested_exported_name = if meta.declaration.resolved_name.is_empty() {
                entry.name.as_str()
            } else {
                meta.declaration.resolved_name.as_str()
            };
            let Some(resolved) = query_engine.resolve_imported_registry_symbol(
                declaration_source.as_str(),
                requested_exported_name,
            ) else {
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
            if imported_registry_alias_should_stay_symbolic(&resolved.body) {
                entry.type_expr =
                    verter_semantic::analysis::type_expr::TypeExpr::named(entry.name.clone());
                continue;
            }
            let materialized = materialize_component_meta_registry_candidate(
                query_engine,
                resolved.canonical_id.as_str(),
                resolved.exported_name.as_str(),
                Some(&resolved.body),
                true,
            )
            .unwrap_or_else(|| resolved.body.clone());
            entry.type_expr = merge_component_meta_registry_candidates(
                Some(entry.type_expr.clone()),
                Some(materialized),
            )
            .unwrap_or_else(|| entry.type_expr.clone());
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_UPDATE owner={} name={} source={} resolved={} elapsed_ms={:.1}",
                        owner_canonical,
                        entry.name,
                        declaration_source,
                        meta.declaration.resolved_name,
                        elapsed_ms,
                    ));
                }
            }
        }
        let import_refresh_elapsed_ms = import_refresh_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        let mut referenced_names: VecDeque<PendingComponentMetaRegistryRef> = VecDeque::new();
        let mut queued_names = rustc_hash::FxHashSet::default();
        let mut published_names: rustc_hash::FxHashSet<String> = resolved_type_registry
            .iter()
            .map(|entry| entry.name.clone())
            .collect();
        let public_field_collect_started = debug_enabled.then(Instant::now);
        if let Some(evaluated_types) = evaluated_types {
            for field in &evaluated_types.props {
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    snapshot,
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
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
            let define_props_roots = collect_define_props_root_names(snapshot);
            for field in &evaluated_types.slot_bindings {
                if slot_binding_targets_define_props_root(field, &define_props_roots) {
                    crate::capture_token::with_active_capture(|t| {
                        t.record_counter(
                            crate::meta_resolve::SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER,
                            1,
                        );
                    });
                    continue;
                }
                collect_component_meta_registry_public_field_refs(
                    self,
                    owner_canonical,
                    snapshot,
                    field,
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    Some(owner_canonical),
                );
            }
        }
        let public_field_collect_elapsed_ms = public_field_collect_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        let seed_scan_started = debug_enabled.then(Instant::now);
        for (index, entry) in resolved_type_registry.iter().enumerate() {
            let Some(meta) = resolved_type_registry_meta.get(index) else {
                continue;
            };
            let source_hint = Some(meta.declaration.canonical_source.as_str());
            let entry_import_root = owner_component_meta_registry_import_root(
                self,
                owner_canonical,
                snapshot,
                entry.name.as_str(),
            );
            let entry_is_imported = entry_import_root.as_ref().is_some_and(|(canonical_id, _)| {
                !canonical_id.is_empty() && canonical_id != owner_canonical
            }) || (!meta.declaration.canonical_source.is_empty()
                && meta.declaration.canonical_source != owner_canonical);
            if should_skip_imported_registry_seed_refresh(
                owner_canonical,
                &meta.declaration,
                &entry.type_expr,
            ) {
                continue;
            }
            let source_expr = source_hint
                .filter(|source| source.is_empty() || *source == owner_canonical)
                .and_then(|_| {
                    query_engine.owner_collection_expr(owner_canonical, entry.name.as_str())
                });
            if entry_is_imported {
                collect_imported_component_meta_registry_seed_refs(
                    source_expr.as_ref().unwrap_or(&entry.type_expr),
                    &published_names,
                    &mut queued_names,
                    &mut referenced_names,
                    source_hint,
                );
            } else {
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
        let seed_scan_elapsed_ms = seed_scan_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();

        // Names referenced from already-seeded registry entries.
        // Helpers that a published type transitively references should
        // still be published even when they are imported generic aliases.
        let seeded_dependency_names: rustc_hash::FxHashSet<String> = {
            let mut names = rustc_hash::FxHashSet::default();
            for entry in resolved_type_registry.iter() {
                collect_type_expr_ref_names(&entry.type_expr, &mut names);
            }
            // Also include owner-local names queued alongside a seeded
            // published entry. When the registry already has published
            // entries, any owner-local pending name was transitively
            // enqueued through seed scanning and must keep its own
            // registry entry instead of being inlined as an indexed-access
            // alias. When there are no published entries yet, pending
            // names come purely from public-field scanning and may still
            // be inlined; do not protect them here.
            if !published_names.is_empty() {
                for pending in referenced_names.iter() {
                    if pending
                        .source_hint
                        .as_deref()
                        .is_none_or(|s| s.is_empty() || s == owner_canonical)
                    {
                        names.insert(pending.name.clone());
                    }
                }
            }
            names
        };
        let mut _loop_iterations: usize = 0;
        let mut _loop_materializations: usize = 0;
        let _loop_start = Instant::now();
        while let Some(pending) = referenced_names.pop_front() {
            _loop_iterations += 1;
            if !query_engine.allow_registry_deepening() {
                break;
            }
            let _pending_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
            let PendingComponentMetaRegistryRef {
                name: type_name,
                source_hint: pending_source_hint_owned,
                exported_name: pending_exported_name_owned,
                route: pending_route,
            } = pending;
            let imported_owner_route = owner_component_meta_registry_import_root(
                self,
                owner_canonical,
                snapshot,
                type_name.as_str(),
            )
            .filter(|_| {
                pending_source_hint_owned
                    .as_deref()
                    .is_none_or(|source| source.is_empty() || source == owner_canonical)
            });
            let pending_source_hint = imported_owner_route
                .as_ref()
                .map(|(canonical_id, _)| canonical_id.as_str())
                .or(pending_source_hint_owned.as_deref());
            let pending_exported_name = imported_owner_route
                .as_ref()
                .map(|(_, exported_name)| exported_name.as_str())
                .or(pending_exported_name_owned.as_deref());
            if matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                && imported_owner_route
                    .as_ref()
                    .is_some_and(|(canonical_id, _)| canonical_id.contains("/node_modules/"))
            {
                continue;
            }
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
            let _can_resolve = query_engine.can_resolve_registry_symbol(
                owner_canonical,
                pending_exported_name.unwrap_or(type_name.as_str()),
                pending_source_hint,
            );
            if crate::host_manage::component_meta_debug_enabled() && !_can_resolve {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_SKIP_UNRESOLVABLE owner={} name={} source_hint={:?} exported={:?}",
                    owner_canonical, type_name, pending_source_hint, pending_exported_name,
                ));
            }
            if !_can_resolve {
                continue;
            }
            let requested_exported_name = pending_exported_name.unwrap_or(type_name.as_str());
            if let Some(source_hint) = pending_source_hint
                .filter(|source| !source.is_empty() && *source != owner_canonical)
            {
                if !query_engine.allow_imported_root() {
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_SKIP_BUDGET owner={} name={}",
                            owner_canonical, type_name,
                        ));
                    }
                    continue;
                }
                track_component_meta_dependency(tracked_dependencies, owner_canonical, source_hint);
                let _imported_pending_started =
                    crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                let _resolved_import = query_engine
                    .resolve_imported_registry_symbol(source_hint, requested_exported_name);
                if crate::host_manage::component_meta_debug_enabled() && _resolved_import.is_none()
                {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_IMPORT_MISS owner={} name={} source={} exported={}",
                        owner_canonical, type_name, source_hint, requested_exported_name,
                    ));
                }
                if let Some(resolved) = _resolved_import {
                    let imported_resolve_elapsed_ms = _imported_pending_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
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
                    let declaration_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let mut declaration =
                        if matches!(pending_route, crate::resolver_core::RouteDemand::Whole) {
                            query_engine.resolve_type_declaration(
                                resolved.canonical_id.as_str(),
                                resolved.exported_name.as_str(),
                            )
                        } else {
                            query_engine
                                .resolve_direct_prepared_type_declaration_metadata(
                                    resolved.canonical_id.as_str(),
                                    resolved.exported_name.as_str(),
                                )
                                .unwrap_or_else(|| {
                                    query_engine.resolve_type_declaration(
                                        resolved.canonical_id.as_str(),
                                        resolved.exported_name.as_str(),
                                    )
                                })
                        };
                    let declaration_elapsed_ms = declaration_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
                    if declaration.canonical_source.is_empty() {
                        declaration.canonical_source = resolved.canonical_id.clone();
                    }
                    let pending_route_is_whole = match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => true,
                        crate::resolver_core::RouteDemand::MemberPath(path) => path.is_empty(),
                        _ => false,
                    };
                    if crate::host_manage::component_meta_debug_enabled() {
                        crate::host_manage::component_meta_debug(format!(
                            "REGISTRY_IMPORTED_GATE owner={} name={} stay_symbolic={} route_whole={} body_variant={:?}",
                            owner_canonical, type_name,
                            imported_registry_alias_should_stay_symbolic(&resolved.body),
                            pending_route_is_whole,
                            std::mem::discriminant(&resolved.body),
                        ));
                    }
                    if pending_route_is_whole
                        && imported_registry_alias_should_stay_symbolic(&resolved.body)
                    {
                        // Imported non-object helpers (mapped/conditional/
                        // indexed-access/typeof aliases) must not be expanded
                        // into the owner registry on a whole-type route — the
                        // consumer will resolve them through member paths.
                        //
                        // If we already published a richer entry under this
                        // name, refresh its declaration metadata (the merge in
                        // upsert_component_meta_registry_entry keeps the
                        // richer body, so the bare Named placeholder is
                        // discarded by `merge_component_meta_registry_candidates`).
                        //
                        // If the name was never published, skip publication
                        // entirely — a bare Named placeholder only leaks a
                        // symbolic helper that the consumer didn't ask for.
                        if published_names.contains(&type_name) {
                            upsert_component_meta_registry_entry(
                                owner_canonical,
                                resolved_type_registry,
                                resolved_type_registry_meta,
                                &mut published_names,
                                &mut queued_names,
                                &mut referenced_names,
                                type_name.clone(),
                                verter_semantic::analysis::type_expr::TypeExpr::named(
                                    type_name.clone(),
                                ),
                                declaration,
                                None,
                            );
                        }
                        continue;
                    }
                    track_component_meta_dependency(
                        tracked_dependencies,
                        owner_canonical,
                        declaration.canonical_source.as_str(),
                    );
                    let surface_started =
                        crate::host_manage::component_meta_debug_enabled().then(Instant::now);
                    let type_expr = materialize_component_meta_registry_candidate_for_route(
                        query_engine,
                        resolved.canonical_id.as_str(),
                        resolved.exported_name.as_str(),
                        &pending_route,
                        Some(&resolved.body),
                        true,
                    )
                    .or_else(|| match &pending_route {
                        crate::resolver_core::RouteDemand::Whole => Some(resolved.body.clone()),
                        crate::resolver_core::RouteDemand::MemberPath(path) if path.is_empty() => {
                            Some(resolved.body.clone())
                        }
                        _ => None,
                    });
                    let Some(type_expr) = type_expr else {
                        continue;
                    };
                    let surface_elapsed_ms = surface_started
                        .map(|started| started.elapsed().as_secs_f64() * 1000.0)
                        .unwrap_or_default();
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
                    if let Some(started) = _pending_started {
                        let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                        if total_elapsed_ms >= 5.0 {
                            crate::host_manage::component_meta_debug(format!(
                                "REGISTRY_PENDING_IMPORTED owner={} name={} source={} resolved={} resolve_ms={:.1} declaration_ms={:.1} surface_ms={:.1} total_ms={:.1}",
                                owner_canonical,
                                type_name,
                                source_hint,
                                resolved.canonical_id,
                                imported_resolve_elapsed_ms,
                                declaration_elapsed_ms,
                                surface_elapsed_ms,
                                total_elapsed_ms,
                            ));
                        }
                    }
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
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    declaration_owner,
                    type_name.as_str(),
                    &pending_route,
                    declaration_body.as_ref(),
                    true,
                )
            } else {
                None
            };
            let owner_collection_expr =
                query_engine.owner_collection_expr(owner_canonical, type_name.as_str());
            // Owner-local type aliases whose body is a generic ref to an
            // imported type should resolve inline via indexed access rather
            // than creating a separate registry entry.
            let pending_route_is_whole_local =
                matches!(pending_route, crate::resolver_core::RouteDemand::Whole)
                    || matches!(
                        pending_route,
                        crate::resolver_core::RouteDemand::MemberPath(ref p) if p.is_empty(),
                    );
            if declaration_owner == owner_canonical
                && !pending_route_is_whole_local
                && !seeded_dependency_names.contains(&type_name)
            {
                if let Some(verter_semantic::analysis::type_expr::TypeExpr::Ref {
                    name: body_ref_name,
                    type_arguments,
                }) = owner_collection_expr.as_ref()
                {
                    if !type_arguments.is_empty() {
                        let body_decl =
                            query_engine.resolve_type_declaration(owner_canonical, body_ref_name);
                        let body_scope = if body_decl.canonical_source.is_empty() {
                            owner_canonical
                        } else {
                            body_decl.canonical_source.as_str()
                        };
                        if body_scope != owner_canonical {
                            continue;
                        }
                    }
                }
            }
            // Owner-local generic aliases publish the full shape so all
            // members (including those from deep indexed-access paths that
            // were already resolved inline) appear in the registry entry.
            let effective_local_route =
                if declaration_owner == owner_canonical && !pending_route_is_whole_local {
                    if owner_collection_expr.as_ref().is_some_and(|expr| {
                        matches!(
                            expr,
                            verter_semantic::analysis::type_expr::TypeExpr::Ref {
                                type_arguments, ..
                            } if !type_arguments.is_empty()
                        )
                    }) {
                        crate::resolver_core::RouteDemand::Whole
                    } else {
                        pending_route.clone()
                    }
                } else {
                    pending_route.clone()
                };
            materialized = materialized.or_else(|| {
                materialize_component_meta_registry_candidate_for_route(
                    query_engine,
                    owner_canonical,
                    type_name.as_str(),
                    &effective_local_route,
                    owner_collection_expr.as_ref(),
                    true,
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
            let collection_expr = if owner_collection_expr.as_ref().is_some_and(|expr| {
                !component_meta_registry_has_explicit_object_surface(expr)
                    && component_meta_registry_has_explicit_object_surface(&materialized)
            }) {
                Some(materialized.clone())
            } else {
                owner_collection_expr.clone()
            };
            if crate::host_manage::component_meta_debug_enabled() {
                crate::host_manage::component_meta_debug(format!(
                    "REGISTRY_PENDING_LOCAL_SURFACE owner={} name={} route={:?} materialized={:?}",
                    owner_canonical, type_name, pending_route, materialized
                ));
            }
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
                collection_expr.as_ref(),
            );
            if let Some(started) = _pending_started {
                let total_elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if total_elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_PENDING_LOCAL owner={} name={} declaration_owner={} route={:?} total_ms={:.1}",
                        owner_canonical, type_name, declaration_owner, pending_route, total_elapsed_ms,
                    ));
                }
            }
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
        let loop_elapsed_ms = _loop_start.elapsed().as_secs_f64() * 1000.0;
        let enrich_started = debug_enabled.then(Instant::now);

        // Registry enrichment: materialize imported type expressions through
        // the shared request-scoped engine so projection/instantiation caches
        // are reused across all registry entries in one request.
        for (index, entry) in resolved_type_registry.iter_mut().enumerate() {
            let _entry_started =
                crate::host_manage::component_meta_debug_enabled().then(Instant::now);
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
            if component_meta_registry_has_non_object_top_level_surface(&entry.type_expr)
                && component_meta_registry_should_keep_raw_symbolic_non_object_alias(
                    &entry.type_expr,
                    scope_canonical,
                    query_engine,
                )
            {
                continue;
            }
            let raw_body = query_engine.named_decl_body(
                scope_canonical,
                if !meta.declaration.resolved_name.is_empty() {
                    meta.declaration.resolved_name.as_str()
                } else {
                    entry.name.as_str()
                },
            );
            let materialized = query_engine.materialize_member_surface_expr(
                scope_canonical,
                &entry.type_expr,
                false,
            );
            let preserved_nested_routes = raw_body
                .as_ref()
                .filter(|raw| type_expr_needs_nested_symbolic_route_preservation(raw))
                .map_or(materialized.clone(), |raw| {
                    preserve_nested_symbolic_member_routes(
                        &materialized,
                        raw,
                        scope_canonical,
                        query_engine,
                        false,
                    )
                });
            entry.type_expr = raw_body
                .as_ref()
                .map_or(preserved_nested_routes.clone(), |raw| {
                    preserve_registry_callable_param_member_routes(&preserved_nested_routes, raw)
                });
            if let Some(started) = _entry_started {
                let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
                if elapsed_ms >= 5.0 {
                    crate::host_manage::component_meta_debug(format!(
                        "REGISTRY_ENRICH_ENTRY owner={} name={} scope={} elapsed_ms={:.1}",
                        owner_canonical, entry.name, scope_canonical, elapsed_ms,
                    ));
                }
            }
        }
        let enrich_elapsed_ms = enrich_started
            .map(|started| started.elapsed().as_secs_f64() * 1000.0)
            .unwrap_or_default();
        if debug_enabled {
            crate::host_manage::component_meta_debug(format!(
                "PROFILE_PHASES owner={} import_refresh_ms={:.1} public_field_collect_ms={:.1} seed_scan_ms={:.1} loop_ms={:.1} enrich_ms={:.1}",
                owner_canonical,
                import_refresh_elapsed_ms,
                public_field_collect_elapsed_ms,
                seed_scan_elapsed_ms,
                loop_elapsed_ms,
                enrich_elapsed_ms,
            ));
        }
    }

    /// Get a raw analysis snapshot without any enrichment.
    ///
    /// For owner files in the scheduler, reads the scheduler's latest analysis
    /// (which reflects post-recompile state). For imported deps and non-scheduler
    /// files, reads from `IndexedReadyDb` (materializing on miss). Both paths enrich
    /// the snapshot with resolved imports, destructured bindings, and template
    /// analysis.
    pub(crate) fn get_raw_analysis_snapshot(
        &self,
        canonical: &str,
    ) -> Option<FileAnalysisSnapshot> {
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot",
            format!("owner={} store_view={}", canonical, false),
        );
        let normalized_canonical = self.normalized_analysis_canonical(canonical);
        let canonical = normalized_canonical.as_ref();

        {
            if self.is_canonical_evicted(canonical) {
                return None;
            }

            // Route-owned cache fast path for imported-only files: if we
            // already built a raw snapshot via the route-owned shallow state
            // pipeline, reuse it here instead of rebuilding from the
            // scheduler. This is gated on module_facts not holding it (= fully lazy).
            if self
                .project_type_store
                .indexed()
                .get_any(canonical)
                .is_none()
            {
                if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                    self.provenance
                        .route_owned_snapshot_cache_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let mut snapshot = (*raw_snapshot).clone();
                    self.resolve_snapshot_imports(canonical, &mut snapshot);
                    self.enrich_destructured_bindings(&mut snapshot);
                    if self.config.effective_scope().needs_template_analysis() {
                        self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                    }
                    return Some(snapshot);
                }
            }

            // Scheduler-first path for owner files: the scheduler has the
            // latest analysis after recompile, including updated import
            // routes for newly-added dependencies. IndexedReadyDb may hold
            // stale import routes for owner files whose deps changed after
            // materialization.
            if let Some(snapshot) = self.build_snapshot_from_scheduler(canonical) {
                let whole_hash = self
                    .current_or_read_whole_hash(canonical)
                    .unwrap_or_default();
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                let mut snapshot = snapshot;
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
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

        if self
            .project_type_store
            .indexed()
            .get_any(canonical)
            .is_none()
        {
            if let Some(raw_snapshot) = self.cached_route_owned_snapshot(canonical) {
                self.provenance
                    .route_owned_snapshot_cache_hits
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let mut snapshot = (*raw_snapshot).clone();
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_snapshot_cache",
                        canonical,
                        snapshot.imports.len(),
                        snapshot.macros.len(),
                        snapshot.bindings.len(),
                        snapshot.template.is_some(),
                    ),
                );
                return Some(snapshot);
            }

            if let Some((raw_source, cached_parse, whole_hash)) =
                self.cached_route_owned_eval_state(canonical)
            {
                if !self.store_view_allows_current_whole_hash(canonical, whole_hash) {
                    return None;
                }
                if cached_parse.is_some() {
                    self.provenance
                        .route_owned_snapshot_cached_parse_hits
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                let mut snapshot = self.build_snapshot_from_source_state(
                    canonical,
                    &raw_source,
                    cached_parse.as_deref(),
                );
                self.resolve_snapshot_imports(canonical, &mut snapshot);
                self.enrich_destructured_bindings(&mut snapshot);
                if self.config.effective_scope().needs_template_analysis() {
                    self.compute_template_analysis_if_missing(canonical, &mut snapshot);
                }
                component_meta_trace_custom!(
                    "get_raw_analysis_snapshot_result",
                    format!(
                        "owner={} imports={} macros={} bindings={} has_template={} source=route_owned_cache",
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

        // IndexedReadyDb path: covers imported deps and non-scheduler files.
        let facts = self.ensure_indexed_ready(canonical)?;
        let mut snapshot = (*facts.snapshot).clone();
        self.resolve_snapshot_imports(canonical, &mut snapshot);
        self.enrich_destructured_bindings(&mut snapshot);
        if self.config.effective_scope().needs_template_analysis() {
            self.compute_template_analysis_if_missing(canonical, &mut snapshot);
        }
        component_meta_trace_custom!(
            "get_raw_analysis_snapshot_result",
            format!(
                "owner={} imports={} macros={} bindings={} has_template={} source=indexed_ready",
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
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        let cache_key = resolved_meta_cache_key(canonical, mode);
        let view_for_get = self.resolver_store_view();
        if let Some(cached) = self
            .resolver_runtime()
            .component_meta
            .get_if_valid(&cache_key, &view_for_get)
        {
            self.mirror_cached_resolved_meta_arc(canonical, mode, cached.clone());
            return Some(cached.as_ref().clone());
        }

        // cached_resolved_meta lives on DerivedRawState (D48 split).
        let entry = self.derived_raw_cache().get(canonical)?;
        let cached = entry.cached_resolved_meta.get(&mode)?;
        let view = self.resolver_store_view();
        let invalid_details = view.invalid_fact_details(&cached.fact_versions, 6);
        if !invalid_details.is_empty() {
            component_meta_trace_custom!(
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

    pub(crate) fn store_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: &ResolvedComponentMetaState,
        fact_versions: &[crate::resolver_core::FactVersionRef],
    ) {
        component_meta_trace_custom!(
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

    pub(crate) fn mirror_cached_resolved_meta_arc(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        state: Arc<ResolvedComponentMetaState>,
    ) {
        let cached = crate::types::ResolvedComponentMetaCacheEntry {
            fact_versions: state.fact_versions.clone(),
            state,
        };

        // cached_resolved_meta lives on DerivedRawState (D48 split).
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical.to_string())
                .or_default();
            derived_ref
                .value_mut()
                .cached_resolved_meta
                .insert(mode, cached);
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // Encoded payload cache (shared by NAPI/WASM)
    // ───────────────────────────────────────────────────────────────────────

    /// Try to return a cached encoded payload for the canonical
    /// component-meta query. Validates fact versions against the live host
    /// state.
    pub(crate) fn try_get_cached_meta_payload(&self, canonical: &str) -> Option<Vec<u8>> {
        use crate::resolver_core::StoreView;
        // cached_meta_payload lives on DerivedRawState (D48 split).
        let entry = self.derived_raw_cache().get(canonical)?;
        let cached = entry.cached_meta_payload.as_ref()?;
        let view = self.resolver_store_view();
        if cached.fact_versions.iter().all(|fact| view.validates(fact)) {
            return Some(cached.payload.clone());
        }
        None
    }

    /// Store an encoded payload in the per-file cache.
    pub(crate) fn store_meta_payload(
        &self,
        canonical: &str,
        fact_versions: &[crate::resolver_core::FactVersionRef],
        payload: Vec<u8>,
    ) {
        let cached = crate::types::CachedMetaPayload {
            fact_versions: fact_versions.to_vec(),
            payload,
        };

        // cached_meta_payload lives on DerivedRawState (D48 split).
        {
            let mut derived_ref = self
                .derived_raw_cache()
                .entry(canonical.to_string())
                .or_default();
            derived_ref.value_mut().cached_meta_payload = Some(cached);
        }
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn current_dependency_fact_versions(
        &self,
        canonical: &str,
        tracked_deps: &std::collections::BTreeSet<String>,
    ) -> Vec<crate::resolver_core::FactVersionRef> {
        let mut facts = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        self.append_dependency_fact_versions(canonical, &mut facts, &mut seen);
        for dep in tracked_deps {
            self.append_dependency_fact_versions(dep.as_str(), &mut facts, &mut seen);
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

    pub(crate) fn append_dependency_fact_versions(
        &self,
        canonical: &str,
        facts: &mut Vec<crate::resolver_core::FactVersionRef>,
        seen: &mut rustc_hash::FxHashSet<crate::resolver_core::FactVersionRef>,
    ) {
        if let Some(hash) = self.current_or_read_whole_hash(canonical) {
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
            if let Some(hash) = self.current_derived_fact_hash(canonical, kind) {
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

    pub(crate) fn current_derived_fact_hash(
        &self,
        canonical_id: &str,
        kind: crate::resolver_core::DerivedFactKind,
    ) -> Option<Hash16> {
        match kind {
            crate::resolver_core::DerivedFactKind::DirectSource => {
                self.current_or_read_whole_hash(canonical_id)
            }
            crate::resolver_core::DerivedFactKind::Route => {
                // Step 8 / F5: read the cached `route_hash` from
                // IndexedReady when available — symmetric to
                // `import_route_hash`. Falls back to recomputing via
                // `hash_route_surface` only when the canonical isn't
                // yet indexed (read-only: this code path must NOT
                // call ensure_indexed because fact validation is
                // side-effect-free). Same content-hash invalidation
                // lifecycle as IndexedReady itself, so the cached
                // hash is current as long as the entry is.
                if let Some(cached) = self
                    .project_type_store
                    .indexed()
                    .get_any(canonical_id)
                    .and_then(|facts| facts.route_hash)
                {
                    return Some(cached);
                }
                let state = self.shallow_file_state(canonical_id)?;
                state
                    .has_resolvable_surface()
                    .then(|| crate::resolver_store::hash_route_surface(&state))
            }
            crate::resolver_core::DerivedFactKind::ImportRoute => {
                // Read-only: ImportRoute fact capture must not promote a
                // shallow-only tracked dependency into full IndexedReady.
                self.current_cached_import_route_hash(canonical_id)
            }
        }
    }

    pub(crate) fn current_cached_import_route_hash(&self, canonical_id: &str) -> Option<Hash16> {
        self.project_type_store
            .indexed()
            .get_any(canonical_id)
            .and_then(|facts| facts.import_route_hash)
            .or_else(|| {
                {
                    // import_routes lives on DerivedRawState (D48 split).
                    self.derived_raw_cache()
                        .get(canonical_id)
                        .and_then(|entry| {
                            (!entry.import_routes.is_empty()).then(|| {
                                crate::resolver_store::hash_import_route_targets(
                                    &entry.import_routes,
                                )
                            })
                        })
                }
            })
    }
}
