#![deny(missing_docs)]
//! Public host-level bytes facade over the dispatch raise pipeline.
//!
//! `VerterHost::project_node_to_type_expr_json_bytes` is the FFI substrate
//! the native adapters (NAPI / WASM) call to project a `SemanticNodeId`
//! (returned by [`crate::VerterHost::resolve_named_symbol_with_audit`] or
//! [`crate::VerterHost::evaluate_type_expression_with_audit`]) into the
//! wire payload that carries the typeinfo "TypeExpr at the boundary"
//! contract.
//!
//! The facade returns ENCODED WIRE BYTES (`Vec<u8>` of UTF-8 JSON), NOT a
//! [`verter_type_expr::TypeExpr`] and NOT a sealed carrier. The reverse
//! materialization runs INTERNALLY through the sealed
//! [`crate::project_semantic_dispatch::output_materialization::OutputProjector`]
//! capability — the typeinfo output sink mints the capability, materializes
//! the node into a sealed `OutputTypeExpr`, unwraps it via the capability,
//! and serializes the resulting `TypeExpr` here. FFI callers therefore
//! never touch a `TypeExpr`, the carrier, or the projector — they receive
//! opaque wire bytes (NAPI wraps them in a `Buffer`; WASM does
//! `String::from_utf8`). The schema is byte-identical to the previous
//! FFI-side encoder; only the encoding LOCATION moved into `verter_session`.
//!
//! The raw `SemanticNodeId -> TypeExpr` raise primitive
//! `raise_node_to_type_expr` stays module-private to
//! [`crate::project_semantic_dispatch::raise`]; this facade reaches it only
//! through the sealed output capability, never directly.

use crate::project_semantic_dispatch::output_materialization::OutputProjector;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::SemanticNodeId;
use crate::VerterHost;

/// Render one graph node as terminal TypeScript display text through the
/// registered TypeInfo output sink.
///
/// The caller receives only text. The sealed materialized carrier and its
/// `TypeExpr` never cross this module boundary, so graph-oriented consumers
/// cannot branch on a reverse-materialized shape.
pub(crate) fn render_node_display_with_ctx(
    ctx: &dyn crate::resolver_core::ResolverContext,
    node: SemanticNodeId,
) -> Option<String> {
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let cap = TypeinfoRaiseOutputCap::new(&dispatch);
    let type_expr = cap.materialize_output_type_expr(node)?.into_type_expr(&cap);
    verter_type_expr::render_type_expr_display(&type_expr)
        .ok()
        .map(|rendered| rendered.text)
}

crate::project_semantic_dispatch::output_materialization::define_output_capability! {
    /// The typeinfo FFI bytes-facade output-sink capability: the facade here
    /// holds this to materialize a graph node into a sealed output carrier
    /// and unwrap it before JSON-encoding. Its constructor is visible ONLY
    /// within `crate::typeinfo::raise` — NOT the whole `typeinfo` subtree —
    /// so no other `typeinfo` sibling (e.g. `framework_surface::executor`,
    /// `framework_surface::graph_export`, `resolve_named_symbol`) can mint
    /// it; a planted `TypeinfoRaiseOutputCap::new` outside this leaf is
    /// `E0624`.
    pub(crate) struct TypeinfoRaiseOutputCap;
    mint: pub(in crate::typeinfo::raise)
}

impl VerterHost {
    /// Project a [`SemanticNodeId`] to its wire-encoded `TypeExpr` bytes
    /// for FFI use.
    ///
    /// Materializes the node into a sealed output carrier through the
    /// typeinfo [`OutputProjector`] capability, unwraps it, and serializes
    /// the resulting [`verter_type_expr::TypeExpr`] to UTF-8 JSON bytes.
    /// Returns `None` when the node id has no current graph entry (the
    /// typical "miss" case — e.g. resolution returned a node that has since
    /// been evicted by a generation flip) OR when serialization fails (a
    /// `TypeExpr` is always serializable, so the latter is unreachable in
    /// practice and collapses to the same miss signal the FFI surface maps
    /// to `null`).
    ///
    /// [`OutputProjector`]: crate::project_semantic_dispatch::output_materialization::OutputProjector
    #[must_use]
    pub fn project_node_to_type_expr_json_bytes(&self, node: SemanticNodeId) -> Option<Vec<u8>> {
        // Query-RETURNER: it returns the encoded `TypeExpr` with no outer
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
        // Mint the typeinfo bytes-facade output capability (constructor
        // visible only within `crate::typeinfo::raise`): this FFI facade is a
        // true output sink.
        let cap = TypeinfoRaiseOutputCap::new(&dispatch);
        let type_expr = cap.materialize_output_type_expr(node)?.into_type_expr(&cap);
        serde_json::to_vec(&type_expr).ok()
    }

    /// Test-only sibling of [`Self::project_node_to_type_expr_json_bytes`]
    /// that returns the raised [`verter_type_expr::TypeExpr`] directly
    /// (rather than wire bytes) so the typeinfo / dispatch-equivalence /
    /// lazy-decl / cold-dedup test suites can assert on the projected
    /// `TypeExpr` shape without decoding JSON. It mints the typeinfo output
    /// capability internally and unwraps the sealed carrier — tests never
    /// hold the capability or the carrier. Gated to `test` + the
    /// `oracle-gen` feature (the oracle snapshot generator's `gen.rs` also
    /// needs the projected `TypeExpr`), so it is NOT a production
    /// reverse-materialization path (the structural fence is unaffected).
    #[cfg(any(test, feature = "oracle-gen"))]
    #[must_use]
    pub(crate) fn project_node_to_type_expr_for_test(
        &self,
        node: SemanticNodeId,
    ) -> Option<verter_type_expr::TypeExpr> {
        let current_view = crate::typeinfo::current_store_view_for_query(self)?;
        let overlay = std::sync::Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx =
            crate::resolver_core::HostResolverContext::from_current(self, &current_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        let cap = TypeinfoRaiseOutputCap::new(&dispatch);
        Some(cap.materialize_output_type_expr(node)?.into_type_expr(&cap))
    }
}
