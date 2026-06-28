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
use crate::types::ProjectionMode;

pub(crate) const STORE_VIEW_STABILITY_MAX_ATTEMPTS: usize = 3;

// The output-sink capabilities for this subtree are defined PER-SINK in the
// exact output-SINK modules that legitimately project — NOT subtree-wide:
// `MetaResolveProjectorsOutputCap` in the dedicated TERMINAL sink submodule
// `projectors::output_sink` (re-exported at `projectors::` for the owner impl
// to name — extracted so the parent `projectors`' non-sink helpers cannot
// mint), and `MetaResolveFieldTypesOutputCap` in `materialize/field_types.rs`.
// `pub(in P)` grants the mint to `P` and every module at-or-under it, so each
// cap's mint scope is a TERMINAL sink whose whole reachable production module
// tree is output-only: a subtree-wide cap (`pub(in crate::meta_resolve)`), OR a
// non-sink helper reachable under a sink's mint scope, would let the non-sink
// sibling `meta_resolve::dispatch_helpers` (or that helper) mint the cap and
// launder a bare `TypeExpr`, making the fence convention-based; terminal-sink
// minting makes it compiler-enforced.

// sub-module split — siblings live in `crates/verter_session/src/meta_resolve/`.
// The shell re-exports the moved `pub(crate)` surface so existing
// `crate::meta_resolve::*` paths keep working without callsite churn.
mod dep_signature;
pub(crate) mod diagnostic_convert;
pub(crate) mod dispatch_helpers;
pub(crate) mod exactness;
mod graph_predicates;
#[cfg(test)]
#[path = "meta_resolve/handle_capable_equivalence_tests.rs"]
mod handle_capable_equivalence_tests;
mod macro_member_walk;
pub(crate) mod materialize;
mod origin_graph;
pub(crate) mod projection_demand;
pub(crate) mod projectors;
#[cfg(test)]
#[path = "meta_resolve/projectors_peek_tests.rs"]
mod projectors_peek_tests;
#[cfg(test)]
#[path = "meta_resolve/projectors_silent_miss_tests.rs"]
mod projectors_silent_miss_tests;
mod registry_materialize;
#[cfg(test)]
#[path = "meta_resolve/registry_materialize_tests.rs"]
mod registry_materialize_tests;
mod resolved_state;
mod scoring;
pub(crate) mod slot_binding_graph;
#[cfg(test)]
mod slot_binding_graph_tests;
#[cfg(test)]
#[path = "meta_resolve/typed_ir_consumer_tests.rs"]
mod typed_ir_consumer_tests;
#[cfg(test)]
pub(crate) use dep_signature::{
    bfs_compute_counter_for_test, reset_bfs_compute_counter_for_test,
    with_bfs_child_refs_observer_for_test, with_visited_counter,
};
pub(crate) use dep_signature::{
    drain_dispatch_dep_signature_accumulator, emit_dispatch_dep_signature_facts,
    reset_dispatch_dep_signature_accumulator,
};
// `accumulate_dispatch_dep_signature` is re-exported only for the
// `for_tests` bridge shim that exercises the dispatch accumulator
// directly; production callers route through
// `emit_dispatch_dep_signature_facts`, enforced by the
// `no_accumulate_dispatch_dep_signature_outside_helpers` guard.
#[cfg(any(test, debug_assertions))]
pub(crate) use dep_signature::accumulate_dispatch_dep_signature;
pub(crate) use dispatch_helpers::{
    lower_and_project_to_expanded_node_via_host_threaded,
    project_admitted_node_to_expanded_node_via_host_threaded, project_expr_class_a_via_dispatch,
    project_expr_class_a_via_dispatch_threaded, project_expr_surface_expr_node_via_host_threaded,
    project_expr_surface_shape_via_host_threaded, project_type_surface_expr_via_host_threaded,
};
// Test-only re-export — exercised by the kept dispatch-route-helper
// coverage tests (`project_route_surface_expr_pick_*` in the engine
// tests + the routed-surface classification fixture). This is the
// surviving routed-surface bridge; it routes through the dispatch
// keeper `dispatch_routed_expr_surface_expr`.
#[cfg(test)]
pub(crate) use dispatch_helpers::project_route_surface_expr_via_host_threaded;
pub(crate) use graph_predicates::{
    build_keys_union_node, component_meta_ref_resolves_to_package_node,
    extract_route_root_identity_node, node_package_backed_object_like_root_with_fence,
    node_root_reaches_transitive_cycle_with_fence, ref_root_reaches_transitive_cycle_node,
};
// / clippy cleanup — these graph-native predicates
// have no non-test consumers in the landed tree but are exercised by
// `meta_resolve_tests.rs` and other integration-test targets via
// `crate::meta_resolve::*` paths. Gating with `#[cfg(test)]` keeps the
// non-test build surface clean while preserving the test re-export
// contract.
#[cfg(test)]
pub(crate) use graph_predicates::{
    collect_ref_identities_node, declaration_body_prefers_inline_materialization_node,
    type_node_has_package_backed_root,
};
// `jsdoc_resolve` source moved to
// `host_manage/jsdoc_resolve.rs` (host-impl tier; the
// `HostComponentMetaResolver` adapter and `read_full_source` helper
// belong with the host).
#[cfg(any(test, debug_assertions))]
#[allow(unused_imports)]
pub(crate) use crate::host_manage::jsdoc_resolve::resolve_type_declaration;
#[allow(unused_imports)]
pub(crate) use crate::host_manage::jsdoc_resolve::resolve_type_declaration_with_context;
// Test-only re-exports — `meta_resolve_tests.rs` references
// `super::HostComponentMetaResolver` and bare `resolve_jsdoc_tag_type`
// via `super::*` glob.
#[cfg(test)]
pub(crate) use crate::host_manage::jsdoc_resolve::{
    resolve_jsdoc_tag_type, HostComponentMetaResolver,
};
pub(crate) use macro_member_walk::{
    collect_define_props_root_names, slot_binding_targets_define_props_root,
};
// Capture-token counter names — test/debug instrumentation only; gated to
// match their definitions (absent in release).
#[cfg(any(test, debug_assertions))]
pub(crate) use macro_member_walk::{
    PICK_MEMBER_ROUTE_CALLABLE_DESCENT_COUNTER, SLOT_BINDING_REGISTRY_COLLECTION_SKIP_COUNTER,
};
pub(crate) use materialize::{
    collect_type_expr_ref_names, lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_type_expr_until_stable,
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
// `request_host` source moved to
// `host_manage/component_meta_request_impl.rs` (host-impl tier per
// §10a.0.A). The re-export re-points at the new home so the
// `crate::meta_resolve::*` public surface stays intact for callers.
pub(crate) use crate::host_manage::component_meta_request_impl::{
    next_component_meta_audit_request_id, request_source_performed_compute,
    should_skip_imported_registry_seed_refresh, trace_request_source,
};
pub use crate::host_manage::component_meta_request_impl::{
    CapturedComponentMetaInputs, ResolvedComponentMetaComputeAudit, ResolvedDeclarationKind,
    ResolvedJsdocBlock, ResolvedJsdocTag, ResolvedMacroMeta, ResolvedNativeProp,
    ResolvedTypeDeclaration, ResolvedTypeRegistryMeta, SessionRequestHost,
};
pub(crate) use resolved_state::{lowered_root_reaches_transitive_cycle, RegistryMaterialization};
pub use resolved_state::{ResolvedComponentMetaState, SurfaceNodeIdentities};
pub(crate) use scoring::compare_type_expr_improvement;
pub(crate) use scoring::{compare_node_improvement, node_root_is_explicit_selector_operator};
// promoted to `pub(crate)` so the moved
// `host_manage::component_meta_methods.rs` (formerly the in-tree
// `host_methods.rs`) reaches the function via the `crate::meta_resolve`
// re-export instead of the now-out-of-scope `super::scoring::...`.
pub(crate) use scoring::component_meta_registry_prefers_structural_materialization;

#[cfg(test)]
#[path = "meta_resolve_tests.rs"]
mod meta_resolve_tests;
