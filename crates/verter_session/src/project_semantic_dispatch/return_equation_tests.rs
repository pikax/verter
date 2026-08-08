//! Mixed FlowReturn/ResolveCall return-equation discriminators.

use std::sync::Arc;

use super::dispatch_txn::{
    CheckerDispatchTransaction, FlowReturnPendingOutcome, InferenceOccurrence,
    InferenceSessionSetup, ObligationIdentity, PendingObligation, PendingObligationDomain,
    PendingVerdict, RelationPendingState, ResolveCallPendingState, ResolveCallSelection,
    ReturnDomainMetadata, ReturnEquationFailure, ReturnEquationMember, ReturnObligationIdentity,
    SessionId,
};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    BudgetExceededKind, CallArgKey, CallKind, ConstParamPolicy, ContextualInferenceMode,
    FlowNarrowingKey, FlowReturnFailure, FlowReturnResult, FlowReturnStep,
    InferenceCandidatePriority, InferencePassKind, NoInferMask, PrimitiveKind, ProgramPointId,
    RecursionOrBudgetCap, ResolveCallKey, SemanticNodeData, VariancePhase,
};
use crate::{HostConfig, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};
use verter_type_expr::{
    IndexedValueCall, IndexedValueCallKind, IndexedValueExpression, PrimitiveName,
};

const CANONICAL: &str = "/ws/mixed-return-equation.ts";

fn flow_identity(name: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(CANONICAL),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(name),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

fn call_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    callee: crate::semantic_query::SemanticNodeId,
    offset: u32,
) -> ResolveCallKey {
    ResolveCallKey {
        point: ProgramPointId {
            canonical_id: Arc::from(CANONICAL),
            offset,
        },
        callee,
        kind: CallKind::Call,
        receiver: None,
        args: Arc::from(Vec::<CallArgKey>::new().into_boxed_slice()),
        explicit_type_args: Arc::from([]),
        flow: FlowNarrowingKey::empty(),
        context: dispatch.resolve_call_context_for(CANONICAL),
    }
}

/// A session with no inferable parameters: it stages an empty immutable
/// snapshot, so its lifecycle transitions are the only thing under test.
fn empty_session_setup() -> InferenceSessionSetup {
    InferenceSessionSetup::new(
        Arc::from([]),
        VariancePhase::Covariant,
        InferencePassKind::CallApplicability,
        InferenceCandidatePriority::Argument,
        NoInferMask::empty(),
        ConstParamPolicy::NonConst,
        ContextualInferenceMode::None,
    )
}

fn commit_session(txn: &mut CheckerDispatchTransaction, session: SessionId) {
    let session = txn
        .relation
        .sessions
        .iter_mut()
        .find(|candidate| candidate.id == session)
        .expect("the fixture session was just pushed");
    session
        .stage_fixation(|_, _| unreachable!("an empty session fixes no parameter"))
        .expect("a collecting session stages");
    assert!(session.commit_completed());
}

fn flow_domain() -> ReturnDomainMetadata {
    ReturnDomainMetadata::FlowReturn {
        can_fall_through: false,
    }
}

/// A concrete seed on either side of a mixed call/flow SCC reaches both
/// members. Mutation recipe: ignore `ResolveCall` hold targets or initialize
/// the lattice from hold targets instead of concrete seeds; one result stays
/// empty and this test fails.
#[test]
fn mixed_seeded_call_flow_cycle_converges_for_both_members() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow = ReturnObligationIdentity::FlowReturn(
        dispatch.flow_return_key_for(&flow_identity("seeded")),
    );
    let call = ReturnObligationIdentity::ResolveCall(call_key(&dispatch, callee, 11));
    let members = vec![
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: flow.clone(),
            concrete_seeds: vec![number],
            holds: vec![call.clone()],
            domain: flow_domain(),
        },
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: call,
            concrete_seeds: Vec::new(),
            holds: vec![flow],
            domain: ReturnDomainMetadata::ResolveCall,
        },
    ];

    let solved = dispatch
        .solve_return_equation(&members, &Default::default())
        .expect("a mixed seeded cycle converges");
    assert_eq!(solved, vec![number, number]);
}

/// The production flow-root close routes both solved members into the same
/// completed batch. Mutation recipe: omit the ResolveCall drain/equation arm;
/// only the flow member is queued.
#[test]
fn mixed_seeded_component_close_stages_both_domains() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow_key = dispatch.flow_return_key_for(&flow_identity("seededClose"));
    let call_key = call_key(&dispatch, callee, 15);
    let idx = {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::ResolveCall(call_key.clone()),
            domain: PendingObligationDomain::ResolveCall(Box::new(ResolveCallPendingState {
                selection: ResolveCallSelection::DynamicAny,
                concrete_seeds: Vec::new(),
                holds: vec![ReturnObligationIdentity::FlowReturn(flow_key.clone())],
                staged_session: None,
                replay_applicability: false,
                inline_flight: None,
                self_roots: Vec::new(),
            })),
        });
        idx
    };

    assert!(matches!(
        dispatch.flow_frame_close_for_tests(
            idx,
            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
                &dispatch.graph(),
                number,
                false,
                None,
            )),
            vec![ReturnObligationIdentity::ResolveCall(call_key)],
        ),
        FlowReturnStep::Complete(ref result) if result.return_type() == number
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert_eq!(txn.flow.completed_members.len(), 1);
    assert_eq!(txn.call.completed_members.len(), 1);
    assert_eq!(
        super::return_equation::resolved_call_return_type(
            txn.call.completed_members[0].result.get()
        ),
        number
    );
}

/// A flow-owned indexed call contributes a tagged ResolveCall hold, even when
/// the nested call already has a stable result. Mutation recipe: return the
/// call node directly without recording the tagged hold; the frame has no
/// cross-domain edge.
#[test]
fn flow_call_expression_records_a_resolve_call_hold() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let flow_key = dispatch.flow_return_key_for(&flow_identity("indexedCall"));
    let idx = dispatch
        .dispatch_txn
        .borrow_mut()
        .reentry_mut()
        .push_flow_return(flow_key, 0);
    let expression = IndexedValueExpression::Call(IndexedValueCall {
        point: 17,
        kind: IndexedValueCallKind::Call,
        callee: Box::new(IndexedValueExpression::Value(
            verter_type_expr::TypeExpr::Primitive(PrimitiveName::Any),
        )),
        receiver: None,
        args: Arc::from([]),
        explicit_type_args: Arc::from([]),
    });

    assert!(dispatch
        .evaluate_indexed_value_expression_node(
            CANONICAL,
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            &expression,
        )
        .is_none());
    let popped = dispatch.dispatch_txn.borrow_mut().reentry_mut().pop();
    assert_eq!(idx, 0);
    let super::dispatch_txn::ObligationFrameDomain::FlowReturn(state) = popped.domain else {
        panic!("test flow frame changed domain");
    };
    assert!(matches!(
        state.holds.as_slice(),
        [ReturnObligationIdentity::ResolveCall(_)]
    ));
    dispatch.relation_abort_completed_members();
}

/// The lattice bottom is not semantic `never`: a hold-only mixed component
/// fails as EmptyCycle in every domain. Mutation recipe: normalize an empty
/// seed set through the ordinary union helper; it becomes `never` and this
/// test changes from RED to an unsound success.
#[test]
fn mixed_empty_cycle_is_return_only_for_the_whole_component() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let callee = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow =
        ReturnObligationIdentity::FlowReturn(dispatch.flow_return_key_for(&flow_identity("empty")));
    let call = ReturnObligationIdentity::ResolveCall(call_key(&dispatch, callee, 19));
    let members = vec![
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: flow.clone(),
            concrete_seeds: Vec::new(),
            holds: vec![call.clone()],
            domain: flow_domain(),
        },
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: call,
            concrete_seeds: Vec::new(),
            holds: vec![flow],
            domain: ReturnDomainMetadata::ResolveCall,
        },
    ];

    assert_eq!(
        dispatch.solve_return_equation(&members, &Default::default()),
        Err(ReturnEquationFailure::EmptyCycle)
    );
}

/// EmptyCycle poisons publication at the real mixed close, not merely in the
/// algebra helper. Mutation recipe: admit either member after solver failure;
/// one completed queue becomes non-empty.
#[test]
fn mixed_empty_component_close_admits_nothing() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let callee = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow_key = dispatch.flow_return_key_for(&flow_identity("emptyClose"));
    let call_key = call_key(&dispatch, callee, 23);
    let idx = {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::ResolveCall(call_key.clone()),
            domain: PendingObligationDomain::ResolveCall(Box::new(ResolveCallPendingState {
                selection: ResolveCallSelection::DynamicAny,
                concrete_seeds: Vec::new(),
                holds: vec![ReturnObligationIdentity::FlowReturn(flow_key.clone())],
                staged_session: None,
                replay_applicability: false,
                inline_flight: None,
                self_roots: Vec::new(),
            })),
        });
        idx
    };

    assert!(matches!(
        dispatch.flow_frame_close_for_tests(
            idx,
            FlowReturnPendingOutcome::NoValue {
                failure: FlowReturnFailure::EmptyCycle,
                degradation: None,
            },
            vec![ReturnObligationIdentity::ResolveCall(call_key)],
        ),
        FlowReturnStep::NoValue(FlowReturnFailure::EmptyCycle)
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert!(txn.flow.completed_members.is_empty());
    assert!(txn.call.completed_members.is_empty());
    assert!(txn.relation.completed_members.is_empty());
}

/// Direct self recursion is the same two-edge SCC: without a base it is
/// empty; adding one concrete flow seed preserves the historical base-seed
/// verdict on both sides. Mutation recipe: collapse the two identities into a
/// self FlowReturn edge; the mixed-domain assertions no longer hold.
#[test]
fn self_recursion_two_edge_scc_preserves_empty_and_base_seed_verdicts() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow = ReturnObligationIdentity::FlowReturn(
        dispatch.flow_return_key_for(&flow_identity("selfRecursive")),
    );
    let call = ReturnObligationIdentity::ResolveCall(call_key(&dispatch, callee, 27));
    let mut members = vec![
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: flow.clone(),
            concrete_seeds: Vec::new(),
            holds: vec![call.clone()],
            domain: flow_domain(),
        },
        ReturnEquationMember {
            fresh_literal_returns: Vec::new(),
            identity: call,
            concrete_seeds: Vec::new(),
            holds: vec![flow],
            domain: ReturnDomainMetadata::ResolveCall,
        },
    ];
    assert_eq!(
        dispatch.solve_return_equation(&members, &Default::default()),
        Err(ReturnEquationFailure::EmptyCycle)
    );

    members[0].concrete_seeds.push(string);
    assert_eq!(
        dispatch
            .solve_return_equation(&members, &Default::default())
            .expect("the base seed closes the two-edge SCC"),
        vec![string, string]
    );
}

/// A hold leaving the closed component is usable only through a stable memo
/// value. Mutation recipe: silently treat a missing outside target as bottom;
/// the component would report EmptyCycle instead of Unresolved.
#[test]
fn unresolved_outside_hold_fails_closed() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let member = ReturnEquationMember {
        fresh_literal_returns: Vec::new(),
        identity: ReturnObligationIdentity::FlowReturn(
            dispatch.flow_return_key_for(&flow_identity("outsideOwner")),
        ),
        concrete_seeds: Vec::new(),
        holds: vec![ReturnObligationIdentity::ResolveCall(call_key(
            &dispatch, callee, 33,
        ))],
        domain: flow_domain(),
    };

    assert_eq!(
        dispatch.solve_return_equation(&[member], &Default::default()),
        Err(ReturnEquationFailure::UnresolvedOutsideHold)
    );
}

/// A call-resolution budget edge poisons the entire mixed component; neither
/// domain reaches its completed-member publication queue. Mutation recipe:
/// omit the root budget override/component poison and this seeded cycle
/// closes successfully instead.
#[test]
fn call_budget_trip_poisons_the_whole_mixed_component() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow_key = dispatch.flow_return_key_for(&flow_identity("budgetOwner"));
    let call_key = call_key(&dispatch, callee, 41);
    let idx = {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
        txn.reentry_mut().note_budget_edge(
            idx,
            RecursionOrBudgetCap {
                kind: BudgetExceededKind::CallResolutionBudget,
                limit: 64,
            },
        );
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::ResolveCall(call_key.clone()),
            domain: PendingObligationDomain::ResolveCall(Box::new(ResolveCallPendingState {
                selection: ResolveCallSelection::DynamicAny,
                concrete_seeds: vec![number],
                holds: vec![ReturnObligationIdentity::FlowReturn(flow_key.clone())],
                staged_session: None,
                replay_applicability: false,
                inline_flight: None,
                self_roots: Vec::new(),
            })),
        });
        idx
    };

    let step = dispatch.flow_frame_close_for_tests(
        idx,
        FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
            &dispatch.graph(),
            number,
            false,
            None,
        )),
        vec![ReturnObligationIdentity::ResolveCall(call_key)],
    );
    assert!(matches!(
        step,
        FlowReturnStep::NoValue(FlowReturnFailure::Budget(_))
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert!(txn.flow.completed_members.is_empty());
    assert!(txn.call.completed_members.is_empty());
    assert!(txn.relation.completed_members.is_empty());
}

/// A refused staged-call commit stops the whole component before the
/// publication tail: the relation member's deferred ledger entry survives
/// intact, because the drain runs only after every staged call session
/// commits. Mutation recipe: move the ledger drain above
/// `commit_call_sessions`; the surviving-entry assertion fails.
#[test]
fn refused_call_commit_leaves_the_relation_ledger_undrained() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow_key = dispatch.flow_return_key_for(&flow_identity("commitOrder"));
    let call_key = call_key(&dispatch, callee, 51);
    let relate_key = dispatch.relate_key_for(string, string);

    let (idx, relation_session) = {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        // A relation member whose session already committed and whose
        // deferred admission entry is intact: draining it is the last step
        // of the publication tail.
        let relation_session = txn.push_collecting_session(empty_session_setup(), None);
        commit_session(&mut txn, relation_session);
        txn.relation
            .session_admission
            .defer(relation_session, relate_key.clone());
        // A call member staged on a session that never reached
        // `StagedDeterministic`: the commit gate refuses it.
        let call_session = txn.push_collecting_session(empty_session_setup(), None);
        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::Relate {
                key: relate_key.clone(),
                occurrence: InferenceOccurrence::ARGUMENT_COVARIANT,
            },
            domain: PendingObligationDomain::Relate(RelationPendingState {
                verdict: PendingVerdict::Assignable {
                    bindings: Arc::from([]),
                },
                session_delta: false,
                opened_session: Some(relation_session),
                inline_flight: None,
            }),
        });
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::ResolveCall(call_key.clone()),
            domain: PendingObligationDomain::ResolveCall(Box::new(ResolveCallPendingState {
                selection: ResolveCallSelection::DynamicAny,
                concrete_seeds: vec![number],
                holds: Vec::new(),
                staged_session: Some(call_session),
                replay_applicability: false,
                inline_flight: None,
                self_roots: Vec::new(),
            })),
        });
        (idx, relation_session)
    };

    assert!(matches!(
        dispatch.flow_frame_close_for_tests(
            idx,
            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
                &dispatch.graph(),
                number,
                false,
                None,
            )),
            Vec::new(),
        ),
        FlowReturnStep::NoValue(_)
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert!(txn.relation.completed_members.is_empty());
    assert!(txn.flow.completed_members.is_empty());
    assert!(txn.call.completed_members.is_empty());
    assert!(txn
        .relation
        .session_admission
        .contains(relation_session, &relate_key));
}

/// A mixed return component re-discharges every relation member and demands
/// an exact polarity match, even for a member that opened no session and
/// closed positive without a negative peer. Mutation recipe: drop the
/// `has_return_member` term from the mixed stability gate; the flipped member
/// publishes its re-discharged verdict instead of poisoning the component.
#[test]
fn mixed_component_relation_member_flip_publishes_nothing() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let callee = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let flow_key = dispatch.flow_return_key_for(&flow_identity("flipOwner"));
    let call_key = call_key(&dispatch, callee, 57);
    // `string` is not assignable to `number`, so the re-discharge flips the
    // provisional positive verdict deposited below.
    let relate_key = dispatch.relate_key_for(string, number);

    let idx = {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        let idx = txn.reentry_mut().push_flow_return(flow_key.clone(), 0);
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::Relate {
                key: relate_key,
                occurrence: InferenceOccurrence::ARGUMENT_COVARIANT,
            },
            domain: PendingObligationDomain::Relate(RelationPendingState {
                verdict: PendingVerdict::Assignable {
                    bindings: Arc::from([]),
                },
                session_delta: false,
                opened_session: None,
                inline_flight: None,
            }),
        });
        txn.obligations.pending_mut().deposit(PendingObligation {
            identity: ObligationIdentity::ResolveCall(call_key.clone()),
            domain: PendingObligationDomain::ResolveCall(Box::new(ResolveCallPendingState {
                selection: ResolveCallSelection::DynamicAny,
                concrete_seeds: vec![number],
                holds: Vec::new(),
                staged_session: None,
                replay_applicability: false,
                inline_flight: None,
                self_roots: Vec::new(),
            })),
        });
        idx
    };

    assert!(matches!(
        dispatch.flow_frame_close_for_tests(
            idx,
            FlowReturnPendingOutcome::Complete(FlowReturnResult::new(
                &dispatch.graph(),
                number,
                false,
                None,
            )),
            Vec::new(),
        ),
        FlowReturnStep::NoValue(_)
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert!(txn.relation.completed_members.is_empty());
    assert!(txn.flow.completed_members.is_empty());
    assert!(txn.call.completed_members.is_empty());
}
