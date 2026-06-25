//! Shared owner of the unmaterialised-`Unknown { raw }` sentinel spelling.
//!
//! The shared `shape_engine::fold_node` materialisation algebra emits a small
//! fixed set of `TypeExpr::Unknown { raw }` sentinel strings when dispatch cannot
//! materialise a node (an unrepresentable surface, an alias cycle, a Vue
//! macro placeholder, a budget-exceeded carrier, …). Two surfaces classify
//! that spelling: the `TypeExpr`-domain recogniser
//! [`dispatch_route_expr_is_materialized`](crate::resolver_core::component_meta_query_engine::dispatch_route_expr_is_materialized)
//! reaches the SINGLE classifier here DIRECTLY, and the node-domain
//! raised-shape projection (owner-local in [`super::raise`]) reaches it
//! TRANSITIVELY — through `dispatch_route_expr_is_materialized` (the
//! node-domain projection raises to a `TypeExpr` and classifies it via that
//! recogniser, not by calling the classifier itself). So the spelling has
//! exactly one owner and can never drift between the two surfaces.
//!
//! The set is the EXACT spelling `dispatch_route_expr_is_materialized`
//! historically inlined: the three [`SEMANTIC_MISS`] / [`SEMANTIC_OBJECT_SURFACE`]
//! / [`SEMANTIC_SURFACE_MEMBER`] consts, the four exact strings
//! (`semanticAliasCycle`, `semanticFunction`, `VueMacroElements`,
//! `projectedOpenSurface`), and the five prefixes (`materialize:`,
//! `unsupportedIntrinsic(`, the [`BUDGET_EXCEEDED_SENTINEL_PREFIX`],
//! `unstableState(`, `aliasCycle(`). Everything else — including the
//! `<raise miss>` carrier-arg placeholder and `semanticTypeParamCycle` —
//! is MATERIALISED (returns `false`), exactly as the legacy inline check
//! treated them.

use crate::resolver_core::component_meta_query_engine::{
    BUDGET_EXCEEDED_SENTINEL_PREFIX, SEMANTIC_MISS, SEMANTIC_OBJECT_SURFACE,
    SEMANTIC_SURFACE_MEMBER,
};

/// Returns `true` when `raw` is one of the sentinel spellings that marks a
/// `TypeExpr::Unknown { raw }` as UNMATERIALISED (a dispatch miss the
/// dispatch-first path falls back to `owner_engine` for).
///
/// This is the single shared owner of the sentinel set:
/// [`dispatch_route_expr_is_materialized`](crate::resolver_core::component_meta_query_engine::dispatch_route_expr_is_materialized)
/// calls this DIRECTLY, and the node-domain raised-shape projection (owner-local
/// in [`super::raise`]) reaches it TRANSITIVELY through that recogniser, so the
/// spelling has exactly one home.
#[must_use]
pub(crate) fn raw_is_unmaterialized_sentinel(raw: &str) -> bool {
    let is_exact_sentinel = matches!(
        raw,
        SEMANTIC_MISS
            | SEMANTIC_OBJECT_SURFACE
            | SEMANTIC_SURFACE_MEMBER
            | "semanticAliasCycle"
            | "semanticFunction"
            | "VueMacroElements"
            | "projectedOpenSurface"
    );
    let is_prefix_sentinel = raw.starts_with("materialize:")
        || raw.starts_with("unsupportedIntrinsic(")
        || raw.starts_with(BUDGET_EXCEEDED_SENTINEL_PREFIX)
        || raw.starts_with("unstableState(")
        || raw.starts_with("aliasCycle(");
    is_exact_sentinel || is_prefix_sentinel
}
