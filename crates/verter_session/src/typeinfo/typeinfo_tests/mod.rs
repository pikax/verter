//! @ai-generated - Host-backed typeinfo tests for component-shaped
//! generic, utility, conditional, slot, and path-projection type
//! surfaces.
//!
//! These fixtures are synthetic and hermetic. They intentionally use
//! generic type and file names so the tests encode resolver contracts,
//! not behaviour copied from any external component library.

mod apparent_types;
mod basic;
mod branded_types;
mod cache_invalidation;
mod call_resolution;
mod class_features;
mod conditional_infer;
mod const_type_param;
mod contextual_typing;
mod cross_file;
mod declaration_merge;
mod decorators;
mod deep_path;
mod demand_boundary;
mod enums;
mod expansion_boundaries;
mod flow_invalidations;
mod flow_return_catalog;
mod flow_return_edge_catalog;
mod flow_return_parity_contracts;
mod flow_return_path_contracts;
mod footprint;
mod function_advanced;
mod generic_defaults;
mod index_signatures;
mod indexed_utilities;
mod jsdoc_types;
mod jsx;
mod mapped_modifiers;
mod mapped_template;
mod menu_like;
mod message_list_like;
mod mode_boundary_invariants;
mod modern_ts_features;
mod module_features;
mod narrow_discriminated_union;
mod narrow_equality;
mod narrow_in_operator;
mod narrow_instanceof;
mod narrow_truthiness;
mod narrow_typeof;
mod no_infer;
// The oracle harness core now lives at `crate::typeinfo::oracle_core` (moved out
// of the `#[cfg(test)]` `typeinfo_tests` tree so the `oracle-gen` generator can
// reach it). This alias keeps every `super::oracle::*` / `oracle::*` call site in
// the test tree (the spike, the guards, future lifted rows) working unchanged.
pub(crate) use crate::typeinfo::oracle_core as oracle;
// The §4 generation SPIKE — drives the pinned tsgo via verter_type_runtime. Gated
// behind `oracle-gen` so the default gate stays tsgo-free (design §3 inv 1).
#[cfg(feature = "oracle-gen")]
mod oracle_gen_spike;
// `oracle_query_specs` (the pure-data registry) lives physically at
// `typeinfo_tests/oracle_query_specs.rs` (design-pinned path) but is compiled as
// `oracle_core::query_specs` (reachable in non-test `oracle-gen` mode). The test
// tree reaches it via the `oracle::query_specs` alias above.
mod oracle_query_specs_guard;
mod oracle_raw_surface_capture;
mod recursive_conditional;
mod recursive_union;
mod relation_semantics;
mod shallow_surface_facts;
mod substitution_types;
// `pub(crate)` so the moved `oracle_core::driver` (consumption side, `#[cfg(test)]`)
// can reach the shared test helpers it dispatches through — it is no longer a
// descendant of `typeinfo_tests`, so the previously-private visibility no longer
// covers it.
pub(crate) mod support;
mod surface_jsdoc_provenance;
mod surface_spans;
mod table_like;
mod template_literal_inference;
mod tuple_labels;
mod typescript_rules;
mod union_key_access;
mod unique_symbol;
mod utility_composition;
mod utility_edge;
mod utility_top_bottom;
mod value_inference;
mod variadic_tuples;
mod vue_adapter;
mod vue_adapter_cache;
mod vue_import_recursion;
mod vue_sfc_absolute_spans;
mod wide_deep;
