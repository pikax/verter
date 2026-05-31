//! Materialization core: TypeExpr stabilizer + macro-shape producers.
//!
//! Module split:
//!
//! - [`field_types`] — bounded fixed-point reducer
//!   (`materialize_component_meta_type_expr_until_stable` and `_full`)
//!   + package-backed-root predicate that gates reduction.
//!
//! - [`macro_shapes`] — the surviving `collect_type_expr_ref_names`
//!   `TypeExpr`-reference name collector. The macro-object materialiser
//!   that previously lived here is retired; `define_*` shapes are produced
//!   by the dispatch projectors (`crate::meta_resolve::projectors::define_shapes`).
//!
//! Both children are private modules; this submodule re-exports their
//! `pub(crate)` surface to the parent so existing `crate::meta_resolve::*`
//! callsites keep working without churn.

mod field_types;
pub(crate) mod macro_shapes;
pub(crate) mod utility_types;

pub(crate) use field_types::{
    lowered_preserve_package_backed_symbolic_refs,
    materialize_component_meta_type_expr_until_stable,
    materialize_component_meta_type_expr_until_stable_full,
    reduce_member_value_graph_native_with_context, type_expr_has_package_backed_object_like_root,
    type_expr_has_package_backed_object_like_root_with_fence,
    type_expr_materialize_reduction_context, type_expr_materializer_context,
};

pub(crate) use macro_shapes::collect_type_expr_ref_names;
