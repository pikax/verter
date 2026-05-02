//! Materialization core: TypeExpr stabilizer + macro-shape producers.
//!
//! Phase 11a domain 7 — split into two children to keep each below the
//! 4,000-line `god_module_size_budget`:
//!
//! - [`field_types`] — stabilizer
//!   (`materialize_component_meta_type_expr_until_stable` and `_full`),
//!   the `materialize_component_meta_field_types` driver, and the
//!   field-rescue / shallow-symbolic / package-backed predicates that
//!   gate it. Lines 99-1827 of the pre-split `meta_resolve.rs`.
//!
//! - [`macro_shapes`] — `produce_macro_object_shapes` + `_for_purpose`,
//!   `MacroShapeSource`, and the macro-shape synthesis / projection /
//!   penalty helpers that produce `define_props` / `define_emits` /
//!   `define_slots` shapes for `ExpandedComponentTypes`. Lines 1828-4102
//!   of the pre-split `meta_resolve.rs`.
//!
//! Both children are private modules; this submodule re-exports their
//! `pub(crate)` surface to the parent so existing `crate::meta_resolve::*`
//! callsites keep working without churn.

mod field_types;
mod macro_shapes;
pub(crate) mod utility_types;

pub(crate) use field_types::{
    define_props_member_can_stay_symbolic_without_rescue,
    field_should_preserve_shallow_symbolic_raw_type, lowered_needs_member_route_materialization,
    lowered_preserve_package_backed_symbolic_refs, materialize_component_meta_field_types,
    materialize_component_meta_type_expr_until_stable, type_expr_is_slots_member_route,
};

pub(crate) use macro_shapes::{
    collect_type_expr_ref_names, expr_needs_projection_rescue, has_prop_shape_surface,
    produce_macro_object_shapes_for_purpose, projection_result_beats_solver_shape,
};
// Test-only macro-shape re-exports — exercised via the `meta_resolve.rs`
// shell's `#[cfg(test)] pub(crate) use materialize::{…}` block by
// `meta_resolve_tests.rs` (bare-name `super::*` glob).
#[cfg(test)]
pub(crate) use macro_shapes::{
    define_props_fields_fast_path_allowed, produce_macro_object_shapes,
    produce_one_macro_object_shape, registry_entry_to_expanded_shape,
    synthesize_define_props_shape_from_known_surface_with_authority, MacroShapeSource,
};
