//! The semantic graph store as one execution unit reaches it.
//!
//! An execution unit — a semantic dispatcher and the request context it
//! resolves through — runs compiled for one capture mode
//! ([`verter_audit::observe::Captured`] or
//! [`verter_audit::observe::Uncaptured`]). [`GraphIn`] carries that mode as a
//! type, so every store operation the unit performs that records optional
//! capture (shard-lock timing on intern, per-request cache counters, cache
//! events) is compiled for the unit's mode: the uncaptured expansion tests a
//! constant and reads no request-context, timing or observer slot. Every
//! other store operation is reached through `Deref`.

use std::marker::PhantomData;

use verter_audit::observe::ObserveExec;

use super::{node_roots, NodeScopeId, SemanticGraphStore, SemanticNodeData, SemanticNodeId};
use crate::semantic_query::QueryError;

/// A borrowed semantic graph store compiled for the capture mode `M`.
pub struct GraphIn<'a, M: ObserveExec> {
    store: &'a SemanticGraphStore,
    mode: PhantomData<M>,
}

impl<M: ObserveExec> Clone for GraphIn<'_, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M: ObserveExec> Copy for GraphIn<'_, M> {}

impl<'a, M: ObserveExec> GraphIn<'a, M> {
    /// The store `store`, reached by execution compiled for `M`.
    #[inline(always)]
    #[must_use]
    pub fn of(store: &'a SemanticGraphStore) -> Self {
        Self {
            store,
            mode: PhantomData,
        }
    }

    /// The underlying store, for the full lifetime of the borrow.
    #[inline(always)]
    #[must_use]
    pub fn store(self) -> &'a SemanticGraphStore {
        self.store
    }

    /// [`SemanticGraphStore::intern_node_in`] under this unit's mode.
    #[inline(always)]
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node(self, data: SemanticNodeData) -> SemanticNodeId {
        self.store.intern_node_in::<M>(data)
    }

    /// [`SemanticGraphStore::intern_node_with_scope_in`] under this unit's
    /// mode.
    #[inline(always)]
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_node_with_scope(
        self,
        data: SemanticNodeData,
        scope: NodeScopeId,
    ) -> SemanticNodeId {
        self.store.intern_node_with_scope_in::<M>(data, scope)
    }

    /// [`SemanticGraphStore::intern_preserving_scope_in`] under this unit's
    /// mode.
    #[inline(always)]
    #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
    pub fn intern_preserving_scope(
        self,
        origin: SemanticNodeId,
        data: SemanticNodeData,
    ) -> SemanticNodeId {
        self.store.intern_preserving_scope_in::<M>(origin, data)
    }

    /// [`SemanticGraphStore::intern_rooted_node_in`] under this unit's mode.
    #[inline(always)]
    pub fn intern_rooted_node(
        self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        operands: &node_roots::NodeRootSet,
    ) -> Result<node_roots::NodeRoot, QueryError> {
        self.store.intern_rooted_node_in::<M>(data, scope, operands)
    }
}

impl<M: ObserveExec> GraphIn<'_, M> {
    /// [`SemanticGraphStore::complete_request_local_in`] under this unit's
    /// mode.
    #[inline(always)]
    pub(crate) fn complete_request_local(
        self,
        attempt: super::producer::ClaimAttempt,
        flags: &crate::resolver_core::resolver_context::RequestFlags,
        output: super::producer::BuildOutput,
        cost_receipt: Option<
            std::sync::Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
        >,
        capture: &mut super::producer::ReadCapture<'_>,
    ) -> (
        super::producer::ValueRead,
        Option<verter_session_query::facts::fact_cache::ReadSetSignature>,
    ) {
        self.store
            .complete_request_local_in::<M>(attempt, flags, output, cost_receipt, capture)
    }
}

impl<M: ObserveExec> GraphIn<'_, M> {
    /// Whether this unit records origin edges: a captured execution does,
    /// and so does every execution of a test or `test-support` build (the
    /// derivation-layer tests read them). An uncaptured execution of an
    /// instrumented production build records none and builds no edge.
    #[inline(always)]
    #[must_use]
    pub const fn records_origin_edges(self) -> bool {
        cfg!(any(test, feature = "test-support")) || M::CAPTURED
    }

    /// Record a derivation/origin edge for `result` (see
    /// `record_origin_edge!`), compiled for this unit's mode.
    #[inline(always)]
    pub(crate) fn record_origin_edge(
        self,
        result: SemanticNodeId,
        kind: crate::semantic_query::OriginEdgeKind,
        sources: std::sync::Arc<[SemanticNodeId]>,
        meta: crate::semantic_query::OriginMeta,
        builder_fence: crate::semantic_query::DepSignature,
    ) {
        self.store
            .record_origin_edge_in::<M>(result, kind, sources, meta, builder_fence);
    }
}

impl<M: ObserveExec> std::ops::Deref for GraphIn<'_, M> {
    type Target = SemanticGraphStore;

    #[inline(always)]
    fn deref(&self) -> &SemanticGraphStore {
        self.store
    }
}

impl<M: ObserveExec> crate::semantic_query::NodeRead for GraphIn<'_, M> {
    fn node_data(&self, node: SemanticNodeId) -> Option<super::NodeData> {
        self.store.node_data(node)
    }
}
