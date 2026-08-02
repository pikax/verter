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
    let _bindings = session.fixate(|nodes, _| nodes.first().copied().unwrap_or(SemanticNodeId(0)));
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
    let fixed = session.fixate(|nodes, _| nodes[0]);
    assert_eq!(
        fixed.len(),
        1,
        "fixation must emit one binding for one exact infer parameter"
    );
    assert_eq!(fixed[0].param, param);
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
        ProvisionalVerdict::Relate(RelationStep::Assumed),
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
        context: crate::semantic_query::FlowReturnContext {
            parse_env_hash: crate::semantic_query::HashValue::default(),
            resolve_env_hash: crate::semantic_query::HashValue::default(),
            type_env_hash: crate::semantic_query::HashValue::default(),
            lib_env_hash: crate::semantic_query::HashValue::default(),
            project_identity: crate::semantic_query::HashValue::default(),
            type_substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
            policy: crate::semantic_query::FlowReturnPolicy {},
        },
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
