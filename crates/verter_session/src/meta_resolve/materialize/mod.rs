//! Materialization core: TypeExpr stabilizer + macro-shape ref-name collector.
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
    package_backed_object_like_root_identity_with_fence,
    reduce_member_value_graph_native_with_context, type_expr_has_package_backed_object_like_root,
    type_expr_materialize_reduction_context, type_expr_materializer_context,
};
// The `_with_fence` TypeExpr front is reached in production only INTERNALLY (via
// the bare `type_expr_has_package_backed_object_like_root` wrapper); the crate
// re-export is consumed solely by the node-vs-TypeExpr front differential.
#[cfg(test)]
pub(crate) use field_types::type_expr_has_package_backed_object_like_root_with_fence;
// Re-export ONLY the per-sink output capability TYPE so the
// `output_materialization` owner module can name it for its explicit
// `impl OutputProjector for MetaResolveFieldTypesOutputCap` registration pair.
// The `new()` CONSTRUCTOR stays sink-private
// (`mint: pub(in crate::meta_resolve::materialize::field_types)`), so this
// re-export does NOT widen who can mint — only who can name the type.
pub(crate) use field_types::MetaResolveFieldTypesOutputCap;
// Re-export the non-output member-shape-key capability TYPE so
// `component_meta_caches` can name it in the registry member-value-node key
// constructor's signature. Like the output-cap re-export above, the `new()`
// CONSTRUCTOR stays sink-private (`mint: pub(in …::field_types)`), so this
// widens only who can NAME the type, not who can mint it.
pub(crate) use field_types::RegistryMemberShapeKeyCap;
// The node-domain registry member-surface stabiliser + the stabilised-value
// carrier: the node-first second pass that reduces a first-pass
// `MaterializeStructureDb` node through the `ShapeCacheDb` member-node slot,
// reproducing the `_until_stable_full` reduction context.
pub(crate) use field_types::{
    stabilize_registry_member_surface_node_with_shape_cache, RegistryMemberStabilizedValue,
};
// The reduction-context helper is consumed in production INTERNALLY by the
// stabiliser (same module); only the node-vs-TypeExpr reduction-context parity
// differential reaches it through this path.
#[cfg(test)]
pub(crate) use field_types::node_materialize_reduction_context;

pub(crate) use macro_shapes::collect_type_expr_ref_names;
