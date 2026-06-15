//! `impl VerterHost` — resolve and virtual file retrieval methods,
//! split across sub-modules per Tier 2 §4 of the
//! `D:/tmp/verter-debt-and-deferred-fixes-plan.md` god-module split.
//!
//! Public surface unchanged: every item that was reachable through
//! `crate::host_resolve::*` before the split is re-exported below so
//! existing callers (`crate::resolver_store`, `crate::host_manage`,
//! `crate::frontier_tests`, etc.) compile without modification.
//!
//! Cross-file component-meta / analysis rule: host-backed consumers share
//! one resolver and one traversal policy.
//! - `Type` mode resolves symbol identity + canonical source location only.
//! - `Expanded` mode uses the same traversal, then materializes expanded
//!   shape.
//! - Component-meta must use the shared expanded path for all macro-facing
//!   surfaces, including Options API metadata.
//! - Traversal only follows imports reachable from the requested
//!   declaration graph.
//! - Barrel and `export *` hops must be cached once discovered because
//!   repeated wildcard re-export scans are expensive.
//!
//! Module layout:
//! - [`frontier_helpers`] — shared types, traces, and helpers.
//! - [`test_guards`] — test-only `forbid_*` thread-local guards.
//! - [`external_macro_collector`] — adapter into
//!   `resolver_core::collect_external_macro_types`.
//! - [`dependency_resolution`] — import-route + dependency canonical
//!   resolution.
//! - [`external_type_resolution`] — `resolve_external_type_from_loaded_files`
//!   + component-meta macro element entry points.
//! - [`frontier_engine`] — frontier closure, materialisation, and
//!   named-type export route resolution (the file's main intra-SCC).
//! - [`route_owned_shallow`] — route-only shallow cache materialiser,
//!   prepared-decl walking, and dependency-source readers.
//! - [`virtual_file_pipeline`] — `resolve` / `ensure_compiled` /
//!   `get_virtual_file` / `get_ide` / `get_public_api*` / `compile_entry`.
//! - [`vue_script_extract`] — free helpers for SFC `<script>` extraction
//!   and template-converter input shaping.
//! - [`frontier_adapter`] — `HostFrontierAdapter` request-scoped bridge
//!   into `resolver_core::FrontierHost`.

mod dependency_resolution;
mod external_macro_collector;
mod external_type_resolution;
mod frontier_adapter;
mod frontier_engine;
mod frontier_helpers;
mod route_owned_shallow;
mod rune_ambient;
mod test_guards;
mod virtual_file_pipeline;
mod vue_script_extract;

// Re-exports preserving the pre-split public surface at
// `crate::host_resolve::*`. `#[allow(unused_imports)]` because some
// re-exports are consumed only by `#[cfg(test)]` callers
// (frontier_tests / host_resolve_tests) or by external sibling modules
// that name them via `crate::host_resolve::*`; the lint cannot see
// through the cfg gate / sibling-path resolution.
#[allow(unused_imports)]
pub(crate) use frontier_adapter::HostFrontierAdapter;
#[allow(unused_imports)]
pub(crate) use frontier_helpers::RouteOwnedShallowStateSnapshot;
pub(crate) use rune_ambient::apply_svelte_rune_ambient_env;
#[allow(unused_imports)]
pub(crate) use rune_ambient::is_svelte_rune_module;
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use test_guards::{
    forbid_route_frontier_for_tests, route_frontier_forbidden_for_current_thread,
    RouteFrontierGuard,
};
pub(crate) use vue_script_extract::{
    apply_sfc_script_setup_type_params, build_position_preserving_script_source,
    extract_vue_script_content, populate_sfc_blocks_sidecar, sfc_script_setup_type_params,
    template_converter_inputs,
};

// Test-only knob: arm the compile-tier producer's fact-injection slot.
// Re-exported through the parent module so `crate::for_tests` can
// publish it without forming a name dependency on the
// `virtual_file_pipeline` private module path.
//
// The cfg gate MUST match the target's gate in `virtual_file_pipeline.rs`
// (`CompileForceOverflowGuard` is `#[cfg(any(test, debug_assertions))]`).
// A `pub use` of a cfg-stripped item is an unresolved-import error in
// release builds (`cargo build --release`, where `debug_assertions` is
// off), so the gate is required, not optional.
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub use virtual_file_pipeline::CompileForceOverflowGuard;

// Test-only introspection for the Session-only compile-tier prefetch
// gate. Same cfg gate as above (a `pub use` of a cfg-stripped item is an
// unresolved-import error in release builds).
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub use virtual_file_pipeline::{
    compile_tier_prefetch_invocations, reset_compile_tier_prefetch_invocations,
};

// Test-only re-exports: `host_resolve_tests.rs` and the inline frontier
// tests reference internal helpers via `super::*`. After the split,
// `super` from those tests resolves to this `mod.rs`, so we re-expose
// the internals here under the same names.
#[cfg(test)]
#[allow(unused_imports)]
pub(crate) use frontier_helpers::{
    external_type_frontier_layer_result_detail, external_type_frontier_layer_start_detail,
    external_type_trace_deltas, external_type_trace_error_status,
    external_type_trace_success_status, ExternalTypeTraceBaseline, FrontierCompanionPlans,
    FrontierRequestedRoutes, PlannedFrontierCompanion,
};

#[cfg(test)]
#[path = "../host_resolve_tests.rs"]
mod host_resolve_tests;

#[cfg(test)]
#[path = "../frontier_tests.rs"]
mod frontier_tests;
