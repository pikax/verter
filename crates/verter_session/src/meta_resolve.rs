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
mod macro_member_walk;
pub(crate) mod materialize;
mod origin_graph;
mod registry_materialize;
mod resolved_state;
mod scoring;
pub(crate) use dep_signature::{
    accumulate_dispatch_dep_signature, drain_dispatch_dep_signature_accumulator,
    reset_dispatch_dep_signature_accumulator,
};
#[cfg(test)]
pub(crate) use dep_signature::{
    bfs_compute_counter_for_test, reset_bfs_compute_counter_for_test,
    with_bfs_child_refs_observer_for_test, with_visited_counter,
};
pub(crate) use dispatch_helpers::{
    instantiate_local_generic_ref_via_dispatch, lower_and_project_to_expanded_via_host_threaded,
    pick_via_dispatch_pick_helper, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded, project_expr_surface_expr_via_host_threaded,
    project_expr_surface_shape_via_host_threaded,
    project_prepared_type_surface_shape_via_host_threaded,
    project_type_surface_expr_via_host_threaded,
};
// Test-only re-exports — exercised by parity tests for the
// dispatch-route-helper coverage matrix.
#[cfg(test)]
pub(crate) use dispatch_helpers::{
    project_prepared_type_surface_expr_via_host_threaded,
    project_route_surface_expr_via_host_threaded,
};
#[cfg(test)]
pub(crate) use field_state::{
    dispatch_lower_counter_get, dispatch_lower_counter_reset, MacroFieldGraphState,
};
pub(crate) use graph_predicates::{
    build_keys_union_node, canonical_resolves_to_package,
    component_meta_ref_resolves_to_package_node, extract_route_root_identity_node,
    ref_root_reaches_transitive_cycle_node,
};
// Phase 11a / post-cutover clippy cleanup — these graph-native predicates
// have no non-test consumers in the landed tree but are exercised by
// `meta_resolve_tests.rs` and other integration-test targets via
// `crate::meta_resolve::*` paths. Gating with `#[cfg(test)]` keeps the
// non-test build surface clean while preserving the test re-export
// contract.
#[cfg(test)]
pub(crate) use graph_predicates::{
    collect_ref_identities_node, declaration_body_prefers_inline_materialization_node,
    slot_binding_param_can_stay_symbolic_node, type_node_has_package_backed_root,
    type_node_needs_member_route_materialization,
};
// Phase 10a: `jsdoc_resolve` source moved to
// `host_manage/jsdoc_resolve.rs` (host-impl tier; the
// `HostComponentMetaResolver` adapter and `read_full_source` helper
// belong with the host).
pub(crate) use crate::host_manage::jsdoc_resolve::resolve_type_declaration;
// Test-only re-exports — `meta_resolve_tests.rs` references
// `super::HostComponentMetaResolver` and bare `resolve_jsdoc_tag_type`
// via `super::*` glob.
#[cfg(test)]
pub(crate) use crate::host_manage::jsdoc_resolve::{
    resolve_jsdoc_tag_type, HostComponentMetaResolver,
};
pub(crate) use macro_member_walk::{
    collect_define_props_root_names, slot_binding_targets_define_props_root,
    walk_component_meta_macro_shape_member_types, PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER,
    SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER,
};
pub(crate) use materialize::{
    collect_type_expr_ref_names, lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_field_types, materialize_component_meta_type_expr_until_stable,
    produce_macro_object_shapes_for_purpose,
};
// Test-only re-exports — exercised by `meta_resolve_tests.rs` via
// `super::*` glob import from the `meta_resolve_tests` child mod
// (`#[path = "meta_resolve_tests.rs"] mod meta_resolve_tests;` at the
// bottom of this file). The symbols are bare-name in the test bodies;
// removing the re-export breaks compilation of those tests.
#[cfg(test)]
pub(crate) use materialize::{
    define_props_fields_fast_path_allowed, expr_needs_projection_rescue, has_prop_shape_surface,
    produce_macro_object_shapes, produce_one_macro_object_shape, registry_entry_to_expanded_shape,
    synthesize_define_props_shape_from_known_surface_with_authority, MacroShapeSource,
};
pub(crate) use origin_graph::build_origin_graph;
pub(crate) use registry_materialize::{
    component_meta_registry_prefers_structural_materialization_node,
    component_meta_registry_should_keep_raw_symbolic_non_object_alias,
    materialize_component_meta_registry_structural_expr, preserve_nested_symbolic_member_routes,
    preserve_registry_callable_param_member_routes,
    type_expr_needs_nested_symbolic_route_preservation,
};
// Test-only registry-materialise predicates — exercised by parity tests
// for symbolic-route preservation contract.
#[cfg(test)]
pub(crate) use registry_materialize::preserve_package_backed_symbolic_refs_node;
// Phase 10a: `request_host` source moved to
// `host_manage/component_meta_request_impl.rs` (host-impl tier per
// §10a.0.A). The re-export re-points at the new home so the
// `crate::meta_resolve::*` public surface stays intact for callers.
pub(crate) use crate::host_manage::component_meta_request_impl::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    resolved_meta_cache_key, should_skip_imported_registry_seed_refresh, trace_request_source,
};
pub use crate::host_manage::component_meta_request_impl::{
    CapturedComponentMetaInputs, ResolvedComponentMetaComputeAudit, ResolvedDeclarationKind,
    ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta, ResolvedNativeProp,
    ResolvedTypeDeclaration, ResolvedTypeRegistryMeta, SessionRequestHost,
};
pub(crate) use resolved_state::{
    component_meta_owner_local_shallow_substituted_alias_body, enrich_missing_slot_bindings,
    select_imported_materialization_scope, RegistryMaterialization,
};
pub use resolved_state::{ResolvedComponentMetaState, SurfaceNodeIdentities};
pub(crate) use scoring::compare_type_expr_improvement;
// Phase 10a: promoted to `pub(crate)` so the moved
// `host_manage::component_meta_methods.rs` (formerly the in-tree
// `host_methods.rs`) reaches the function via the `crate::meta_resolve`
// re-export instead of the now-out-of-scope `super::scoring::...`.
pub(crate) use scoring::component_meta_registry_prefers_structural_materialization;

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
