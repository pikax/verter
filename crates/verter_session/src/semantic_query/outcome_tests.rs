use super::outcome::{
    DependencyProofId, IncompleteReason, OutcomeEvidence, OutcomeEvidenceId, QueryOutcome, Ready,
    ResultEvaluationContextId, CONTEXT_FREE_EVALUATION, CONTEXT_FREE_EVIDENCE, EMPTY_DIAGNOSTICS,
    EMPTY_PROOF, NO_RECOVERY,
};
use super::{QueryError, QueryResult};

#[test]
fn reserved_ids_are_stable_and_distinct_from_recorded_evidence() {
    assert_eq!(CONTEXT_FREE_EVIDENCE, OutcomeEvidenceId::from_raw(0));
    assert!(CONTEXT_FREE_EVIDENCE.is_context_free());
    assert!(EMPTY_DIAGNOSTICS.is_empty());
    assert!(NO_RECOVERY.is_none());
    assert!(EMPTY_PROOF.is_empty());
    let recorded = OutcomeEvidenceId::from_raw(1);
    assert!(!recorded.is_context_free());
    assert_ne!(recorded, CONTEXT_FREE_EVIDENCE);
    assert_eq!(
        CONTEXT_FREE_EVALUATION,
        ResultEvaluationContextId::from_raw(0)
    );
    assert!(CONTEXT_FREE_EVALUATION.is_context_free());
    assert!(!ResultEvaluationContextId::from_raw(1).is_context_free());
}

#[test]
fn ready_never_claims_an_empty_proof_when_evidence_recorded_dependencies() {
    let evidence = OutcomeEvidence {
        proof: DependencyProofId::from_raw(7),
        diagnostics: EMPTY_DIAGNOSTICS,
        recovery: NO_RECOVERY,
    };
    assert!(!evidence.proof.is_empty());
    let ready = Ready {
        value: 1u8,
        evidence: OutcomeEvidenceId::from_raw(3),
    };
    assert!(!ready.evidence.is_context_free());
}

#[test]
fn complete_value_maps_to_ready_without_new_allocation() {
    let result = QueryResult::Value(42u32);
    match result.to_outcome(CONTEXT_FREE_EVIDENCE) {
        QueryOutcome::Ready(Ready { value, evidence }) => {
            assert_eq!(*value, 42);
            assert_eq!(evidence, CONTEXT_FREE_EVIDENCE);
        }
        other => panic!("expected Ready, got {other:?}"),
    }
}

#[test]
fn cancelled_and_budget_map_to_closed_incomplete_reasons() {
    let cancelled = QueryResult::<u8>::Error(QueryError::Cancelled);
    assert!(matches!(
        cancelled.to_outcome(CONTEXT_FREE_EVIDENCE),
        QueryOutcome::Incomplete(IncompleteReason::Cancelled)
    ));
    let budget = QueryResult::<u8>::Error(QueryError::BudgetExceeded(
        crate::resolver_core::BudgetExceededFailure {
            domain: crate::resolver_core::BudgetDomain::ProjectionOperation,
            limit: 1,
            actual: 2,
            context: "outcome-map".into(),
        },
    ));
    assert!(matches!(
        budget.to_outcome(CONTEXT_FREE_EVIDENCE),
        QueryOutcome::Incomplete(IncompleteReason::Budget)
    ));
}

#[test]
fn context_free_evidence_record_uses_reserved_empty_ids() {
    let evidence = OutcomeEvidence::context_free();
    assert!(evidence.proof.is_empty());
    assert!(evidence.diagnostics.is_empty());
    assert!(evidence.recovery.is_none());
}
