//! Outcome of a per-symbol body demand: a completed run, or a broken lease that
//! lowered nothing.
use crate::source::indexed_call::IndexedFlowCallExpression;
use verter_type_expr::TypeExpr;

use std::sync::Arc;

/// Outcome of a per-symbol body DEMAND ([`DeclBodyMemo::demand_and_commit`])
/// as seen by a caller that needs to DISTINGUISH the two `None`-shaped miss
/// classes (the locator-deref path, which must not collapse a transient
/// ReturnOnly into a cacheable resolution result):
///
/// - [`Ready`](Self::Ready) — the lease-only run completed. `Some` is the
///   demanded decl; `None` is a GENUINE, cacheable miss (the symbol is not
///   inventoried, or the run produced a fatal-parse empty).
/// - [`LeaseMiss`](Self::LeaseMiss) — the lease pin was broken: the demand
///   ran NOTHING and committed NOTHING (`ReturnOnly`). A caller must route
///   this to a no-warm signal, never treat it as a genuine miss.
pub enum DemandOutcome<D> {
    Ready(Option<Arc<D>>),
    LeaseMiss,
}

impl<D> DemandOutcome<D> {
    /// Collapse to the plain `Option` API: a lease-miss reads as `None`. Used
    /// by the broad `Option`-returning demand accessors whose consumers do
    /// NOT distinguish the transient ReturnOnly from a genuine miss (the
    /// per-symbol demand cell already fails closed by evicting the poisoned
    /// cell, so a later demand under a live lease recovers).
    ///
    /// The `LeaseMiss` arm marks the generalized non-cacheability rail: this
    /// is the ONE central collapse point for the plain type / value /
    /// augmentation decl-body accessors, so a transient broken-lease read
    /// consumed by an enclosing traced compute refuses that compute's
    /// shared-cache admission (structural, not per-name). A `Ready(None)`
    /// genuine absence stays cacheable and marks nothing.
    pub fn into_option(self) -> Option<Arc<D>> {
        match self {
            DemandOutcome::Ready(value) => value,
            DemandOutcome::LeaseMiss => {
                crate::facts::reuse::note_non_cacheable_read_fan_out(
                    crate::facts::reuse::NonCacheableReadReason::LeaseMiss,
                );
                None
            }
        }
    }
}

/// The expression-source capability of one exact observed source: function
/// program structure, indexed expression IR and flow-slice content, each
/// demanded lazily from the source's retained parse. Selecting the
/// capability performs no parsing or lowering.
pub trait ExpressionSourceDemand: Send + Sync {
    fn function_program_index(&self) -> Arc<crate::function_program::FunctionProgramIndex>;

    fn indexed_program_expression_ir(
        &self,
        record: &crate::function_program::ProgramExpressionRecord,
    ) -> Option<Arc<verter_type_expr::IndexedValueExpression>>;

    fn indexed_call_expression_over_frame_at(
        &self,
        span: verter_span::Span,
        frame_lowered: Arc<[bool]>,
    ) -> Option<Arc<IndexedFlowCallExpression>>;

    fn function_type_param_clause(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
    ) -> Option<Vec<crate::flow::slice::SliceTypeParam>>;

    fn flow_slice_content(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
        selection: crate::flow::slice::FlowSliceSelection,
        bound: &crate::flow::bundle::BoundFlowGraph,
        policy: crate::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<crate::flow::slice::SliceContent>>;

    fn flow_slice_content_with_context(
        &self,
        entry: &crate::function_program::FunctionProgramEntry,
        selection: Option<crate::flow::slice::FlowSliceSelection>,
        bound: &crate::flow::bundle::BoundFlowGraph,
        context: Option<Arc<crate::flow::slice::NestedFlowContext>>,
        policy: crate::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<crate::flow::slice::SliceContent>>;

    fn flow_capture_authorities(
        &self,
        locators: &[crate::flow::slice::SliceCaptureAuthorityLocator],
    ) -> Option<Vec<Option<Option<crate::flow::slice::SliceCaptureAuthority>>>>;

    fn transient_macro_type_argument(
        &self,
        macro_span: verter_span::Span,
    ) -> DemandOutcome<TypeExpr>;
}
