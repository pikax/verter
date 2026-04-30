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

// Used by `meta_resolve_tests.rs` via `#[path]` inclusion at the bottom
// of this shell — bare-name references in tests need the imports in
// scope here.
#[cfg(test)]
use crate::types::{FileAnalysisSnapshot, ProjectionMode};

pub(crate) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

// Phase 11a sub-module split — siblings live in `crates/verter_session/src/meta_resolve/`.
// The shell re-exports the moved `pub(crate)` surface so existing
// `crate::meta_resolve::*` paths keep working without callsite churn.
mod dep_signature;
mod dispatch_helpers;
mod field_state;
mod graph_predicates;
mod host_methods;
mod jsdoc_resolve;
mod macro_member_walk;
mod materialize;
mod origin_graph;
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
pub(crate) use jsdoc_resolve::{
    resolve_jsdoc_tag_type, resolve_type_declaration, HostComponentMetaResolver,
};
pub(crate) use macro_member_walk::{
    materialize_component_meta_macro_shape_member_type_expr,
    walk_component_meta_macro_shape_member_types,
};
pub(crate) use origin_graph::build_origin_graph;
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









#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
