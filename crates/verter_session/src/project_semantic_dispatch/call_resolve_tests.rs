//! @ai-generated - Direct applicability-executor tests for overload order,
//! arity/bucket/spread mapping, receiver handling, per-parameter const
//! inference, constraint rollback, and final return substitution.

use std::sync::Arc;

use super::dispatch_txn::{InferenceOccurrence, InferenceSessionState, PendingObligationDomain};
use super::*;
use crate::semantic_query::{
    ArgumentLiteralMode, CallArgKey, CallKind, FunctionParam, PrimitiveKind, ProgramPointId,
    QueryResult, ResolveCallContext, ResolveCallKey, ResolvedCallResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    SignatureKind, SignatureNodeOccurrence, SignatureReturnCarrier, TupleElement, TypeParamDecl,
};
use crate::types::UpsertRequest;
use crate::{HostConfig, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

const CANONICAL: &str = "/ws/call-resolve.ts";

fn host() -> Arc<VerterHost> {
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}

fn occurrence(name: &str, ordinal: u32) -> SignatureNodeOccurrence {
    SignatureNodeOccurrence {
        function: FlowFunctionReturnIdentity {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from(CANONICAL),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from(name),
                space: LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: ordinal,
        },
        signature_ordinal: ordinal,
    }
}

fn signature(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    ordinal: u32,
    kind: SignatureKind,
    params: Vec<FunctionParam>,
    type_parameters: Vec<TypeParamDecl>,
    return_type: SemanticNodeId,
) -> SemanticNodeId {
    signature_with_carrier(
        dispatch,
        name,
        ordinal,
        kind,
        params,
        type_parameters,
        return_type,
        SignatureReturnCarrier::Declared(return_type),
    )
}

#[allow(clippy::too_many_arguments)]
fn signature_with_carrier(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    ordinal: u32,
    kind: SignatureKind,
    params: Vec<FunctionParam>,
    type_parameters: Vec<TypeParamDecl>,
    return_type: SemanticNodeId,
    return_carrier: SignatureReturnCarrier,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Signature {
        kind,
        params: Arc::from(params.into_boxed_slice()),
        return_type,
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        occurrence: Some(occurrence(name, ordinal)),
        return_carrier,
        signature_span: None,
        return_type_span: None,
    })
}

/// A relation-only applicability assumption stays provisional and selects;
/// when the same closure reaches this candidate's own return equation it is a
/// typed refusal. Mutation: erase exact assumption evidence or treat every
/// `Assumed` alike; one polarity fails.
#[test]
fn relation_only_assumption_replays_but_own_return_assumption_refuses() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let declared = signature(
        &dispatch,
        "relationOnly",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        number,
    );
    let declared_callee = callable(&dispatch, vec![declared], Vec::new());
    let relation_key = dispatch.relate_key_for(number, number);
    dispatch
        .dispatch_txn
        .borrow_mut()
        .reentry_mut()
        .push_relate(
            relation_key.clone(),
            InferenceOccurrence::ARGUMENT_COVARIANT,
            0,
        );
    assert!(matches!(
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            declared_callee,
            CallKind::Call,
            None,
            vec![eager(number)],
        )),
        super::call_resolve::ResolveCallStep::Complete(ResolvedCallResult::Selected { .. })
    ));
    dispatch.dispatch_txn.borrow_mut().reentry_mut().pop();
    abandon_provisional_call_members(&dispatch);
    assert!(matches!(
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            declared_callee,
            CallKind::Call,
            None,
            vec![eager(number)],
        )),
        super::call_resolve::ResolveCallStep::Complete(ResolvedCallResult::Selected { .. })
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert_eq!(
        txn.relation
            .sessions
            .iter()
            .filter(|session| session.state == InferenceSessionState::Abandoned)
            .count(),
        1,
        "the provisional attempt is abandoned before replay"
    );
    assert_eq!(
        txn.relation
            .sessions
            .iter()
            .filter(|session| session.state == InferenceSessionState::CommittedDeterministic)
            .count(),
        1,
        "stable root replay commits its fresh session at atomic publication"
    );
    drop(txn);

    let recursive_occurrence = occurrence("recursive", 0);
    let recursive_identity = recursive_occurrence.function.clone();
    let recursive = signature_with_carrier(
        &dispatch,
        "recursive",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        number,
        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
            recursive_identity.clone(),
        )),
    );
    let recursive_callee = callable(&dispatch, vec![recursive], Vec::new());
    let flow_key = dispatch.flow_return_key_for(&recursive_identity);
    {
        let mut txn = dispatch.dispatch_txn.borrow_mut();
        txn.reentry_mut()
            .push_relate(relation_key, InferenceOccurrence::ARGUMENT_COVARIANT, 0);
        txn.reentry_mut().push_flow_return(flow_key, 0);
    }
    assert!(matches!(
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            recursive_callee,
            CallKind::Call,
            None,
            vec![eager(number)],
        )),
        super::call_resolve::ResolveCallStep::Degraded(
            crate::semantic_query::ResolveCallFailure::Undecidable
        )
    ));
    let mut txn = dispatch.dispatch_txn.borrow_mut();
    txn.reentry_mut().pop();
    txn.reentry_mut().pop();
}

/// Dispose of the deferred call members the way a relation SCC root does when
/// it abandons a provisional attempt: release each member's inline flight and
/// its staged session. A test that pops the relation frame directly runs no
/// root drain, so it performs the disposal itself; without it the member's
/// flight stays claimed and the next demand for that key waits on it.
fn abandon_provisional_call_members(dispatch: &ProjectSemanticDispatch<'_>) {
    let drained = dispatch
        .dispatch_txn
        .borrow_mut()
        .obligations
        .pending_mut()
        .drain_scc(0);
    for member in drained {
        if let PendingObligationDomain::ResolveCall(state) = member.domain {
            dispatch.resolve_call_abort_inline_flight(state.inline_flight.as_ref());
            if let Some(session) = state.staged_session {
                dispatch.abandon_session(session);
            }
        }
    }
}

fn callable(
    dispatch: &ProjectSemanticDispatch<'_>,
    calls: Vec<SemanticNodeId>,
    constructs: Vec<SemanticNodeId>,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(calls.into_boxed_slice()),
            construct_signatures: Arc::from(constructs.into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ))
}

fn call_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    callee: SemanticNodeId,
    kind: CallKind,
    receiver: Option<SemanticNodeId>,
    args: Vec<CallArgKey>,
) -> ResolveCallKey {
    let ResolveCallContext {
        parse_env_hash,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
    } = dispatch.resolve_call_context_for(CANONICAL);
    ResolveCallKey {
        point: ProgramPointId {
            canonical_id: Arc::from(CANONICAL),
            offset: 41,
        },
        callee,
        kind,
        receiver,
        args: Arc::from(args.into_boxed_slice()),
        explicit_type_args: Arc::from(Vec::new().into_boxed_slice()),
        flow: crate::semantic_query::FlowNarrowingKey::empty(),
        context: ResolveCallContext {
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
        },
    }
}

fn eager(ty: SemanticNodeId) -> CallArgKey {
    CallArgKey::Eager {
        ty,
        spread: false,
        literal_mode: ArgumentLiteralMode::Literal,
        context_sensitive: false,
    }
}

/// An argument authored as a bare literal expression: its inference
/// candidate widens under the inferring parameter's const policy.
fn fresh_literal(ty: SemanticNodeId) -> CallArgKey {
    CallArgKey::Eager {
        ty,
        spread: false,
        literal_mode: ArgumentLiteralMode::Widened,
        context_sensitive: false,
    }
}

fn selected(step: super::call_resolve::ResolveCallStep) -> ResolvedCallResult {
    match step {
        super::call_resolve::ResolveCallStep::Complete(result) => result,
        other => panic!("call must select, got {other:?}"),
    }
}

fn selected_query(
    dispatch: &ProjectSemanticDispatch<'_>,
    key: ResolveCallKey,
) -> ResolvedCallResult {
    match dispatch.execute(SemanticQueryKey::ResolveCall(Box::new(key))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::ResolveCall(result),
            ..
        }) => result.as_ref().clone(),
        other => panic!("ResolveCall query must return its staged winner, got {other:?}"),
    }
}

/// Declaration order is the ranking rule: the literal overload wins before
/// the later primitive overload — for a pinned literal argument AND for a
/// bare-literal one, because applicability relates the argument's actual
/// type in both. Mutation: reverse candidate iteration, or widen the
/// applicability source for a bare-literal argument; the selected ordinal
/// and return literal both change and this test fails.
#[test]
fn first_fully_applicable_literal_overload_wins() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let literal_a = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("a".into()),
    ));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let first_ret = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("first".into()),
    ));
    let second_ret = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("second".into()),
    ));
    let first = signature(
        &dispatch,
        "pick",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, literal_a, false, false)],
        Vec::new(),
        first_ret,
    );
    let second = signature(
        &dispatch,
        "pick",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        second_ret,
    );
    let callee = callable(&dispatch, vec![first, second], Vec::new());
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![eager(literal_a)],
    )));
    let ResolvedCallResult::Selected {
        selected: selected_identity,
        return_type,
        ..
    } = result
    else {
        panic!("a declared overload must be selected")
    };
    assert!(
        matches!(
            selected_identity,
            crate::semantic_query::SignatureCandidateOrigin::Authored(
                crate::semantic_query::SignatureOccurrenceIdentity {
                    signature_ordinal: 0,
                    ..
                }
            )
        ),
        "the first declared overload wins: {selected_identity:?}"
    );
    assert_eq!(return_type, first_ret);
    // A BARE-literal argument selects the same overload: widening is an
    // inference-result rule, so the literal-typed first overload still
    // matches. `second_ret` is never reached from this call.
    let fresh = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![fresh_literal(literal_a)],
    )));
    let ResolvedCallResult::Selected {
        selected: fresh_identity,
        return_type: fresh_return,
        ..
    } = fresh
    else {
        panic!("a declared overload must be selected")
    };
    assert!(
        matches!(
            fresh_identity,
            crate::semantic_query::SignatureCandidateOrigin::Authored(
                crate::semantic_query::SignatureOccurrenceIdentity {
                    signature_ordinal: 0,
                    ..
                }
            )
        ),
        "a bare literal argument relates its literal type: {fresh_identity:?}"
    );
    assert_eq!(fresh_return, first_ret);
    assert_ne!(fresh_return, second_ret);
}

/// Bucket and arity filtering happen before a session opens; fixed tuple
/// spreads expand positionally and an indefinite spread without a rest target
/// is a typed undecidable. Mutation: count construct signatures as calls or
/// treat a tuple spread as one argument; one of the three assertions fails.
#[test]
fn bucket_arity_rest_and_spread_mapping_are_decisive() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let exact_ret = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("exact".into()),
    ));
    let rest_ret = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("rest".into()),
    ));
    let construct_ret = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("construct".into()),
    ));
    let number_array = graph.intern_node(SemanticNodeData::Array {
        element: number,
        readonly: false,
    });
    let exact = signature(
        &dispatch,
        "hybrid",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        exact_ret,
    );
    let rest = signature(
        &dispatch,
        "hybrid",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number_array, false, true)],
        Vec::new(),
        rest_ret,
    );
    let construct = signature(
        &dispatch,
        "hybrid",
        2,
        SignatureKind::Construct,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        construct_ret,
    );
    let callee = callable(&dispatch, vec![exact, rest], vec![construct]);
    let tuple = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![
                TupleElement {
                    label: None,
                    value: number,
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    value: number,
                    optional: false,
                    rest: false,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let spread_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![CallArgKey::Eager {
            ty: tuple,
            spread: true,
            literal_mode: ArgumentLiteralMode::Literal,
            context_sensitive: false,
        }],
    )));
    assert!(matches!(
        spread_result,
        ResolvedCallResult::Selected { return_type, .. } if return_type == rest_ret
    ));
    let construct_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Construct,
        None,
        vec![eager(string)],
    )));
    assert!(matches!(
        construct_result,
        ResolvedCallResult::Selected { return_type, .. } if return_type == construct_ret
    ));
    let exact_only = callable(&dispatch, vec![exact], Vec::new());
    assert!(matches!(
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            exact_only,
            CallKind::Call,
            None,
            vec![CallArgKey::Eager {
                ty: number_array,
                spread: true,
                literal_mode: ArgumentLiteralMode::Widened,
                context_sensitive: false,
            }],
        )),
        super::call_resolve::ResolveCallStep::Degraded(
            crate::semantic_query::ResolveCallFailure::Undecidable
        )
    ));
}

/// The leading authored `this` parameter is checked against the receiver and
/// excluded from ordinary arity. Mutation: include it in arity or check the
/// argument before the receiver; the one-argument call no longer selects.
#[test]
fn authored_this_is_receiver_only() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let sig = signature(
        &dispatch,
        "withThis",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(Some(Arc::from("this")), string, false, false),
            FunctionParam::synthetic(Some(Arc::from("value")), number, false, false),
        ],
        Vec::new(),
        ret,
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    assert!(matches!(
        selected(dispatch.execute_resolve_call(call_key(
            &dispatch,
            callee,
            CallKind::Call,
            Some(string),
            vec![eager(number)],
        ))),
        ResolvedCallResult::Selected { return_type, .. } if return_type == ret
    ));
}

/// Const policy is per declaration: for a bare-literal argument,
/// `<const T, U>` constifies only `T` while `U` follows ordinary array
/// widening. Mutation: use one session-wide const flag; both bindings become
/// tuples and the negative assertion fails.
#[test]
fn mixed_const_policy_is_applied_per_parameter() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let u = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("U"),
        param_index: 1,
        constraint: None,
        default: None,
        display_name: Arc::from("U"),
    });
    let string_a = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("a".into()),
    ));
    let tuple_arg = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                value: string_a,
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let ret = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![
                TupleElement {
                    label: None,
                    value: t,
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: None,
                    value: u,
                    optional: false,
                    rest: false,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let sig = signature(
        &dispatch,
        "mixed",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, t, false, false),
            FunctionParam::synthetic(None, u, false, false),
        ],
        vec![
            TypeParamDecl {
                name: Arc::from("T"),
                param: t,
                constraint: None,
                default: None,
                is_const: true,
            },
            TypeParamDecl {
                name: Arc::from("U"),
                param: u,
                constraint: None,
                default: None,
                is_const: false,
            },
        ],
        ret,
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![fresh_literal(tuple_arg), fresh_literal(tuple_arg)],
    )));
    let ResolvedCallResult::Selected { substitution, .. } = result else {
        panic!("generic candidate must select")
    };
    let t_bound = substitution
        .bindings()
        .iter()
        .find_map(|(param, bound)| (*param == t).then_some(*bound))
        .expect("T binding");
    let u_bound = substitution
        .bindings()
        .iter()
        .find_map(|(param, bound)| (*param == u).then_some(*bound))
        .expect("U binding");
    assert!(matches!(
        graph.node_data(t_bound).as_deref(),
        Some(SemanticNodeData::Tuple { readonly: true, .. })
    ));
    assert!(matches!(
        graph.node_data(u_bound).as_deref(),
        Some(SemanticNodeData::Array {
            readonly: false,
            ..
        })
    ));
}

/// A constraint failure abandons its candidate-local session and publishes
/// nothing; the next overload wins and commits only at the mixed-component
/// publication boundary. Mutation: skip the constraint recheck or commit the
/// losing session; the state/selection rails fail.
#[test]
fn constraint_rejection_rolls_back_and_commits_only_the_winner() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let bad_ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let good_ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("ConstrainedT"),
        param_index: 0,
        constraint: Some(string),
        default: None,
        display_name: Arc::from("T"),
    });
    let bad = signature(
        &dispatch,
        "constrained",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, t, false, false)],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: Some(string),
            default: None,
            is_const: false,
        }],
        bad_ret,
    );
    let good = signature(
        &dispatch,
        "constrained",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        good_ret,
    );
    let callee = callable(&dispatch, vec![bad, good], Vec::new());
    let key = call_key(&dispatch, callee, CallKind::Call, None, vec![eager(number)]);
    let result = selected_query(&dispatch, key.clone());
    assert!(matches!(
        result,
        ResolvedCallResult::Selected { return_type, .. } if return_type == good_ret
    ));
    let txn = dispatch.dispatch_txn.borrow();
    assert!(txn
        .relation
        .sessions
        .iter()
        .any(|session| session.state == InferenceSessionState::Abandoned));
    assert_eq!(
        txn.relation
            .sessions
            .iter()
            .filter(|session| session.state == InferenceSessionState::CommittedDeterministic)
            .count(),
        1
    );
    assert!(!txn
        .relation
        .sessions
        .iter()
        .any(|session| session.state == InferenceSessionState::StagedDeterministic));
    drop(txn);
    assert!(matches!(
        selected_query(&dispatch, key.clone()),
        ResolvedCallResult::Selected { return_type, .. } if return_type == good_ret
    ));
    assert_eq!(
        dispatch
            .dispatch_txn
            .borrow()
            .relation
            .sessions
            .iter()
            .filter(|session| session.state == InferenceSessionState::CommittedDeterministic)
            .count(),
        1,
        "the second demand warm-serves without opening another candidate session"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(key))),
        1,
        "the committed winner admits exactly once at atomic component publication"
    );
}

/// Declared generic returns substitute the final immutable binding snapshot.
/// Mutation: return the pre-fixation carrier; the member still points at `T`.
#[test]
fn declared_return_uses_the_final_substitution() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("DeclaredT"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let ret = graph.intern_node(SemanticNodeData::Array {
        element: t,
        readonly: false,
    });
    let sig = signature(
        &dispatch,
        "declared",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, t, false, false)],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        ret,
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![eager(number)],
    )));
    let ResolvedCallResult::Selected { return_type, .. } = result else {
        panic!("generic declared candidate must select")
    };
    assert!(matches!(
        graph.node_data(return_type).as_deref(),
        Some(SemanticNodeData::Array { element, .. }) if *element == number
    ));
}

/// Body-derived returns receive the same declaration-ordered normalized args
/// and full substitution as the selected signature. Mutation: restore the
/// empty FlowReturn constructor; the body returns its binder instead of number.
#[test]
fn body_derived_return_uses_the_final_substitution() {
    let host = host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from("export function body<T>(x: T) { return x; }"),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("BodyT"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let identity = occurrence("body", 0).function;
    let sig = signature_with_carrier(
        &dispatch,
        "body",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, t, false, false)],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        t,
        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
            identity,
        )),
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![eager(number)],
    )));
    assert!(matches!(
        result,
        ResolvedCallResult::Selected { return_type, .. } if return_type == number
    ));
}

/// A body-derived callee return whose INLINE flow evaluation is degraded
/// (an unapplied write effect) must not launder into a warm-admissible
/// call: the inline close's unproven verdict folds into the enclosing
/// build's partial/ReturnOnly rails — the inline path produces no memo
/// read for the universal read funnel to carry them — so the call returns
/// its usable value but the `ResolveCall` slot stays cold. The
/// clean-callee control warms. Mutation: drop the inline fold — the
/// degraded leg admits.
#[test]
fn degraded_inline_flow_return_never_warms_the_enclosing_call() {
    let host = host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from(
            "export function degraded(x: string | number) { return { a: (x = \"s\"), b: x }; }\nexport function clean() { return 1; }",
        ),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let degraded_identity = occurrence("degraded", 0).function;
    let degraded_sig = signature_with_carrier(
        &dispatch,
        "degraded",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        number,
        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
            degraded_identity,
        )),
    );
    let degraded_callee = callable(&dispatch, vec![degraded_sig], Vec::new());
    let degraded_key = call_key(
        &dispatch,
        degraded_callee,
        CallKind::Call,
        None,
        vec![eager(number)],
    );
    let step = dispatch.execute_resolve_call(degraded_key.clone());
    assert!(
        matches!(step, super::call_resolve::ResolveCallStep::Complete(_)),
        "the degraded value stays usable at the call, got {step:?}"
    );
    assert_eq!(
        graph
            .slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(degraded_key))),
        0,
        "an unproven inline flow return must not warm the enclosing call"
    );

    // Control: a clean body-derived callee's enclosing call warms.
    let clean_identity = occurrence("clean", 0).function;
    let clean_sig = signature_with_carrier(
        &dispatch,
        "clean",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        number,
        SignatureReturnCarrier::Function(verter_type_expr::facts::FunctionReturnSource::Flow(
            clean_identity,
        )),
    );
    let clean_callee = callable(&dispatch, vec![clean_sig], Vec::new());
    let clean_key = call_key(&dispatch, clean_callee, CallKind::Call, None, Vec::new());
    let step = dispatch.execute_resolve_call(clean_key.clone());
    assert!(
        matches!(step, super::call_resolve::ResolveCallStep::Complete(_)),
        "the clean call resolves, got {step:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(clean_key))),
        1,
        "a proven inline flow return keeps the enclosing call warm"
    );
}

/// The candidate-open cap is runtime state, not key identity: a trip abandons
/// every session opened by this call and admits no value. Mutation: remove the
/// open charge or leave a loser staged; the typed outcome/state assertions fail.
#[test]
fn call_resolution_budget_exceeded_admits_nothing() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let candidates = (0..65)
        .map(|ordinal| {
            signature(
                &dispatch,
                "budgeted",
                ordinal,
                SignatureKind::Call,
                vec![FunctionParam::synthetic(None, string, false, false)],
                Vec::new(),
                number,
            )
        })
        .collect();
    let callee = callable(&dispatch, candidates, Vec::new());
    let key = call_key(&dispatch, callee, CallKind::Call, None, vec![eager(number)]);
    assert!(matches!(
        dispatch.execute_resolve_call(key.clone()),
        super::call_resolve::ResolveCallStep::Degraded(
            crate::semantic_query::ResolveCallFailure::Budget
        )
    ));
    assert!(dispatch
        .dispatch_txn
        .borrow()
        .relation
        .sessions
        .iter()
        .all(|session| session.state == InferenceSessionState::Abandoned));
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(key))),
        0
    );
}

/// Genuine dynamic `any` is the only no-candidate complete result; it is not
/// an undecidable fallback. Mutation: route `any` through overload acquisition
/// or use it for unknown callees; this exact positive case fails.
#[test]
fn genuine_any_returns_dynamic_any() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let any = dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    assert!(matches!(
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            any,
            CallKind::Call,
            None,
            Vec::new(),
        )),
        super::call_resolve::ResolveCallStep::Complete(ResolvedCallResult::DynamicAny {
            return_type
        }) if return_type == any
    ));
}

fn anonymous_signature(
    dispatch: &ProjectSemanticDispatch<'_>,
    params: Vec<FunctionParam>,
    type_parameters: Vec<TypeParamDecl>,
    return_type: SemanticNodeId,
) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from(params.into_boxed_slice()),
        return_type,
        type_parameters: Arc::from(type_parameters.into_boxed_slice()),
        occurrence: None,
        return_carrier: SignatureReturnCarrier::Declared(return_type),
        signature_span: None,
        return_type_span: None,
    })
}

/// A candidate with no authored declaration position is paired with its own
/// INSTANTIATION through its content-free origin, never through the
/// graph-instance signature node (instantiation mints a NEW node).
/// `f<string>("x")` on an anonymous `<T>(x: T) => T` selects and returns
/// `string`. Mutation recipe: pair raw and instantiated candidates by
/// signature node id; the call degrades to `Undecidable`.
#[test]
fn anonymous_candidate_pairs_with_its_own_instantiation() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("AnonT"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let sig = anonymous_signature(
        &dispatch,
        vec![FunctionParam::synthetic(None, t, false, false)],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        t,
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    let mut key = call_key(&dispatch, callee, CallKind::Call, None, vec![eager(string)]);
    key.explicit_type_args = Arc::from(vec![string].into_boxed_slice());
    let result = selected(dispatch.execute_resolve_call(key));
    let ResolvedCallResult::Selected { return_type, .. } = result else {
        panic!("an instantiated anonymous candidate selects, got {result:?}");
    };
    assert_eq!(
        return_type, string,
        "the instantiated anonymous candidate returns `string`"
    );
}

/// A winner with NO authored origin is genuinely rootless: its result stays
/// transaction-local and is NEVER admitted to the shared `ResolveCall`
/// family memo, while a winner with a content-free authored occurrence IS.
/// Mutation recipe: admit rootless results; the first assertion fails.
/// Mutation recipe: suppress every admission; the authored control fails.
#[test]
fn rootless_winner_is_transaction_local_but_authored_winner_admits() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let rootless = anonymous_signature(
        &dispatch,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        string,
    );
    let rootless_callee = callable(&dispatch, vec![rootless], Vec::new());
    let rootless_key = call_key(
        &dispatch,
        rootless_callee,
        CallKind::Call,
        None,
        vec![eager(string)],
    );
    let _ = selected_query(&dispatch, rootless_key.clone());
    assert_eq!(
        graph
            .slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(rootless_key))),
        0,
        "a rootless winner never enters the shared cache"
    );

    let authored = signature(
        &dispatch,
        "authoredWinner",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        string,
    );
    let authored_callee = callable(&dispatch, vec![authored], Vec::new());
    let authored_key = call_key(
        &dispatch,
        authored_callee,
        CallKind::Call,
        None,
        vec![eager(string)],
    );
    let _ = selected_query(&dispatch, authored_key.clone());
    assert!(
        graph
            .slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(authored_key)))
            > 0,
        "an authored-occurrence winner IS admitted"
    );
}

/// A context-sensitive argument (a function value with an un-annotated
/// parameter) is withheld from the FIRST inference pass: the eager argument
/// alone fixes `T`, and the withheld argument is then checked for
/// applicability under that fixed substitution. Mutation: drop the
/// context-sensitivity withholding — the lambda's `any` parameter deposits
/// and beats the literal, and `T` binds `any`. The second half is the
/// negative control: the SAME nodes with the argument marked
/// context-FREE still deposit `any`, proving the assertion tracks the flag
/// and not the node shapes.
#[test]
fn context_sensitive_argument_is_withheld_from_the_first_inference_pass() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let literal = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("literal".into()),
    ));
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    // `cb: (item: T) => unknown`
    let callback_param = signature(
        &dispatch,
        "callbackParam",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, t, false, false)],
        Vec::new(),
        unknown,
    );
    // The authored argument `(item) => item`: an un-annotated parameter
    // lowers to `any`, and so does the arrow's own return.
    let untyped_lambda = signature(
        &dispatch,
        "untypedLambda",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, any, false, false)],
        Vec::new(),
        any,
    );
    // `declare function withCallback<T>(cb: (item: T) => unknown, item: T): T`
    let with_callback = signature(
        &dispatch,
        "withCallback",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, callback_param, false, false),
            FunctionParam::synthetic(None, t, false, false),
        ],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        t,
    );
    let callee = callable(&dispatch, vec![with_callback], Vec::new());

    let lambda_arg = |context_sensitive| CallArgKey::Eager {
        ty: untyped_lambda,
        spread: false,
        literal_mode: ArgumentLiteralMode::Literal,
        context_sensitive,
    };

    let withheld = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![lambda_arg(true), eager(literal)],
    )));
    let ResolvedCallResult::Selected { return_type, .. } = withheld else {
        panic!("the generic candidate must select")
    };
    assert_eq!(
        return_type, literal,
        "the eager argument alone fixes T; the withheld lambda contributes no candidate"
    );
    assert_ne!(return_type, any);

    // Negative control: the same nodes, marked context-FREE, DO deposit.
    let eager_lambda = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![lambda_arg(false), eager(literal)],
    )));
    let ResolvedCallResult::Selected { return_type, .. } = eager_lambda else {
        panic!("the generic candidate must select")
    };
    assert_eq!(
        return_type, any,
        "a context-FREE function argument still deposits its parameter type"
    );
}

/// A receiver-LESS call site still satisfies a candidate whose authored
/// `this` ACCEPTS `undefined`. `this: void` is the canonical TypeScript
/// "callable without a receiver" annotation (Svelte's `Snippet` declares
/// `(this: void, ...a: P): void`), and `this: unknown` / `this: any` accept
/// it too. The verdict is the ordinary typed assignability relation run
/// against the `undefined` the absent receiver supplies — never a name or
/// kind special-case — so a `this` demanding a concrete object is still
/// rejected receiver-less.
///
/// Mutation recipe: reject unconditionally whenever the key carries no
/// receiver and the `void` / `unknown` calls stop selecting; accept
/// unconditionally instead and the object-`this` negative selects.
#[test]
fn receiverless_call_admits_only_a_this_that_accepts_undefined() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let void = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let data_object = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(
                vec![crate::semantic_query::SurfaceMember {
                    excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                    key: crate::semantic_query::AuthoredPropertyKey::string("data"),
                    value: string,
                    optional: false,
                    readonly: false,
                    method_kind: None,
                    has_implementation_body: false,
                    visibility: verter_type_expr::MemberVisibility::Public,
                    spans: Default::default(),
                    declaration_origin: None,
                    declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                    merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                }]
                .into_boxed_slice(),
            ),
            call_signatures: Arc::from(Vec::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        },
    ));

    let receiverless = |this_ty: SemanticNodeId, name: &str| {
        let sig = signature(
            &dispatch,
            name,
            0,
            SignatureKind::Call,
            vec![
                FunctionParam::synthetic(Some(Arc::from("this")), this_ty, false, false),
                FunctionParam::synthetic(Some(Arc::from("value")), number, false, false),
            ],
            Vec::new(),
            ret,
        );
        let callee = callable(&dispatch, vec![sig], Vec::new());
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            callee,
            CallKind::Call,
            None,
            vec![eager(number)],
        ))
    };

    assert!(
        matches!(
            selected(receiverless(void, "voidThis")),
            ResolvedCallResult::Selected { return_type, .. } if return_type == ret
        ),
        "`this: void` is callable with no receiver — `undefined` is assignable to `void`"
    );
    assert!(
        matches!(
            selected(receiverless(unknown, "unknownThis")),
            ResolvedCallResult::Selected { return_type, .. } if return_type == ret
        ),
        "`this: unknown` accepts the absent receiver's `undefined` too"
    );
    assert!(
        matches!(
            receiverless(data_object, "objectThis"),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "a `this` demanding a concrete object is NOT satisfied by the absent \
         receiver's `undefined` — the gate stays a typed relation"
    );
}

/// A GENERIC rest parameter (`...args: A`) is ONE inference position over
/// the whole trailing argument list: the arguments assemble into a TUPLE
/// candidate for the parameter itself, and the declaration-site constraint
/// is checked against that assembled tuple. The constraint decides neither
/// the per-argument target nor the arity — a generic rest can never
/// DEFINITELY mismatch on arity, because candidates are acquired
/// uninstantiated and the rest's type is still the `TypeParam`.
///
/// The three rows discriminate independently:
///
/// - `A extends unknown[]`, returning `A`, is called with `(number,
///   string)`. Mapping each argument onto the CONSTRAINT's element type
///   (`unknown`) leaves the parameter with no candidate at all, so it
///   falls back to `unknown`, `unknown extends unknown[]` fails, and the
///   call degrades. The exact returned tuple therefore fails on any
///   element-mapped implementation, not just on an arity cap.
/// - `A extends [string, string]` called with `(number, number)` still
///   REJECTS: the assembled tuple is constraint-checked, so the inference
///   path is not a blanket accept.
/// - A concrete one-parameter candidate still caps arity at one.
///
/// Mutation recipe: map each trailing argument onto the constraint's
/// element type and the first row degrades; drop the post-fixation
/// constraint check and the second row selects; fail open for every rest
/// shape and the third row selects.
#[test]
fn generic_rest_infers_a_tuple_candidate_and_never_caps_arity() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let unknown_array = graph.intern_node(SemanticNodeData::Array {
        element: unknown,
        readonly: false,
    });
    let tuple = |elements: Vec<SemanticNodeId>| {
        graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from(
                elements
                    .into_iter()
                    .map(|value| TupleElement {
                        label: None,
                        value,
                        optional: false,
                        rest: false,
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            ),
            readonly: false,
        })
    };

    // `<A extends CONSTRAINT>(...args: A): RETURN`
    let variadic = |name: &str, constraint: SemanticNodeId, return_type: SemanticNodeId| {
        let param = graph.intern_node(SemanticNodeData::TypeParam {
            decl: crate::semantic_query::DeclIdentity::synthetic(name),
            param_index: 0,
            constraint: Some(constraint),
            default: None,
            display_name: Arc::from("A"),
        });
        let return_type = if return_type == unknown {
            param
        } else {
            return_type
        };
        let sig = signature(
            &dispatch,
            name,
            0,
            SignatureKind::Call,
            vec![FunctionParam::synthetic(
                Some(Arc::from("args")),
                param,
                false,
                true,
            )],
            vec![TypeParamDecl {
                name: Arc::from("A"),
                param,
                constraint: Some(constraint),
                default: None,
                is_const: false,
            }],
            return_type,
        );
        callable(&dispatch, vec![sig], Vec::new())
    };

    // ROW 1 — the parameter itself collects the trailing arguments as a
    // tuple, and the signature's `A` return publishes exactly that tuple.
    let inferring = variadic("variadicInfers", unknown_array, unknown);
    let ResolvedCallResult::Selected { return_type, .. } =
        selected(dispatch.execute_resolve_call(call_key(
            &dispatch,
            inferring,
            CallKind::Call,
            None,
            vec![eager(number), eager(string)],
        )))
    else {
        panic!("`...args: A` with `A extends unknown[]` selects for two arguments");
    };
    assert_eq!(
        return_type,
        tuple(vec![number, string]),
        "the generic rest infers the trailing arguments as the tuple [number, string]"
    );

    // ROW 2 — the assembled tuple is still constraint-checked.
    let constrained = variadic("variadicConstrained", tuple(vec![string, string]), ret);
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                constrained,
                CallKind::Call,
                None,
                vec![eager(number), eager(number)],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "an assembled tuple that violates the declaration-site constraint is rejected"
    );

    // ROW 3 — negative control: a concrete fixed-arity candidate is still capped.
    let fixed = signature(
        &dispatch,
        "fixedArity",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        ret,
    );
    let fixed_callee = callable(&dispatch, vec![fixed], Vec::new());
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                fixed_callee,
                CallKind::Call,
                None,
                vec![eager(number), eager(number)],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "a concrete one-parameter candidate still caps arity at one"
    );
}

/// Excess-property checking is an ARGUMENT-position rule. The receiver
/// (`this`) position runs the SAME typed assignability relation but never
/// the excess prepass, so a fresh object literal carrying a member the
/// `this` surface does not declare still binds — while the identical
/// literal in an argument position against the identical target is
/// rejected.
///
/// The two rows differ ONLY in the position the literal occupies, so the
/// pair discriminates the position policy rather than the relation.
///
/// Mutation recipe: excess-check the receiver position and the first row
/// degrades; stop excess-checking arguments and the second row selects.
#[test]
fn excess_property_checking_is_an_argument_position_rule() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let ret = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let member =
        |name: &str, value: SemanticNodeId, origin: verter_type_expr::ExcessPropertyOrigin| {
            crate::semantic_query::SurfaceMember {
                excess_origin: origin,
                key: crate::semantic_query::AuthoredPropertyKey::string(name),
                value,
                optional: false,
                readonly: false,
                method_kind: None,
                has_implementation_body: false,
                visibility: verter_type_expr::MemberVisibility::Public,
                spans: Default::default(),
                declaration_origin: None,
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            }
        };
    let object = |members: Vec<crate::semantic_query::SurfaceMember>| {
        graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ))
    };
    // The declared surface: `{ data: string }`.
    let declared = object(vec![member(
        "data",
        string,
        verter_type_expr::ExcessPropertyOrigin::NonLiteral,
    )]);
    // The FRESH literal `{ data: "…", extra: 1 }` — one member the declared
    // surface does not know.
    let fresh = object(vec![
        member(
            "data",
            string,
            verter_type_expr::ExcessPropertyOrigin::FreshOwn,
        ),
        member(
            "extra",
            number,
            verter_type_expr::ExcessPropertyOrigin::FreshOwn,
        ),
    ]);
    assert_eq!(
        dispatch.freshness_for_source_node(fresh),
        crate::semantic_query::FreshnessKey::Fresh,
        "the fixture literal is a proven fresh source on both rows"
    );

    // ROW 1 — the literal is the RECEIVER.
    let receiver_sig = signature(
        &dispatch,
        "receiverPosition",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(
            Some(Arc::from("this")),
            declared,
            false,
            false,
        )],
        Vec::new(),
        ret,
    );
    let receiver_callee = callable(&dispatch, vec![receiver_sig], Vec::new());
    assert!(
        matches!(
            selected(dispatch.execute_resolve_call(call_key(
                &dispatch,
                receiver_callee,
                CallKind::Call,
                Some(fresh),
                Vec::new(),
            ))),
            ResolvedCallResult::Selected { return_type, .. } if return_type == ret
        ),
        "a receiver position never excess-checks its fresh literal"
    );

    // ROW 2 — the SAME literal against the SAME target, as an argument.
    let argument_sig = signature(
        &dispatch,
        "argumentPosition",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(
            Some(Arc::from("config")),
            declared,
            false,
            false,
        )],
        Vec::new(),
        ret,
    );
    let argument_callee = callable(&dispatch, vec![argument_sig], Vec::new());
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                argument_callee,
                CallKind::Call,
                None,
                vec![eager(fresh)],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "an argument position DOES excess-check the identical fresh literal"
    );

    // Regularizing that literal removes its freshness — and reaches every
    // member, including a value node SHARED between two members.
    let shared = object(vec![member(
        "inner",
        number,
        verter_type_expr::ExcessPropertyOrigin::FreshOwn,
    )]);
    let nested = object(vec![
        member(
            "a",
            shared,
            verter_type_expr::ExcessPropertyOrigin::FreshOwn,
        ),
        member(
            "b",
            shared,
            verter_type_expr::ExcessPropertyOrigin::FreshOwn,
        ),
    ]);
    let regular = dispatch.regularized_source_node(nested);
    assert_eq!(
        dispatch.freshness_for_source_node(regular),
        crate::semantic_query::FreshnessKey::Regular,
        "a regularized surface is no longer a fresh source"
    );
    let Some(SemanticNodeData::Object(view)) = graph.node_data(regular).as_deref().cloned() else {
        panic!("the regularized peer is still an object surface");
    };
    for slot in view.positive_members() {
        assert_eq!(
            dispatch.freshness_for_source_node(slot.value),
            crate::semantic_query::FreshnessKey::Regular,
            "every member value — including the SHARED one reached twice — is regularized"
        );
    }
}

const OVERLOAD_CANONICAL: &str = "/ws/call-resolve-invalidation.ts";

fn overload_source(argument: &str) -> String {
    format!(
        "export declare function over(k: \"alpha\"): \"picked-alpha\";\n\
         export declare function over(k: \"gamma\"): \"picked-gamma\";\n\
         export const argValue = \"{argument}\";\n\
         export const chosen = over(argValue);\n"
    )
}

fn upsert_overload_source(host: &VerterHost, argument: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(OVERLOAD_CANONICAL.to_string()),
        input_id: OVERLOAD_CANONICAL.to_string(),
        source: Arc::from(overload_source(argument).as_str()),
        file_language: crate::LanguageRegistry::global()
            .classify_static(OVERLOAD_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// The offset of the authored call argument inside `over("…")`. Both
/// revisions are byte-length identical, so this offset is the SAME in
/// both.
fn overload_argument_offset(argument: &str) -> u32 {
    const PREFIX: &str = "export const argValue = ";
    let source = overload_source(argument);
    let start = source
        .find(PREFIX)
        .expect("the argument binding is authored");
    u32::try_from(start + PREFIX.len()).expect("offset fits")
}

fn with_overload_dispatch<R>(
    host: &Arc<VerterHost>,
    f: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R,
) -> R {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    f(&dispatch)
}

/// Build the two-overload callee plus the CONTENT-FREE call key whose
/// single argument is identified by its program point — no argument type
/// node, no content hash, no version in the key.
fn overload_call_key(dispatch: &ProjectSemanticDispatch<'_>, offset: u32) -> ResolveCallKey {
    let graph = dispatch.graph();
    let alpha = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("alpha".to_owned()),
    ));
    let gamma = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("gamma".to_owned()),
    ));
    let picked_alpha = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("picked-alpha".to_owned()),
    ));
    let picked_gamma = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("picked-gamma".to_owned()),
    ));
    let first = signature(
        dispatch,
        "over",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, alpha, false, false)],
        Vec::new(),
        picked_alpha,
    );
    let second = signature(
        dispatch,
        "over",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, gamma, false, false)],
        Vec::new(),
        picked_gamma,
    );
    let callee = callable(dispatch, vec![first, second], Vec::new());
    let ResolveCallContext {
        parse_env_hash,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
    } = dispatch.resolve_call_context_for(OVERLOAD_CANONICAL);
    ResolveCallKey {
        point: ProgramPointId {
            canonical_id: Arc::from(OVERLOAD_CANONICAL),
            offset: 0,
        },
        callee,
        kind: CallKind::Call,
        receiver: None,
        args: Arc::from(
            vec![CallArgKey::ProgramExpression {
                point: ProgramPointId {
                    canonical_id: Arc::from(OVERLOAD_CANONICAL),
                    offset,
                },
                spread: false,
                literal_mode: ArgumentLiteralMode::Literal,
                context_sensitive: false,
            }]
            .into_boxed_slice(),
        ),
        explicit_type_args: Arc::from(Vec::new().into_boxed_slice()),
        flow: crate::semantic_query::FlowNarrowingKey::empty(),
        context: ResolveCallContext {
            parse_env_hash,
            resolve_env_hash,
            type_env_hash,
            lib_env_hash,
            project_identity,
            substitution,
        },
    }
}

fn selected_return_literal(dispatch: &ProjectSemanticDispatch<'_>, key: ResolveCallKey) -> String {
    let result = selected_query(dispatch, key);
    let return_type = match result {
        ResolvedCallResult::Selected { return_type, .. }
        | ResolvedCallResult::UnionSelected { return_type, .. }
        | ResolvedCallResult::DynamicAny { return_type } => return_type,
    };
    match dispatch.graph().node_data(return_type).as_deref() {
        Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(value))) => {
            value.to_string()
        }
        other => panic!("the call must select a string-literal return, got {other:?}"),
    }
}

/// A `ResolveCall` key is CONTENT-FREE: its argument is identified by
/// program point, so editing that argument at the identical canonical and
/// identical byte offsets leaves the FULL key bit-equal while flipping
/// overload selection. Validity therefore rests ENTIRELY on the value side
/// — the read-set signature and self-roots recorded on the cached result,
/// revalidated against the caller's live view. This is that rail's
/// producer-specific proof: the warm entry under the equal key is
/// REJECTED after the edit, the call RECOMPUTES, and it answers the NEW
/// overload's return.
///
/// Mutation recipe: drop the `entry.validate(ctx)` value-side check from
/// `get_resolve_call_result` and the post-edit warm read serves
/// `"picked-alpha"` for a file that now says `"gamma"` — the project
/// generation is UNCHANGED by the edit, so the generation gate alone does
/// not catch it.
#[test]
fn resolve_call_same_key_argument_edit_rejects_warm_and_recomputes_the_new_overload() {
    let host = host();
    upsert_overload_source(&host, "alpha");
    let offset = overload_argument_offset("alpha");
    assert_eq!(
        offset,
        overload_argument_offset("gamma"),
        "both revisions author the argument at the identical offset"
    );

    let before = with_overload_dispatch(&host, |dispatch| {
        let key = overload_call_key(dispatch, offset);
        assert_eq!(
            selected_return_literal(dispatch, key.clone()),
            "picked-alpha",
            "the first revision selects the `\"alpha\"` overload"
        );
        assert!(
            dispatch
                .graph()
                .get_resolve_call_result(dispatch.ctx, &key)
                .is_some(),
            "the first revision's result is warm under the content-free key"
        );
        key
    });

    upsert_overload_source(&host, "gamma");

    with_overload_dispatch(&host, |dispatch| {
        let key = overload_call_key(dispatch, offset);
        assert_eq!(
            key, before,
            "the edit leaves the FULL ResolveCall key bit-equal — same point, \
             same callee node, same content-free argument identity, same env"
        );
        assert!(
            dispatch
                .graph()
                .get_resolve_call_result(dispatch.ctx, &key)
                .is_none(),
            "the value-side read set / self-roots REJECT the warm entry the \
             equal key would otherwise serve"
        );
        assert_eq!(
            selected_return_literal(dispatch, key.clone()),
            "picked-gamma",
            "the recomputation selects the `\"gamma\"` overload"
        );
        assert!(
            dispatch
                .graph()
                .get_resolve_call_result(dispatch.ctx, &key)
                .is_some(),
            "the recomputed result warms under the same key"
        );
    });
}

// ---------------------------------------------------------------------------
// Source-driven call-applicability regressions
// ---------------------------------------------------------------------------

/// Upsert `source` at `canonical` into a fresh standalone host.
fn source_host(canonical: &str, source: &str) -> Arc<VerterHost> {
    let host = host();
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// The whole-function return of `name` in `canonical`, projected to its
/// authored `TypeExpr`.
fn source_flow_type(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
) -> verter_type_expr::TypeExpr {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = SemanticQueryApi::execute(&dispatch, SemanticQueryKey::FlowReturn(Box::new(key)))
    else {
        panic!("{name} must produce a complete flow return");
    };
    host.project_node_to_type_expr_for_test(result.return_type())
        .expect("the flow return node must project to a TypeExpr")
}

const TUPLE_REST_CANONICAL: &str = "/ws/call-tuple-rest.ts";
const TUPLE_REST_SOURCE: &str = r#"
export declare function tupleRest(...args: [number, ...string[]]): "ok";
export function callTupleRestOne() {
  return tupleRest(1);
}
export function callTupleRestTwo() {
  return tupleRest(1, "a");
}
export function callTupleRestThree() {
  return tupleRest(1, "a", "b");
}
export function callTupleRestSpread(rest: string[]) {
  return tupleRest(1, ...rest);
}
"#;

/// A positional argument landing on a tuple rest parameter's TRAILING rest
/// ELEMENT relates against that element's element type, not against the
/// rest element's own array type. Without the unwrap every tuple-rest call
/// with more arguments than the tuple's fixed prefix is inapplicable.
///
/// Mutation recipe: return the trailing rest element's own value from the
/// positional lookup and the two-and-three-argument rows stop resolving.
#[test]
fn tuple_rest_positional_argument_unwraps_the_trailing_rest_element() {
    let host = source_host(TUPLE_REST_CANONICAL, TUPLE_REST_SOURCE);
    let ok = verter_type_expr::TypeExpr::string_literal("ok");
    assert_eq!(
        source_flow_type(&host, TUPLE_REST_CANONICAL, "callTupleRestOne"),
        ok,
        "the tuple's fixed prefix alone is applicable"
    );
    assert_eq!(
        source_flow_type(&host, TUPLE_REST_CANONICAL, "callTupleRestTwo"),
        ok,
        "one argument past the fixed prefix maps onto the rest element's element type"
    );
    assert_eq!(
        source_flow_type(&host, TUPLE_REST_CANONICAL, "callTupleRestThree"),
        ok,
        "further arguments keep mapping onto the rest element's element type"
    );
}

/// An INDEFINITE spread into a tuple rest parameter relates against the
/// tuple SUFFIX it actually supplies, not against the whole tuple: the
/// fixed prefix is already covered by the preceding positional arguments.
///
/// Mutation recipe: relate the spread against the rest parameter's whole
/// declared tuple and this call stops resolving.
#[test]
fn indefinite_spread_relates_against_the_remaining_tuple_suffix() {
    let host = source_host(TUPLE_REST_CANONICAL, TUPLE_REST_SOURCE);
    assert_eq!(
        source_flow_type(&host, TUPLE_REST_CANONICAL, "callTupleRestSpread"),
        verter_type_expr::TypeExpr::string_literal("ok"),
        "a `string[]` spread covers the `...string[]` tail of the rest tuple"
    );
}

const EXPLICIT_ARGS_CANONICAL: &str = "/ws/call-explicit-type-args.ts";
const EXPLICIT_ARGS_SOURCE: &str = r#"
export function callWideFirst(f: { <T, U>(x: T): "two"; <T>(x: T): "one" }) {
  return f<string>("x");
}
export function callNarrowFirst(f: { <T>(x: T): "one"; <T, U>(x: T): "two" }) {
  return f<string>("x");
}
export function callWideFirstBare(f: { <T, U>(x: T): "two"; <T>(x: T): "one" }) {
  return f("x");
}
"#;

/// Explicit type arguments drop the candidates that cannot accept them,
/// and the surviving candidate keeps its own source ordinal: pairing an
/// instantiated ROOTLESS candidate with its raw form must follow the same
/// drop, never the raw list's unfiltered position. Declaration order of
/// the overloads is irrelevant to the answer.
///
/// Mutation recipe: pair the instantiated candidate with the raw candidate
/// at the same UNFILTERED position and the `wideFirst` row stops resolving
/// while `narrowFirst` keeps passing by coincidence.
#[test]
fn explicit_type_args_pair_the_surviving_candidate_with_its_own_raw_form() {
    let host = source_host(EXPLICIT_ARGS_CANONICAL, EXPLICIT_ARGS_SOURCE);
    let one = verter_type_expr::TypeExpr::string_literal("one");
    assert_eq!(
        source_flow_type(&host, EXPLICIT_ARGS_CANONICAL, "callWideFirstBare"),
        verter_type_expr::TypeExpr::string_literal("two"),
        "without explicit type arguments the first declared overload still wins"
    );
    assert_eq!(
        source_flow_type(&host, EXPLICIT_ARGS_CANONICAL, "callNarrowFirst"),
        one,
        "the one-binder overload is the only candidate that accepts one explicit type argument"
    );
    assert_eq!(
        source_flow_type(&host, EXPLICIT_ARGS_CANONICAL, "callWideFirst"),
        one,
        "the same overload wins when a DROPPED candidate precedes it in declaration order"
    );
}

const DEPENDENT_DEFAULT_CANONICAL: &str = "/ws/call-dependent-default.ts";
const DEPENDENT_DEFAULT_SOURCE: &str = r#"
export declare function dependentDefault<T, U = T>(x: U): U;
export declare function independentDefault<T, U = number>(x: U): U;
export function callDependentDefault() {
  return dependentDefault<string>("x");
}
export function callIndependentDefault() {
  return independentDefault<string>(1);
}
"#;

/// A type parameter default that REFERENCES an earlier parameter is
/// substituted under the bindings already fixed by the explicit arguments
/// before it is used. Reaching for the unsubstituted default node leaves a
/// binder with no binding, and the instantiation collapses.
///
/// Mutation recipe: use `decl.default` verbatim and the dependent row
/// stops resolving while the independent one keeps passing.
#[test]
fn dependent_type_parameter_default_substitutes_earlier_bindings() {
    let host = source_host(DEPENDENT_DEFAULT_CANONICAL, DEPENDENT_DEFAULT_SOURCE);
    assert_eq!(
        source_flow_type(&host, DEPENDENT_DEFAULT_CANONICAL, "callIndependentDefault"),
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        "an independent default instantiates directly"
    );
    assert_eq!(
        source_flow_type(&host, DEPENDENT_DEFAULT_CANONICAL, "callDependentDefault"),
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        "`U = T` takes the type argument already bound to `T`"
    );
}

// ---------------------------------------------------------------------------
// Ambient `Function.prototype.call` rebasing
// ---------------------------------------------------------------------------

/// The vendored non-generic ambient `call`.
const AMBIENT_PLAIN_CALL: &str = r#"
interface Function {
  call(this: Function, thisArg: any, ...argArray: any[]): any;
}
"#;

/// The strict ambient `call`, whose own type parameters bind the receiver,
/// the argument tuple, and the return.
const AMBIENT_STRICT_CALL: &str = r#"
interface Function {
  call<T, A extends any[], R>(this: (this: T, ...args: A) => R, thisArg: T, ...args: A): R;
}
"#;

const PROTOTYPE_CALL_CANONICAL: &str = "/ws/call-prototype.ts";

/// A host with one configured project over `/ws`, `lib` registered as that
/// project's ambient corpus, and `source` upserted at `canonical`.
fn ambient_lib_host(lib: &str, canonical: &str, source: &str) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        verter_workspace::VfsProjectConfig {
            root: "/ws".to_string(),
            rank: verter_workspace::ProjectRank::Explicit,
            tsconfig_path: Some("/ws/tsconfig.json".to_string()),
            root_files: vec![],
            extensions: vec![".ts".into()],
            workspace_root: "/ws".to_string(),
            workspace_aliases: vec![],
            compiler_options: verter_semantic::resolver_core::IdeProjectCompilerOptions::default(),
            references: vec![],
            membership: verter_workspace::configured_membership_match_all_under_root(
                &verter_workspace::CanonicalPath::new("/ws"),
            ),
        },
    ]));
    verter_workspace::WorkspaceAccess::register_ambient_lib(
        workspace.as_ref(),
        verter_workspace::AmbientLibSpec {
            project_id: None,
            canonical_id: Arc::from("lib.es5.d.ts"),
            source: Arc::from(lib),
        },
    )
    .expect("the ambient corpus registers against the configured project");
    let key = verter_workspace::WorkspaceRead::project_stable_key(
        workspace.as_ref(),
        verter_workspace::ProjectId(0),
    )
    .expect("project key");
    let virtual_id = verter_workspace::ambient_virtual_canonical_id(key, "lib.es5.d.ts");
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), access));
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: virtual_id.to_string(),
            source: Arc::from(lib),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the ambient lib serves");
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the fixture serves");
    host
}

const ANCHORED_CALLABLE_SOURCE: &str = r#"
export declare const anchored: (x: string) => 1;
"#;

/// The ambient `call` member of `anchored`'s apparent surface.
fn ambient_call_member(
    host: &Arc<VerterHost>,
    dispatch: &ProjectSemanticDispatch<'_>,
) -> SemanticNodeId {
    let env = host.host_view_env_hashes_for(PROTOTYPE_CALL_CANONICAL);
    let project_identity = host
        .host_view_project_identity_for(PROTOTYPE_CALL_CANONICAL)
        .fold_u32();
    let anchored = match dispatch.execute_type_node(SemanticQueryKey::TypeOf {
        value_root: crate::semantic_query::ValueRootSlotIdentity::new(
            crate::semantic_query::ValueRootKey {
                scope: crate::semantic_query::ScopeId::file(
                    Arc::from(PROTOTYPE_CALL_CANONICAL),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                ),
                name: Arc::from("anchored"),
            },
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        ),
        context: crate::semantic_query::TypeOfContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            env.resolve_env_hash,
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("`typeof anchored` must resolve, got {other:?}"),
    };
    match dispatch.execute_type_node(SemanticQueryKey::ProjectMember {
        base: anchored,
        member: Arc::from("call"),
        mode: crate::semantic_query::ProjectionMode::Expanded,
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("the apparent `call` member must resolve, got {other:?}"),
    }
}

/// Rebasing the ambient `.call` onto a ROOTLESS callable keeps the rebased
/// call's return. The rebased sub-call closed its own component, so its
/// value is already final — a return-equation HOLD on it could never be
/// solved, because a rootless winner is refused by the shared-cache
/// admission fence and so never reaches the completed-member ledger the
/// equation reads. The fence is about publication; the enclosing equation
/// takes the value it has in hand as a concrete seed.
///
/// Mutation recipe: hold the outer equation on the rebased identity
/// unconditionally and this call degrades to `Undecidable`.
#[test]
fn prototype_call_rebase_onto_a_rootless_callable_keeps_its_return() {
    let host = ambient_lib_host(
        AMBIENT_PLAIN_CALL,
        PROTOTYPE_CALL_CANONICAL,
        ANCHORED_CALLABLE_SOURCE,
    );
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let graph = dispatch.graph();
    let call_member = ambient_call_member(&host, &dispatch);
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    let rootless_return = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("rootless".into()),
    ));
    // An ANONYMOUS callable: no authored occurrence, so its winner is
    // `SignatureCandidateOrigin::Rootless`.
    let extracted = graph.intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from(
            vec![FunctionParam::synthetic(None, string, false, false)].into_boxed_slice(),
        ),
        return_type: rootless_return,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        occurrence: None,
        return_carrier: SignatureReturnCarrier::Declared(rootless_return),
        signature_span: None,
        return_type_span: None,
    });
    let key = prototype_call_key(&dispatch, call_member, extracted, vec![undefined, string]);
    let result = match dispatch.execute_resolve_call(key) {
        super::call_resolve::ResolveCallStep::Complete(result) => result,
        other => panic!("the rebased `.call` must complete, got {other:?}"),
    };
    let ResolvedCallResult::Selected { return_type, .. } = result else {
        panic!("the rebased `.call` selects a signature, got {result:?}");
    };
    assert_eq!(
        host.project_node_to_type_expr_for_test(return_type),
        Some(verter_type_expr::TypeExpr::string_literal("rootless")),
        "the outer call returns the rebased callable's declared return"
    );
}

/// A `ResolveCallKey` for `<receiver>.call(<args…>)` authored in the
/// prototype-call fixture.
fn prototype_call_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    callee: SemanticNodeId,
    receiver: SemanticNodeId,
    args: Vec<SemanticNodeId>,
) -> ResolveCallKey {
    ResolveCallKey {
        point: ProgramPointId {
            canonical_id: Arc::from(PROTOTYPE_CALL_CANONICAL),
            offset: 0,
        },
        callee,
        kind: CallKind::Call,
        receiver: Some(receiver),
        args: Arc::from(
            args.into_iter()
                .map(eager)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        explicit_type_args: Arc::from(Vec::new().into_boxed_slice()),
        flow: crate::semantic_query::FlowNarrowingKey::empty(),
        context: dispatch.resolve_call_context_for(PROTOTYPE_CALL_CANONICAL),
    }
}

const EXPLICIT_CALL_SOURCE: &str = r#"
export declare const bound: (this: { x: number }, n: number) => string;
export function withExplicitTypeArgs() {
  return bound.call<{ x: number }, [number], string>({ x: 1 }, 1);
}
export function withoutExplicitTypeArgs() {
  return bound.call({ x: 1 }, 1);
}
"#;

/// Explicit type arguments authored on `.call` bind the AMBIENT method's
/// own type parameters. Rebasing onto the extracted callable discards them
/// — the extracted callable is a different function and never accepts
/// them.
///
/// Mutation recipe: carry the ambient method's explicit type arguments
/// onto the rebased callee and the explicit row stops resolving while the
/// implicit one keeps passing.
#[test]
fn prototype_call_rebase_discards_the_ambient_methods_explicit_type_args() {
    let host = ambient_lib_host(
        AMBIENT_STRICT_CALL,
        PROTOTYPE_CALL_CANONICAL,
        EXPLICIT_CALL_SOURCE,
    );
    let string = verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String);
    assert_eq!(
        source_flow_type(&host, PROTOTYPE_CALL_CANONICAL, "withoutExplicitTypeArgs"),
        string,
        "an inferred `.call` rebases onto the extracted callable"
    );
    assert_eq!(
        source_flow_type(&host, PROTOTYPE_CALL_CANONICAL, "withExplicitTypeArgs"),
        string,
        "explicitly instantiating the ambient `.call` yields the same return"
    );
}

fn tuple_of(dispatch: &ProjectSemanticDispatch<'_>, values: Vec<SemanticNodeId>) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            values
                .into_iter()
                .map(|value| TupleElement {
                    label: None,
                    value,
                    optional: false,
                    rest: false,
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ),
        readonly: false,
    })
}

fn type_param(dispatch: &ProjectSemanticDispatch<'_>, name: &str, index: u16) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic(name),
        param_index: index,
        constraint: None,
        default: None,
        display_name: Arc::from(name),
    })
}

fn plain_decl(param: SemanticNodeId, name: &str) -> TypeParamDecl {
    TypeParamDecl {
        name: Arc::from(name),
        param,
        constraint: None,
        default: None,
        is_const: false,
    }
}

/// Literal elements inside a tuple ARGUMENT relate covariantly to concrete
/// tuple-element targets, exactly as `tsc --strict` accepts
/// `k([1, 1])` against `(x: [number, number])` and `k3(["a"])` against
/// `(x: [string])`. Mutation recipe: re-adding a reverse
/// (`target-element ≤ source-element`) leg to the tuple/tuple relation arm
/// rejects the literal-element tuples again and both selections fail.
#[test]
fn tuple_argument_literal_elements_select_concrete_tuple_targets() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let one = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(1.0),
    ));
    let lit_a = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("a".into()),
    ));
    let pair_param = tuple_of(&dispatch, vec![number, number]);
    let pair_sig = signature(
        &dispatch,
        "k",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, pair_param, false, false)],
        Vec::new(),
        number,
    );
    let pair_callee = callable(&dispatch, vec![pair_sig], Vec::new());
    let pair_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        pair_callee,
        CallKind::Call,
        None,
        vec![fresh_literal(tuple_of(&dispatch, vec![one, one]))],
    )));
    assert!(
        matches!(
            pair_result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "k([1, 1]) selects against (x: [number, number]), got {pair_result:?}"
    );
    let single_param = tuple_of(&dispatch, vec![string]);
    let single_sig = signature(
        &dispatch,
        "k3",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, single_param, false, false)],
        Vec::new(),
        number,
    );
    let single_callee = callable(&dispatch, vec![single_sig], Vec::new());
    let single_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        single_callee,
        CallKind::Call,
        None,
        vec![fresh_literal(tuple_of(&dispatch, vec![lit_a]))],
    )));
    assert!(
        matches!(
            single_result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "k3([\"a\"]) selects against (x: [string]), got {single_result:?}"
    );
    // Negative rail: a primitive-element tuple still never satisfies a
    // narrower literal-element tuple target.
    let literal_param = tuple_of(&dispatch, vec![one]);
    let literal_sig = signature(
        &dispatch,
        "kl",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, literal_param, false, false)],
        Vec::new(),
        number,
    );
    let literal_callee = callable(&dispatch, vec![literal_sig], Vec::new());
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                literal_callee,
                CallKind::Call,
                None,
                vec![eager(tuple_of(&dispatch, vec![number]))],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "[number] must not satisfy (x: [1])"
    );
}

/// A generic tuple-shaped parameter (`<T>(x: [T, T]): T`) DECIDES from its
/// element deposits: a literal tuple argument, a typed tuple value, and a
/// heterogeneous `<A, B>(x: [A, B])` pair all select with the substituted
/// return — `tsc --strict` accepts every one. Mutation recipe: returning
/// `Unknown` from the tuple arm after successful element deposits degrades
/// all three calls to `Undecidable`.
#[test]
fn generic_tuple_parameter_decides_from_element_deposits() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let one = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(2.0),
    ));
    let lit_a = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("a".into()),
    ));
    let t = type_param(&dispatch, "T", 0);
    let homo_sig = signature(
        &dispatch,
        "kg",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(
            None,
            tuple_of(&dispatch, vec![t, t]),
            false,
            false,
        )],
        vec![plain_decl(t, "T")],
        t,
    );
    let homo_callee = callable(&dispatch, vec![homo_sig], Vec::new());
    // A bare literal tuple argument: elements deposit and widen → number.
    let literal_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        homo_callee,
        CallKind::Call,
        None,
        vec![fresh_literal(tuple_of(&dispatch, vec![one, two]))],
    )));
    assert!(
        matches!(
            literal_result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "kg([1, 2]) selects T := number, got {literal_result:?}"
    );
    // A typed [number, number] value: elements deposit as authored.
    let typed_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        homo_callee,
        CallKind::Call,
        None,
        vec![eager(tuple_of(&dispatch, vec![number, number]))],
    )));
    assert!(
        matches!(
            typed_result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "kg(tv) with tv: [number, number] selects T := number, got {typed_result:?}"
    );
    // Heterogeneous binders: <A, B>(x: [A, B]): B with [1, "a"] → string.
    let a = type_param(&dispatch, "A", 0);
    let b = type_param(&dispatch, "B", 1);
    let hetero_sig = signature(
        &dispatch,
        "kh",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(
            None,
            tuple_of(&dispatch, vec![a, b]),
            false,
            false,
        )],
        vec![plain_decl(a, "A"), plain_decl(b, "B")],
        b,
    );
    let hetero_callee = callable(&dispatch, vec![hetero_sig], Vec::new());
    let hetero_result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        hetero_callee,
        CallKind::Call,
        None,
        vec![fresh_literal(tuple_of(&dispatch, vec![one, lit_a]))],
    )));
    assert!(
        matches!(
            hetero_result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == string
        ),
        "kh([1, \"a\"]) selects B := string, got {hetero_result:?}"
    );
}

/// Required arity is the LAST required position + 1, not the count of
/// non-optional parameters: `(a?: number, b: string)` requires TWO
/// arguments (`tsc --strict`: TS2554 "Expected 2 arguments, but got 1"),
/// even though only one parameter is non-optional. Mutation recipe:
/// counting `!optional` instead of `rposition` re-accepts the one-argument
/// call. The one-argument rejection is the wrong-accept rail; the
/// two-argument selection pins the fix against over-tightening.
#[test]
fn required_arity_ends_at_last_required_position() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let one = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(1.0),
    ));
    let lit_x = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("x".into()),
    ));
    let sig = signature(
        &dispatch,
        "f",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, number, true, false),
            FunctionParam::synthetic(None, string, false, false),
        ],
        Vec::new(),
        number,
    );
    let callee = callable(&dispatch, vec![sig], Vec::new());
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                callee,
                CallKind::Call,
                None,
                vec![fresh_literal(one)],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "f(1) must reject: the required span ends at the second parameter"
    );
    let both = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![fresh_literal(one), fresh_literal(lit_x)],
    )));
    assert!(
        matches!(
            both,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "f(1, \"x\") selects, got {both:?}"
    );
}

/// A rest tuple with required elements makes EVERY fixed parameter
/// required by position: `(a?: number, ...rest: [string])` requires two
/// arguments (`tsc --strict`: TS2554 "Expected 2" for both `g()` and
/// `g(1)`), while `(a?: number, ...rest: [string?])` accepts zero.
/// Mutation recipe: adding only the tuple's required count on top of the
/// optional-blind fixed count re-accepts `g(1)`.
#[test]
fn rest_tuple_required_elements_count_all_fixed_params() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let one = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(1.0),
    ));
    let lit_x = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("x".into()),
    ));
    let required_rest = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                value: string,
                optional: false,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let required_sig = signature(
        &dispatch,
        "g",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, number, true, false),
            FunctionParam::synthetic(None, required_rest, false, true),
        ],
        Vec::new(),
        number,
    );
    let required_callee = callable(&dispatch, vec![required_sig], Vec::new());
    for args in [Vec::new(), vec![fresh_literal(one)]] {
        let arg_count = args.len();
        assert!(
            matches!(
                dispatch.execute_resolve_call(call_key(
                    &dispatch,
                    required_callee,
                    CallKind::Call,
                    None,
                    args,
                )),
                super::call_resolve::ResolveCallStep::Degraded(
                    crate::semantic_query::ResolveCallFailure::NoApplicableOverload
                )
            ),
            "(a?: number, ...rest: [string]) requires two arguments, accepted {arg_count}"
        );
    }
    let full = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        required_callee,
        CallKind::Call,
        None,
        vec![fresh_literal(one), fresh_literal(lit_x)],
    )));
    assert!(
        matches!(
            full,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "g(1, \"x\") selects, got {full:?}"
    );
    let optional_rest = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![TupleElement {
                label: None,
                value: string,
                optional: true,
                rest: false,
            }]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let optional_sig = signature(
        &dispatch,
        "h",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, number, true, false),
            FunctionParam::synthetic(None, optional_rest, false, true),
        ],
        Vec::new(),
        number,
    );
    let optional_callee = callable(&dispatch, vec![optional_sig], Vec::new());
    let none = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        optional_callee,
        CallKind::Call,
        None,
        Vec::new(),
    )));
    assert!(
        matches!(
            none,
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "h() selects: the optional-element rest tuple requires nothing, got {none:?}"
    );
}

/// The `inference_deposits` fuse charges each ACCEPTED deposit, not one
/// unit per top-level argument: 1025 element deposits from a single tuple
/// argument exceed the 1024-deposit cap and degrade the call to the typed
/// `Budget` (admitting nothing), while the 1024-element call still
/// selects. Mutation recipe: charging once per argument re-accepts the
/// 1025-element call with a single unit on the counter.
#[test]
fn inference_deposit_budget_charges_each_accepted_deposit() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let one = graph.intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::Number(1.0),
    ));
    let t = type_param(&dispatch, "T", 0);
    let run = |element_count: usize| {
        let sig = signature(
            &dispatch,
            "wide",
            0,
            SignatureKind::Call,
            vec![FunctionParam::synthetic(
                None,
                tuple_of(&dispatch, vec![t; element_count]),
                false,
                false,
            )],
            vec![plain_decl(t, "T")],
            t,
        );
        let callee = callable(&dispatch, vec![sig], Vec::new());
        dispatch.execute_resolve_call(call_key(
            &dispatch,
            callee,
            CallKind::Call,
            None,
            vec![fresh_literal(tuple_of(&dispatch, vec![one; element_count]))],
        ))
    };
    let under_cap = run(1024);
    assert!(
        matches!(
            selected(under_cap),
            ResolvedCallResult::Selected { return_type, .. } if return_type == number
        ),
        "1024 accepted deposits stay within the cap and select T := number"
    );
    let over_cap = run(1025);
    assert!(
        matches!(
            over_cap,
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::Budget
            )
        ),
        "1025 accepted deposits trip the deposit fuse, got {over_cap:?}"
    );
}

// ---------------------------------------------------------------------------
// Wrong-pick discipline: approximate rejection never falls through
// ---------------------------------------------------------------------------

/// An earlier candidate may be skipped ONLY on complete proof of definite
/// inapplicability. An INDEFINITE spread argument cannot map onto the
/// first candidate's positional parameters — that is APPROXIMATE argument
/// mapping, not a mismatch, so the call stops as the typed `Undecidable`
/// instead of silently selecting the weaker rest-parameter overload behind
/// it. This shape already stopped as `Undecidable` when the guard was
/// authored (a red run required mutating the verdict), so discrimination
/// rests on the mutation recipe: classify the unmappable-spread candidate
/// as `Mismatch` in `check_call_candidate` and this call selects
/// `"weaker"`.
#[test]
fn approximate_spread_mapping_degrades_instead_of_selecting_weaker_overload() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let string_array = graph.intern_node(SemanticNodeData::Array {
        element: string,
        readonly: false,
    });
    let exact = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("exact".to_owned()),
    ));
    let weaker = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("weaker".to_owned()),
    ));
    let first = signature(
        &dispatch,
        "approx",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        exact,
    );
    let second = signature(
        &dispatch,
        "approx",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string_array, false, true)],
        Vec::new(),
        weaker,
    );
    let callee = callable(&dispatch, vec![first, second], Vec::new());
    let spread_of_array = CallArgKey::Eager {
        ty: string_array,
        spread: true,
        literal_mode: ArgumentLiteralMode::Literal,
        context_sensitive: false,
    };
    let key = call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![spread_of_array],
    );
    assert!(
        matches!(
            dispatch.execute_resolve_call(key.clone()),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::Undecidable
            )
        ),
        "an indefinite spread against a positional first candidate is approximate \
         mapping — the call degrades, never falls through to the rest overload"
    );
    // The degraded case admits nothing.
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(key))),
        0,
        "an Undecidable call never enters the family memo"
    );
}

/// Positive control: a CONCLUSIVE arity mismatch on the first candidate
/// still selects the second — first-applicable declaration order is
/// unchanged by the wrong-pick discipline. Mutation recipe: degrade the
/// whole call on ANY first-candidate rejection and this row stops
/// selecting `"second"`.
#[test]
fn exact_arity_mismatch_still_selects_the_later_overload() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let first_ret = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("first".to_owned()),
    ));
    let second_ret = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("second".to_owned()),
    ));
    let first = signature(
        &dispatch,
        "control",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, string, false, false),
            FunctionParam::synthetic(None, string, false, false),
        ],
        Vec::new(),
        first_ret,
    );
    let second = signature(
        &dispatch,
        "control",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        second_ret,
    );
    let callee = callable(&dispatch, vec![first, second], Vec::new());
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![eager(string)],
    )));
    assert!(
        matches!(
            result,
            ResolvedCallResult::Selected { return_type, .. } if return_type == second_ret
        ),
        "a conclusive two-required-vs-one-argument mismatch on the first overload \
         selects the second, got {result:?}"
    );
}

/// An UNRESOLVED argument mapping (a program-expression argument whose
/// indexed record does not exist) stops the whole call as `Undecidable`
/// before any candidate can be arity-compared — no weaker overload is
/// reachable through unresolved inputs, and nothing is admitted. This
/// shape already stopped as `Undecidable` when the guard was authored;
/// discrimination rests on the mutation recipe: map an unresolvable
/// argument to an `any` placeholder in `acquire_call_arguments` and the
/// call selects `"weaker"`.
#[test]
fn unresolved_argument_mapping_degrades_and_admits_nothing() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let weaker = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("weaker".to_owned()),
    ));
    let first = signature(
        &dispatch,
        "unresolved",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        string,
    );
    let second = signature(
        &dispatch,
        "unresolved",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, true, false)],
        Vec::new(),
        weaker,
    );
    let callee = callable(&dispatch, vec![first, second], Vec::new());
    let unresolved = CallArgKey::ProgramExpression {
        point: ProgramPointId {
            canonical_id: Arc::from("/ws/definitely-not-indexed.ts"),
            offset: 999,
        },
        spread: false,
        literal_mode: ArgumentLiteralMode::Literal,
        context_sensitive: false,
    };
    let key = call_key(&dispatch, callee, CallKind::Call, None, vec![unresolved]);
    assert!(
        matches!(
            dispatch.execute_resolve_call(key.clone()),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::Undecidable
            )
        ),
        "an unresolved argument stops the call as Undecidable"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(key))),
        0,
        "the degraded call never enters the family memo"
    );
}

// ---------------------------------------------------------------------------
// Union callees: ONE composite union-signature group
// ---------------------------------------------------------------------------

fn union_callee(
    dispatch: &ProjectSemanticDispatch<'_>,
    arms: Vec<SemanticNodeId>,
) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Union(Arc::from(arms.into_boxed_slice())))
}

/// `(() => 1) | (() => 2)` selects a first-applicable signature in EVERY
/// arm and unions the selected returns (`tsc --strict`: `1 | 2`).
/// Mutation recipe: route a `Union` callee to the settle loop's miss arm
/// and this call degrades `NotCallable`.
#[test]
fn union_callee_selects_every_arm_and_unions_returns() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let arm_a = signature(
        &dispatch,
        "armA",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        one,
    );
    let arm_b = signature(
        &dispatch,
        "armB",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        two,
    );
    let callee = union_callee(&dispatch, vec![arm_a, arm_b]);
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        Vec::new(),
    )));
    let expected = dispatch.intern_normalized_union_or_intersection(&[one, two], true);
    let ResolvedCallResult::UnionSelected {
        selections,
        return_type,
    } = &result
    else {
        panic!("a union callee closes on the composite union selection, got {result:?}");
    };
    assert_eq!(selections.len(), 2, "one winner per callable arm");
    assert_eq!(
        *return_type, expected,
        "the selected returns union (tsc --strict: 1 | 2)"
    );
}

/// A union of shape-identical arms still selects per arm; the unioned
/// returns collapse to the single shared return. Mutation recipe: treat
/// arm order as overload precedence (first arm wins, others unchecked)
/// and the two-winner assertion fails.
#[test]
fn union_of_identical_arms_collapses_the_return_union() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let arm_a = signature(
        &dispatch,
        "sameA",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        string,
    );
    let arm_b = signature(
        &dispatch,
        "sameB",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        string,
    );
    let callee = union_callee(&dispatch, vec![arm_a, arm_b]);
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        Vec::new(),
    )));
    let ResolvedCallResult::UnionSelected {
        selections,
        return_type,
    } = &result
    else {
        panic!("a union callee closes on the composite union selection, got {result:?}");
    };
    assert_eq!(selections.len(), 2, "every arm decided its own winner");
    assert_eq!(*return_type, string, "identical arm returns collapse");
}

/// Declaration order applies INDEPENDENTLY within each arm: with reversed
/// per-arm overload ordinals a `string` argument selects ordinal 0 in the
/// first arm and ordinal 1 in the second (`tsc --strict`:
/// `"a-str" | "b-str"`). Mutation recipe: share one first-applicable
/// cursor across arms and the second arm's number overload wins or the
/// call rejects.
#[test]
fn union_arm_overload_order_is_per_arm_not_cross_arm() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let lit = |value: &str| {
        graph.intern_node(SemanticNodeData::Literal(
            crate::semantic_query::LiteralValue::String(value.to_owned()),
        ))
    };
    let a_str = signature(
        &dispatch,
        "ordA",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        lit("a-str"),
    );
    let a_num = signature(
        &dispatch,
        "ordA",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        lit("a-num"),
    );
    let b_num = signature(
        &dispatch,
        "ordB",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        lit("b-num"),
    );
    let b_str = signature(
        &dispatch,
        "ordB",
        1,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        lit("b-str"),
    );
    let arm_a = callable(&dispatch, vec![a_str, a_num], Vec::new());
    let arm_b = callable(&dispatch, vec![b_num, b_str], Vec::new());
    let callee = union_callee(&dispatch, vec![arm_a, arm_b]);
    let result = selected(dispatch.execute_resolve_call(call_key(
        &dispatch,
        callee,
        CallKind::Call,
        None,
        vec![eager(lit("s"))],
    )));
    let expected =
        dispatch.intern_normalized_union_or_intersection(&[lit("a-str"), lit("b-str")], true);
    let ResolvedCallResult::UnionSelected { return_type, .. } = &result else {
        panic!("a union callee closes on the composite union selection, got {result:?}");
    };
    assert_eq!(
        *return_type, expected,
        "tsc --strict: ordinal 0 wins in arm A, ordinal 1 wins in arm B"
    );
}

/// Partial applicability rejects: `((x: string) => 1) | ((x: number) => 2)`
/// called with `"s"` has NO applicable signature in the number arm — the
/// whole call is `NoApplicableOverload` (tsc TS2349: the union has no
/// common applicable signature). Mutation recipe: let an applicable arm
/// stand in for the whole union and this call silently answers `1`.
#[test]
fn union_partial_applicability_is_no_applicable_overload() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let s_lit = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("s".to_owned()),
    ));
    let arm_a = signature(
        &dispatch,
        "partA",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        one,
    );
    let arm_b = signature(
        &dispatch,
        "partB",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, number, false, false)],
        Vec::new(),
        two,
    );
    let callee = union_callee(&dispatch, vec![arm_a, arm_b]);
    let key = call_key(&dispatch, callee, CallKind::Call, None, vec![eager(s_lit)]);
    assert!(
        matches!(
            dispatch.execute_resolve_call(key.clone()),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NoApplicableOverload
            )
        ),
        "an arm with no applicable signature rejects the whole union call"
    );
    assert_eq!(
        dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(key))),
        0,
        "the rejected union call admits nothing"
    );
}

/// A non-callable arm makes the whole union `NotCallable`. Mutation
/// recipe: skip non-callable arms instead of failing the settle and the
/// call silently answers the callable arm's return.
#[test]
fn union_non_callable_arm_is_not_callable() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let arm_a = signature(
        &dispatch,
        "ncA",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        one,
    );
    let callee = union_callee(&dispatch, vec![arm_a, number]);
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                callee,
                CallKind::Call,
                None,
                Vec::new(),
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NotCallable
            )
        ),
        "a non-callable arm makes the union callee not callable"
    );
}

/// UNCERTAINTY in any arm degrades the whole call: an arm whose parameter
/// type is unresolvable cannot prove or refute applicability, so no other
/// arm's success may stand in. Mutation recipe: treat the undecidable
/// arm's rejection as a mismatch and the call silently selects arm A.
#[test]
fn union_undecidable_arm_degrades_the_whole_call() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let s_lit = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("s".to_owned()),
    ));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let unresolved = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::Miss,
    ));
    let arm_a = signature(
        &dispatch,
        "undA",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, false, false)],
        Vec::new(),
        one,
    );
    let arm_b = signature(
        &dispatch,
        "undB",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, unresolved, false, false)],
        Vec::new(),
        two,
    );
    let callee = union_callee(&dispatch, vec![arm_a, arm_b]);
    assert!(
        matches!(
            dispatch.execute_resolve_call(call_key(
                &dispatch,
                callee,
                CallKind::Call,
                None,
                vec![eager(s_lit)],
            )),
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::Undecidable
            )
        ),
        "an undecidable arm degrades the whole union call"
    );
}

/// A ROOTLESS callee's overload set — and a union containing a rootless
/// arm — stays transaction-local: the query resolves, `cache_suppress`
/// propagates, and no family candidate is admitted for the overload-set
/// OR the call query. Mutation recipe: root the rootless set on the
/// demanding file (or drop the suppression) and the two zero-admission
/// assertions fail.
#[test]
fn rootless_overload_set_and_call_stay_transaction_local() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let anon = anonymous_signature(&dispatch, Vec::new(), Vec::new(), one);
    let callee = callable(&dispatch, vec![anon], Vec::new());
    let env = host.host_view_env_hashes_for(CANONICAL);
    let set_key = SemanticQueryKey::ResolveOverloadSet {
        callee,
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::OverloadSetContext {
            resolve_env_hash: env.resolve_env_hash,
        },
    };
    match dispatch.execute(set_key.clone()) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::OverloadSet(refs),
            ..
        }) => assert_eq!(
            refs.len(),
            1,
            "the rootless set still resolves in-transaction"
        ),
        other => panic!("the rootless overload set must resolve, got {other:?}"),
    }
    assert_eq!(
        graph.slot_candidate_count_for_tests(&set_key),
        0,
        "a rootless callee's overload set admits no family candidate"
    );

    // A union CONTAINING a rootless arm propagates the same suppression.
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let authored = signature(
        &dispatch,
        "unionAuthored",
        0,
        SignatureKind::Call,
        Vec::new(),
        Vec::new(),
        two,
    );
    let union = union_callee(&dispatch, vec![authored, callee]);
    let union_key = SemanticQueryKey::ResolveOverloadSet {
        callee: union,
        type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: crate::semantic_query::OverloadSetContext {
            resolve_env_hash: env.resolve_env_hash,
        },
    };
    match dispatch.execute(union_key.clone()) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::OverloadSet(refs),
            ..
        }) => assert_eq!(refs.len(), 2, "the union still resolves in-transaction"),
        other => panic!("the union overload set must resolve, got {other:?}"),
    }
    assert_eq!(
        graph.slot_candidate_count_for_tests(&union_key),
        0,
        "a union containing a rootless arm admits no family candidate"
    );

    // The CALL through the rootless union resolves and admits nothing.
    let call = call_key(&dispatch, union, CallKind::Call, None, Vec::new());
    let result = selected(dispatch.execute_resolve_call(call.clone()));
    assert!(
        matches!(result, ResolvedCallResult::UnionSelected { .. }),
        "the union call still resolves in-transaction, got {result:?}"
    );
    assert_eq!(
        graph.slot_candidate_count_for_tests(&SemanticQueryKey::ResolveCall(Box::new(call))),
        0,
        "the call through a rootless union admits no family candidate"
    );
}
