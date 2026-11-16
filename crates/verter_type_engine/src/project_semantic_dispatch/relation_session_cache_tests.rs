//! Session-visible verdicts never become answers to a pure relation key.

use super::*;

#[derive(Clone, Copy, Debug)]
pub enum SessionVerdictArm {
    NoInferTarget,
    ArrayNoInferTarget,
    RigidIndexedAccess,
    LooseNullUnion,
    WithheldCallArgument,
}

fn assert_pure_step(arm: SessionVerdictArm, step: RelationStep) {
    match (arm, step) {
        (SessionVerdictArm::LooseNullUnion, RelationStep::Assignable { bindings }) => {
            assert!(bindings.is_empty());
        }
        (
            SessionVerdictArm::NoInferTarget
            | SessionVerdictArm::ArrayNoInferTarget
            | SessionVerdictArm::RigidIndexedAccess
            | SessionVerdictArm::WithheldCallArgument,
            RelationStep::NotAssignable,
        ) => {}
        (_, step) => panic!("{arm:?} must answer the cold pure verdict: {step:?}"),
    }
}

/// Switch collectors within one transaction, retaining its settled-member
/// index. A fresh dispatch supplies the cold pure control after the warm read.
pub fn session_visible_answers_do_not_warm_pure_relations<
    C: crate::resolver_core::ResolverCapabilities,
>(
    dispatch: &ProjectSemanticDispatch<'_, C>,
    arm: SessionVerdictArm,
) {
    let (_demand, trip) = dispatch.enter_connected_demand(false);
    assert!(trip.is_none());
    let graph = dispatch.graph();
    let parameter = |name: &str| {
        graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic(name),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: name.into(),
        })
    };
    let t = parameter("T");
    let u = parameter("U");
    let literal = graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
        "bad".into(),
    )));
    let (source, target) = match arm {
        SessionVerdictArm::NoInferTarget | SessionVerdictArm::ArrayNoInferTarget => {
            let no_infer = graph.intern_node(SemanticNodeData::IntrinsicApplication {
                op: crate::semantic_query::CompilerIntrinsicTypeOp::NoInfer,
                args: [t].into(),
            });
            if matches!(arm, SessionVerdictArm::ArrayNoInferTarget) {
                let array = |element| {
                    graph.intern_node(SemanticNodeData::Array {
                        element,
                        readonly: false,
                    })
                };
                (array(literal), array(no_infer))
            } else {
                (literal, no_infer)
            }
        }
        SessionVerdictArm::RigidIndexedAccess => (
            graph.intern_node(SemanticNodeData::IndexedAccess {
                object: t,
                index: IndexKey::Computed(literal),
                origin: crate::semantic_query::IndexedAccessOrigin::Composed,
            }),
            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)),
        ),
        SessionVerdictArm::WithheldCallArgument => (literal, t),
        SessionVerdictArm::LooseNullUnion => {
            let null = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Null));
            (
                graph.intern_node(SemanticNodeData::Union(
                    crate::semantic_query::composite::CompositeList::authored_shell(Arc::from([
                        t, null,
                    ])),
                )),
                t,
            )
        }
    };
    let mut environment = dispatch.relation_environment();
    environment.strict.strict_null_checks = !matches!(arm, SessionVerdictArm::LooseNullUnion);
    let _environment = super::super::RelationEnvironmentScope::push(
        &dispatch.relation_env_scope,
        &dispatch.dispatch_txn,
        environment,
    );
    let root = dispatch.relation_frame_open(
        &dispatch.relate_key_for(u, literal),
        InferenceOccurrence::ARGUMENT_COVARIANT,
    );
    let setup = |parameter| {
        InferenceSessionSetup::new(
            &graph,
            Arc::from([InferenceInfoSetup::new(parameter, Arc::from("variable"))]),
            VariancePhase::Covariant,
            InferencePassKind::Ordinary,
            InferenceCandidatePriority::Argument,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        )
    };
    let first_parameter = if matches!(arm, SessionVerdictArm::LooseNullUnion) {
        u
    } else {
        t
    };
    let first = dispatch
        .dispatch_txn
        .borrow_mut()
        .push_collecting_session(setup(first_parameter));
    if matches!(arm, SessionVerdictArm::WithheldCallArgument) {
        dispatch.dispatch_txn.borrow_mut().begin_call_argument(
            None,
            vec![t],
            Some(literal),
            Vec::new(),
            Some(vec![t]),
        );
    }
    if matches!(arm, SessionVerdictArm::LooseNullUnion) {
        // The reverse frame keeps a collector visible for the otherwise
        // independent pair. Its variable is absent from both operands.
        let spec = ReverseHomomorphicSpec {
            mapped_node: source,
            base_infer: u,
            mapper_parameter: t,
            template: t,
            modifiers: ReverseMappedModifiers {
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
            },
        };
        assert!(dispatch
            .dispatch_txn
            .borrow_mut()
            .relation
            .session_mut(first)
            .unwrap()
            .open_reverse_frame(spec));
    }
    let initial = dispatch.relate_collecting(source, target);
    assert!(
        matches!(
            (arm, &initial),
            (
                SessionVerdictArm::NoInferTarget
                    | SessionVerdictArm::ArrayNoInferTarget
                    | SessionVerdictArm::WithheldCallArgument,
                RelationStep::Assignable { .. }
            ) | (SessionVerdictArm::RigidIndexedAccess, RelationStep::Unknown)
                | (
                    SessionVerdictArm::LooseNullUnion,
                    RelationStep::NotAssignable
                )
        ),
        "{arm:?} must exercise its session-visible arm: {initial:?}"
    );
    if matches!(arm, SessionVerdictArm::WithheldCallArgument) {
        dispatch.dispatch_txn.borrow_mut().end_call_argument();
    }
    assert_eq!(
        dispatch
            .dispatch_txn
            .borrow()
            .relation
            .accepted_inference_deposits,
        0
    );
    dispatch
        .dispatch_txn
        .borrow_mut()
        .relation
        .session_mut(first)
        .unwrap()
        .abandon();
    let second = dispatch
        .dispatch_txn
        .borrow_mut()
        .push_collecting_session(setup(u));
    let warm = dispatch.relate_collecting(source, target);
    dispatch
        .dispatch_txn
        .borrow_mut()
        .relation
        .session_mut(second)
        .unwrap()
        .abandon();
    // The warm transaction owns inline flights until its root closes. End
    // those flights before a separate dispatch asks the cold control.
    let _ = dispatch.relation_frame_close(root, RelationResult::NotAssignable, Vec::new());
    dispatch.relation_abort_completed_members();
    assert_pure_step(arm, warm);
    let cold = ProjectSemanticDispatch::new(dispatch.ctx);
    let (_cold_demand, trip) = cold.enter_connected_demand(false);
    assert!(trip.is_none());
    let _cold_environment = super::super::RelationEnvironmentScope::push(
        &cold.relation_env_scope,
        &cold.dispatch_txn,
        environment,
    );
    assert_pure_step(
        arm,
        cold.execute_relate_without_binding(cold.relate_key_for(source, target)),
    );
}
