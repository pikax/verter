//! `impl VerterHost` — resolve and virtual file retrieval methods,
//! split across sub-modules per the Tier 2 §4 god-module split
//! (debt-and-deferred-fixes plan).
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
//! - [`frontier_helpers`] — route-cache and wildcard-ranking helpers.
//! - [`dependency_resolution`] — import-route + dependency canonical
//!   resolution.
//! - [`frontier_engine`] — named-type export route resolution.
//! - [`route_surface`] — route-surface facts, prepared-decl walking,
//!   and dependency-source readers.
//! - [`virtual_file_pipeline`] — `resolve` / `ensure_compiled` /
//!   `get_virtual_file` / `get_ide` / `get_public_api*` / `compile_entry`.
//! - [`vue_script_extract`] — free helpers for SFC `<script>` extraction
//!   and template-converter input shaping.

mod dependency_resolution;
mod external_type_resolution;
mod frontier_engine;
mod frontier_helpers;
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
pub(crate) use vue_script_extract::{
    build_position_preserving_script_source, extract_vue_script_content,
    populate_sfc_blocks_sidecar, sfc_script_setup_type_params, template_converter_inputs,
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

#[cfg(test)]
#[path = "../host_resolve_tests.rs"]
mod host_resolve_tests;

#[cfg(test)]
#[path = "../frontier_tests.rs"]
mod frontier_tests;
