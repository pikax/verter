//! `impl VerterHost` — resolve and virtual file retrieval methods, split
//! across sub-modules by concern.
//!
//! `crate::host_resolve::*` is the module's whole public surface: every
//! item a sub-module owns is re-exported here, so a caller
//! (`crate::resolver_store`, `crate::host_manage`, …) names this module
//! and never a sub-module path.
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
//! - [`compile_request_build`] — the bound compile lanes' session side:
//!   the framework host-backend demand constructors, the host-backed
//!   bound execution dispatch (`execute_bound_host_products`) with its
//!   arm-local framework execution-input preparation, refusal mapping,
//!   and the shared result carriers both `compile_entry` and
//!   `compile_entry_runtime_render` consume.
//! - [`frontier_helpers`] — route-cache and wildcard-ranking helpers.
//! - [`native_host_binding`] — the sealed request-scoped
//!   `BoundNativeHostRequest` binding substrate over the registered
//!   framework host-integration catalog.
//! - [`dependency_resolution`] — import-route + dependency canonical
//!   resolution.
//! - [`frontier_engine`] — named-type export route resolution.
//! - [`route_surface`] — route-surface facts, prepared-decl walking,
//!   and dependency-source readers.
//! - [`virtual_file_pipeline`] — `resolve` / `ensure_compiled` /
//!   `get_virtual_file` / `get_ide` / `get_public_api*` / `compile_entry`.
//! - [`vue_script_extract`] — free helpers for SFC `<script>` extraction
//!   and template-converter input shaping.

mod compile_request_build;
mod dependency_resolution;
mod external_type_resolution;
pub(crate) mod fallthrough_props;
mod frontier_engine;
mod frontier_helpers;
pub mod native_host_binding;
mod route_surface;
mod rune_ambient;
mod virtual_file_pipeline;
mod vue_macro_dependency_diagnostics;
mod vue_script_extract;

#[cfg(test)]
pub(crate) use virtual_file_pipeline::vue_macro_output_matches_revision;

// Re-exports preserving the pre-split public surface at
// `crate::host_resolve::*`. `#[allow(unused_imports)]` because some
// re-exports are consumed only by `#[cfg(test)]` callers
// (frontier_tests / host_resolve_tests) or by external sibling modules
// that name them via `crate::host_resolve::*`; the lint cannot see
// through the cfg gate / sibling-path resolution.
#[allow(unused_imports)]
pub(crate) use rune_ambient::is_svelte_rune_module;
#[allow(unused_imports)]
pub(crate) use rune_ambient::{
    merge_rune_ambient_into_env, merge_rune_ambient_inventory_into_env, rune_ambient_has_type,
    rune_ambient_has_value, rune_ambient_type_decl, rune_ambient_value_decl,
};
#[cfg(test)]
pub(crate) use vue_script_extract::extract_vue_script_content;
pub(crate) use vue_script_extract::{
    indexed_script_setup_type_params, ordered_sfc_structure_analysis,
    populate_ordered_sfc_structure, sfc_script_setup_type_params, template_converter_inputs,
};

// Test-only knob: arm the compile-tier producer's fact-injection slot.
// Re-exported through the parent module so `crate::for_tests` can
// publish it without forming a name dependency on the
// `virtual_file_pipeline` private module path.
//
// The cfg gate MUST match the target's gate in `virtual_file_pipeline.rs`
// (`CompileForceOverflowGuard` is `#[cfg(any(test, feature = "test-support"))]`).
// A `pub use` of a cfg-stripped item is an unresolved-import error in
// release builds (`cargo build --release`, where `debug_assertions` is
// off), so the gate is required, not optional.
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub use virtual_file_pipeline::CompileForceOverflowGuard;

// Test-only introspection for the Session-only compile-tier prefetch
// gate. Same cfg gate as above (a `pub use` of a cfg-stripped item is an
// unresolved-import error in release builds).
#[cfg(any(test, feature = "test-support"))]
#[doc(hidden)]
pub use virtual_file_pipeline::{
    compile_tier_prefetch_invocations, reset_compile_tier_prefetch_invocations,
};

// Test-only introspection for the host-backed bound compile lane: the
// lane tests match on the typed transaction outcome directly.
#[cfg(test)]
pub(crate) use virtual_file_pipeline::CompileEntryOutcome;

// The caller-supplied canonical-request execution route: the session
// entry, its lane, and the virtual-node publication both lanes share.
// Available on EVERY target, browser included: the route is synchronous
// and single-threaded, and the browser binding is one of its callers.
mod compile_request_execute;

// The canonical-request compile seam's own tests, housed with the route
// they drive.
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../compile_request_seam_tests.rs"]
mod compile_request_seam_tests;

// The bound host-backed compile lane's own tests, housed with the routes
// they drive (same `#[path]` pattern as the sibling suites below).
#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "../host_backed_lane_tests.rs"]
mod host_backed_lane_tests;

#[cfg(test)]
#[path = "../host_resolve_tests.rs"]
mod host_resolve_tests;

#[cfg(test)]
#[path = "../host_resolve_creo_tests.rs"]
mod host_resolve_creo_tests;

#[cfg(test)]
#[path = "../frontier_tests.rs"]
mod frontier_tests;
