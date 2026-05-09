//! Materialization core: TypeExpr stabilizer + macro-shape producers.
//!
//! Module split:
//!
//! - [`field_types`] — bounded fixed-point reducer
//!   (`materialize_component_meta_type_expr_until_stable` and `_full`)
//!   + package-backed-root predicate that gates reduction.
//!
//! - [`macro_shapes`] — `produce_macro_object_shapes` + `_for_purpose`,
//!   `MacroShapeSource`, and the macro-shape synthesis / projection /
//!   penalty helpers that produce `define_props` / `define_emits` /
//!   `define_slots` shapes for `ExpandedComponentTypes`.
//!
//! Both children are private modules; this submodule re-exports their
//! `pub(crate)` surface to the parent so existing `crate::meta_resolve::*`
//! callsites keep working without churn.

mod field_types;
mod macro_shapes;
pub(crate) mod utility_types;

pub(crate) use field_types::{
    lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_type_expr_until_stable,
    type_expr_has_package_backed_object_like_root,
};

pub(crate) use macro_shapes::expr_needs_projection_rescue;
pub(crate) use macro_shapes::{
    collect_type_expr_ref_names, produce_macro_object_shapes_for_purpose,
};
// Test-only re-exports consumed by `meta_resolve_tests.rs` via the
// `meta_resolve.rs` shell's `#[cfg(test)] pub(crate) use materialize::{…}` block.
#[cfg(test)]
pub(crate) use macro_shapes::has_prop_shape_surface;
// Test-only macro-shape re-exports — exercised via the `meta_resolve.rs`
// shell's `#[cfg(test)] pub(crate) use materialize::{…}` block by
// `meta_resolve_tests.rs` (bare-name `super::*` glob).
#[cfg(test)]
pub(crate) use macro_shapes::{
    define_props_fields_fast_path_allowed, produce_macro_object_shapes,
    produce_one_macro_object_shape, registry_entry_to_expanded_shape,
    synthesize_define_props_shape_from_known_surface_with_authority, MacroShapeSource,
};
