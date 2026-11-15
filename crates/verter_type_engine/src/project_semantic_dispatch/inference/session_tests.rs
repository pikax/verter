use super::*;

fn session(params: &[SemanticNodeId]) -> InferenceSession {
    InferenceSession::new(
        SessionId(1),
        InferenceSessionSetup::new(
            crate::semantic_query_memo::test_support::synthetic_operand_graph(),
            Arc::from(
                params
                    .iter()
                    .map(|&param| InferenceInfoSetup::new(param, Arc::from("T")))
                    .collect::<Vec<_>>(),
            ),
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
    )
}

#[test]
fn repeated_inference_candidates_are_stored_once_per_list() {
    let param = SemanticNodeId(101);
    let candidate = SemanticNodeId(201);
    let mut session = session(&[param]);
    for variance in [VariancePhase::Covariant, VariancePhase::Contravariant] {
        // bounded-loop: 500 deposits of one candidate per variance list.
        for _ in 0..500 {
            assert!(session.deposit(
                param,
                candidate,
                InferenceCandidatePriority::Argument,
                variance,
            ));
        }
        assert_eq!(
            session.infos[0]
                .candidates
                .iter()
                .filter(|candidate| candidate.variance == variance)
                .count(),
            1,
        );
    }
    let inputs = session.fixation_inputs_for(&[param]).unwrap();
    assert_eq!(inputs[0].candidates.covariant, [candidate]);
    assert_eq!(inputs[0].candidates.contravariant, [candidate]);
}

#[test]
fn distinct_inference_candidates_keep_first_occurrence_order() {
    let param = SemanticNodeId(101);
    let first = SemanticNodeId(203);
    let second = SemanticNodeId(201);
    let third = SemanticNodeId(202);
    let mut session = session(&[param]);
    for variance in [VariancePhase::Covariant, VariancePhase::Contravariant] {
        for candidate in [first, second, first, third, second, third] {
            assert!(session.deposit(
                param,
                candidate,
                InferenceCandidatePriority::Argument,
                variance,
            ));
        }
    }
    let inputs = session.fixation_inputs_for(&[param]).unwrap();
    assert_eq!(inputs[0].candidates.covariant, [first, second, third]);
    assert_eq!(inputs[0].candidates.contravariant, [first, second, third]);
}

#[test]
fn repeated_candidate_trials_remain_observable_and_roll_back() {
    let param = SemanticNodeId(101);
    let other = SemanticNodeId(102);
    let candidate = SemanticNodeId(201);
    let priority = InferenceCandidatePriority::Argument;
    let second_priority = InferenceCandidatePriority::ReturnType;
    let mut session = session(&[param, other]);
    for (parameter, priority) in [
        (param, priority),
        (param, second_priority),
        (other, priority),
    ] {
        assert!(session.deposit(parameter, candidate, priority, VariancePhase::Covariant));
    }
    let checkpoint = session.checkpoint();
    assert!(!session.inferred_since(&checkpoint, param, priority));
    assert!(session.deposit(param, candidate, priority, VariancePhase::Covariant));
    assert!(!session.inferred_since(&checkpoint, param, second_priority));
    let one_parameter = session.checkpoint();
    assert!(session.deposit(param, candidate, second_priority, VariancePhase::Covariant));
    let nested = session.checkpoint();
    assert!(session.deposit(other, candidate, priority, VariancePhase::Covariant));
    assert!(session.deposit(param, candidate, priority, VariancePhase::Covariant));
    assert!(session.inferred_since(&nested, param, priority));
    assert!(session.inferred_since(&nested, other, priority));
    assert!(!session.inferred_since(&nested, param, second_priority));
    session.rollback_to(&nested);
    assert!(!session.inferred_since(&nested, param, priority));
    assert!(!session.inferred_since(&nested, other, priority));
    assert!(session.inferred_since(&checkpoint, param, priority));
    assert!(session.inferred_since(&checkpoint, param, second_priority));
    session.rollback_to(&one_parameter);
    assert!(session.inferred_since(&checkpoint, param, priority));
    assert!(!session.inferred_since(&checkpoint, param, second_priority));
    session.rollback_to(&checkpoint);
    assert!(!session.inferred_since(&checkpoint, param, priority));
    assert!(!session.inferred_since(&checkpoint, param, second_priority));
    assert!(!session.inferred_since(&checkpoint, other, priority));
}

#[test]
fn inference_candidate_lists_remain_separate_across_priorities() {
    let param = SemanticNodeId(101);
    let candidate = SemanticNodeId(201);
    let mut session = session(&[param]);
    let weaker = InferenceCandidatePriority::ReturnType;
    let stronger = InferenceCandidatePriority::NakedTypeParameter;
    assert!(session.deposit(param, candidate, weaker, VariancePhase::Covariant));
    let checkpoint = session.checkpoint();
    assert!(session.deposit(param, candidate, stronger, VariancePhase::Invariant));
    assert!(session.deposit(param, candidate, stronger, VariancePhase::Covariant));
    assert_eq!(session.infos[0].candidates.len(), 2);
    let inputs = session.fixation_inputs_for(&[param]).unwrap();
    assert_eq!(inputs[0].candidates.priority, Some(stronger));
    assert_eq!(inputs[0].candidates.covariant, [candidate]);
    session.rollback_to(&checkpoint);
    let inputs = session.fixation_inputs_for(&[param]).unwrap();
    assert_eq!(inputs[0].candidates.priority, Some(weaker));
    assert_eq!(inputs[0].candidates.covariant, [candidate]);
}
