//! Materialization core: TypeExpr stabilizer.
//!
//! Module split:
//!
//! - [`field_types`] — bounded fixed-point reducer
//!   (`materialize_component_meta_type_expr_until_stable` and `_full`)
//!   + package-backed-root predicate that gates reduction.
//!
//! The children are private modules; this submodule re-exports their
//! `pub(crate)` surface to the parent so existing `crate::meta_resolve::*`
//! callsites keep working without churn.

mod field_types;

pub(crate) use field_types::{
    package_backed_object_like_root_identity_with_fence,
    reduce_member_value_graph_native_with_context,
};
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
// The node-domain registry member-surface stabiliser + the stabilised-value
// carrier: the node-first second pass that reduces a first-pass
// `MaterializeStructureDb` node through the `ShapeCacheDb` member-node slot,
// reproducing the `_until_stable_full` reduction context.
// The node-domain reduction-context helper: consumed by the stabiliser
// (same module) and by the publication finaliser's node-start per-field
// reducer (`output_sink::reduce_field_value_node`).
pub(crate) use field_types::node_materialize_reduction_context;
