//! The flow-return fact mapping: usable unfinished values, no value, and
//! aborted reads stay distinct, and no partial read reports Complete.

use super::flow_return_fact;
use crate::semantic_query::{
    ExecutionAbort, FactResult, FactStatus, FlowReturnDegradation, FlowReturnFailure,
    FlowReturnResult, FlowReturnStep, PartialReason, PartialReasonSet, PrimitiveKind,
    ResultCompleteness, SemanticNodeData,
};
use crate::semantic_query_memo::SemanticGraphStore;
use verter_session_query::flow::completion::NormalCompletion;

fn evaluated(degradation: Option<FlowReturnDegradation>) -> FlowReturnStep {
    let graph = SemanticGraphStore::new();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    FlowReturnStep::Complete(FlowReturnResult::new(
        &graph,
        number,
        NormalCompletion::minted_for_fixture(false),
        degradation,
    ))
}

fn status(step: &FlowReturnStep, observed: ResultCompleteness) -> FactStatus {
    flow_return_fact(step, observed)
        .expect("a read without cancellation or a torn view is a fact")
        .status()
}

#[test]
fn an_undegraded_value_with_a_complete_read_is_exact() {
    let step = evaluated(None);
    let fact = flow_return_fact(&step, ResultCompleteness::Complete).unwrap();
    assert!(matches!(fact, FactResult::Complete(_)));
}

#[test]
fn a_degraded_success_is_a_usable_approximation_named_by_its_degradation() {
    for (degradation, reason) in [
        (
            FlowReturnDegradation::UnmodeledPosition,
            PartialReason::FlowReturnUninferred,
        ),
        (
            FlowReturnDegradation::UnappliedWriteEffect,
            PartialReason::FlowReturnUnverified,
        ),
        (
            FlowReturnDegradation::OperationBudget,
            PartialReason::OperationBudget,
        ),
    ] {
        let step = evaluated(Some(degradation));
        let fact = flow_return_fact(&step, ResultCompleteness::Complete).unwrap();
        let FactResult::Approximate { value, causes } = fact else {
            panic!("{degradation:?} keeps its value as an approximation, got {fact:?}");
        };
        assert_eq!(value.degradation(), Some(degradation));
        assert_eq!(causes.get(), reason.bit());
    }
}

#[test]
fn an_undegraded_value_over_a_partial_read_is_never_exact() {
    let step = evaluated(None);
    let observed = ResultCompleteness::partial(PartialReasonSet::FLOW_RETURN_UNVERIFIED);
    assert_eq!(
        status(&step, observed),
        FactStatus::Approximate(
            crate::semantic_query::surface_resolution::NonEmptyReasons::of(
                PartialReason::FlowReturnUnverified
            )
        )
    );
}

#[test]
fn a_no_value_failure_is_unavailable_with_the_no_surface_class() {
    let step = FlowReturnStep::NoValue(FlowReturnFailure::Missing);
    let observed = ResultCompleteness::partial(PartialReasonSet::BUDGET_EXCEEDED);
    let FactStatus::Unavailable(causes) = status(&step, observed) else {
        panic!("a no-value failure has no representation");
    };
    assert_eq!(
        causes.get(),
        PartialReasonSet::FLOW_RETURN_NO_SURFACE.union(PartialReasonSet::BUDGET_EXCEEDED)
    );
    assert!(matches!(
        flow_return_fact(&step, ResultCompleteness::Complete),
        Ok(FactResult::Unavailable { .. })
    ));
}

#[test]
fn a_cancelled_or_torn_read_aborts_instead_of_answering() {
    let value = evaluated(None);
    let no_value = FlowReturnStep::NoValue(FlowReturnFailure::Unresolved);
    for step in [&value, &no_value] {
        assert_eq!(
            flow_return_fact(
                step,
                ResultCompleteness::partial(PartialReasonSet::CANCELLED)
            ),
            Err(ExecutionAbort::Cancelled)
        );
        assert_eq!(
            flow_return_fact(
                step,
                ResultCompleteness::partial(PartialReasonSet::UNSTABLE_STATE)
            ),
            Err(ExecutionAbort::Superseded)
        );
    }
}
