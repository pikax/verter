//! @ai-generated - `CheckerDispatchTransaction` unit tests: session setup
//! fingerprinting, provisional-verdict stability, session-delta marking, and
//! nested re-discharge context save/restore over the tagged substitution
//! table.

use super::*;

fn setup(
    param_node: SemanticNodeId,
    variance_phase: VariancePhase,
    pass_kind: InferencePassKind,
    candidate_priority: InferenceCandidatePriority,
    no_infer_mask: NoInferMask,
    const_param_policy: ConstParamPolicy,
    contextual_inference_mode: ContextualInferenceMode,
) -> InferenceSessionSetup {
    InferenceSessionSetup::new(
        Arc::from(vec![InferenceInfoSetup::new(param_node, Arc::from("T"))].into_boxed_slice()),
        variance_phase,
        pass_kind,
        candidate_priority,
        no_infer_mask,
        const_param_policy,
        contextual_inference_mode,
    )
}

#[test]
fn inference_context_key_projects_every_session_setup_axis() {
    let active_param = SemanticNodeId(101);
    let baseline = setup(
        active_param,
        VariancePhase::Covariant,
        InferencePassKind::Ordinary,
        InferenceCandidatePriority::Argument,
        NoInferMask::empty(),
        ConstParamPolicy::NonConst,
        ContextualInferenceMode::None,
    );
    let baseline_key = baseline.context_key().clone();

    // Exhaustive patterns make a new field on either authoritative setup
    // record a compile error until this behavioral guard classifies it.
    let InferenceSessionSetup {
        context_key: _,
        infos,
    } = baseline.clone();
    let [info] = infos.as_ref() else {
        panic!("the fixture has exactly one inferable parameter");
    };
    let InferenceInfoSetup {
        param_node: _,
        param_name: _,
        const_policy: _,
        has_constraint: _,
    } = info;
    let InferenceContextKey {
        inferable_params: _,
        variance_phase: _,
        pass_kind: _,
        candidate_priority: _,
        no_infer_mask: _,
        const_param_policy: _,
        contextual_inference_mode: _,
    } = baseline_key.clone();

    let variants = [
        setup(
            SemanticNodeId(102),
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Contravariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Covariant,
            InferencePassKind::ReverseHomomorphicMapped,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::ReturnType,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask(1),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::Const,
            ContextualInferenceMode::None,
        ),
        setup(
            active_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::Contextual,
        ),
    ];
    for variant in &variants {
        assert_ne!(
            variant.context_key(),
            &baseline_key,
            "changing any setup axis must select a distinct relation key"
        );
    }
    let distinct: std::collections::HashSet<_> = std::iter::once(baseline_key.clone())
        .chain(variants.iter().map(|variant| variant.context_key().clone()))
        .collect();
    assert_eq!(
        distinct.len(),
        variants.len() + 1,
        "each setup-axis mutation must have a distinct fingerprint"
    );

    let mut session = InferenceSession::new(SessionId(1), baseline, None);
    assert_eq!(session.context_key(), &baseline_key);
    assert!(session.deposit(
        active_param,
        SemanticNodeId(201),
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant,
    ));
    assert!(
        !session.deposit(
            SemanticNodeId(999),
            SemanticNodeId(202),
            InferenceCandidatePriority::Argument,
            VariancePhase::Covariant,
        ),
        "an infer absent from the frozen setup must not accept a candidate"
    );
    assert_eq!(
        session.context_key(),
        &baseline_key,
        "candidate collection cannot mutate the frozen setup key"
    );
    let _bindings = session
        .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(SemanticNodeId(0)))
        .expect("collecting session stages");
    assert_eq!(
        session.context_key(),
        &baseline_key,
        "fixation cannot mutate the frozen setup key"
    );
}

fn relation_key(seed: u64) -> RelateMemoKey {
    RelateMemoKey::assignable(
        SemanticNodeId(seed),
        SemanticNodeId(seed + 1),
        crate::semantic_query::RelationContext::default(),
    )
}

fn relate_identity(seed: u64, occurrence: InferenceOccurrence) -> ObligationIdentity {
    ObligationIdentity::Relate {
        key: relation_key(seed),
        occurrence,
    }
}

/// A zero-env `FlowReturn` key over one served declaration-body position.
fn flow_return_key() -> FlowReturnKey {
    FlowReturnKey {
        function: crate::semantic_query::FlowFunctionSlotIdentity {
            declaration_slot: crate::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                Arc::from("/ws/txn.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("f"),
                0,
                crate::semantic_query::HashValue::default(),
                crate::semantic_query::HashValue::default(),
            ),
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        context: crate::semantic_query::FlowReturnContext {
            parse_env_hash: crate::semantic_query::HashValue::default(),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
            type_env_hash: crate::semantic_query::HashValue::default(),
            lib_env_hash: crate::semantic_query::HashValue::default(),
            project_identity: crate::semantic_query::HashValue::default(),
            type_substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
            policy: crate::semantic_query::FlowReturnPolicy {},
        },
        result_contract: super::super::flow_solve::flow_return_result_contract_id(),
    }
}

/// A zero-env, argument-free `ResolveCall` key over one call site.
fn resolve_call_key() -> crate::semantic_query::ResolveCallKey {
    crate::semantic_query::ResolveCallKey {
        point: crate::semantic_query::ProgramPointId {
            canonical_id: Arc::from("/ws/txn.ts"),
            offset: 11,
        },
        callee: SemanticNodeId(4),
        kind: crate::semantic_query::CallKind::Call,
        receiver: None,
        args: Arc::from(Vec::new().into_boxed_slice()),
        explicit_type_args: Arc::from(Vec::new().into_boxed_slice()),
        flow: crate::semantic_query::FlowNarrowingKey::empty(),
        context: crate::semantic_query::ResolveCallContext {
            parse_env_hash: crate::semantic_query::HashValue::default(),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
            type_env_hash: crate::semantic_query::HashValue::default(),
            lib_env_hash: crate::semantic_query::HashValue::default(),
            project_identity: crate::semantic_query::HashValue::default(),
            substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
        },
    }
}

#[test]
fn repeated_infer_sites_share_one_setup_record_and_one_fixed_binding() {
    let param = SemanticNodeId(501);
    let setup = InferenceSessionSetup::new(
        Arc::from(
            vec![
                InferenceInfoSetup::new(param, Arc::from("T")),
                InferenceInfoSetup::new(param, Arc::from("T")),
            ]
            .into_boxed_slice(),
        ),
        VariancePhase::Covariant,
        InferencePassKind::Ordinary,
        InferenceCandidatePriority::Argument,
        NoInferMask::empty(),
        ConstParamPolicy::NonConst,
        ContextualInferenceMode::None,
    );
    assert_eq!(
        setup.infos.len(),
        1,
        "the immutable setup authority must deduplicate repeated sites by exact param"
    );
    let mut session = InferenceSession::new(SessionId(17), setup, None);
    assert!(session.deposit(
        param,
        SemanticNodeId(601),
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant,
    ));
    assert!(session.deposit(
        param,
        SemanticNodeId(602),
        InferenceCandidatePriority::ReturnType,
        VariancePhase::Contravariant,
    ));
    let fixed = session
        .stage_fixation(|nodes, _| nodes[0])
        .expect("collecting session stages");
    assert_eq!(
        fixed.len(),
        1,
        "fixation must emit one binding for one exact infer parameter"
    );
    assert_eq!(fixed[0].param, param);
}

/// A staged session has a fixed immutable snapshot, is no longer active for
/// deposits, and commits only through the staged state.
///
/// Mutation recipe: make `active_session()` include `StagedDeterministic`, or
/// let `deposit()` accept a staged session; either mutation makes this test
/// fail while the unmodified control remains green.
#[test]
fn staged_session_is_deposit_inactive_and_commits_exactly_once() {
    let param = SemanticNodeId(621);
    let setup = setup(
        param,
        VariancePhase::Covariant,
        InferencePassKind::Ordinary,
        InferenceCandidatePriority::Argument,
        NoInferMask::empty(),
        ConstParamPolicy::NonConst,
        ContextualInferenceMode::None,
    );
    let mut txn = CheckerDispatchTransaction::default();
    let session_id = txn.push_collecting_session(setup, None);
    assert_eq!(
        txn.active_session().map(|session| session.id),
        Some(session_id)
    );
    assert!(txn
        .active_session_mut()
        .expect("collecting session")
        .deposit(
            param,
            SemanticNodeId(622),
            InferenceCandidatePriority::Argument,
            VariancePhase::Covariant,
        ));

    let session = txn
        .relation
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .expect("pushed session");
    assert!(
        !session.commit_completed(),
        "a collecting session cannot commit before fixation"
    );
    let staged = session
        .stage_fixation(|nodes, _| nodes[0])
        .expect("collecting session stages once");
    assert_eq!(session.state, InferenceSessionState::StagedDeterministic);
    assert_eq!(staged[0].bound, SemanticNodeId(622));
    assert!(
        !session.deposit(
            param,
            SemanticNodeId(623),
            InferenceCandidatePriority::Argument,
            VariancePhase::Covariant,
        ),
        "a staged binding snapshot must be immutable"
    );
    assert!(session.stage_fixation(|nodes, _| nodes[0]).is_none());
    assert!(txn.active_session().is_none());

    let session = txn
        .relation
        .sessions
        .iter_mut()
        .find(|session| session.id == session_id)
        .expect("staged session");
    assert!(session.commit_completed());
    assert_eq!(session.state, InferenceSessionState::CommittedDeterministic);
    assert!(
        !session.commit_completed(),
        "commit is a one-way transition"
    );
}

/// Collecting and staged sessions may be abandoned; committed sessions are
/// past the rollback boundary.
///
/// Mutation recipe: allow `abandon()` to rewrite `CommittedDeterministic`, or
/// refuse the staged-to-abandoned edge; the corresponding assertion fails.
#[test]
fn abandonment_respects_the_commit_boundary() {
    let param = SemanticNodeId(625);
    let make = || {
        InferenceSession::new(
            SessionId(91),
            setup(
                param,
                VariancePhase::Covariant,
                InferencePassKind::Ordinary,
                InferenceCandidatePriority::Argument,
                NoInferMask::empty(),
                ConstParamPolicy::NonConst,
                ContextualInferenceMode::None,
            ),
            None,
        )
    };

    let mut collecting = make();
    assert!(collecting.abandon());
    assert_eq!(collecting.state, InferenceSessionState::Abandoned);
    assert!(!collecting.deposit(
        param,
        SemanticNodeId(626),
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant,
    ));

    let mut staged = make();
    staged
        .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(SemanticNodeId(0)))
        .expect("collecting session stages");
    assert!(staged.abandon());
    assert_eq!(staged.state, InferenceSessionState::Abandoned);

    let mut committed = make();
    committed
        .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(SemanticNodeId(0)))
        .expect("collecting session stages");
    assert!(committed.commit_completed());
    assert!(!committed.abandon());
    assert_eq!(
        committed.state,
        InferenceSessionState::CommittedDeterministic
    );
}

/// `outer<T>(x: T) { return id(x) }` / `id<U>(y: U): U`: a nested call owns
/// a distinct collecting session even while `outer` is collecting. Its `U`
/// deposit fixes only `id`; after the nested session stages, `outer` resumes
/// with only its original `T` candidate.
///
/// Mutation recipe: make `push_collecting_session()` reuse the current active
/// session instead of always pushing; the distinct-id assertion fails. Make
/// `active_session()` return the outermost collector; the inner-deposit and
/// final-binding assertions fail. The unmodified control remains green.
#[test]
fn nested_call_owned_session_isolates_outer_and_resumes_it() {
    let outer_param = SemanticNodeId(631);
    let inner_param = SemanticNodeId(632);
    let outer_candidate = SemanticNodeId(633);
    let inner_candidate = SemanticNodeId(634);
    let mut txn = CheckerDispatchTransaction::default();

    let outer_id = txn.push_collecting_session(
        setup(
            outer_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        None,
    );
    assert!(txn.active_session_mut().expect("outer collector").deposit(
        outer_param,
        outer_candidate,
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant,
    ));

    let inner_id = txn.push_collecting_session(
        setup(
            inner_param,
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        None,
    );
    assert_ne!(inner_id, outer_id, "the nested call must own its session");
    assert_eq!(
        txn.active_session().map(|session| session.id),
        Some(inner_id)
    );
    assert!(txn.active_session_mut().expect("inner collector").deposit(
        inner_param,
        inner_candidate,
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant,
    ));

    let inner = txn
        .relation
        .sessions
        .iter_mut()
        .find(|session| session.id == inner_id)
        .expect("inner session");
    let inner_bindings = inner
        .stage_fixation(|nodes, _| nodes[0])
        .expect("inner collector stages");
    assert_eq!(inner_bindings[0].param, inner_param);
    assert_eq!(inner_bindings[0].bound, inner_candidate);
    assert_eq!(
        txn.active_session().map(|session| session.id),
        Some(outer_id)
    );

    let outer = txn
        .relation
        .sessions
        .iter_mut()
        .find(|session| session.id == outer_id)
        .expect("outer session");
    let outer_bindings = outer
        .stage_fixation(|nodes, _| nodes[0])
        .expect("outer collector stages after nested call");
    assert_eq!(outer_bindings[0].param, outer_param);
    assert_eq!(outer_bindings[0].bound, outer_candidate);
    assert_ne!(outer_bindings[0].bound, inner_candidate);
}

/// A final applicability recheck suppresses pre-existing collectors, while a
/// truly nested call may still open and use a fresh session above the barrier.
/// Mutation recipe: use a boolean global suppression flag; the nested session
/// is invisible and this test fails.
#[test]
fn binding_disabled_barrier_hides_outer_but_allows_nested_call_session() {
    let mut txn = CheckerDispatchTransaction::default();
    let outer = txn.push_collecting_session(
        setup(
            SemanticNodeId(641),
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        None,
    );
    txn.begin_binding_disabled();
    assert!(txn.active_session().is_none());
    let nested = txn.push_collecting_session(
        setup(
            SemanticNodeId(642),
            VariancePhase::Covariant,
            InferencePassKind::CallApplicability,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        ),
        None,
    );
    assert_eq!(txn.active_session().map(|session| session.id), Some(nested));
    txn.end_binding_disabled();
    assert_eq!(txn.active_session().map(|session| session.id), Some(nested));
    assert_ne!(nested, outer);
}

#[test]
fn redischarge_stability_requires_same_polarity_and_binding_snapshot() {
    let binding = |bound| InferBinding {
        param: SemanticNodeId(710),
        name: Arc::from("T"),
        bound: SemanticNodeId(bound),
    };
    let original = PendingVerdict::Assignable {
        bindings: Arc::from(vec![binding(810)].into_boxed_slice()),
    };
    let same = PendingVerdict::Assignable {
        bindings: Arc::from(vec![binding(810)].into_boxed_slice()),
    };
    let changed_binding = PendingVerdict::Assignable {
        bindings: Arc::from(vec![binding(811)].into_boxed_slice()),
    };
    assert!(redischarge_is_stable(&original, &same));
    assert!(
        !redischarge_is_stable(&original, &PendingVerdict::NotAssignable),
        "a polarity flip must refuse publication"
    );
    assert!(
        !redischarge_is_stable(&original, &changed_binding),
        "a changed fixed-binding snapshot must refuse publication"
    );
    assert!(redischarge_is_stable(
        &PendingVerdict::NotAssignable,
        &PendingVerdict::NotAssignable,
    ));
}

#[test]
fn candidate_writes_mark_every_non_owner_ancestor_mutating_an_outer_session() {
    let mut txn = CheckerDispatchTransaction::default();
    let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
    let session = SessionId(7);
    let owner = txn
        .reentry_mut()
        .push_relate(relation_key(301), occurrence, 0);
    txn.note_opened_session(owner, session);
    txn.note_candidate_write(Some(session));

    txn.reentry_mut()
        .push_relate(relation_key(303), occurrence, 0);
    txn.reentry_mut()
        .push_relate(relation_key(305), occurrence, 0);
    txn.note_candidate_write(Some(session));

    let leaf = txn.reentry_mut().pop();
    let middle = txn.reentry_mut().pop();
    let owner = txn.reentry_mut().pop();
    let session_delta =
        |frame: &ObligationFrame| frame.relation().expect("relation frame").session_delta;
    assert!(
        session_delta(&leaf),
        "the leaf frame writing the outer session must be ReturnOnly"
    );
    assert!(
        session_delta(&middle),
        "every non-owner ancestor between the writer and session owner must be ReturnOnly"
    );
    assert!(
        !session_delta(&owner),
        "the frame that owns the session may publish its fixed bindings"
    );
}

#[test]
fn nested_redischarge_restores_the_enclosing_substitution_and_occurrence() {
    let mut txn = CheckerDispatchTransaction::default();
    let outer_occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
    let inner_occurrence = InferenceOccurrence {
        priority: InferenceCandidatePriority::ReturnType,
        variance: VariancePhase::Contravariant,
    };
    let deepest_occurrence = InferenceOccurrence {
        priority: InferenceCandidatePriority::NakedTypeParameter,
        variance: VariancePhase::Covariant,
    };

    let mut outer_substitution = ProvisionalSubstitution::default();
    outer_substitution.insert(
        relate_identity(401, outer_occurrence),
        ProvisionalVerdict::Relate(RelationStep::NotAssignable),
    );
    txn.obligations.replace_substitution(outer_substitution);
    txn.relation.redischarge_occurrence = Some((1, outer_occurrence));

    let mut inner_substitution = ProvisionalSubstitution::default();
    inner_substitution.insert(
        relate_identity(403, inner_occurrence),
        ProvisionalVerdict::Relate(RelationStep::Unknown),
    );
    let saved_outer = txn.replace_redischarge_context(inner_substitution, inner_occurrence);

    let mut deepest_substitution = ProvisionalSubstitution::default();
    deepest_substitution.insert(
        relate_identity(405, deepest_occurrence),
        ProvisionalVerdict::Relate(RelationStep::Assumed(
            super::RelationAssumptionEvidence::empty_for_tests(),
        )),
    );
    let saved_inner = txn.replace_redischarge_context(deepest_substitution, deepest_occurrence);
    txn.restore_redischarge_context(saved_inner);

    assert_eq!(
        txn.relation.redischarge_occurrence,
        Some((0, inner_occurrence))
    );
    assert!(matches!(
        provisional_relate_step(
            txn.obligations.substitution(),
            &relation_key(403),
            inner_occurrence
        ),
        Some(RelationStep::Unknown)
    ));
    assert_eq!(txn.obligations.substitution().len(), 1);

    txn.restore_redischarge_context(saved_outer);
    assert_eq!(
        txn.relation.redischarge_occurrence,
        Some((1, outer_occurrence))
    );
    assert!(matches!(
        provisional_relate_step(
            txn.obligations.substitution(),
            &relation_key(401),
            outer_occurrence
        ),
        Some(RelationStep::NotAssignable)
    ));
    assert_eq!(txn.obligations.substitution().len(), 1);
}

#[test]
fn nearest_relate_ancestor_supplies_relation_axes_never_the_untyped_top() {
    // A relation subkey inherits from the nearest open RELATE frame; the
    // generic stack top is only consulted through the tagged walk.
    let mut stack = ObligationReentryStack::default();
    let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
    stack.push_relate(relation_key(501), occurrence, 0);
    let (key, found_occurrence) = stack
        .nearest_relate()
        .expect("a relate frame is on the stack");
    assert_eq!(key, &relation_key(501));
    assert_eq!(found_occurrence, occurrence);
    stack.push_relate(relation_key(503), occurrence, 0);
    let (key, _) = stack.nearest_relate().expect("the top relate frame wins");
    assert_eq!(key, &relation_key(503));
    stack.pop();
    let (key, _) = stack
        .nearest_relate()
        .expect("the outer relate frame remains after the pop");
    assert_eq!(key, &relation_key(501));
}

#[test]
fn nearest_relate_walks_past_flow_frames_to_the_nearest_relation_ancestor() {
    // The tagged walk answers the nearest open RELATE frame even with a
    // FLOW frame interposed — a `frames.last().and_then(as_relate)`
    // implementation would falsely answer None in the first half.
    let mut stack = ObligationReentryStack::default();
    let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
    let flow_key = |name: &str| FlowReturnKey {
        function: crate::semantic_query::FlowFunctionSlotIdentity {
            declaration_slot: crate::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                Arc::from("/ws/txn.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from(name),
                0,
                crate::semantic_query::HashValue::default(),
                crate::semantic_query::HashValue::default(),
            ),
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        context: crate::semantic_query::FlowReturnContext {
            parse_env_hash: crate::semantic_query::HashValue::default(),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
            type_env_hash: crate::semantic_query::HashValue::default(),
            lib_env_hash: crate::semantic_query::HashValue::default(),
            project_identity: crate::semantic_query::HashValue::default(),
            type_substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
            policy: crate::semantic_query::FlowReturnPolicy {},
        },
        result_contract: super::super::flow_solve::flow_return_result_contract_id(),
    };
    stack.push_relate(relation_key(501), occurrence, 0);
    stack.push_flow_return(flow_key("nested"), 0);
    let (key, _) = stack
        .nearest_relate()
        .expect("a flow frame never shadows the open relation ancestor");
    assert_eq!(key, &relation_key(501));
    stack.push_relate(relation_key(503), occurrence, 0);
    let (key, _) = stack
        .nearest_relate()
        .expect("the relation frame above the flow frame wins");
    assert_eq!(key, &relation_key(503));
}

#[test]
fn tagged_pending_ledger_drains_exactly_the_push_time_watermark_suffix() {
    let mut ledger = ObligationPendingLedger::default();
    let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
    let deposit = |ledger: &mut ObligationPendingLedger, seed: u64| {
        ledger.deposit(PendingObligation {
            identity: relate_identity(seed, occurrence),
            domain: PendingObligationDomain::Relate(RelationPendingState {
                verdict: PendingVerdict::NotAssignable,
                session_delta: false,
                opened_session: None,
                inline_flight: None,
            }),
        });
    };
    deposit(&mut ledger, 601);
    let watermark = ledger.pending_len();
    deposit(&mut ledger, 603);
    deposit(&mut ledger, 605);
    let drained = ledger.drain_scc(watermark);
    assert_eq!(
        drained.len(),
        2,
        "only the suffix past the watermark drains"
    );
    let (first_key, _) = drained[0].identity.expect_relate();
    assert_eq!(first_key, &relation_key(603));
    assert_eq!(
        ledger.pending_len(),
        1,
        "the pre-watermark member belongs to a still-open outer SCC"
    );
}

/// SESSION-STATE guard — the candidate inference-session lifecycle is the
/// closed four-state machine `Collecting → StagedDeterministic →
/// CommittedDeterministic` with `Abandoned` reachable from the two
/// pre-publication states. The exhaustive `match` below fails to COMPILE if
/// a state is renamed, dropped, or added; the transitions are then driven on
/// a real session so a staged session is proven deposit-INACTIVE (the unsound
/// shape a staged-but-still-`Collecting` session would create: it stays
/// deposit-active and blocks nested candidate sessions).
#[test]
fn candidate_session_lifecycle_states_are_collecting_staged_committed_abandoned() {
    fn is_pre_publication(state: InferenceSessionState) -> bool {
        match state {
            InferenceSessionState::Collecting | InferenceSessionState::StagedDeterministic => true,
            InferenceSessionState::CommittedDeterministic | InferenceSessionState::Abandoned => {
                false
            }
        }
    }

    let param = SemanticNodeId(9001);
    let candidate = SemanticNodeId(9002);
    let new_session = || {
        InferenceSession::new(
            SessionId(1),
            setup(
                param,
                VariancePhase::Covariant,
                InferencePassKind::CallApplicability,
                InferenceCandidatePriority::Argument,
                NoInferMask::empty(),
                ConstParamPolicy::NonConst,
                ContextualInferenceMode::None,
            ),
            None,
        )
    };

    // Collecting: deposit-ACTIVE and pre-publication.
    let mut session = new_session();
    assert_eq!(session.state, InferenceSessionState::Collecting);
    assert!(is_pre_publication(session.state));
    assert!(session.deposit(
        param,
        candidate,
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant
    ));

    // Staged: immutable + deposit-INACTIVE, still pre-publication.
    let staged = session
        .stage_fixation(|candidates, _| candidates[0])
        .expect("a collecting session stages its fixation");
    assert_eq!(staged.len(), 1);
    assert_eq!(session.state, InferenceSessionState::StagedDeterministic);
    assert!(is_pre_publication(session.state));
    assert!(
        !session.deposit(
            param,
            SemanticNodeId(9003),
            InferenceCandidatePriority::Argument,
            VariancePhase::Covariant
        ),
        "a staged session is deposit-INACTIVE"
    );
    assert!(
        session
            .stage_fixation(|candidates, _| candidates[0])
            .is_none(),
        "a staged snapshot cannot re-stage"
    );

    // Committed: the only state that admits, and no longer pre-publication.
    assert!(session.commit_completed());
    assert_eq!(session.state, InferenceSessionState::CommittedDeterministic);
    assert!(!is_pre_publication(session.state));
    assert!(
        !session.abandon(),
        "a committed snapshot cannot be rolled back"
    );

    // Abandoned is reachable from BOTH pre-publication states, and terminal.
    let mut collecting = new_session();
    assert!(collecting.abandon());
    assert_eq!(collecting.state, InferenceSessionState::Abandoned);
    assert!(!is_pre_publication(collecting.state));
    assert!(!collecting.abandon(), "abandonment is terminal");

    let mut staged_then_abandoned = new_session();
    assert!(staged_then_abandoned.deposit(
        param,
        candidate,
        InferenceCandidatePriority::Argument,
        VariancePhase::Covariant
    ));
    assert!(staged_then_abandoned
        .stage_fixation(|candidates, _| candidates[0])
        .is_some());
    assert!(staged_then_abandoned.abandon());
    assert_eq!(
        staged_then_abandoned.state,
        InferenceSessionState::Abandoned
    );
    assert!(
        !staged_then_abandoned.commit_completed(),
        "an abandoned session never commits"
    );
}

/// EQUATION guard — the shared return equation spans BOTH domains: its
/// obligation identity is exactly `FlowReturn | ResolveCall`, and one member
/// carries the concrete seeds, the tagged hold targets, and the domain
/// metadata. Both are enforced by exhaustive destructuring / matching: a
/// dropped arm or a dropped member field fails to COMPILE (the flow-only
/// algebra cannot silently return).
#[test]
fn return_equation_identity_spans_flow_return_and_resolve_call() {
    fn domain_of(identity: &ReturnObligationIdentity) -> &'static str {
        match identity {
            ReturnObligationIdentity::FlowReturn(_) => "flow-return",
            ReturnObligationIdentity::ResolveCall(_) => "resolve-call",
        }
    }

    let flow = ReturnObligationIdentity::FlowReturn(flow_return_key());
    let call = ReturnObligationIdentity::ResolveCall(resolve_call_key());
    assert_eq!(domain_of(&flow), "flow-return");
    assert_eq!(domain_of(&call), "resolve-call");
    assert_ne!(flow, call);

    let member = ReturnEquationMember {
        fresh_literal_returns: Vec::new(),
        identity: call.clone(),
        concrete_seeds: vec![SemanticNodeId(7)],
        holds: vec![flow.clone()],
        domain: ReturnDomainMetadata::ResolveCall,
    };
    // Exhaustive destructuring: a new / renamed / removed field fails here.
    let ReturnEquationMember {
        fresh_literal_returns,
        identity,
        concrete_seeds,
        holds,
        domain,
    } = &member;
    assert!(fresh_literal_returns.is_empty());
    assert_eq!(identity, &call);
    assert_eq!(concrete_seeds, &vec![SemanticNodeId(7)]);
    assert_eq!(holds, &vec![flow]);
    assert!(matches!(domain, ReturnDomainMetadata::ResolveCall));
    // The flow arm carries its own domain metadata, not a shared shell.
    assert!(matches!(
        ReturnDomainMetadata::FlowReturn {
            can_fall_through: true
        },
        ReturnDomainMetadata::FlowReturn { .. }
    ));
}

/// The flow-demand carriers: the in-flight flow frame and the deferred
/// SCC member each carry the demand's `FlowDemandCarrier` (handle + plan +
/// provenance) — the member's demand SURVIVES the pop so the component
/// close finalizes the member against exactly its own demand. The slot
/// defaults to `None` (a demand the planner refused installs nothing) and
/// round-trips a carrier when set.
#[test]
fn flow_demand_carriers_default_none_and_round_trip() {
    use crate::for_tests::{
        flow_graph_fixture_for_tests, flow_return_result_contract_id, FlowDemandRequest,
        FlowResourcePolicy,
    };
    use crate::semantic_query::{
        CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
        FlowReturnKey, FlowReturnPolicy, ReturnProjectionDemand, SemanticQueryKey,
    };

    let fixture = flow_graph_fixture_for_tests("function carry_me(x) { return x; }\n", 31);
    let query = SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            declaration_slot: crate::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                Arc::from("/flow_solve_fixture.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("carry_me"),
                0,
                [0; 16],
                [0; 16],
            ),
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        normalized_type_args: Arc::from([]),
        context: FlowReturnContext {
            parse_env_hash: [0; 16],
            resolve_env_hash: [0; 16],
            type_env_hash: [0; 16],
            lib_env_hash: [0; 16],
            project_identity: [0; 16],
            type_substitution: CanonicalTypeSubstitution::empty(),
            policy: FlowReturnPolicy {},
        },
        demand: ReturnProjectionDemand::whole_return(),
        input: FlowInputContext::empty(),
        result_contract: flow_return_result_contract_id(),
    }));
    let provenance = super::flow_obligation_state::FlowEvaluationProvenance::new(7, 3, 5, 0);
    let plan = fixture
        .build_plan(FlowDemandRequest {
            query,
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources: FlowResourcePolicy::default(),
            additional_requirements: Arc::from([]),
        })
        .expect("the carrier fixture plans");
    let mut runtime = ObligationRuntime::default();
    let carrier = super::flow_obligation_state::FlowDemandCarrier {
        handle: runtime.install_flow_demand(&plan),
        plan: Arc::new(plan),
        provenance,
    };

    // The in-flight frame carrier.
    let mut frame = FlowReturnFrameState::default();
    assert!(
        frame.flow_demand.is_none(),
        "a fresh flow frame carries no demand carrier"
    );
    frame.flow_demand = Some(carrier.clone());
    assert_eq!(
        frame.flow_demand.as_ref().map(|carrier| carrier.handle),
        Some(carrier.handle),
        "the frame carrier round-trips its handle"
    );

    // The deferred SCC member carrier.
    let mut pending = FlowReturnPendingState {
        plan_refusal: None,
        outcome: FlowReturnPendingOutcome::NoValue {
            failure: FlowReturnFailure::Budget(
                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
            ),
            degradation: None,
        },
        inline_flight: None,
        holds: Vec::new(),
        self_roots: Vec::new(),
        materialized: crate::semantic_query::demand::MaterializedSet::default(),
        fresh_seed: false,
        flow_demand: None,
        discharge: None,
        provenance,
    };
    assert!(
        pending.flow_demand.is_none(),
        "a deferred flow member carries no demand carrier by default"
    );
    pending.flow_demand = Some(carrier.clone());
    assert_eq!(
        pending.flow_demand.as_ref().map(|carrier| carrier.handle),
        Some(carrier.handle),
        "the pending carrier round-trips its handle"
    );
}

/// A demand with ZERO installed obligations is an EMPTY proof universe, not
/// a trivially proved one: `iter().all(Discharged)` is vacuously true over
/// it, so without an explicit emptiness refusal the demand would pass the
/// convergence gate on its first observation and seal a completion whose
/// `proofs` slice is empty — an evidence-free artifact from the sole
/// warm-admission authority. No constructible `FlowDemandPlan` plans an
/// empty spec set (family-coverage + domain obligations come from the
/// closed contract registry: "proved empty" is a DISCHARGED obligation,
/// never an absent one), so the state is reachable only through the
/// test-only installer — and both runtime gates must refuse it fail-closed.
#[test]
fn zero_obligation_demand_never_converges_or_seals() {
    use super::flow_obligation_state::{FlowSealError, FlowTransitionError};
    use crate::for_tests::{
        flow_graph_fixture_for_tests, flow_return_result_contract_id, FlowDemandRequest,
        FlowResourcePolicy,
    };
    use crate::semantic_query::{
        CanonicalTypeSubstitution, FlowFunctionSlotIdentity, FlowInputContext, FlowReturnContext,
        FlowReturnKey, FlowReturnPolicy, ReturnProjectionDemand, SemanticQueryKey,
    };

    let fixture = flow_graph_fixture_for_tests("function seal_me(x) { return x; }\n", 33);
    let query = SemanticQueryKey::FlowReturn(Box::new(FlowReturnKey {
        function: FlowFunctionSlotIdentity {
            declaration_slot: crate::semantic_query::ResolvedDeclSlotIdentity::value_slot(
                Arc::from("/flow_solve_fixture.ts"),
                verter_type_expr::TopLevelOwnerId::ordinary_file(),
                Arc::from("seal_me"),
                0,
                [0; 16],
                [0; 16],
            ),
            function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        normalized_type_args: Arc::from([]),
        context: FlowReturnContext {
            parse_env_hash: [0; 16],
            resolve_env_hash: [0; 16],
            type_env_hash: [0; 16],
            lib_env_hash: [0; 16],
            project_identity: [0; 16],
            type_substitution: CanonicalTypeSubstitution::empty(),
            policy: FlowReturnPolicy {},
        },
        demand: ReturnProjectionDemand::whole_return(),
        input: FlowInputContext::empty(),
        result_contract: flow_return_result_contract_id(),
    }));
    let provenance = super::flow_obligation_state::FlowEvaluationProvenance::new(11, 3, 5, 0);
    let plan = fixture
        .build_plan(FlowDemandRequest {
            query,
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources: FlowResourcePolicy::default(),
            additional_requirements: Arc::from([]),
        })
        .expect("the fixture plans");
    assert!(
        !plan.obligation_specs().is_empty(),
        "the production planner never yields an empty obligation set — the \
         empty demand below is reachable only through the test installer"
    );

    let mut runtime = ObligationRuntime::default();
    let handle = runtime.install_flow_demand_without_obligations_for_tests(&plan);

    // Convergence is never observed over an EMPTY obligation universe: the
    // all-discharged gate must not pass by vacuous truth.
    assert!(
        matches!(
            runtime.observe_flow_iteration(handle, false),
            Err(FlowTransitionError::IllegalTransition)
        ),
        "a zero-obligation demand must not converge — vacuous all-discharged \
         is not a discharged universe"
    );

    // The seal refuses the same universe: no evidence-free completion mints.
    let graph = crate::semantic_query_memo::SemanticGraphStore::new();
    let number = graph.intern_node(crate::semantic_query::SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::Number,
    ));
    let value = crate::semantic_query::FlowReturnResult::new(&graph, number, false, None);
    assert!(
        matches!(
            runtime.seal_flow_completion(handle, value),
            Err(FlowSealError::UndischargedObligations)
        ),
        "a zero-obligation demand must never seal — an empty proofs slice is \
         evidence-free, not trivially proved"
    );
}
