//! Shared materialization and resolved-meta owner for component-meta.
//!
//! This module owns:
//! - mode selection (`ProjectionMode::Identity` vs `ProjectionMode::Expanded`)
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
    component_meta_debug, component_meta_debug_enabled, component_meta_trace_custom,
};
use crate::resolver_core::{
    run_component_meta_request, ComponentMetaEvalOutputs, ComponentMetaRequestHost, RequestSource,
    SingleflightRole,
};
use crate::types::{FileAnalysisSnapshot, Hash16, ProjectionMode};
use crate::VerterHost;
use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

use verter_semantic::analysis::types::AnalyzedMacro;

pub(crate) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

// Phase 11a sub-module split — siblings live in `crates/verter_session/src/meta_resolve/`.
// The shell re-exports the moved `pub(crate)` surface so existing
// `crate::meta_resolve::*` paths keep working without callsite churn.
mod dep_signature;
mod dispatch_helpers;
mod field_state;
mod graph_predicates;
mod host_methods;
mod macro_member_walk;
mod materialize;
mod registry_materialize;
mod request_host;
mod resolved_state;
mod scoring;
pub(crate) use dep_signature::{
    accumulate_dispatch_dep_signature, drain_dispatch_dep_signature_accumulator,
    reset_dispatch_dep_signature_accumulator,
};
#[cfg(test)]
pub(crate) use dep_signature::{
    bfs_compute_counter_for_test, record_bfs_child_refs_count_for_test,
    reset_bfs_compute_counter_for_test, with_bfs_child_refs_observer_for_test, with_visited_counter,
};
pub(crate) use dispatch_helpers::{
    instantiate_local_generic_ref_via_dispatch, lower_and_project_to_expanded_via_host_threaded,
    pick_via_dispatch_pick_helper, project_expr_class_a_shape_via_dispatch,
    project_expr_class_a_shape_via_dispatch_threaded, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded,
    project_expr_surface_expr_via_host_threaded,
    project_expr_surface_expr_with_compound_objects_via_host_threaded,
    project_expr_surface_shape_via_host_threaded,
    project_prepared_type_surface_expr_via_host_threaded,
    project_prepared_type_surface_shape_via_host,
    project_prepared_type_surface_shape_via_host_threaded,
    project_route_surface_expr_via_host_threaded, project_type_surface_expr_via_host,
    project_type_surface_expr_via_host_threaded, project_type_surface_shape_via_host,
    project_type_surface_shape_via_host_threaded,
};
pub(crate) use field_state::MacroFieldGraphState;
#[cfg(test)]
pub(crate) use field_state::{dispatch_lower_counter_get, dispatch_lower_counter_reset};
pub use request_host::{
    CapturedComponentMetaInputs, ResolvedComponentMetaComputeAudit, ResolvedDeclarationKind,
    ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta, ResolvedNativeProp,
    ResolvedTypeDeclaration, ResolvedTypeRegistryMeta, SessionRequestHost,
};
pub(crate) use request_host::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    resolved_meta_cache_key, should_skip_imported_registry_seed_refresh, trace_request_source,
};
pub use resolved_state::{ResolvedComponentMetaState, SurfaceNodeIdentities};
pub(crate) use resolved_state::{
    collect_expanded_slot_binding_param_types, collect_expanded_slot_bindings_from_object_type,
    component_meta_owner_local_shallow_substituted_alias_body, component_meta_substitute_typeexpr,
    decide_typeexpr_conditional_with_function_extends, enrich_missing_slot_bindings,
    lowered_root_reaches_transitive_cycle, select_imported_materialization_scope,
    substitute_infer_in_typeexpr, walk_substitute_typeexpr, RegistryMaterialization,
};
pub(crate) use graph_predicates::{
    build_keys_union_node, canonical_resolves_to_package, collect_ref_identities_node,
    component_meta_ref_resolves_to_package_node,
    declaration_body_prefers_inline_materialization_node, extract_route_root_identity_node,
    node_has_non_object_top_level_surface, ref_root_reaches_transitive_cycle_node,
    slot_binding_param_can_stay_symbolic_node, type_node_has_package_backed_root,
    type_node_needs_member_route_materialization, RouteExtraction,
};
pub(crate) use macro_member_walk::{
    materialize_component_meta_macro_shape_member_type_expr,
    walk_component_meta_macro_shape_member_types,
};
pub(crate) use registry_materialize::{
    component_meta_registry_prefers_structural_materialization_node,
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    materialize_component_meta_registry_structural_expr,
    nested_symbolic_member_route_should_stay_symbolic,
    preserve_nested_symbolic_member_routes, preserve_package_backed_symbolic_refs_node,
    preserve_registry_callable_param_member_routes, type_expr_contains_public_member_route,
    type_expr_needs_nested_symbolic_route_preservation,
};
pub(crate) use scoring::compare_type_expr_improvement;
use scoring::component_meta_registry_prefers_structural_materialization;
pub(crate) use materialize::{
    collect_type_expr_ref_names, define_props_fields_fast_path_allowed,
    define_props_member_can_stay_symbolic_without_rescue, expr_needs_projection_rescue,
    field_should_preserve_shallow_symbolic_raw_type, has_prop_shape_surface,
    lowered_needs_member_route_materialization,
    lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_field_types,
    materialize_component_meta_type_expr_until_stable,
    materialize_component_meta_type_expr_until_stable_full,
    parsed_field_raw_type, produce_macro_object_shapes,
    produce_macro_object_shapes_for_purpose, produce_one_macro_object_shape,
    projection_result_beats_solver_shape, registry_entry_to_expanded_shape,
    synthesize_define_props_shape_from_known_surface_with_authority,
    top_level_imported_ref_can_stay_symbolic,
    type_expr_has_non_object_top_level_surface,
    type_expr_has_package_backed_object_like_root,
    type_expr_is_slots_member_route, MacroShapeSource,
};
#[cfg(test)]
pub(crate) use materialize::{mtl_call_count_for_tests, reset_mtl_call_count_for_tests};



use crate::resolver_core::component_meta_registry::{
    collect_component_meta_registry_public_field_refs, collect_component_meta_registry_refs,
    component_meta_registry_expr_references_name,
    component_meta_registry_has_explicit_object_surface,
    component_meta_registry_has_non_object_top_level_surface,
    component_meta_registry_public_indexed_access_route,
    component_meta_registry_public_utility_route, component_meta_registry_raw_member_path_surface,
    enqueue_component_meta_registry_ref, merge_component_meta_registry_candidates,
    owner_component_meta_registry_import_root, upsert_component_meta_registry_entry,
    PendingComponentMetaRegistryRef,
};




// Plan §6.10 sub-task 4 / §4.19 — registry-route inline composition
// predicate deleted (verified callerless in production; the only
// consumer was a composition test that has also been deleted in this
// commit).

#[cfg_attr(feature = "hotpath", hotpath::measure)]
fn build_origin_graph(
    graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>,
    surface_identities: Option<&SurfaceNodeIdentities>,
) -> verter_protocol::types::OriginGraphDto {
    use crate::semantic_query::OriginEdgeKind;
    use rustc_hash::{FxHashMap, FxHashSet};
    use std::collections::VecDeque;
    use verter_protocol::types::{OriginEdgeDto, OriginGraphDto, OriginNodeDto};

    // Step 9.2 / F6 scoped origin export: when surface_identities are
    // populated, reverse-walk via walk_origin_chain starting from each
    // surface node and collect only the reachable subgraph. Falls back
    // to export_all_origin_edges when surface_identities is None
    // (audit-off path or pre-populated state).
    let all_edges = if let Some(ids) = surface_identities {
        let mut roots: Vec<crate::semantic_query::SemanticNodeId> = Vec::new();
        let push_some =
            |roots: &mut Vec<_>, opt: &Option<crate::semantic_query::SemanticNodeId>| {
                if let Some(id) = opt {
                    roots.push(*id);
                }
            };
        for id in &ids.prop_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.emit_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.slot_binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.binding_node_ids {
            push_some(&mut roots, id);
        }
        for id in &ids.registry_node_ids {
            push_some(&mut roots, id);
        }
        if roots.is_empty() {
            return OriginGraphDto::default();
        }
        let mut reached: FxHashSet<crate::semantic_query::SemanticNodeId> = FxHashSet::default();
        let mut worklist: VecDeque<crate::semantic_query::SemanticNodeId> =
            roots.into_iter().collect();
        let mut collected: Vec<(
            crate::semantic_query::SemanticNodeId,
            OriginEdgeKind,
            crate::semantic_query::OriginEdge,
        )> = Vec::new();
        while let Some(node) = worklist.pop_front() {
            if !reached.insert(node) {
                continue;
            }
            graph.walk_origin_chain(node, |kind, edge| {
                collected.push((node, kind, edge.clone()));
                for source in edge.sources.iter() {
                    if !reached.contains(source) {
                        worklist.push_back(*source);
                    }
                }
            });
        }
        collected
    } else {
        graph.export_all_origin_edges()
    };

    if all_edges.is_empty() {
        return OriginGraphDto::default();
    }

    let mut node_index: FxHashMap<crate::semantic_query::SemanticNodeId, u32> =
        FxHashMap::default();
    let mut nodes: Vec<OriginNodeDto> = Vec::new();
    let mut meta_strings: Vec<String> = Vec::new();
    let mut meta_index_map: FxHashMap<String, u32> = FxHashMap::default();

    let mut intern_node = |id: crate::semantic_query::SemanticNodeId,
                           graph: &Arc<crate::semantic_query_memo::SemanticGraphStore>|
     -> u32 {
        if let Some(&idx) = node_index.get(&id) {
            return idx;
        }
        let idx = nodes.len() as u32;
        let (kind, label) = graph
            .node_data(id)
            .map(|d| {
                use crate::semantic_query::SemanticNodeData;
                let k = format!("{:?}", &*d).split_once('{').map_or_else(
                    || {
                        format!("{:?}", &*d)
                            .split_once('(')
                            .map_or_else(|| format!("{:?}", &*d), |(name, _)| name.to_string())
                    },
                    |(name, _)| name.to_string(),
                );
                let l = match &*d {
                    SemanticNodeData::Primitive(p) => Some(format!("{p:?}").to_lowercase()),
                    SemanticNodeData::Object(_) => Some("{...}".to_string()),
                    SemanticNodeData::TypeParam { display_name, .. } => {
                        Some(display_name.to_string())
                    }
                    SemanticNodeData::Literal(lit) => Some(format!("{lit:?}")),
                    SemanticNodeData::Array { readonly, .. } => {
                        Some(if *readonly { "readonly T[]" } else { "T[]" }.to_string())
                    }
                    SemanticNodeData::Tuple { .. } => Some("[...]".to_string()),
                    SemanticNodeData::Union(_) => Some("A | B".to_string()),
                    SemanticNodeData::Intersection(_) => Some("A & B".to_string()),
                    SemanticNodeData::Function { .. } => Some("(...) => R".to_string()),
                    _ => None,
                };
                (k, l)
            })
            .unwrap_or_else(|| ("Unknown".to_string(), None));
        nodes.push(OriginNodeDto {
            id: idx,
            kind,
            label,
        });
        node_index.insert(id, idx);
        idx
    };

    let mut edges_dto: Vec<OriginEdgeDto> = Vec::new();
    for (target_node, kind, edge) in &all_edges {
        let target_idx = intern_node(*target_node, graph);
        let edge_kind = match kind {
            OriginEdgeKind::Instantiate => "instantiate",
            OriginEdgeKind::SubstituteTypeParam => "substituteTypeParam",
            OriginEdgeKind::ConditionalSelect => "conditionalSelect",
            OriginEdgeKind::InferBind => "inferBind",
            OriginEdgeKind::ProjectMember => "projectMember",
            OriginEdgeKind::ProjectIndex => "projectIndex",
            OriginEdgeKind::ProjectPath => "projectPath",
            OriginEdgeKind::Normalize => "normalize",
            OriginEdgeKind::AliasResolve => "aliasResolve",
        };
        let meta_str = format!("{:?}", edge.meta);
        let meta_idx = if meta_str == "None" {
            None
        } else {
            let idx = if let Some(&existing) = meta_index_map.get(&meta_str) {
                existing
            } else {
                let idx = meta_strings.len() as u32;
                meta_strings.push(meta_str.clone());
                meta_index_map.insert(meta_str, idx);
                idx
            };
            Some(idx)
        };
        for source in edge.sources.iter() {
            let source_idx = intern_node(*source, graph);
            edges_dto.push(OriginEdgeDto {
                source: source_idx,
                target: target_idx,
                kind: edge_kind.to_string(),
                meta_index: meta_idx,
            });
        }
    }

    OriginGraphDto {
        nodes,
        edges: edges_dto,
        meta_strings,
    }
}


struct HostComponentMetaResolver<'a> {
    host: &'a VerterHost,
}

impl crate::resolver_core::DeclarationMetadataResolver for HostComponentMetaResolver<'_> {
    fn resolve_export_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<crate::resolver_core::ResolvedExportTarget> {
        self.host
            .resolve_named_type_export_target(dep_canonical, requested_name)
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
            .get_export_span_follow_reexports(dep_canonical, requested_name)
            .map(|(_, start, end)| verter_span::Span::new(start, end))
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
        from_canonical: &str,
        import_source: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(from_canonical, import_source)
    }

    fn resolve_direct_type_reexport_target(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_direct_type_reexport_target(dep_canonical, requested_name)
    }

    fn resolve_local_import_symbol_target(
        &self,
        dep_canonical: &str,
        resolved_name: &str,
    ) -> Option<(String, String)> {
        self.host
            .resolve_local_import_symbol_target(dep_canonical, resolved_name)
    }

    fn resolve_local_export_symbol_target(
        &self,
        canonical_source: &str,
        exported_name: &str,
    ) -> Option<String> {
        self.host
            .resolve_local_export_symbol_target(canonical_source, exported_name)
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

impl crate::resolver_core::ComponentMetaResolverHost for HostComponentMetaResolver<'_> {
    type Snapshot = FileAnalysisSnapshot;
    type EvalContext = CapturedComponentMetaInputs;

    fn resolve_type_declaration(
        &self,
        dep_canonical: &str,
        requested_name: &str,
    ) -> ResolvedTypeDeclaration {
        resolve_type_declaration(self.host, dep_canonical, requested_name)
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
        purpose: crate::resolver_core::ComponentMetaResolutionPurpose,
    ) -> ComponentMetaEvalOutputs {
        let eval_started = component_meta_debug_enabled().then(Instant::now);
        if component_meta_debug_enabled() {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} step=evaluated_types:start imports={} macro_type_deps={}",
                owner_canonical,
                ProjectionMode::Expanded,
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
                    self.host
                        .cache_dependency_candidates_from_snapshot(owner_canonical, snapshot)
                }),
        );
        let compute_eval_start = component_meta_debug_enabled().then(Instant::now);
        // D-Cutover §5.8 WIP-W: the retired `shared_owner_engine` path
        // is gone; all callers go through
        // `compute_evaluated_types_with_tracking_from_owner_context`
        // which internally builds any needed host bridge.
        let computed_eval_types = self
            .host
            .compute_evaluated_types_with_tracking_from_owner_context(
                owner_canonical,
                snapshot,
                eval_context.and_then(|captured| captured.owner_eval_source.as_deref()),
                purpose,
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
        let (evaluated_types, surface_identities) = computed_eval_types
            .map(|computed| (computed.evaluated_types, computed.surface_identities))
            .unwrap_or((None, None));
        if let Some(eval_started) = eval_started {
            component_meta_debug(format!(
                "resolve_component_meta owner={} mode={:?} evaluated_types took {:?} has_output={}",
                owner_canonical,
                ProjectionMode::Expanded,
                eval_started.elapsed(),
                evaluated_types
                    .as_ref()
                    .is_some_and(|types| !types.is_empty()),
            ));
        }
        ComponentMetaEvalOutputs {
            evaluated_types,
            tracked_dependencies,
            surface_identities,
        }
    }

    fn projectable_owner_local_macro_roots(
        &self,
        owner_canonical: &str,
        mac: &verter_semantic::analysis::types::AnalyzedMacro,
    ) -> Vec<String> {
        fn macro_lacks_direct_local_surface(
            mac: &verter_semantic::analysis::types::AnalyzedMacro,
        ) -> bool {
            match mac.kind {
                verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                | verter_semantic::analysis::AnalyzedMacroKind::DefineModel => {
                    mac.prop_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                    mac.emit_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => {
                    mac.slot_fields.is_empty()
                }
                verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
            }
        }

        let mut candidate_roots = Vec::new();
        let mut seen = rustc_hash::FxHashSet::default();

        for resolved in &mac.resolved_local_types {
            let is_direct_local = mac
                .type_references
                .iter()
                .any(|type_name| type_name == &resolved.name);
            if is_direct_local && seen.insert(resolved.name.as_str()) {
                candidate_roots.push(resolved.name.as_str());
            }
        }

        if candidate_roots.is_empty() && macro_lacks_direct_local_surface(mac) {
            let owner_has_symbol = self.host.route_owned_shallow_state(owner_canonical);
            for type_name in &mac.type_references {
                if type_name.contains('.') || !seen.insert(type_name.as_str()) {
                    continue;
                }
                let owner_local_decl = owner_has_symbol
                    .as_ref()
                    .is_some_and(|state| state.symbol(type_name).is_some())
                    || self
                        .resolve_type_declaration(owner_canonical, type_name)
                        .canonical_source
                        == owner_canonical;
                if owner_local_decl {
                    candidate_roots.push(type_name.as_str());
                }
            }
        }

        if candidate_roots.is_empty() {
            return Vec::new();
        }

        // Phase 5m §5.13a.2 — bridge via per-engine helper.
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);

        candidate_roots
            .into_iter()
            .filter(|root_name| {
                project_prepared_type_surface_shape_via_host_threaded(
                    &mut query_engine,
                    owner_canonical,
                    root_name,
                )
                .is_some_and(|shape| match mac.kind {
                    verter_semantic::analysis::AnalyzedMacroKind::DefineProps
                    | verter_semantic::analysis::AnalyzedMacroKind::WithDefaults
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineModel
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineSlots => true,
                    verter_semantic::analysis::AnalyzedMacroKind::DefineEmits => {
                        !shape.properties.is_empty() || !shape.call_signatures.is_empty()
                    }
                    verter_semantic::analysis::AnalyzedMacroKind::DefineExpose
                    | verter_semantic::analysis::AnalyzedMacroKind::DefineOptions => false,
                })
            })
            .map(str::to_string)
            .collect()
    }

    fn resolve_owner_local_macro_surface(
        &self,
        owner_canonical: &str,
        root_name: &str,
        macro_kind: verter_semantic::analysis::types::AnalyzedMacroKind,
    ) -> Option<crate::resolver_core::surface_projector::ProjectedMacroSurfaces> {
        // Phase 5m §5.13a.2 — bridge via per-engine helper.
        let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(self.host);
        let shape = project_prepared_type_surface_shape_via_host_threaded(
            &mut query_engine,
            owner_canonical,
            root_name,
        )?;
        Some(
            crate::resolver_core::component_meta::project_macro_surfaces_from_expanded_shape(
                macro_kind, &shape,
            ),
        )
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
        self.host.resolve_component_meta_macro_elements(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
        )
    }

    fn resolve_imported_macro_surface(
        &self,
        owner_canonical: &str,
        import_source: &str,
        exported_name: &str,
        tracked_deps: &mut std::collections::BTreeSet<String>,
        resolution_deps: &mut std::collections::BTreeSet<String>,
        cache: &mut crate::resolver_core::ExternalTypeBodyCache,
        visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    ) -> Option<crate::resolver_core::ResolvedImportedMacroSurface> {
        let _ = visiting;
        self.host.resolve_component_meta_macro_surface(
            owner_canonical,
            import_source,
            exported_name,
            tracked_deps,
            resolution_deps,
            cache,
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
                ProjectionMode::Expanded
            } else {
                ProjectionMode::Identity
            },
            tracked_deps,
            cache,
            visiting,
            verter_workspace::ResolveRequestKind::TypeImport,
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
            .current_dependency_fact_versions(canonical, tracked_deps)
    }
}

pub(crate) fn resolve_type_declaration(
    host: &VerterHost,
    dep_canonical: &str,
    requested_name: &str,
) -> ResolvedTypeDeclaration {
    let resolver = HostComponentMetaResolver { host };
    let key =
        crate::resolver_core::symbol_resolver::declaration_node_key(dep_canonical, requested_name);
    let mut ctx = crate::resolver_core::symbol_resolver::ResolveContext::new();
    let permissive_view = crate::resolver_core::PermissiveStoreView;
    let result =
        host.resolver_runtime()
            .symbol
            .resolve_node(key, &permissive_view, &mut ctx, |_| {
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
                    facts: host.current_dependency_fact_versions(dep_canonical, &tracked_deps),
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

fn read_full_source(host: &VerterHost, canonical_source: &str) -> Option<String> {
    host.read_analysis_source(canonical_source)
        .map(|source| source.to_string())
}

#[allow(clippy::too_many_arguments)]
fn resolve_jsdoc_block(
    host: &VerterHost,
    canonical_source: &str,
    span: verter_span::Span,
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    kind: verter_workspace::ResolveRequestKind,
) -> Option<ResolvedJsdocBlock> {
    if span.start == 0 && span.end == 0 {
        return None;
    }

    let source = read_full_source(host, canonical_source)?;
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
    mode: ProjectionMode,
    tracked_deps: &mut std::collections::BTreeSet<String>,
    _cache: &mut crate::resolver_core::ExternalTypeBodyCache,
    _visiting: &mut rustc_hash::FxHashSet<(String, String)>,
    _kind: verter_workspace::ResolveRequestKind,
    tag: verter_semantic::analysis::types::JsdocTag,
) -> ResolvedJsdocTag {
    let (text, raw_type, subject_name) = parse_jsdoc_tag_payload(tag.name.as_str(), tag.text);
    let resolved_type = if mode == ProjectionMode::Expanded {
        raw_type.as_deref().and_then(|raw_type| {
            resolve_jsdoc_tag_type(host, canonical_source, raw_type, tracked_deps)
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
) -> Option<verter_semantic::analysis::type_expr::TypeExpr> {
    let parsed = verter_semantic::analysis::type_expr_lower::parse_type_annotation(raw_type);
    let parsed = if parsed.is_unknown() {
        verter_semantic::analysis::type_expr::TypeExpr::Unknown {
            raw: raw_type.to_string(),
        }
    } else {
        parsed
    };

    // Ensure module facts are materialized so the dispatch path can
    // resolve imports through host-owned caches.
    let _facts = host.ensure_indexed_ready(canonical_source)?;
    tracked_deps.extend(
        host.imported_symbol_dependencies_for_expr(canonical_source, &parsed)
            .into_iter()
            .map(|dependency| dependency.canonical_id),
    );
    // Phase 5d (sub-plan §4.1): route directly through the shared
    // dispatch ProjectPath helper. Falls back to the raw parsed
    // annotation when projection misses so the caller still receives
    // the unresolved TypeExpr rather than `None`.
    Some(project_expr_class_a_via_dispatch(host, canonical_source, &parsed).unwrap_or(parsed))
}

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
