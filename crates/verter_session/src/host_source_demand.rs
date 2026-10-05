//! The request-side face of a source's expression demands.
//!
//! A source read over a retained program carries a refused walk-stack lease
//! BY VALUE ([`WalkedRead`]): the source may run on a lowering worker, where
//! no request is observable. The first request-side consumer of such a read
//! applies it here, on the request's own thread, by marking the request
//! result partial before using the value — so nothing computed from the
//! fail-closed answer is retained as complete.

use std::sync::Arc;

use verter_session_query::source::demand::{DemandOutcome, ExpressionSourceDemand};
use verter_session_query::source::indexed_call::IndexedFlowCallExpression;
use verter_type_expr::TypeExpr;

use crate::decl_body_memo::IndexedExpressionDemand;
use crate::parsed_eval_program::WalkedRead;

/// Take a source read's value, first marking the running request's result
/// partial when a program walk the read depends on was refused its
/// walk-stack lease. No-op on the evidence when no request is installed.
#[inline]
pub(crate) fn consume_walked_read<T>(read: WalkedRead<T>) -> T {
    if read.refusal.is_some() {
        crate::request_context::mark_request_result_partial();
    }
    read.value
}

/// The expression-source capability the host's request contexts hand the
/// engine: one source's demands, with each refused program walk applied to
/// the running request as it is served.
#[derive(Clone)]
pub struct HostExpressionDemand(IndexedExpressionDemand);

impl HostExpressionDemand {
    pub(crate) fn new(source: IndexedExpressionDemand) -> Self {
        Self(source)
    }
}

impl ExpressionSourceDemand for HostExpressionDemand {
    fn function_program_index(
        &self,
    ) -> Arc<verter_session_query::function_program::FunctionProgramIndex> {
        consume_walked_read(self.0.function_program_index())
    }

    fn indexed_program_expression_ir(
        &self,
        record: &verter_session_query::function_program::ProgramExpressionRecord,
    ) -> Option<Arc<verter_type_expr::IndexedValueExpression>> {
        self.0.indexed_program_expression_ir(record)
    }

    fn indexed_call_expression_over_frame_at(
        &self,
        span: verter_span::Span,
        frame_lowered: Arc<[bool]>,
    ) -> Option<Arc<IndexedFlowCallExpression>> {
        consume_walked_read(
            self.0
                .indexed_call_expression_over_frame_at(span, frame_lowered),
        )
    }

    fn function_type_param_clause(
        &self,
        matched: verter_session_query::function_program::FunctionProgramMatch<'_>,
    ) -> Option<Vec<verter_session_query::flow::slice::SliceTypeParam>> {
        consume_walked_read(self.0.function_type_param_clause(matched))
    }

    fn flow_slice_content(
        &self,
        matched: verter_session_query::function_program::FunctionProgramMatch<'_>,
        selection: verter_session_query::flow::slice::FlowSliceSelection,
        bound: &verter_session_query::flow::bundle::BoundFlowGraph,
        policy: verter_session_query::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<verter_session_query::flow::slice::SliceContent>> {
        consume_walked_read(self.0.flow_slice_content(matched, selection, bound, policy))
    }

    fn flow_slice_content_with_context(
        &self,
        matched: verter_session_query::function_program::FunctionProgramMatch<'_>,
        selection: Option<verter_session_query::flow::slice::FlowSliceSelection>,
        bound: &verter_session_query::flow::bundle::BoundFlowGraph,
        context: Option<Arc<verter_session_query::flow::slice::NestedFlowContext>>,
        policy: verter_session_query::flow::policy::FlowReturnPolicy,
    ) -> Option<Arc<verter_session_query::flow::slice::SliceContent>> {
        consume_walked_read(
            self.0
                .flow_slice_content_with_context(matched, selection, bound, context, policy),
        )
    }

    fn flow_capture_authorities(
        &self,
        locators: &[verter_session_query::flow::slice::SliceCaptureAuthorityLocator],
    ) -> Option<Vec<Option<Option<verter_session_query::flow::slice::SliceCaptureAuthority>>>> {
        consume_walked_read(self.0.flow_capture_authorities(locators))
    }

    fn transient_macro_type_argument(
        &self,
        macro_span: verter_span::Span,
    ) -> DemandOutcome<TypeExpr> {
        self.0.transient_macro_type_argument(macro_span)
    }
}

#[cfg(test)]
mod tests {
    use oxc_span::SourceType;
    use verter_parser::oxc_parse::faults::{
        fail_reservations_needing, force_reservations, Reservation,
    };

    use super::*;
    use crate::parsed_eval_program::WalkStackRefused;

    /// A module whose first constant nests `depth` parentheses deep, beside
    /// one function. The depth sizes the walk-stack lease of its index walks
    /// to a reservation no other test makes, so the injected refusal reaches
    /// this test's lowering worker alone.
    fn nested_module(depth: usize) -> String {
        format!(
            "const v = {}1{};\nfunction f() {{ return 1; }}\n",
            "(".repeat(depth),
            ")".repeat(depth)
        )
    }

    /// A refused walk-stack lease for the program index's walks runs on the
    /// lowering worker, where no request is observable: the refusal travels
    /// back by value, the fail-closed empty index is not memoized, and the
    /// request-side capability marks the request result partial on the
    /// request's own thread. A later demand whose lease is granted indexes
    /// the program and is clean.
    #[test]
    fn a_refused_index_walk_reaches_the_request_by_value() {
        let source = nested_module(97);
        let needed = verter_parser::oxc_parse::parse_stack_bytes(&source, SourceType::ts());
        let state = crate::resolver_core::ShallowFileState::service_backed_for_test_at(
            "/ws/refused_index_walk.ts",
            &source,
        );
        let memo = state.decl_bodies();
        let _request =
            crate::request_context::install_test_request_for("/ws/refused_index_walk.ts");
        let _forcing = force_reservations(&[Reservation::Lease], needed);

        fail_reservations_needing(Reservation::Lease, needed, 1);
        let refused = memo.function_program_index();
        fail_reservations_needing(Reservation::Lease, needed, 0);
        assert_eq!(refused.refusal, Some(WalkStackRefused));
        assert_eq!(
            refused.value.len(),
            0,
            "the refused read is the empty index"
        );
        assert!(
            !crate::request_context::current_request_result_is_partial(),
            "the source side marks nothing; its consumer applies the refusal"
        );

        fail_reservations_needing(Reservation::Lease, needed, 1);
        let served =
            HostExpressionDemand::new(memo.indexed_expression_demand()).function_program_index();
        fail_reservations_needing(Reservation::Lease, needed, 0);
        assert_eq!(served.len(), 0, "the refused index was not memoized");
        assert!(
            crate::request_context::current_request_result_is_partial(),
            "serving a refused read marks the request result partial"
        );

        let granted = memo.function_program_index();
        assert_eq!(granted.refusal, None);
        assert!(
            granted.value.matches_named("f").next().is_some(),
            "a granted lease indexes the program"
        );
    }
}
