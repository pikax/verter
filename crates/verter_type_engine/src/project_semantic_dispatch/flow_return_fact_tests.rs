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

#[test]
fn abandoned_read_observations_restore_the_degraded_counter() {
    use crate::request_context::{
        ColdComputeCompletenessScope, DeferredPartialStickyScope, ExecutionValidityScope,
    };
    let degraded = verter_execution::sync_cell::SyncU32::new(3);
    let frames = verter_execution::sync_cell::SyncRefCell::new(smallvec::SmallVec::new());
    let scope = || super::ReadObservationScope {
        validity: ExecutionValidityScope::enter(),
        sticky: DeferredPartialStickyScope::enter(),
        completeness: ColdComputeCompletenessScope::enter(),
        frame: super::BuildLocalTaintGuard::push(&frames),
        degraded: super::DegradedCounterScope {
            counter: &degraded,
            before: degraded.get(),
        },
    };
    {
        let _read = scope();
        degraded.set(5);
    }
    assert_eq!(
        degraded.get(),
        3,
        "an abandoned probe cannot taint its parent"
    );
    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _read = scope();
        degraded.set(7);
        panic!("abandon the probe during unwinding");
    }));
    assert!(unwind.is_err());
    assert_eq!(degraded.get(), 3);
    let read = scope();
    degraded.set(8);
    let observed = read.finish(());
    assert_eq!(observed.degraded_member_reads, 5);
    assert_eq!(
        degraded.get(),
        3,
        "finishing retains only the private delta"
    );
    assert!(frames.borrow().is_empty());
}

fn status(step: &FlowReturnStep, observed: ResultCompleteness) -> FactStatus {
    flow_return_fact(step, observed)
        .expect("a read without cancellation or a torn view is a fact")
        .status()
}

#[test]
fn declining_an_aborted_probe_cannot_extract_its_apparent_value() {
    let read = super::ObservedRead {
        degraded_member_reads: 0,
        value: evaluated(None),
        abort: Some(ExecutionAbort::Superseded),
        completeness: ResultCompleteness::Complete,
        frame: super::BuildLocalTaint::default(),
    };
    assert_eq!(read.fact(), Err(ExecutionAbort::Superseded));
    assert!(matches!(read.discard(), Err(ExecutionAbort::Superseded)));
}

#[test]
fn a_complete_payload_cannot_certify_a_probe_with_incomplete_observations() {
    for (completeness, frame, degraded_member_reads) in [
        (
            ResultCompleteness::partial(PartialReasonSet::BUDGET_EXCEEDED),
            super::BuildLocalTaint::default(),
            0,
        ),
        (
            ResultCompleteness::Complete,
            super::BuildLocalTaint {
                result_is_partial: true,
                ..Default::default()
            },
            0,
        ),
        (
            ResultCompleteness::Complete,
            super::BuildLocalTaint::default(),
            1,
        ),
    ] {
        let read = super::ObservedRead {
            value: evaluated(None),
            abort: None,
            completeness,
            frame,
            degraded_member_reads,
        };
        assert!(!read.observations_are_complete());
    }
}

#[test]
fn discarded_quality_observations_cannot_discard_execution_aborts() {
    use crate::request_context::{
        current_execution_abort, observe_execution_abort, ColdComputeCompletenessScope,
        ExecutionValidityScope,
    };
    for abort in [
        ExecutionAbort::Cancelled,
        ExecutionAbort::Superseded,
        ExecutionAbort::Shutdown,
    ] {
        {
            let _outer = ExecutionValidityScope::enter();
            {
                let _inner = ExecutionValidityScope::enter();
                let quality = ColdComputeCompletenessScope::enter();
                observe_execution_abort(abort);
                quality.discard();
            }
            assert_eq!(current_execution_abort(), Some(abort));
        }
        let _retry = ExecutionValidityScope::enter();
        assert_eq!(
            current_execution_abort(),
            None,
            "a new execution has its own validity"
        );
    }
}

#[test]
fn unnamed_partial_quality_cannot_become_a_complete_fact() {
    let partial = ResultCompleteness::Partial(PartialReasonSet::empty());
    let step = evaluated(None);
    assert!(matches!(
        flow_return_fact(&step, partial),
        Ok(FactResult::Approximate { .. })
    ));
    let FlowReturnStep::Complete(value) = step else {
        unreachable!()
    };
    assert!(matches!(
        value.with_observed_completeness(partial).fact(),
        FactResult::Approximate { .. }
    ));
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
            ),
            None,
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
fn refused_preparation_does_not_abort_an_independently_evaluated_value() {
    let step = evaluated(None);
    let fact = flow_return_fact(
        &step,
        ResultCompleteness::partial(PartialReasonSet::UNSTABLE_STATE),
    )
    .expect("a preparation cause alone does not invalidate evaluated evidence");
    assert!(matches!(fact, FactResult::Approximate { .. }));
}

#[test]
fn a_cancelled_or_torn_read_aborts_instead_of_answering() {
    for abort in [
        ExecutionAbort::Cancelled,
        ExecutionAbort::Superseded,
        ExecutionAbort::Shutdown,
    ] {
        let step = FlowReturnStep::Aborted(abort);
        assert_eq!(
            flow_return_fact(&step, ResultCompleteness::Complete),
            Err(abort)
        );
        assert_eq!(
            flow_return_fact(
                &step,
                ResultCompleteness::partial(PartialReasonSet::FLOW_RETURN_UNVERIFIED)
            ),
            Err(abort)
        );
    }
}

#[test]
fn independent_adoption_preserves_observed_value_partiality() {
    let graph = SemanticGraphStore::new();
    let node = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let key = super::super::dispatch_txn::dispatch_txn_tests::flow_return_key();
    for partial in [false, true] {
        let result = FlowReturnResult::new(
            &graph,
            node,
            NormalCompletion::minted_for_fixture(false),
            None,
        )
        .with_independent_member(super::super::flow_return::ExactMemberValue::for_tests(
            key.clone(),
            node,
        ));
        let completeness = if partial {
            ResultCompleteness::partial(PartialReasonSet::MISSING_DEPENDENCY)
        } else {
            ResultCompleteness::Complete
        };
        let read = super::ObservedRead {
            value: FlowReturnStep::Complete(result),
            abort: None,
            completeness,
            frame: super::BuildLocalTaint {
                result_is_partial: partial,
                partial_reasons: completeness.reasons(),
                cache_suppress: true,
                ..Default::default()
            },
            degraded_member_reads: u32::from(partial),
        }
        .into_independent_member(&key)
        .unwrap();
        assert_eq!(read.value, node);
        assert_eq!(
            read.completeness, completeness,
            "an independent certificate cannot discard observed value partiality"
        );
        assert_eq!(read.frame.result_is_partial, partial);
        assert_eq!(read.frame.partial_reasons, completeness.reasons());
        assert_eq!(read.degraded_member_reads, u32::from(partial));
        assert!(read.frame.cache_suppress);
    }
}
