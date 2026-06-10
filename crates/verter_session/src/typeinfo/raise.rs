#![deny(missing_docs)]
//! Public host-level wrapper around the dispatch raise pipeline.
//!
//! `VerterHost::project_node_to_type_expr` is the substrate that the
//! FFI adapter (NAPI / WASM) calls to project a `SemanticNodeId`
//! (returned by [`crate::VerterHost::resolve_named_symbol_with_audit`]
//! or [`crate::VerterHost::evaluate_type_expression_with_audit`]) into
//! a [`verter_type_expr::TypeExpr`] so the wire payload carries
//! the typeinfo "TypeExpr at the boundary" contract.
//!
//! The dispatch implementation in
//! [`crate::project_semantic_dispatch::ProjectSemanticDispatch::raise_node_to_type_expr`]
//! stays the single source of truth (architectural guard
//! `semantic_node_to_type_expr_has_exactly_one_path`). This shell is
//! a thin FFI delegator — it does not duplicate the raise logic.

use verter_type_expr::TypeExpr;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::SemanticNodeId;
use crate::VerterHost;

impl VerterHost {
    /// Project a [`SemanticNodeId`] to a [`TypeExpr`] for FFI use.
    ///
    /// Delegates to
    /// [`ProjectSemanticDispatch::raise_node_to_type_expr`] — the
    /// single canonical raise path. Returns `None` when the node id
    /// has no current graph entry (the typical "miss" case — e.g.
    /// resolution returned a node that has since been evicted by a
    /// generation flip).
    #[must_use]
    pub fn project_node_to_type_expr(&self, node: SemanticNodeId) -> Option<TypeExpr> {
        // Query-RETURNER: it returns the raised `TypeExpr` with no outer
        // publish fence, so it MUST raise against a PROVEN-CURRENT
        // snapshot. A known-stale (`ReturnOnly`) read would raise the node
        // against superseded graph state; on sustained churn surface a
        // miss (`None`) — the established raise miss signal — rather than
        // a stale projection. The bounded retry terminates.
        let current_view = crate::typeinfo::current_store_view_for_query(self)?;
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        dispatch.raise_node_to_type_expr(node)
    }
}
