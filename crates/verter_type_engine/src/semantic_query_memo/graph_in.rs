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
    #[cfg(feature = "semantic-observe")]
    observation: &'a crate::request_observers::CaptureTargets,
}

impl<M: ObserveExec> Clone for GraphIn<'_, M> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M: ObserveExec> Copy for GraphIn<'_, M> {}

impl<M: ObserveExec> AsRef<SemanticGraphStore> for GraphIn<'_, M> {
    fn as_ref(&self) -> &SemanticGraphStore {
        self.store
    }
}

impl<'a, M: ObserveExec> GraphIn<'a, M> {
    /// The store `store`, reached by execution compiled for `M`.
    #[inline(always)]
    #[must_use]
    pub fn observed(
        store: &'a SemanticGraphStore,
        observation: &'a crate::request_observers::CaptureTargets,
    ) -> Self {
        let _ = observation;
        Self {
            store,
            mode: PhantomData,
            #[cfg(feature = "semantic-observe")]
            observation,
        }
    }

    #[inline(always)]
    fn capture_targets(self) -> &'a crate::request_observers::CaptureTargets {
        #[cfg(feature = "semantic-observe")]
        {
            self.observation
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            &crate::request_observers::CaptureTargets::EMPTY
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
        self.store.intern_node_in::<M>(self.capture_targets(), data)
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
        self.store
            .intern_node_with_scope_in::<M>(self.capture_targets(), data, scope)
    }

    construction_ordinal_seam! {
        /// [`SemanticGraphStore::intern_preserving_scope_in`] under this
        /// unit's mode. Copying an origin's scope by bare ordinal is a
        /// construction lane: outside the engine it exists only for tests.
        #[inline(always)]
        #[must_use = "the returned SemanticNodeId is the only way to reach the interned node"]
        fn intern_preserving_scope(
            self,
            origin: SemanticNodeId,
            data: SemanticNodeData,
        ) -> SemanticNodeId {
            self.store
                .intern_preserving_scope_in::<M>(self.capture_targets(), origin, data)
        }
    }

    /// [`SemanticGraphStore::intern_rooted_node_in`] under this unit's mode.
    #[inline(always)]
    pub fn intern_rooted_node(
        self,
        data: SemanticNodeData,
        scope: NodeScopeId,
        operands: &node_roots::NodeRootSet,
    ) -> Result<node_roots::NodeRoot, QueryError> {
        self.store
            .intern_rooted_node_in::<M>(self.capture_targets(), data, scope, operands)
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
        self.store.complete_request_local_in::<M>(
            self.capture_targets(),
            attempt,
            flags,
            output,
            cost_receipt,
            capture,
        )
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
        self.store.record_origin_edge_in::<M>(
            self.capture_targets(),
            result,
            kind,
            sources,
            meta,
            builder_fence,
        );
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

impl<M: ObserveExec> GraphIn<'_, M> {
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_instantiate(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_instantiate();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_awaited_normalize(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_awaited_normalize();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_async_return_payload(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_async_return_payload();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_macro_payload_owner_read(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_macro_payload_owner_read();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_conditional_decided(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_conditional_decided();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_conditional_deferred(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_conditional_deferred();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_branch_selection_true(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_branch_selection_true();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_branch_selection_false(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_branch_selection_false();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_decl_subexpression_lowering(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_decl_subexpression_lowering();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_relation_check(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_relation_check();
        }
    }
    /// Record optional telemetry for this execution.
    #[inline(always)]
    pub fn record_mapped_per_k_materialization(self) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_mapped_per_k_materialization();
        }
    }
    /// Record a path sample only for captured execution or test work accounting.
    #[inline(always)]
    pub fn record_path_length(self, length: u32) {
        if cfg!(any(test, feature = "test-support")) || M::CAPTURED {
            self.store.record_path_length(length);
        }
    }
}

impl<'a> GraphIn<'a, verter_audit::observe::Uncaptured> {
    /// An uncaptured fixture's graph, with no observation receivers.
    pub fn of(store: &'a SemanticGraphStore) -> Self {
        Self::observed(store, &crate::request_observers::CaptureTargets::EMPTY)
    }
}

impl<M: ObserveExec> GraphIn<'_, M> {
    /// A substitution memo read under this execution's telemetry mode.
    pub(crate) fn substitute_memo_get_priced(
        self,
        node: SemanticNodeId,
        parameter: SemanticNodeId,
        arg: SemanticNodeId,
    ) -> Option<(
        SemanticNodeId,
        std::sync::Arc<crate::project_semantic_dispatch::cost_receipt::DemandCostReceipt>,
    )> {
        self.store
            .substitute_memo_get_priced::<M>(node, parameter, arg)
    }

    /// A validated deferred-evaluation read under this execution's telemetry mode.
    pub(crate) fn evaluate_deferred_memo_get(
        self,
        node: SemanticNodeId,
        context: crate::semantic_query::ProjectionReductionContext,
        demand: crate::project_semantic_dispatch::evaluate::DeferredEvaluationDemand,
        ctx: &dyn crate::resolver_core::fact_validation_port::FactValidation,
    ) -> Option<super::hash_cons_memos::DeferredEvaluationRead> {
        self.store
            .evaluate_deferred_memo_get::<M>(node, context, demand, ctx)
    }
}
