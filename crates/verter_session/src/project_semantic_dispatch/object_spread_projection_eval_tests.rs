use std::sync::Arc;

use verter_type_expr::{ExcessPropertyOrigin, MemberSpans, MemberVisibility, ObjectMethodKind};

use super::ProjectSemanticDispatch;
use crate::semantic_query::object_spread_projection::test_support;
use crate::semantic_query::{
    AuthoredAccessorEffect, AuthoredIndexEffect, AuthoredMethodEffect, AuthoredPropertyEffect,
    AuthoredPropertyKey, ExactOptionalPropertyPolicy, ExcessEligibility, IndexDomain,
    IndexSignature, MacroOwnBodyStamp, MergeRoleStamp, ObjectConstructionEffect,
    ObjectProjectionSelector, ObjectSignatureKind, ObjectSpreadProgram, PositiveKeyPresence,
    PrimitiveKind, ProjectionEvidence, ProjectionMode, ProjectionReductionContext, PropertyKey,
    QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
    SemanticQueryOutput, SemanticQueryValue, SubstitutionCanonicalHash, SurfaceMember,
};
use crate::{HostConfig, VerterHost};

fn host() -> VerterHost {
    VerterHost::new_standalone(HostConfig::default())
}

fn property(name: &str, value: SemanticNodeId, optional: bool) -> ObjectConstructionEffect {
    ObjectConstructionEffect::DirectProperty(AuthoredPropertyEffect {
        key: AuthoredPropertyKey::string(name),
        value,
        optional,
        readonly: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::FreshOwn,
    })
}

fn method(name: &str, signature: SemanticNodeId) -> ObjectConstructionEffect {
    ObjectConstructionEffect::DirectMethod(AuthoredMethodEffect {
        key: AuthoredPropertyKey::string(name),
        signature,
        optional: false,
        has_implementation_body: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::FreshOwn,
    })
}

fn accessor(name: &str, signature: SemanticNodeId, getter: bool) -> ObjectConstructionEffect {
    let effect = AuthoredAccessorEffect {
        key: AuthoredPropertyKey::string(name),
        signature,
        optional: false,
        has_implementation_body: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::FreshOwn,
    };
    if getter {
        ObjectConstructionEffect::DirectGet(effect)
    } else {
        ObjectConstructionEffect::DirectSet(effect)
    }
}

fn surface_member(name: &str, value: SemanticNodeId, optional: bool) -> SurfaceMember {
    SurfaceMember {
        key: AuthoredPropertyKey::string(name),
        value,
        optional,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::NonLiteral,
    }
}

fn object(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    members: impl IntoIterator<Item = SurfaceMember>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from(members.into_iter().collect::<Vec<_>>()),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([]),
            keyspace: None,
            has_index_signature: false,
        },
    ))
}

fn program(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    effects: impl IntoIterator<Item = ObjectConstructionEffect>,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::ObjectSpreadProgram(ObjectSpreadProgram {
        effects: Arc::from(effects.into_iter().collect::<Vec<_>>()),
    }))
}

fn context(
    policy: ExactOptionalPropertyPolicy,
) -> crate::semantic_query::ObjectSpreadProjectionContext {
    test_support::context(
        ProjectionReductionContext::published(ProjectionMode::Shallow),
        [1; 16],
        [2; 16],
        [3; 16],
        [4; 16],
        SubstitutionCanonicalHash::distinct_for_test(1),
        policy,
    )
}

fn project(
    dispatch: &ProjectSemanticDispatch<'_>,
    program: SemanticNodeId,
    selector: ObjectProjectionSelector,
    policy: ExactOptionalPropertyPolicy,
) -> crate::semantic_query::ObjectProjectionFormula {
    match dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program,
        selector,
        context: context(policy),
    }) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::ObjectProjection(formula),
            ..
        }) => formula,
        other => panic!("expected object projection, got {other:?}"),
    }
}

fn key(name: &str) -> PropertyKey {
    PropertyKey::identifier(name)
}

#[test]
fn finite_union_spread_keeps_correlated_alternatives_through_final_overwrite() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let program = program(
        graph,
        [
            ObjectConstructionEffect::Spread(union),
            property("x", boolean, false),
        ],
    );

    let formula = project(
        &dispatch,
        program,
        ObjectProjectionSelector::RelationShape(Arc::from([key("a"), key("b"), key("x")])),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert_eq!(formula.alternatives().len(), 2);
    let complete = formula.closed().expect("finite enumerable union is closed");
    let alternatives = complete.alternatives().collect::<Vec<_>>();
    assert!(alternatives.iter().all(|alternative| matches!(
        alternative.lookup(&key("x")),
        Some(crate::semantic_query::ClosedKeyLookup::Present(fact))
            if fact.presence() == PositiveKeyPresence::Required
                && *fact.value() == ProjectionEvidence::Proven(boolean)
    )));
    assert!(matches!(
        alternatives[0].lookup(&key("a")),
        Some(crate::semantic_query::ClosedKeyLookup::Present(_))
    ));
    assert!(matches!(
        alternatives[0].lookup(&key("b")),
        Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
    ));
    assert!(matches!(
        alternatives[1].lookup(&key("a")),
        Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
    ));
    assert!(matches!(
        alternatives[1].lookup(&key("b")),
        Some(crate::semantic_query::ClosedKeyLookup::Present(_))
    ));
}

#[test]
fn optional_spread_write_fold_distinguishes_policy_and_live_open_state() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    let optional_value = graph.intern_node(SemanticNodeData::Union(Arc::from([two, undefined])));
    let optional = object(graph, [surface_member("a", optional_value, true)]);
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });

    let exact = program(
        graph,
        [
            property("a", one, false),
            ObjectConstructionEffect::Spread(optional),
        ],
    );
    let disabled = project(
        &dispatch,
        exact,
        ObjectProjectionSelector::Key(key("a")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    let enabled = project(
        &dispatch,
        exact,
        ObjectProjectionSelector::Key(key("a")),
        ExactOptionalPropertyPolicy::Enabled,
    );
    let value = |formula: &crate::semantic_query::ObjectProjectionFormula| match formula
        .alternatives()[0]
        .selected_key(&key("a"))
    {
        crate::semantic_query::OpenSafeKeyEvidence::Positive(fact) => fact.value().clone(),
        other => panic!("expected exact a, got {other:?}"),
    };
    assert_ne!(value(&disabled), value(&enabled));
    assert!(matches!(
        value(&disabled),
        ProjectionEvidence::Proven(node)
            if matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Union(arms))
                    if arms.contains(&one) && arms.contains(&two) && !arms.contains(&undefined)
            )
    ));
    assert!(matches!(
        value(&enabled),
        ProjectionEvidence::Proven(node)
            if matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Union(arms))
                    if arms.contains(&one) && arms.contains(&two) && arms.contains(&undefined)
            )
    ));

    let live_open = program(
        graph,
        [
            property("a", one, false),
            ObjectConstructionEffect::Spread(generic),
            ObjectConstructionEffect::Spread(optional),
        ],
    );
    assert!(matches!(
        project(
            &dispatch,
            live_open,
            ObjectProjectionSelector::Key(key("a")),
            ExactOptionalPropertyPolicy::Disabled,
        )
        .alternatives()[0]
            .selected_key(&key("a")),
        crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
            if fact.presence() == PositiveKeyPresence::Required
                && fact.value() == &ProjectionEvidence::Indeterminate
    ));
}

#[test]
fn selector_liveness_prunes_only_shadowed_recursive_key_effects() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    let recursive = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::RecursiveRef {
            name: Arc::from("Self"),
        },
    ));
    let late_write = program(
        graph,
        [
            ObjectConstructionEffect::Spread(recursive),
            property("x", number, false),
        ],
    );
    let exact = project(
        &dispatch,
        late_write,
        ObjectProjectionSelector::Key(key("x")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(matches!(
        exact.alternatives()[0].selected_key(&key("x")),
        crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
            if fact.value() == &ProjectionEvidence::Proven(number)
    ));

    let late_spread = program(
        graph,
        [
            property("x", number, false),
            ObjectConstructionEffect::Spread(recursive),
        ],
    );
    let unresolved = dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program: late_spread,
        selector: ObjectProjectionSelector::Key(key("x")),
        context: context(ExactOptionalPropertyPolicy::Disabled),
    });
    assert!(
        !matches!(
            unresolved,
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::ObjectProjection(ref formula),
                ..
            }) if matches!(
                formula.alternatives()[0].selected_key(&key("x")),
                crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                    if fact.value() == &ProjectionEvidence::Proven(number)
            )
        ),
        "a live later recursive spread cannot reuse stale pre-spread x"
    );
}

#[test]
fn whole_program_excess_and_direct_signature_rules_survive_open_spreads() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let call = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([]),
        return_type: number,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let callable_operand = object(graph, []);
    let callable_operand = match graph.node_data(callable_operand).as_deref() {
        Some(SemanticNodeData::Object(_)) => graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from([]),
                call_signatures: Arc::from([call]),
                construct_signatures: Arc::from([]),
                index_signatures: Arc::from([]),
                keyspace: None,
                has_index_signature: false,
            },
        )),
        _ => unreachable!(),
    };
    let program = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(generic),
            ObjectConstructionEffect::DirectCall(call),
            ObjectConstructionEffect::Spread(callable_operand),
        ],
    );
    let excess = project(
        &dispatch,
        program,
        ObjectProjectionSelector::ExcessEligibility,
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert_eq!(
        excess.alternatives()[0].excess(),
        &ExcessEligibility::SuppressedByGenericSpread
    );

    let signatures = project(
        &dispatch,
        program,
        ObjectProjectionSelector::Signature(ObjectSignatureKind::Call),
        ExactOptionalPropertyPolicy::Disabled,
    );
    signatures.alternatives()[0]
        .positive()
        .visit(|_| panic!("signatures are not named members"));
    let seen = signatures.alternatives()[0]
        .signatures()
        .iter()
        .map(|signature| signature.node())
        .collect::<Vec<_>>();
    assert_eq!(seen, vec![call], "only the direct call effect contributes");
}

#[test]
fn copied_index_is_writable_while_authored_direct_index_retains_readonly() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let indexed = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from([]),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([IndexSignature {
                key_type: string,
                value_type: number,
                readonly: true,
                spans: Default::default(),
                declaration_origin: None,
            }]),
            keyspace: None,
            has_index_signature: true,
        },
    ));
    let spread_program = program(graph, [ObjectConstructionEffect::Spread(indexed)]);
    let direct_program = program(
        graph,
        [ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
            key_type: string,
            value_type: number,
            readonly: true,
            spans: Default::default(),
            declaration_origin: None,
        })],
    );

    for (program, expected_readonly) in [(spread_program, false), (direct_program, true)] {
        let formula = project(
            &dispatch,
            program,
            ObjectProjectionSelector::IndexDomain(IndexDomain::String),
            ExactOptionalPropertyPolicy::Disabled,
        );
        let _closed = formula.closed().expect("both index programs are closed");
        // An `IndexDomain` selector's closed domain is selector-local:
        // read the (domain-complete) index evidence through the plain
        // alternative accessor, never a whole-domain witness op.
        let indices = formula.alternatives()[0].indices();
        assert_eq!(indices.len(), 1);
        assert_eq!(
            indices[0].readonly(),
            &ProjectionEvidence::Proven(expected_readonly)
        );
    }
}

#[test]
fn accessor_effects_normalize_to_writable_property_values() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let getter = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([]),
        return_type: number,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let setter = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([crate::semantic_query::FunctionParam::synthetic(
            Some(Arc::from("value")),
            number,
            false,
            false,
        )]),
        return_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)),
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let program = program(
        graph,
        [
            accessor("value", getter, true),
            accessor("value", setter, false),
            method("method", getter),
            ObjectConstructionEffect::Spread(object(graph, [])),
        ],
    );
    let formula = project(
        &dispatch,
        program,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula.closed().expect("closed accessor program");
    let members = closed
        .alternatives()
        .next()
        .expect("one alternative")
        .surface()
        .expect("a Surface-selector closed domain is whole-domain")
        .members();
    let value = members
        .iter()
        .find(|fact| fact.key() == &key("value"))
        .expect("accessor property");
    assert_eq!(value.value(), &ProjectionEvidence::Proven(number));
    assert!(matches!(
        value.facets(),
        ProjectionEvidence::Proven(facets)
            if !facets.readonly() && facets.method_kind().is_none()
    ));
    let method = members
        .iter()
        .find(|fact| fact.key() == &key("method"))
        .expect("ordinary method");
    assert!(matches!(
        method.facets(),
        ProjectionEvidence::Proven(facets)
            if facets.method_kind() == Some(ObjectMethodKind::Method)
    ));
}

#[test]
fn lowering_and_navigation_use_the_program_without_eager_surface_collapse() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let expr = verter_type_expr::TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: vec![
            verter_type_expr::ObjectMember::Spread(verter_type_expr::SpreadMember::new(
                verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Object),
            )),
            verter_type_expr::ObjectMember::Property(
                verter_type_expr::ObjectProperty::synthetic_public_key(
                    verter_type_expr::TypeAuthoredPropertyKey::string("x"),
                    verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
                    false,
                    false,
                ),
            ),
        ],
    }));
    let lowered = dispatch
        .lower_type_expr_in_scope_with_mode("/w/spread.ts", &expr, ProjectionMode::Navigate)
        .expect("spread expression lowers");
    assert!(matches!(
        graph.node_data(lowered).as_deref(),
        Some(SemanticNodeData::ObjectSpreadProgram(_))
    ));

    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: lowered,
        path: Arc::from([crate::semantic_query::PathSegment::Member(key("x"))]),
        context: ProjectionReductionContext::published(ProjectionMode::Navigate),
    });
    assert!(matches!(
        projected,
        QueryResult::Value(SemanticQueryOutput { value, .. })
            if matches!(
                graph.node_data(value).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Number))
            )
    ));
}

#[test]
fn keyof_and_substitution_consume_the_canonical_program() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let left = object(
        graph,
        [
            surface_member("a", number, false),
            surface_member("x", number, false),
        ],
    );
    let right = object(
        graph,
        [
            surface_member("b", string, false),
            surface_member("x", number, false),
        ],
    );
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let finite = program(
        graph,
        [
            ObjectConstructionEffect::Spread(union),
            property("z", string, false),
        ],
    );
    assert_eq!(
        dispatch.key_names_from_base_node(finite),
        Some(vec![key("x"), key("z")]),
        "exact keyof is minted only by the closed correlated formula witness"
    );

    let binder = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(binder),
            property("x", number, false),
        ],
    );
    assert_eq!(
        dispatch.key_names_from_base_node(open),
        None,
        "a positive post-open key cannot fabricate an exact keyof domain"
    );

    let authored = AuthoredPropertyEffect {
        key: AuthoredPropertyKey::Computed(binder),
        value: binder,
        optional: false,
        readonly: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::FreshOwn,
    };
    let index = AuthoredIndexEffect {
        key_type: binder,
        value_type: binder,
        readonly: true,
        spans: verter_type_expr::IndexSignatureSpans::default(),
        declaration_origin: None,
    };
    let substitution_subject = program(
        graph,
        [
            ObjectConstructionEffect::DirectProperty(authored),
            ObjectConstructionEffect::DirectIndex(index),
            ObjectConstructionEffect::DirectCall(binder),
            ObjectConstructionEffect::DirectConstruct(binder),
            ObjectConstructionEffect::Spread(binder),
        ],
    );
    let substituted = dispatch.substitute_semantic_type_param(substitution_subject, binder, string);
    let Some(SemanticNodeData::ObjectSpreadProgram(substituted)) =
        graph.node_data(substituted).as_deref().cloned()
    else {
        panic!("substitution must preserve the canonical program carrier");
    };
    assert!(
        substituted.child_nodes().all(|child| child == string),
        "computed keys, values, indices, signatures, and operands all substitute"
    );
}

#[test]
fn relation_quantifies_correlated_alternatives_and_never_shortcuts_open_identity() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let finite = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let correlated = program(graph, [ObjectConstructionEffect::Spread(finite)]);
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(correlated, correlated),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));

    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let unresolved = program(graph, [ObjectConstructionEffect::Spread(generic)]);
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(unresolved, unresolved),
        crate::semantic_query::RelationResult::Unknown,
        "node identity cannot publish an unresolved program relation"
    );

    let post_open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    let target = object(graph, [surface_member("x", number, false)]);
    assert!(matches!(
        dispatch.execute_relate_pair_as_result_for_tests(post_open, target),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(target, post_open),
        crate::semantic_query::RelationResult::Unknown,
        "an open target can carry additional obligations"
    );
}

#[test]
fn distribution_cap_is_a_typed_budget_partial_and_never_a_miss() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    // One arm past the alternative-product distribution cap.
    let arms = (0..1025).map(|_| object(graph, [])).collect::<Vec<_>>();
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from(arms)));
    let capped = program(graph, [ObjectConstructionEffect::Spread(union)]);
    let key = SemanticQueryKey::ProjectObjectSpread {
        program: capped,
        selector: ObjectProjectionSelector::Surface,
        context: context(ExactOptionalPropertyPolicy::Disabled),
    };
    for attempt in 0..2 {
        match dispatch.execute(key.clone()) {
            QueryResult::Error(crate::semantic_query::QueryError::BudgetExceeded(failure)) => {
                assert_eq!(
                    failure.domain,
                    crate::resolver_core::BudgetDomain::ProjectionOperation
                );
            }
            other => panic!("attempt {attempt}: expected typed budget partial, got {other:?}"),
        }
    }

    // A nested program propagates the typed partial instead of collapsing to a
    // genuine miss.
    let outer = program(
        graph,
        [
            property(
                "x",
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number)),
                false,
            ),
            ObjectConstructionEffect::Spread(capped),
        ],
    );
    match dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program: outer,
        selector: ObjectProjectionSelector::Surface,
        context: context(ExactOptionalPropertyPolicy::Disabled),
    }) {
        QueryResult::Error(crate::semantic_query::QueryError::BudgetExceeded(_)) => {}
        other => panic!("nested cap must propagate as a budget partial, got {other:?}"),
    }
}

#[test]
fn unclassifiable_spread_residual_yields_indeterminate_excess_not_generic_suppression() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let concrete = object(graph, [surface_member("known", number, false)]);
    let intersection = graph.intern_node(SemanticNodeData::Intersection(Arc::from([concrete])));

    let excess_of = |program: SemanticNodeId| {
        project(
            &dispatch,
            program,
            ObjectProjectionSelector::ExcessEligibility,
            ExactOptionalPropertyPolicy::Disabled,
        )
        .alternatives()[0]
            .excess()
            .clone()
    };

    // A semantically generic spread suppresses every direct candidate.
    let generic_program = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    assert_eq!(
        excess_of(generic_program),
        ExcessEligibility::SuppressedByGenericSpread
    );

    // An unclassifiable residual is indeterminate, not stable generic
    // suppression and not eligible silence.
    let unclassified = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(intersection),
        ],
    );
    assert_eq!(excess_of(unclassified), ExcessEligibility::Indeterminate);

    // Classification propagates through nested programs.
    let nested_generic = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(program(
                graph,
                [ObjectConstructionEffect::Spread(generic)],
            )),
        ],
    );
    assert_eq!(
        excess_of(nested_generic),
        ExcessEligibility::SuppressedByGenericSpread
    );
    let nested_unclassified = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(program(
                graph,
                [ObjectConstructionEffect::Spread(intersection)],
            )),
        ],
    );
    assert_eq!(
        excess_of(nested_unclassified),
        ExcessEligibility::Indeterminate
    );

    // A fully enumerable program keeps its direct candidates eligible.
    let closed = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(concrete),
        ],
    );
    assert_eq!(
        excess_of(closed),
        ExcessEligibility::Eligible {
            direct_candidates: Arc::from([key("extra")]),
        }
    );
}

fn index_object(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    key_type: SemanticNodeId,
    value_type: SemanticNodeId,
) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from([]),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([IndexSignature {
                key_type,
                value_type,
                readonly: false,
                spans: Default::default(),
                declaration_origin: None,
            }]),
            keyspace: None,
            has_index_signature: true,
        },
    ))
}

fn relate(
    dispatch: &ProjectSemanticDispatch<'_>,
    source: SemanticNodeId,
    target: SemanticNodeId,
) -> crate::semantic_query::RelationResult {
    dispatch.execute_relate_pair_as_result_for_tests(source, target)
}

#[test]
fn correlated_union_spread_rejects_empty_and_accepts_each_arm() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let correlated = program(graph, [ObjectConstructionEffect::Spread(union)]);

    let empty = object(graph, []);
    assert_eq!(
        relate(&dispatch, empty, correlated),
        crate::semantic_query::RelationResult::NotAssignable,
        "every target alternative demands a required key the empty object cannot prove"
    );
    assert!(matches!(
        relate(&dispatch, left, correlated),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));
    assert!(matches!(
        relate(&dispatch, right, correlated),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));

    let weak = object(
        graph,
        [
            surface_member("a", number, true),
            surface_member("b", string, true),
        ],
    );
    assert!(
        matches!(
            relate(&dispatch, correlated, weak),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "each correlated arm assigns to the all-optional target"
    );
}

#[test]
fn spread_index_signature_never_manufactures_required_named_presence() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let record = index_object(graph, string, number);
    let spread_record = program(graph, [ObjectConstructionEffect::Spread(record)]);

    let required = object(graph, [surface_member("x", number, false)]);
    assert_eq!(
        relate(&dispatch, spread_record, required),
        crate::semantic_query::RelationResult::NotAssignable,
        "an index signature constrains values; it never proves x exists"
    );
    let optional = object(graph, [surface_member("x", number, true)]);
    assert!(
        matches!(
            relate(&dispatch, spread_record, optional),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "optional absence succeeds and the index value is compatible"
    );
    let optional_bad = object(graph, [surface_member("x", string, true)]);
    assert_eq!(
        relate(&dispatch, spread_record, optional_bad),
        crate::semantic_query::RelationResult::NotAssignable,
        "a present-via-index x would carry the index value type"
    );

    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(record),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    assert_eq!(
        relate(&dispatch, open, required),
        crate::semantic_query::RelationResult::Unknown,
        "an open residual leaves required named presence undecidable"
    );
}

#[test]
fn finite_and_broad_record_targets_consume_named_presence_and_envelopes() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let broad_number = index_object(graph, string, number);

    let finite_source = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [surface_member("x", number, false)],
        ))],
    );
    let finite_target = object(graph, [surface_member("x", number, false)]);
    assert!(matches!(
        relate(&dispatch, finite_source, finite_target),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));
    let finite_bad = object(graph, [surface_member("x", string, false)]);
    assert_eq!(
        relate(&dispatch, finite_source, finite_bad),
        crate::semantic_query::RelationResult::NotAssignable
    );

    let closed_members = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [
                surface_member("a", number, false),
                surface_member("b", number, false),
            ],
        ))],
    );
    assert!(
        matches!(
            relate(&dispatch, closed_members, broad_number),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "every known contribution satisfies the broad value obligation"
    );
    let known_bad = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [
                surface_member("a", number, false),
                surface_member("bad", string, false),
            ],
        ))],
    );
    assert_eq!(
        relate(&dispatch, known_bad, broad_number),
        crate::semantic_query::RelationResult::NotAssignable,
        "an exact known bad contribution rejects"
    );

    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_residual = program(
        graph,
        [
            ObjectConstructionEffect::Spread(object(graph, [surface_member("a", number, false)])),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    assert_eq!(
        relate(&dispatch, open_residual, broad_number),
        crate::semantic_query::RelationResult::Unknown,
        "a live residual without an exact envelope cannot close a broad obligation"
    );

    let indexed_source = program(graph, [ObjectConstructionEffect::Spread(broad_number)]);
    assert!(matches!(
        relate(&dispatch, indexed_source, broad_number),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));
    let bad_index = index_object(graph, string, string);
    let bad_indexed_source = program(graph, [ObjectConstructionEffect::Spread(bad_index)]);
    assert_eq!(
        relate(&dispatch, bad_indexed_source, broad_number),
        crate::semantic_query::RelationResult::NotAssignable
    );
}

#[test]
fn open_targets_reject_on_exact_mismatch_and_stay_unknown_on_known_subset() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let post_open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    let mismatch = object(graph, [surface_member("x", string, false)]);
    assert_eq!(
        relate(&dispatch, mismatch, post_open),
        crate::semantic_query::RelationResult::NotAssignable,
        "an exact known mismatch rejects before any closure proof"
    );
    let pre_open = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    let wants_a = object(graph, [surface_member("a", number, false)]);
    assert_eq!(
        relate(&dispatch, pre_open, wants_a),
        crate::semantic_query::RelationResult::Unknown,
        "a pre-open key keeps an indeterminate value behind a live spread"
    );
    let wants_wrong_x = object(graph, [surface_member("x", string, false)]);
    let exact_post_open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    assert_eq!(
        relate(&dispatch, exact_post_open, wants_wrong_x),
        crate::semantic_query::RelationResult::NotAssignable,
        "an exact post-open key rejects an incompatible binary target"
    );
}

#[test]
fn optional_spread_write_fold_matrix_under_fixed_policy() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let optional_string = object(graph, [surface_member("a", string, true)]);

    let key_a = |program: SemanticNodeId| {
        let formula = project(
            &dispatch,
            program,
            ObjectProjectionSelector::Key(key("a")),
            ExactOptionalPropertyPolicy::Disabled,
        );
        match formula.alternatives()[0].selected_key(&key("a")) {
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact) => {
                (fact.presence(), fact.value().clone())
            }
            other => panic!("expected positive a, got {other:?}"),
        }
    };
    let union_of = |value: ProjectionEvidence<SemanticNodeId>, arms: &[SemanticNodeId]| {
        let ProjectionEvidence::Proven(node) = value else {
            panic!("expected proven union, got {value:?}");
        };
        match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Union(found)) => {
                assert!(
                    arms.iter().all(|arm| found.contains(arm)),
                    "union {found:?} must contain {arms:?}"
                );
            }
            other => panic!("expected union, got {other:?}"),
        }
    };

    // Absent left + optional write -> optional right-present value.
    let (presence, value) = key_a(program(
        graph,
        [ObjectConstructionEffect::Spread(optional_string)],
    ));
    assert_eq!(presence, PositiveKeyPresence::Optional);
    assert!(matches!(value, ProjectionEvidence::Proven(node) if node == string));

    // Required exact left + optional write -> required union of both presents.
    let (presence, value) = key_a(program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(optional_string),
        ],
    ));
    assert_eq!(presence, PositiveKeyPresence::Required);
    union_of(value, &[number, string]);

    // Optional exact left + optional write -> optional union of both presents.
    let (presence, value) = key_a(program(
        graph,
        [
            property("a", number, true),
            ObjectConstructionEffect::Spread(optional_string),
        ],
    ));
    assert_eq!(presence, PositiveKeyPresence::Optional);
    union_of(value, &[number, string]);

    // A later required write restores exact presence and value.
    let (presence, value) = key_a(program(
        graph,
        [
            ObjectConstructionEffect::Spread(optional_string),
            property("a", boolean, false),
        ],
    ));
    assert_eq!(presence, PositiveKeyPresence::Required);
    assert!(matches!(value, ProjectionEvidence::Proven(node) if node == boolean));
}

#[test]
fn unique_symbol_keys_survive_spreads_without_aliasing_same_spelling() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let identity = |canonical: &str| verter_type_expr::facts::ValueDeclIdentityPart {
        canonical_id: Arc::from(canonical),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        symbol: Arc::from("k"),
        member_path: Arc::from([]),
    };
    let first = PropertyKey::UniqueSymbol(identity("/w/first.ts"));
    let twin = PropertyKey::UniqueSymbol(identity("/w/twin.ts"));
    let string_k = PropertyKey::identifier("k");
    assert_ne!(first, twin, "nominal identity, never display spelling");
    assert_ne!(first, string_k);

    let symbol_object = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from([SurfaceMember {
                key: AuthoredPropertyKey::from_known(first.clone()),
                value: number,
                optional: false,
                readonly: false,
                method_kind: None,
                has_implementation_body: false,
                visibility: MemberVisibility::Public,
                spans: MemberSpans::default(),
                declaration_origin: None,
                declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                merge_role: MergeRoleStamp::NEUTRAL,
                excess_origin: ExcessPropertyOrigin::NonLiteral,
            }]),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([]),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    let spread = program(
        graph,
        [
            ObjectConstructionEffect::Spread(symbol_object),
            ObjectConstructionEffect::Spread(object(graph, [])),
        ],
    );
    let formula = project(
        &dispatch,
        spread,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula.closed().expect("closed symbol spread");
    let alternative = closed.alternatives().next().expect("one alternative");
    match alternative.lookup(&first) {
        Some(crate::semantic_query::ClosedKeyLookup::Present(fact)) => {
            assert_eq!(fact.value(), &ProjectionEvidence::Proven(number));
        }
        other => panic!("the nominal symbol key must survive the spread: {other:?}"),
    }
    assert!(
        matches!(
            alternative.lookup(&twin),
            Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
        ),
        "a same-spelling symbol from another declaration must not alias"
    );
    assert!(
        matches!(
            alternative.lookup(&string_k),
            Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
        ),
        "a string key must not alias a unique symbol"
    );
}

#[test]
fn accessor_checker_parity_getter_setter_paired_duplicate_around_spreads() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let getter = |value: SemanticNodeId| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from([]),
            return_type: value,
            type_parameters: Arc::from([]),
            signature_span: None,
            return_type_span: None,
        })
    };
    let setter = |value: SemanticNodeId| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from([crate::semantic_query::FunctionParam::synthetic(
                Some(Arc::from("value")),
                value,
                false,
                false,
            )]),
            return_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)),
            type_parameters: Arc::from([]),
            signature_span: None,
            return_type_span: None,
        })
    };
    let member_x = |program: SemanticNodeId| {
        let formula = project(
            &dispatch,
            program,
            ObjectProjectionSelector::Surface,
            ExactOptionalPropertyPolicy::Disabled,
        );
        let closed = formula.closed().expect("closed accessor program");
        let members = closed
            .alternatives()
            .next()
            .expect("one alternative")
            .surface()
            .expect("a Surface-selector closed domain is whole-domain")
            .members();
        let fact = members
            .iter()
            .find(|fact| fact.key() == &key("x"))
            .expect("x member")
            .clone();
        (fact.value().clone(), fact.facets().clone())
    };
    let expect_plain_writable =
        |facets: &ProjectionEvidence<crate::semantic_query::MemberFacets>| {
            assert!(
                matches!(
                    facets,
                    ProjectionEvidence::Proven(facets)
                        if !facets.readonly() && facets.method_kind().is_none()
                ),
                "accessors normalize to plain writable properties: {facets:?}"
            );
        };

    // Getter-only: the bounded checker example is the return value, writable.
    let (value, facets) = member_x(program(graph, [accessor("x", getter(number), true)]));
    assert_eq!(value, ProjectionEvidence::Proven(number));
    expect_plain_writable(&facets);

    // Setter-only: the setter parameter type, never a function-valued method.
    let (value, facets) = member_x(program(graph, [accessor("x", setter(string), false)]));
    assert_eq!(value, ProjectionEvidence::Proven(string));
    expect_plain_writable(&facets);

    // Paired get/set: the read value is the getter return.
    let (value, facets) = member_x(program(
        graph,
        [
            accessor("x", getter(number), true),
            accessor("x", setter(string), false),
        ],
    ));
    assert_eq!(value, ProjectionEvidence::Proven(number));
    expect_plain_writable(&facets);

    // Duplicate accessors: the later effect wins in source order.
    let (value, _) = member_x(program(
        graph,
        [
            accessor("x", getter(number), true),
            accessor("x", getter(string), true),
        ],
    ));
    assert_eq!(value, ProjectionEvidence::Proven(string));

    // A required spread write after an accessor replaces it.
    let spread_x = object(graph, [surface_member("x", string, false)]);
    let (value, _) = member_x(program(
        graph,
        [
            accessor("x", getter(number), true),
            ObjectConstructionEffect::Spread(spread_x),
        ],
    ));
    assert_eq!(value, ProjectionEvidence::Proven(string));

    // An accessor after a spread replaces the copied key.
    let (value, _) = member_x(program(
        graph,
        [
            ObjectConstructionEffect::Spread(spread_x),
            accessor("x", getter(number), true),
        ],
    ));
    assert_eq!(value, ProjectionEvidence::Proven(number));

    // A spread-copied accessor evaluates to a plain data property.
    let accessor_object = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from([SurfaceMember {
                key: AuthoredPropertyKey::string("x"),
                value: number,
                optional: false,
                readonly: true,
                method_kind: Some(ObjectMethodKind::Get),
                has_implementation_body: false,
                visibility: MemberVisibility::Public,
                spans: MemberSpans::default(),
                declaration_origin: None,
                declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
                merge_role: MergeRoleStamp::NEUTRAL,
                excess_origin: ExcessPropertyOrigin::NonLiteral,
            }]),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([]),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    let (value, facets) = member_x(program(
        graph,
        [ObjectConstructionEffect::Spread(accessor_object)],
    ));
    assert_eq!(value, ProjectionEvidence::Proven(number));
    expect_plain_writable(&facets);
}

fn relate_fresh_excess(
    dispatch: &ProjectSemanticDispatch<'_>,
    source: SemanticNodeId,
    target: SemanticNodeId,
) -> crate::project_semantic_dispatch::dispatch_txn::RelationStep {
    let mut key = dispatch.relate_key_for(source, target);
    key.source_freshness = crate::semantic_query::FreshnessKey::Fresh;
    key.policy.excess_property_check = true;
    dispatch.execute_relate(key)
}

#[test]
fn generic_spread_excess_suppression_matrix_with_closed_contrast_and_healing() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let empty_target = object(graph, []);
    let knows_a_target = object(graph, [surface_member("a", number, false)]);
    let closed_spread = object(graph, [surface_member("a", number, false)]);

    // A generic spread suppresses direct candidates whether they are authored
    // before or after it. Against a name-knowing target a broken suppression
    // would reject the fresh candidate; with suppression the prepass passes
    // and the open residual leaves the relation Unknown.
    for (label, effects) in [
        (
            "before",
            vec![
                property("extra", number, false),
                ObjectConstructionEffect::Spread(generic),
            ],
        ),
        (
            "after",
            vec![
                ObjectConstructionEffect::Spread(generic),
                property("extra", number, false),
            ],
        ),
    ] {
        let source = program(graph, effects);
        let step = relate_fresh_excess(&dispatch, source, knows_a_target);
        assert!(
            matches!(
                step,
                crate::project_semantic_dispatch::dispatch_txn::RelationStep::Unknown
            ),
            "{label}: generic suppression must not reject, got {step:?}"
        );
    }

    // Closed spreads do not suppress: the same direct property is excess.
    // (The target must know a name — an empty-object-like target skips
    // excess checking entirely.)
    for (label, effects) in [
        (
            "empty spread",
            vec![
                property("extra", number, false),
                ObjectConstructionEffect::Spread(object(graph, [])),
            ],
        ),
        (
            "closed spread",
            vec![
                ObjectConstructionEffect::Spread(closed_spread),
                property("extra", number, false),
            ],
        ),
    ] {
        let source = program(graph, effects);
        let step = relate_fresh_excess(&dispatch, source, knows_a_target);
        assert!(
            matches!(
                step,
                crate::project_semantic_dispatch::dispatch_txn::RelationStep::NotAssignable
            ),
            "{label}: a closed spread keeps the direct candidate eligible, got {step:?}"
        );
    }

    // Spread-provided keys are never direct candidates.
    let spread_only = program(graph, [ObjectConstructionEffect::Spread(closed_spread)]);
    let step = relate_fresh_excess(&dispatch, spread_only, empty_target);
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::Assignable { .. }
        ),
        "spread-provided keys are spread-tainted, not fresh candidates: {step:?}"
    );

    // An unclassifiable residual falls back safely: no rejection, no acceptance.
    let concrete = object(graph, [surface_member("known", number, false)]);
    let intersection = graph.intern_node(SemanticNodeData::Intersection(Arc::from([concrete])));
    let unclassifiable = program(
        graph,
        [
            property("extra", number, false),
            ObjectConstructionEffect::Spread(intersection),
        ],
    );
    let step = relate_fresh_excess(&dispatch, unclassifiable, empty_target);
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::Unknown
        ),
        "indeterminate eligibility falls back to Unknown, got {step:?}"
    );

    // Generic-to-concrete substitution re-evaluates eligibility: once the
    // operand is concrete the direct candidate rejects again.
    let generic_source = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("extra", number, false),
        ],
    );
    let healed = dispatch.substitute_semantic_type_param(generic_source, generic, closed_spread);
    let step = relate_fresh_excess(&dispatch, healed, knows_a_target);
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::NotAssignable
        ),
        "substitution healing restores direct-candidate rejection, got {step:?}"
    );
}

#[test]
fn identical_unresolved_program_relation_is_unknown_and_never_published() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let unresolved = program(graph, [ObjectConstructionEffect::Spread(generic)]);
    let key = dispatch.relate_key_for(unresolved, unresolved);
    let step = dispatch.execute_relate(key.clone());
    assert!(
        matches!(
            step,
            crate::project_semantic_dispatch::dispatch_txn::RelationStep::Unknown
        ),
        "identical unresolved programs stay Unknown, got {step:?}"
    );
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "an unresolved Unknown must never publish to the relation memo"
    );
}

#[test]
fn cap_and_cycle_partials_are_never_admitted_and_later_queries_heal() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // The alternative-product cap is a ReturnOnly operational partial.
    let arms = (0..1025).map(|_| object(graph, [])).collect::<Vec<_>>();
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from(arms)));
    let capped = program(graph, [ObjectConstructionEffect::Spread(union)]);
    let before = graph.memo_entry_count();
    let capped_result = dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program: capped,
        selector: ObjectProjectionSelector::Surface,
        context: context(ExactOptionalPropertyPolicy::Disabled),
    });
    assert!(
        matches!(
            capped_result,
            QueryResult::Error(crate::semantic_query::QueryError::BudgetExceeded(_))
        ),
        "cap must trip a typed budget partial, got {capped_result:?}"
    );
    assert_eq!(
        graph.memo_entry_count(),
        before,
        "the cap partial is admitted nowhere"
    );

    // A live recursive operand is likewise ReturnOnly.
    let recursive = graph.intern_node(SemanticNodeData::Opaque(
        crate::semantic_query::QueryError::RecursiveRef {
            name: Arc::from("Self"),
        },
    ));
    let cyclic = program(
        graph,
        [
            property("x", number, false),
            ObjectConstructionEffect::Spread(recursive),
        ],
    );
    let cyclic_result = dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program: cyclic,
        selector: ObjectProjectionSelector::Surface,
        context: context(ExactOptionalPropertyPolicy::Disabled),
    });
    assert!(
        !matches!(
            cyclic_result,
            QueryResult::Value(SemanticQueryOutput { .. })
        ),
        "a live recursive spread cannot produce a complete surface: {cyclic_result:?}"
    );
    assert_eq!(
        graph.memo_entry_count(),
        before,
        "the cycle partial is admitted nowhere"
    );

    // An adequate later run heals and publishes.
    let adequate = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [surface_member("a", number, false)],
        ))],
    );
    let healed = dispatch.execute(SemanticQueryKey::ProjectObjectSpread {
        program: adequate,
        selector: ObjectProjectionSelector::Surface,
        context: context(ExactOptionalPropertyPolicy::Disabled),
    });
    assert!(
        matches!(healed, QueryResult::Value(SemanticQueryOutput { .. })),
        "an adequate query after the partials succeeds: {healed:?}"
    );
    assert!(
        graph.memo_entry_count() > before,
        "the healed run publishes its value"
    );
}

#[test]
fn inference_deposits_only_from_exact_whole_branch_positions() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let lit_s = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::String("s".to_string()),
    ));
    let never = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let infer_u = graph.intern_node(SemanticNodeData::Infer {
        name: Arc::from("U"),
        binder: graph.alloc_infer_binder_id(),
    });
    let extends = object(graph, [surface_member("x", infer_u, false)]);
    let infer = |check: SemanticNodeId| {
        dispatch.execute_type_node(SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch: infer_u,
            false_branch: never,
            distributive: false,
        })
    };
    let expect_value = |result: QueryResult<SemanticQueryOutput<SemanticNodeId>>| match result {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("expected a conditional value, got {other:?}"),
    };

    // An exact post-open required key deposits.
    let post_open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    assert_eq!(
        expect_value(infer(post_open)),
        number,
        "the exact post-open x deposits number"
    );

    // A pre-open key behind a live spread is indeterminate: no deposit, no
    // acceptance.
    let pre_open = program(
        graph,
        [
            property("x", number, false),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    let pre_open_result = expect_value(infer(pre_open));
    assert!(
        matches!(
            graph.node_data(pre_open_result).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "an indeterminate position deposits nothing and the conditional          stays undecided, got {pre_open_result:?}"
    );

    // Correlated branches deposit per whole branch and aggregate under the
    // union rule.
    let left = object(graph, [surface_member("x", one, false)]);
    let right = object(graph, [surface_member("x", lit_s, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let correlated = program(graph, [ObjectConstructionEffect::Spread(union)]);
    let aggregated = expect_value(infer(correlated));
    match graph.node_data(aggregated).as_deref() {
        Some(SemanticNodeData::Union(arms)) => {
            assert!(
                arms.contains(&one) && arms.contains(&lit_s),
                "whole-branch deposits aggregate to 1 | \"s\", got {arms:?}"
            );
        }
        other => panic!("expected the aggregated union, got {other:?}"),
    }

    // A live residual taints every branch position: nothing is exact, so
    // nothing deposits.
    let tainted = program(
        graph,
        [
            ObjectConstructionEffect::Spread(union),
            ObjectConstructionEffect::Spread(generic),
        ],
    );
    let tainted_result = expect_value(infer(tainted));
    assert!(
        matches!(
            graph.node_data(tainted_result).as_deref(),
            Some(SemanticNodeData::Conditional { .. })
        ),
        "an open residual forbids deposits from every branch, got {tainted_result:?}"
    );
}

#[test]
fn joined_shallow_surface_reports_incomplete_unless_single_closed_witness() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let shallow = |node: SemanticNodeId| {
        host.project_shallow_surface_graph_only(
            &host,
            &dispatch,
            node,
            Arc::from([]),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        )
    };
    fn member_named<'a>(
        surface: &'a crate::typeinfo::surface::TypeInfoSurface,
        name: &str,
    ) -> &'a crate::typeinfo::surface::TypeInfoSurfaceMember {
        surface
            .members
            .iter()
            .find(|member| member.key.as_string() == Some(name))
            .unwrap_or_else(|| panic!("member {name} on the joined surface"))
    }

    // An open program joins its positive members into an explicitly
    // incomplete surface.
    let open = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    let surface = shallow(open).expect("an open program joins a shallow surface");
    assert!(
        !surface.members_complete,
        "a joined open surface never claims completeness"
    );
    let x = member_named(&surface, "x");
    assert!(!x.optional);
    assert_eq!(x.value, number);

    // A single closed alternative yields the exact complete surface.
    let closed = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [surface_member("a", number, false)],
        ))],
    );
    let surface = shallow(closed).expect("a closed program yields a surface");
    assert!(
        surface.members_complete,
        "the single closed witness sets members_complete"
    );
    assert_eq!(surface.members.len(), 1);
    assert!(!member_named(&surface, "a").optional);

    // A correlated closed union joins branch-local keys as optional and
    // stays explicitly incomplete.
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let correlated = program(graph, [ObjectConstructionEffect::Spread(union)]);
    let surface = shallow(correlated).expect("a correlated program joins");
    assert!(
        !surface.members_complete,
        "joining correlated branches forfeits exact-domain claims"
    );
    assert!(member_named(&surface, "a").optional);
    assert!(member_named(&surface, "b").optional);
}

#[test]
fn distributed_and_aliased_program_operands_reach_the_program_relation() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let base = object(graph, [surface_member("a", number, false)]);
    let closed_program = program(graph, [ObjectConstructionEffect::Spread(base)]);
    let other = object(graph, [surface_member("b", number, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([closed_program, other])));
    let target = object(
        graph,
        [
            surface_member("a", number, true),
            surface_member("b", number, true),
        ],
    );

    // Union distribution pushes bare program pairs into the worklist.
    assert!(
        matches!(
            relate(&dispatch, union, target),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a program arm distributed from a union must relate through the \
         program protocol, not the fallthrough NotAssignable"
    );

    // A transparent alias to a program unwraps into the same protocol.
    let alias = graph.intern_node(SemanticNodeData::Alias(closed_program));
    assert!(
        matches!(
            relate(&dispatch, alias, target),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "an alias-wrapped program must reach the program relation"
    );

    // An open program reached through the worklist stays Unknown, never a
    // fallthrough NotAssignable that would poison an enclosing union.
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open = program(graph, [ObjectConstructionEffect::Spread(generic)]);
    let open_union = graph.intern_node(SemanticNodeData::Union(Arc::from([open, other])));
    assert_eq!(
        relate(&dispatch, open_union, target),
        crate::semantic_query::RelationResult::Unknown,
        "an open program arm is undecidable, not a definitive rejection"
    );

    // The identity shortcut must not accept an open program pair: the same
    // open program reached through an alias still refuses publication.
    let open_alias = graph.intern_node(SemanticNodeData::Alias(open));
    let pair_union = graph.intern_node(SemanticNodeData::Union(Arc::from([open_alias])));
    assert_eq!(
        relate(&dispatch, pair_union, open),
        crate::semantic_query::RelationResult::Unknown,
        "an identical open program pair stays Unknown through distribution"
    );
}

#[test]
fn program_relation_unwraps_transparent_carriers_before_projecting() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let base = object(graph, [surface_member("a", number, false)]);
    let closed_program = program(graph, [ObjectConstructionEffect::Spread(base)]);
    let target = object(graph, [surface_member("a", number, false)]);
    let target_alias = graph.intern_node(SemanticNodeData::Alias(target));
    let target_alias_chain = graph.intern_node(SemanticNodeData::Alias(target_alias));

    assert!(
        matches!(
            relate(&dispatch, closed_program, target_alias),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a closed program relates to an aliased Object target structurally"
    );
    assert!(
        matches!(
            relate(&dispatch, closed_program, target_alias_chain),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "alias chains normalize before the program projection"
    );

    let wrong = object(graph, [surface_member("a", string, false)]);
    let wrong_alias = graph.intern_node(SemanticNodeData::Alias(wrong));
    assert_eq!(
        relate(&dispatch, closed_program, wrong_alias),
        crate::semantic_query::RelationResult::NotAssignable,
        "carrier normalization keeps exact mismatch rejections"
    );

    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open = program(graph, [ObjectConstructionEffect::Spread(generic)]);
    assert_eq!(
        relate(&dispatch, open, target_alias),
        crate::semantic_query::RelationResult::Unknown,
        "an open program against an aliased target stays Unknown"
    );
}

#[test]
fn nested_open_program_keeps_post_open_exact_keys_proven() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let inner = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    let outer = program(graph, [ObjectConstructionEffect::Spread(inner)]);

    let formula = project(
        &dispatch,
        outer,
        ObjectProjectionSelector::Key(key("x")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(
        matches!(
            formula.alternatives()[0].selected_key(&key("x")),
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                if fact.presence() == PositiveKeyPresence::Required
                    && fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "a post-open exact key inside a nested open program keeps its proven value"
    );
    assert!(
        formula.closed().is_none(),
        "the outer program still carries the nested open residual"
    );
}

#[test]
fn projected_optional_to_required_honors_the_strict_family_config() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let base = object(graph, [surface_member("a", number, true)]);
    let optional_source = program(graph, [ObjectConstructionEffect::Spread(base)]);
    let required_target = object(graph, [surface_member("a", number, false)]);

    let strict_dispatch = ProjectSemanticDispatch::new(&host);
    assert_eq!(
        relate(&strict_dispatch, optional_source, required_target),
        crate::semantic_query::RelationResult::NotAssignable,
        "strictNullChecks on: an optional source key cannot fill a required slot"
    );

    host.relation_knobs
        .strict_family_relax_bits
        .store(0b01, std::sync::atomic::Ordering::Relaxed);
    let relaxed_dispatch = ProjectSemanticDispatch::new(&host);
    assert!(
        matches!(
            relate(&relaxed_dispatch, optional_source, required_target),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "strictNullChecks off: the pair relates on the value types alone"
    );
}

#[test]
fn projected_relation_matches_numeric_and_canonical_string_keys() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let numeric_key_member = |value: SemanticNodeId| SurfaceMember {
        key: AuthoredPropertyKey::Number(
            crate::semantic_query::CanonicalIndexInt::from_canonical_i64(0).unwrap(),
        ),
        value,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::NonLiteral,
    };
    let source_obj = object(graph, [numeric_key_member(number)]);
    let source = program(graph, [ObjectConstructionEffect::Spread(source_obj)]);
    let target = object(graph, [surface_member("0", number, false)]);
    assert!(
        matches!(
            relate(&dispatch, source, target),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a numeric source key collides with the canonical string target key"
    );

    let string_source_obj = object(graph, [surface_member("0", number, false)]);
    let string_source = program(graph, [ObjectConstructionEffect::Spread(string_source_obj)]);
    let numeric_target = graph.intern_node(SemanticNodeData::Object(
        crate::semantic_query::surface_view! {
            members: Arc::from([numeric_key_member(number)]),
            call_signatures: Arc::from([]),
            construct_signatures: Arc::from([]),
            index_signatures: Arc::from([]),
            keyspace: None,
            has_index_signature: false,
        },
    ));
    assert!(
        matches!(
            relate(&dispatch, string_source, numeric_target),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a canonical string source key collides with the numeric target key"
    );
}

#[test]
fn path_projection_joins_closed_absent_alternatives_as_optional() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let correlated = program(graph, [ObjectConstructionEffect::Spread(union)]);

    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base: correlated,
        path: Arc::from([crate::semantic_query::PathSegment::Member(key("a"))]),
        context: ProjectionReductionContext::published(ProjectionMode::Expanded),
    });
    let QueryResult::Value(SemanticQueryOutput { value, .. }) = projected else {
        panic!("expected a projected value, got {projected:?}")
    };
    let value_data = graph.node_data(value).expect("projected value node");
    let SemanticNodeData::Union(arms) = &*value_data else {
        panic!(
            "a key present in only one alternative must project as a union, got {:?}",
            *value_data
        )
    };
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    assert!(
        arms.contains(&number) && arms.contains(&undefined),
        "the closed-absent alternative contributes undefined: {arms:?}"
    );

    // A key present in every alternative stays exact.
    let both = project(
        &dispatch,
        correlated,
        ObjectProjectionSelector::Key(key("a")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(matches!(
        both.alternatives()[0].selected_key(&key("a")),
        crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
            if fact.value() == &ProjectionEvidence::Proven(number)
    ));
}

#[test]
fn program_relation_applies_top_bottom_rules_before_projecting() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
    let never = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
    let object_prim = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Object));
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open = program(graph, [ObjectConstructionEffect::Spread(generic)]);
    let closed = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [surface_member("a", number, false)],
        ))],
    );

    for (source, target, label) in [
        (open, any, "open <= any"),
        (any, open, "any <= open"),
        (open, unknown, "open <= unknown"),
        (open, object_prim, "open <= object"),
        (closed, any, "closed <= any"),
        (closed, object_prim, "closed <= object"),
        (never, open, "never <= open"),
    ] {
        assert!(
            matches!(
                relate(&dispatch, source, target),
                crate::semantic_query::RelationResult::Assignable { .. }
            ),
            "{label}: top/bottom rules apply to programs"
        );
    }
    assert_eq!(
        relate(&dispatch, open, never),
        crate::semantic_query::RelationResult::NotAssignable,
        "an open program still rejects never"
    );
}

#[test]
fn carrier_wrapped_identical_open_program_stays_unpublished_unknown() {
    let host = host();
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: None,
            input_id: "/w/wrap.ts".to_string(),
            source: Arc::from(
                "export class WrapHolder {\n\
                   static wrap<T>(t: T) { return { ...t }; }\n\
                 }\n\
                 export type W = ReturnType<typeof WrapHolder.wrap>;\n\
                 export type C = { a: number };\n",
            ),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("wrap file upserts");
    host.ensure_indexed_ready("/w/wrap.ts")
        .expect("wrap file indexes");
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();

    let reference = verter_type_expr::TypeExpr::Ref {
        name: Arc::from("W"),
        type_arguments: Arc::from(Vec::<verter_type_expr::TypeExpr>::new()),
    };
    let carrier = dispatch
        .lower_type_expr_in_scope_with_mode("/w/wrap.ts", &reference, ProjectionMode::Navigate)
        .expect("W reference lowers to a carrier");
    // The identical carrier node must not accept on identity: unwrapping it
    // yields an open program (the operand stays a free type parameter), and
    // node identity is not a completeness proof.
    assert_eq!(
        dispatch.execute_relate_pair_as_result_for_tests(carrier, carrier),
        crate::semantic_query::RelationResult::Unknown,
        "an identical carrier-wrapped open program stays Unknown"
    );
    let key = dispatch.relate_key_for(carrier, carrier);
    let step = dispatch.execute_relate(key.clone());
    assert!(
        graph.get_relation_payload(&host, &key).is_none(),
        "the unresolved Unknown never publishes: {step:?}"
    );

    // The benign closed case still decides on identity.
    let closed_reference = verter_type_expr::TypeExpr::Ref {
        name: Arc::from("C"),
        type_arguments: Arc::from(Vec::<verter_type_expr::TypeExpr>::new()),
    };
    let closed_carrier = dispatch
        .lower_type_expr_in_scope_with_mode(
            "/w/wrap.ts",
            &closed_reference,
            ProjectionMode::Navigate,
        )
        .expect("C reference lowers");
    assert!(
        matches!(
            dispatch.execute_relate_pair_as_result_for_tests(closed_carrier, closed_carrier),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "an identical closed carrier still accepts on identity"
    );
}

#[test]
fn program_index_obligations_cover_cross_domain_contributions() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let spread = |node| program(graph, [ObjectConstructionEffect::Spread(node)]);

    let number_index_string_value = spread(index_object(graph, number, string));
    let string_index_number_value = index_object(graph, string, number);
    assert_eq!(
        relate(
            &dispatch,
            number_index_string_value,
            string_index_number_value
        ),
        crate::semantic_query::RelationResult::NotAssignable,
        "a number-index source with a mismatched value rejects the string-index target"
    );

    let string_index_string_value = spread(index_object(graph, string, string));
    let number_index_number_value = index_object(graph, number, number);
    assert_eq!(
        relate(
            &dispatch,
            string_index_string_value,
            number_index_number_value
        ),
        crate::semantic_query::RelationResult::NotAssignable,
        "a string index covers the number domain, so the mismatched payload rejects on VALUES"
    );

    // Same-value cross-domain: tsc and the legacy
    // `relate_target_index_signature` path both accept
    // `{[k: string]: X} <= {[k: number]: X}` (numeric keys are strings at
    // runtime) — the projected path must not reject on domain non-overlap
    // alone.
    let string_index_number_payload = spread(index_object(graph, string, number));
    assert!(
        matches!(
            relate(
                &dispatch,
                string_index_number_payload,
                index_object(graph, number, number)
            ),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a string index covers the number domain for value relating — same-value accepts"
    );

    let numeric_named = spread(object(graph, [surface_member("42", string, false)]));
    assert_eq!(
        relate(&dispatch, numeric_named, number_index_number_value),
        crate::semantic_query::RelationResult::NotAssignable,
        "a numeric-string named member is inside the number index domain"
    );

    let optional_42 = object(graph, [surface_member("42", number, true)]);
    let number_index_string_value2 = spread(index_object(graph, number, string));
    assert_eq!(
        relate(&dispatch, number_index_string_value2, optional_42),
        crate::semantic_query::RelationResult::NotAssignable,
        "an optional numeric target relates the covering index value and rejects"
    );

    let number_index_number_source = spread(index_object(graph, number, number));
    assert!(
        matches!(
            relate(
                &dispatch,
                number_index_number_source,
                string_index_number_value
            ),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a number index with a matching value satisfies the string-index target"
    );
    let string_index_number_source = spread(index_object(graph, string, number));
    let string_index_number_target = index_object(graph, string, number);
    assert!(
        matches!(
            relate(
                &dispatch,
                string_index_number_source,
                string_index_number_target
            ),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "a matching string index satisfies the string-index target"
    );
    let empty_source = spread(object(graph, []));
    assert!(
        matches!(
            relate(&dispatch, empty_source, string_index_number_value),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "no index and no members contributes nothing to reject"
    );
}

#[test]
fn spread_operand_carriers_fold_to_their_concrete_surface() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));

    // A transparent alias to a closed object folds like the object itself.
    let concrete = object(graph, [surface_member("a", number, false)]);
    let alias = graph.intern_node(SemanticNodeData::Alias(concrete));
    let aliased = program(graph, [ObjectConstructionEffect::Spread(alias)]);
    let formula = project(
        &dispatch,
        aliased,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula
        .closed()
        .expect("an aliased closed object folds closed");
    assert!(matches!(
        closed.alternatives().next().expect("one alternative").lookup(&key("a")),
        Some(crate::semantic_query::ClosedKeyLookup::Present(fact))
            if fact.value() == &ProjectionEvidence::Proven(number)
    ));

    // A DeclRef to a real interface folds to its declared surface.
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: None,
            input_id: "/w/iface.ts".to_string(),
            source: Arc::from(
                "export interface Foo { b: number }\nexport type C = { a: number };\n",
            ),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("iface file upserts");
    host.ensure_indexed_ready("/w/iface.ts")
        .expect("iface file indexes");
    let reference = verter_type_expr::TypeExpr::Ref {
        name: Arc::from("Foo"),
        type_arguments: Arc::from(Vec::<verter_type_expr::TypeExpr>::new()),
    };
    let decl_ref = dispatch
        .lower_type_expr_in_scope_with_mode("/w/iface.ts", &reference, ProjectionMode::Navigate)
        .expect("Foo reference lowers to a carrier");
    assert!(
        matches!(
            graph.node_data(decl_ref).as_deref(),
            Some(SemanticNodeData::DeclRef { .. } | SemanticNodeData::BareRef(_))
        ),
        "fixture sanity: Foo lowers to a carrier, got {:?}",
        graph.node_data(decl_ref)
    );
    let with_foo = program(graph, [ObjectConstructionEffect::Spread(decl_ref)]);
    let formula = project(
        &dispatch,
        with_foo,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula
        .closed()
        .expect("a DeclRef to an interface folds to its closed surface");
    assert!(matches!(
        closed
            .alternatives()
            .next()
            .expect("one alternative")
            .lookup(&key("b")),
        Some(crate::semantic_query::ClosedKeyLookup::Present(_))
    ));
}

#[test]
fn key_liveness_keeps_the_getter_of_a_paired_accessor() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let getter = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([]),
        return_type: number,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let setter = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([crate::semantic_query::FunctionParam::synthetic(
            Some(Arc::from("value")),
            string,
            false,
            false,
        )]),
        return_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)),
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let paired = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            accessor("x", getter, true),
            accessor("x", setter, false),
        ],
    );

    let formula = project(
        &dispatch,
        paired,
        ObjectProjectionSelector::Key(key("x")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(
        matches!(
            formula.alternatives()[0].selected_key(&key("x")),
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "a paired accessor reads as the getter return type, never the setter parameter"
    );
}

#[test]
fn exact_optional_property_types_threads_into_consumer_projections() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let one = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(1.0),
    ));
    let two = graph.intern_node(SemanticNodeData::Literal(
        crate::semantic_query::LiteralValue::Number(2.0),
    ));
    let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
    let optional_value = graph.intern_node(SemanticNodeData::Union(Arc::from([two, undefined])));
    let optional = object(graph, [surface_member("a", optional_value, true)]);
    let source = program(
        graph,
        [
            property("a", one, false),
            ObjectConstructionEffect::Spread(optional),
        ],
    );
    let narrow = object(graph, [surface_member("a", one, false)]);
    let wide = object(
        graph,
        [surface_member(
            "a",
            graph.intern_node(SemanticNodeData::Union(Arc::from([one, two, undefined]))),
            false,
        )],
    );

    // Default (exactOptionalPropertyTypes off): the folded value drops the
    // authored `undefined`, so the source relates to the exact narrow target
    // and to the wide one.
    assert!(matches!(
        relate(&dispatch, source, wide),
        crate::semantic_query::RelationResult::Assignable { .. }
    ));

    // With exactOptionalPropertyTypes on, the authored `undefined` is
    // preserved — the source only relates to the wide target.
    host.relation_knobs
        .strict_family_relax_bits
        .store(0b100, std::sync::atomic::Ordering::Relaxed);
    let enabled_dispatch = ProjectSemanticDispatch::new(&host);
    assert!(
        matches!(
            relate(&enabled_dispatch, source, wide),
            crate::semantic_query::RelationResult::Assignable { .. }
        ),
        "exactOptionalPropertyTypes preserves the authored undefined"
    );
    let exact_narrow = object(
        graph,
        [surface_member(
            "a",
            graph.intern_node(SemanticNodeData::Union(Arc::from([one, two]))),
            false,
        )],
    );
    assert_eq!(
        relate(&enabled_dispatch, source, exact_narrow),
        crate::semantic_query::RelationResult::NotAssignable,
        "the preserved undefined breaks assignability to the narrow target"
    );
    let _ = narrow;
}

fn empty_path_shallow_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    base: SemanticNodeId,
) -> crate::semantic_query::SurfaceView {
    let projected = dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(SemanticQueryOutput { value, .. }) = projected else {
        panic!("expected an empty-path Shallow surface, got {projected:?}")
    };
    let data = graph.node_data(value).expect("surface node has data");
    match &*data {
        SemanticNodeData::Object(view) => view.clone(),
        other => {
            panic!("empty-path Shallow terminal must publish an Object surface, got {other:?}")
        }
    }
}

fn surface_member_names(view: &crate::semantic_query::SurfaceView) -> Vec<String> {
    view.positive_members()
        .iter()
        .map(|member| {
            member
                .string_name()
                .expect("string-key fixture")
                .to_string()
        })
        .collect()
}

#[test]
fn empty_path_shallow_surface_over_program_root_publishes_the_construction() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Closed program `{ a: number, ...{ b: string } }`: a single closed
    // alternative. The empty-path Shallow terminal must project the
    // correlated formula and publish BOTH members — fabricating an empty
    // closed surface publishes zero props and proves absence with no
    // witness of the construction.
    let inner = object(graph, [surface_member("b", string, false)]);
    let root = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(inner),
        ],
    );

    let view = empty_path_shallow_surface(&dispatch, graph, root);
    let names = surface_member_names(&view);
    assert!(
        names.iter().any(|name| name == "a") && names.iter().any(|name| name == "b"),
        "the program root's surface must publish the construction members; observed {names:?}"
    );
}

#[test]
fn empty_path_shallow_over_open_program_root_returns_open_typed_evidence() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // Open program `{ a: number, ...T }`: `SurfaceView::closed()` is
    // total, so no honest `Object` materialisation exists — the terminal
    // must return the typed open evidence (the construction program
    // itself), flag the read partial + uncacheable, and surface the
    // open-spread diagnostic. Consumers that need positive names go
    // through the correlated query, never a fabricated closed Object.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let root = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );

    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: root,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(node) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    assert!(
        matches!(
            graph.node_data(node).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        ),
        "an open program root must NOT materialise a closed Object; observed {:?}",
        graph.node_data(node)
    );
    assert!(
        read.result_is_partial,
        "the open-root read is genuinely partial"
    );
    assert!(
        read.cache_suppress,
        "the open-root read must never be cached as a finished surface"
    );
    assert!(
        read.walker_diagnostics.iter().any(|diag| matches!(
            diag,
            crate::project_semantic_dispatch::walk::ShallowDiagnostic::OpenSpreadProgram { .. }
        )),
        "the open-root read carries the explicit incompleteness diagnostic"
    );
}

#[test]
fn empty_path_shallow_over_correlated_program_returns_open_typed_evidence() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Correlated program `{ c: number, ...({ a: number } | { b: string }) }`:
    // two closed alternatives — the construction is `{c, a} | {c, b}`,
    // which no single closed `Object` can represent (a common-member join
    // would fabricate absence of `a`/`b`). The terminal must stay open.
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let root = program(
        graph,
        [
            property("c", number, false),
            ObjectConstructionEffect::Spread(union),
        ],
    );

    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: root,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(node) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    assert!(
        matches!(
            graph.node_data(node).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        ),
        "a multi-alternative program root must NOT materialise a closed Object; observed {:?}",
        graph.node_data(node)
    );
    assert!(read.result_is_partial && read.cache_suppress);
}

/// Nested program arms: a UNION root whose arm is a program projects the
/// arm through the same single-closed rule — a closed arm contributes its
/// real members to the common-member merge, while an open arm flags the
/// whole read partial + uncacheable instead of fabricating an empty
/// closed union surface.
#[test]
fn empty_path_shallow_over_union_with_program_arm_projects_the_arm() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // Closed program arm: `{x, a} | {a, ...{b}}` — the arm projects to
    // `{a, b}`, so the union common-member is `a`.
    let closed_arm = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(object(graph, [surface_member("b", string, false)])),
        ],
    );
    let xa = object(
        graph,
        [
            surface_member("x", string, false),
            surface_member("a", number, false),
        ],
    );
    let union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([xa, closed_arm])));
    let view = empty_path_shallow_surface(&dispatch, graph, union_root);
    let names = surface_member_names(&view);
    assert_eq!(
        names,
        vec!["a".to_string()],
        "the closed program arm contributes its projected members to the union merge; observed {names:?}"
    );

    // Open program arm: the union surface cannot claim a closed domain —
    // the terminal returns the UNION node as the typed open evidence
    // (never a fabricated closed `Object`), partial + uncacheable.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_arm = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let open_union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([xa, open_arm])));
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: open_union_root,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(terminal) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    assert_eq!(
        terminal,
        open_union_root,
        "a union with an open program arm returns the union carrier, not a closed Object; \
         observed {:?}",
        graph.node_data(terminal)
    );
    assert!(
        read.result_is_partial && read.cache_suppress,
        "a union with an open program arm must flag the read partial + uncacheable"
    );
}

/// An INTERSECTION with an open program arm likewise keeps its carrier:
/// the common-member evidence (`{x, a}`) would fabricate completeness
/// over the open arm's unknown domain.
#[test]
fn empty_path_shallow_over_intersection_with_open_program_arm_keeps_the_carrier() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let xa = object(
        graph,
        [
            surface_member("x", string, false),
            surface_member("a", number, false),
        ],
    );
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_arm = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let intersection_root =
        graph.intern_node(SemanticNodeData::Intersection(Arc::from([xa, open_arm])));
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: intersection_root,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(terminal) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    assert_eq!(
        terminal,
        intersection_root,
        "an intersection with an open program arm returns the intersection carrier; \
         observed {:?}",
        graph.node_data(terminal)
    );
    assert!(read.result_is_partial && read.cache_suppress);
}

/// The shared typeinfo surface reader never hands out a
/// completeness-claiming `SurfaceView` for a partial terminal read.
#[test]
fn typeinfo_surface_view_refuses_partial_terminal_reads() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let xa = object(
        graph,
        [
            surface_member("x", string, false),
            surface_member("a", number, false),
        ],
    );
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_arm = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let open_union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([xa, open_arm])));
    assert_eq!(
        dispatch.resolve_typeinfo_surface_view(
            open_union_root,
            ProjectionReductionContext::published(ProjectionMode::Shallow),
        ),
        None,
        "a partial (open-spread) terminal read yields no completeness-claiming view"
    );
    // A closed union still hands out its view.
    let closed_arm = program(
        graph,
        [ObjectConstructionEffect::Spread(object(
            graph,
            [surface_member("b", string, false)],
        ))],
    );
    let closed_union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([xa, closed_arm])));
    assert!(
        dispatch
            .resolve_typeinfo_surface_view(
                closed_union_root,
                ProjectionReductionContext::published(ProjectionMode::Shallow),
            )
            .is_some(),
        "a closed union keeps its exact surface view"
    );
}

/// Projection failure on the program-root path (nested spread depth cap)
/// is typed partial + uncacheable — never a cacheable empty Object.
#[test]
fn empty_path_shallow_over_program_root_failure_is_typed_partial() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    // Chain of nested programs past the spread depth cap (8): the inner
    // projection errors, the root read must surface partiality.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let mut node = program(graph, [ObjectConstructionEffect::Spread(type_param)]);
    // bounded-loop: fixed 12-iteration fixture chain past the depth cap
    for _ in 0..12 {
        node = program(graph, [ObjectConstructionEffect::Spread(node)]);
    }

    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: node,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(terminal) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    assert!(
        !matches!(
            graph.node_data(terminal).as_deref(),
            Some(SemanticNodeData::Object(_))
        ),
        "a failed program-root projection must not publish any closed Object"
    );
    assert!(
        read.result_is_partial && read.cache_suppress,
        "the failure path is typed partial + uncacheable"
    );
}

fn numeric_property_key(value: i64) -> PropertyKey {
    PropertyKey::Number(
        crate::semantic_query::CanonicalIndexInt::from_canonical_i64(value).unwrap(),
    )
}

fn numeric_surface_member(value: i64, node: SemanticNodeId) -> SurfaceMember {
    SurfaceMember {
        key: AuthoredPropertyKey::Number(
            crate::semantic_query::CanonicalIndexInt::from_canonical_i64(value).unwrap(),
        ),
        value: node,
        optional: false,
        readonly: false,
        method_kind: None,
        has_implementation_body: false,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::NonLiteral,
    }
}

#[test]
fn fold_treats_numeric_and_string_spellings_as_one_js_property() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // `{1: string}` spread, then `"1": number` direct write: the two
    // authored spellings address the SAME JS property, so the fold keeps
    // ONE fact and the later write wins (tsc: `{...A, "1": number}` folds
    // the property to `number`).
    let one_obj = object(graph, [numeric_surface_member(1, string)]);
    let root = program(
        graph,
        [
            ObjectConstructionEffect::Spread(one_obj),
            property("1", number, false),
        ],
    );

    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let alternative = &formula.alternatives()[0];
    let mut colliding = Vec::new();
    alternative.positive().visit(|fact| {
        if fact
            .key()
            .element_access_collides(&PropertyKey::string_literal("1"))
        {
            colliding.push(fact.value().clone());
        }
    });
    assert_eq!(
        colliding,
        vec![ProjectionEvidence::Proven(number)],
        "one JS property — one fact, latest write; observed {colliding:?}"
    );
    // The closed lookup under the NUMERIC spelling finds the latest write
    // even though the stored fact carries the string spelling.
    assert!(
        matches!(
            alternative.selected_key(&numeric_property_key(1)),
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "a numeric-spelling read must find the colliding string-spelling fact"
    );
}

#[test]
fn key_projection_over_spelling_overwrite_reads_the_latest_write() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let one_obj = object(graph, [numeric_surface_member(1, string)]);
    let root = program(
        graph,
        [
            ObjectConstructionEffect::Spread(one_obj),
            property("1", number, false),
        ],
    );

    // Key(Number(1)): liveness must see the colliding required `"1"`
    // write and prune the spread, and the selector must keep the
    // colliding fact — the stale spread value must not survive.
    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::Key(numeric_property_key(1)),
        ExactOptionalPropertyPolicy::Disabled,
    );
    let alternative = &formula.alternatives()[0];
    assert!(
        matches!(
            alternative.selected_key(&numeric_property_key(1)),
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "Key(Number(1)) reads the latest write `number`, never the stale spread `string`"
    );
    // The reverse read agrees.
    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::Key(PropertyKey::string_literal("1")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(
        matches!(
            formula.alternatives()[0].selected_key(&PropertyKey::string_literal("1")),
            crate::semantic_query::OpenSafeKeyEvidence::Positive(fact)
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "Key(String(\"1\")) reads the same folded property"
    );
}

/// Consumer (component-meta props): the shared macro member reader must
/// publish an open program root's POSITIVE member names through the
/// correlated query — presence only, never a closed-domain claim.
#[test]
fn macro_member_reader_publishes_open_program_positive_names_without_completeness() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let names_of = |node: SemanticNodeId| -> Vec<String> {
        let members = crate::meta_resolve::projectors::read_positive_surface_members(&host, node);
        let mut names: Vec<String> = members
            .iter()
            .map(|member| {
                member
                    .string_name()
                    .expect("string-key fixture")
                    .to_string()
            })
            .collect();
        names.sort();
        names
    };

    // Open root `{ a: number, ...T }`: positive name `a` publishes.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_root = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    assert_eq!(
        names_of(open_root),
        vec!["a".to_string()],
        "an open program root publishes its positive member names"
    );

    // Multi-alternative root `{ c, ...({a} | {b}) }`: macro enumeration
    // unions the alternatives' declared names.
    let left = object(graph, [surface_member("a", number, false)]);
    let right = object(graph, [surface_member("b", string, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([left, right])));
    let multi_root = program(
        graph,
        [
            property("c", number, false),
            ObjectConstructionEffect::Spread(union),
        ],
    );
    assert_eq!(
        names_of(multi_root),
        vec!["a".to_string(), "b".to_string(), "c".to_string()],
        "macro enumeration unions every alternative's declared names"
    );

    // Closed root `{ a, ...{b} }`: both members, complete.
    let closed_root = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(object(graph, [surface_member("b", string, false)])),
        ],
    );
    assert_eq!(
        names_of(closed_root),
        vec!["a".to_string(), "b".to_string()],
        "a closed program root publishes its exact members"
    );
}

#[test]
fn paired_setter_excess_candidate_tracks_the_fact_key_spelling() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let getter_sig = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([]),
        return_type: string,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let setter_sig = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([crate::semantic_query::FunctionParam::synthetic(
            Some(Arc::from("value")),
            string,
            false,
            false,
        )]),
        return_type: graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)),
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    let accessor_effect = |key: AuthoredPropertyKey, signature| AuthoredAccessorEffect {
        key,
        signature,
        optional: false,
        has_implementation_body: true,
        visibility: MemberVisibility::Public,
        spans: MemberSpans::default(),
        declaration_origin: None,
        declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
        merge_role: MergeRoleStamp::NEUTRAL,
        excess_origin: ExcessPropertyOrigin::FreshOwn,
    };
    // `{...{}, get 1(): string, set "1"(v: string)}` — the getter writes
    // the fact under the NUMERIC spelling; the paired setter only folds
    // facets. The FreshOwn excess candidacy must track the FACT key
    // spelling — a candidacy holding only the setter's `"1"` spelling
    // loses the property to the excess gate's membership test (tsc
    // rejects the excess property; the false Pass is dormant today).
    let root = program(
        graph,
        [
            ObjectConstructionEffect::Spread(object(graph, [])),
            ObjectConstructionEffect::DirectGet(accessor_effect(
                AuthoredPropertyKey::Number(
                    crate::semantic_query::CanonicalIndexInt::from_canonical_i64(1).unwrap(),
                ),
                getter_sig,
            )),
            ObjectConstructionEffect::DirectSet(accessor_effect(
                AuthoredPropertyKey::string("1"),
                setter_sig,
            )),
        ],
    );

    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::ExcessEligibility,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let ExcessEligibility::Eligible { direct_candidates } = formula.alternatives()[0].excess()
    else {
        panic!(
            "a closed accessor program stays excess-eligible; observed {:?}",
            formula.alternatives()[0].excess()
        )
    };
    let fact_key = numeric_property_key(1);
    assert!(
        direct_candidates.contains(&fact_key),
        "the candidacy must contain the FACT key spelling (the getter's numeric key); \
         observed {direct_candidates:?}"
    );
    assert!(
        direct_candidates
            .iter()
            .any(|candidate| candidate.element_access_collides(&PropertyKey::string_literal("1"))),
        "the candidacy still covers the setter's spelling under JS identity"
    );
}

#[test]
fn formula_closed_keyof_intersects_dual_spellings_as_one_js_property() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // `{...({1: string} | {"1": number})}` — both alternatives closed,
    // each holding ONE spelling of the same JS property. The formula's
    // exact keyof must keep the property (first alternative's spelling),
    // not intersect to empty. (Deliberate engine-level JS-identity rule —
    // tsc's nominal `1 & "1"` is never; fold/lookups already collide.)
    let numeric_arm = object(graph, [numeric_surface_member(1, string)]);
    let string_arm = object(graph, [surface_member("1", number, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([
        numeric_arm,
        string_arm,
    ])));
    let root = program(graph, [ObjectConstructionEffect::Spread(union)]);

    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula
        .closed()
        .expect("both alternatives are closed — a formula-wide witness exists");
    assert_eq!(
        closed
            .keyof()
            .expect("a Surface-selector formula is whole-domain"),
        &[numeric_property_key(1)],
        "the dual spellings are one JS property — keyof keeps it (first alternative's spelling)"
    );
}

#[test]
fn direct_write_excess_candidates_replace_dual_spelling_stale_entries() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let numeric_write = |value: SemanticNodeId| {
        ObjectConstructionEffect::DirectProperty(AuthoredPropertyEffect {
            key: AuthoredPropertyKey::Number(
                crate::semantic_query::CanonicalIndexInt::from_canonical_i64(1).unwrap(),
            ),
            value,
            optional: false,
            readonly: false,
            visibility: MemberVisibility::Public,
            spans: MemberSpans::default(),
            declaration_origin: None,
            declared_in_macro_type_arg: MacroOwnBodyStamp::NEUTRAL,
            merge_role: MergeRoleStamp::NEUTRAL,
            excess_origin: ExcessPropertyOrigin::FreshOwn,
        })
    };
    // `{1: number, "1": string}` — one JS property written twice under
    // dual spellings. The fold respells the fact to the LATEST write; the
    // excess candidacy must drop the stale `Number(1)` entry and track
    // exactly the live fact spelling.
    let root = program(graph, [numeric_write(number), property("1", string, false)]);
    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::ExcessEligibility,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let ExcessEligibility::Eligible { direct_candidates } = formula.alternatives()[0].excess()
    else {
        panic!("closed dual-write program stays excess-eligible")
    };
    assert_eq!(
        direct_candidates.as_ref(),
        &[PropertyKey::string_literal("1")],
        "the stale numeric-spelling candidate is replaced, never duplicated; \
         observed {direct_candidates:?}"
    );
}

/// Consumer (props): the shared presence-only reader recurses carrier
/// arms — a union whose arm is an open program publishes EVERY arm's
/// positive members (`defineProps<{a: string} | {b: number, ...T}>()`
/// publishes a and b), never a closed domain.
#[test]
fn macro_member_reader_recurses_union_carriers_for_positive_members() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_arm = program(
        graph,
        [
            property("b", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([
        object(graph, [surface_member("a", string, false)]),
        open_arm,
    ])));
    // The walker's open-safe terminal returns the UNION carrier; the
    // reader must recover both arms' positive evidence from it.
    let dispatch = ProjectSemanticDispatch::new(&host);
    let read = dispatch.execute_read(SemanticQueryKey::ProjectPath {
        base: union_root,
        path: Arc::from(Vec::<crate::semantic_query::PathSegment>::new().into_boxed_slice()),
        context: ProjectionReductionContext::published(ProjectionMode::Shallow),
    });
    let QueryResult::Value(terminal) = read.value else {
        panic!("expected a terminal value, got {:?}", read.value)
    };
    let members = crate::meta_resolve::projectors::read_positive_surface_members(&host, terminal);
    let mut names: Vec<String> = members
        .iter()
        .map(|member| {
            member
                .string_name()
                .expect("string-key fixture")
                .to_string()
        })
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec!["a".to_string(), "b".to_string()],
        "every arm's positive members publish through the carrier; observed {names:?}"
    );

    // Typeinfo over the same carrier: positive members, never complete.
    let surface = host
        .project_shallow_surface_graph_only(
            &host,
            &dispatch,
            union_root,
            Arc::from([]),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        )
        .expect("an open carrier joins a presence-only typeinfo surface");
    assert!(
        !surface.members_complete,
        "the carrier surface never claims completeness"
    );
    let mut typeinfo_names: Vec<String> = surface
        .members
        .iter()
        .filter_map(|member| member.key.as_string().map(str::to_string))
        .collect();
    typeinfo_names.sort();
    assert_eq!(
        typeinfo_names,
        vec!["a".to_string(), "b".to_string()],
        "typeinfo surfaces the same presence-only members; observed {typeinfo_names:?}"
    );
}

/// Consumer (props): an INTERSECTION carrier merges under intersection
/// rules — required-wins, value intersection on same-key collision,
/// readonly-in-any-arm — never the union rule's optional-if-not-universal
/// / value-union / readonly-in-all folds.
#[test]
fn props_reader_applies_intersection_rules_to_intersection_carriers() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let names_of = |node: SemanticNodeId| -> Vec<crate::semantic_query::SurfaceMember> {
        crate::meta_resolve::projectors::read_positive_surface_members(&host, node)
    };

    // `{token: string} & {extra?: number, ...T}` — `token` is REQUIRED
    // (tsc intersection rule: required when required in any declaring
    // arm); `extra` stays optional.
    let token_obj = object(graph, [surface_member("token", string, false)]);
    let open_extra = program(
        graph,
        [
            ObjectConstructionEffect::Spread(type_param),
            property("extra", number, true),
        ],
    );
    let intersection_root = graph.intern_node(SemanticNodeData::Intersection(Arc::from([
        token_obj, open_extra,
    ])));
    let members = names_of(intersection_root);
    let token = members
        .iter()
        .find(|member| member.string_name() == Some("token"))
        .expect("token publishes");
    assert!(
        !token.optional,
        "intersection rule: required in any declaring arm stays required"
    );
    let extra = members
        .iter()
        .find(|member| member.string_name() == Some("extra"))
        .expect("extra publishes");
    assert!(extra.optional, "the authored optional stays optional");

    // `{x: string} & {...T, x: number}` — the same-key collision
    // INTERSECTS the values (never unions them).
    let x_string_obj = object(graph, [surface_member("x", string, false)]);
    let open_x = program(
        graph,
        [
            ObjectConstructionEffect::Spread(type_param),
            property("x", number, false),
        ],
    );
    let collision_root = graph.intern_node(SemanticNodeData::Intersection(Arc::from([
        x_string_obj,
        open_x,
    ])));
    let members = names_of(collision_root);
    let x = members
        .iter()
        .find(|member| member.string_name() == Some("x"))
        .expect("x publishes");
    let value_data = graph.node_data(x.value).expect("x value interned");
    let SemanticNodeData::Intersection(arms) = &*value_data else {
        panic!(
            "intersection rule: a same-key collision intersects the values; observed {:?}",
            *value_data
        )
    };
    assert!(
        arms.contains(&string) && arms.contains(&number),
        "the intersected value carries both contributors: {arms:?}"
    );

    // `{readonly x: string} & {...T, x: number}` — readonly in ANY arm
    // survives the merge.
    let readonly_x_obj = object(
        graph,
        [crate::semantic_query::SurfaceMember {
            readonly: true,
            ..surface_member("x", string, false)
        }],
    );
    let open_x2 = program(
        graph,
        [
            ObjectConstructionEffect::Spread(type_param),
            property("x", number, false),
        ],
    );
    let readonly_root = graph.intern_node(SemanticNodeData::Intersection(Arc::from([
        readonly_x_obj,
        open_x2,
    ])));
    let members = names_of(readonly_root);
    let x = members
        .iter()
        .find(|member| member.string_name() == Some("x"))
        .expect("x publishes");
    assert!(
        x.readonly,
        "intersection rule: readonly in any arm survives"
    );
}

/// The macro union merge aggregates under JS property identity: dual
/// spellings across arms (`{1: string} | {"1": number, ...T}`) are ONE
/// property — one published row, values unioned — never two members.
#[test]
fn macro_union_merge_collapses_dual_spelling_members() {
    let host = host();
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let numeric_arm = object(graph, [numeric_surface_member(1, string)]);
    let open_arm = program(
        graph,
        [
            ObjectConstructionEffect::Spread(type_param),
            property("1", number, false),
        ],
    );
    let union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([numeric_arm, open_arm])));
    let members = crate::meta_resolve::projectors::read_positive_surface_members(&host, union_root);
    assert_eq!(
        members.len(),
        1,
        "dual spellings of one JS property publish ONE member; observed {} members",
        members.len()
    );
    let value_data = graph.node_data(members[0].value).expect("value interned");
    let SemanticNodeData::Union(arms) = &*value_data else {
        panic!(
            "the union merge unions the colliding values; observed {:?}",
            *value_data
        )
    };
    assert!(
        arms.contains(&string) && arms.contains(&number),
        "both spellings' values union: {arms:?}"
    );
}

#[test]
fn raise_fold_keeps_index_signature_off_the_single_call_fast_path() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let void = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
    let call_sig = graph.intern_node(SemanticNodeData::Signature {
        kind: crate::semantic_query::SignatureKind::Call,
        params: Arc::from([]),
        return_type: void,
        type_parameters: Arc::from([]),
        signature_span: None,
        return_type_span: None,
    });
    // Closed program `{ (): void; [k: string]: number }` — one call
    // signature AND one index signature. The raise fold's single-call
    // fast path must NOT fire (it would raise the bare call and drop the
    // index signature): the materialised `SurfaceView` must carry
    // `has_index_signature` truthfully.
    let root = program(
        graph,
        [
            ObjectConstructionEffect::DirectCall(call_sig),
            ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
                key_type: string,
                value_type: number,
                readonly: false,
                spans: Default::default(),
                declaration_origin: None,
            }),
        ],
    );
    let mut active = rustc_hash::FxHashSet::default();
    let raised =
        crate::project_semantic_dispatch::raise::fold_to_type_expr(&dispatch, root, &mut active)
            .expect("a closed program raises");
    let verter_type_expr::TypeExpr::Object(object) = raised.expr() else {
        panic!(
            "the call+index surface raises as an OBJECT — the single-call fast path must not \
             fire; observed {:?}",
            raised.expr()
        )
    };
    assert!(
        object
            .properties
            .iter()
            .any(|member| matches!(member, verter_type_expr::ObjectMember::CallSignature(_))),
        "the call signature survives"
    );
    assert!(
        object
            .properties
            .iter()
            .any(|member| matches!(member, verter_type_expr::ObjectMember::IndexSignature(_))),
        "the index signature survives — it must not be dropped by the single-call fast path"
    );
}

#[test]
fn union_common_member_merge_collapses_dual_spelling_members() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // `type U = {1: string} | {"1": number}` — the two spellings are ONE
    // JS property present in EVERY arm: the common-member rule must keep
    // it with the union of both values (tsc: `"1": string | number`).
    // Strict-key aggregation drops it entirely (the arm-0 entry misses
    // the declaring-arms count; the arm-1 spelling is never emitted).
    let numeric_arm = object(graph, [numeric_surface_member(1, string)]);
    let string_arm = object(graph, [surface_member("1", number, false)]);
    let union_root = graph.intern_node(SemanticNodeData::Union(Arc::from([
        numeric_arm,
        string_arm,
    ])));

    let view = empty_path_shallow_surface(&dispatch, graph, union_root);
    let colliding: Vec<&crate::semantic_query::SurfaceMember> = view
        .positive_members()
        .iter()
        .filter(|member| {
            member
                .key
                .cloned_known()
                .is_some_and(|key| key.element_access_collides(&numeric_property_key(1)))
        })
        .collect();
    assert_eq!(
        colliding.len(),
        1,
        "one JS property — one common member; observed {} members",
        view.positive_members().len()
    );
    let value_data = graph.node_data(colliding[0].value).expect("value interned");
    let SemanticNodeData::Union(arms) = &*value_data else {
        panic!(
            "the common member's value unions both spellings' values; observed {:?}",
            *value_data
        )
    };
    assert!(
        arms.contains(&string) && arms.contains(&number),
        "both spellings' values union: {arms:?}"
    );
}

#[test]
fn intersection_merge_collapses_dual_spelling_members() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // `type I = {1: string} & {"1": number}` — one JS property: ONE
    // member with the INTERSECTED value (`string & number`), never two
    // unintersected rows (a collides-aware lookup would read only the
    // first, widening the value to `string`).
    let numeric_arm = object(graph, [numeric_surface_member(1, string)]);
    let string_arm = object(graph, [surface_member("1", number, false)]);
    let intersection_root = graph.intern_node(SemanticNodeData::Intersection(Arc::from([
        numeric_arm,
        string_arm,
    ])));

    let view = empty_path_shallow_surface(&dispatch, graph, intersection_root);
    let colliding: Vec<&crate::semantic_query::SurfaceMember> = view
        .positive_members()
        .iter()
        .filter(|member| {
            member
                .key
                .cloned_known()
                .is_some_and(|key| key.element_access_collides(&numeric_property_key(1)))
        })
        .collect();
    assert_eq!(
        colliding.len(),
        1,
        "one JS property — one intersected member; observed {} members",
        view.positive_members().len()
    );
    let value_data = graph.node_data(colliding[0].value).expect("value interned");
    let SemanticNodeData::Intersection(arms) = &*value_data else {
        panic!(
            "the collision INTERSECTS the values; observed {:?}",
            *value_data
        )
    };
    assert!(
        arms.contains(&string) && arms.contains(&number),
        "both spellings' values intersect: {arms:?}"
    );
}

#[test]
fn program_index_obligations_relate_every_overlapping_source_index() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let any = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
    // Source `{[s: string]: any, [n: number]: string}` vs target
    // `{[k: string]: number}`: the target string domain overlaps BOTH
    // source indices (a string index covers the number domain for value
    // relating). Relating only the FIRST overlap accepts via the `any`
    // index — order-dependent and unsound; the legacy authority
    // (`relate_target_index_signature`) relates ALL overlaps, and the
    // number-index value `string` refutes `number`.
    let source = program(
        graph,
        [
            ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
                key_type: string,
                value_type: any,
                readonly: false,
                spans: Default::default(),
                declaration_origin: None,
            }),
            ObjectConstructionEffect::DirectIndex(AuthoredIndexEffect {
                key_type: number,
                value_type: string,
                readonly: false,
                spans: Default::default(),
                declaration_origin: None,
            }),
        ],
    );
    let target = index_object(graph, string, number);
    assert_eq!(
        relate(&dispatch, source, target),
        crate::semantic_query::RelationResult::NotAssignable,
        "every domain-overlapping source index relates — the refuting number index rejects"
    );

    // Named fill: an optional numeric target member falls to the source
    // index fill, where the same all-overlapping rule applies — the
    // number-domain index value `string` refutes even though the
    // string-domain index accepts via `any`.
    let target_named = object(graph, [surface_member("42", number, true)]);
    assert_eq!(
        relate(&dispatch, source, target_named),
        crate::semantic_query::RelationResult::NotAssignable,
        "the named index fill relates every applicable source index"
    );
}

#[test]
fn keyof_over_program_base_enumerates_closed_and_defers_open() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let keyof = |base: SemanticNodeId| {
        dispatch.execute_read(SemanticQueryKey::KeyOf {
            base,
            context: ProjectionReductionContext::published(ProjectionMode::Expanded),
        })
    };

    // Closed program `{ a: number, ...{ b: string } }` → the EXACT
    // keyspace `("a" | "b")`, complete — never a bare admittable Miss.
    let closed_program = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(object(graph, [surface_member("b", string, false)])),
        ],
    );
    let read = keyof(closed_program);
    let QueryResult::Value(node) = read.value else {
        panic!("expected a keyspace value, got {:?}", read.value)
    };
    let data = graph.node_data(node).expect("keyof node interned");
    let SemanticNodeData::Union(arms) = &*data else {
        panic!(
            "a closed program's keyof enumerates the exact literal union; observed {:?}",
            *data
        )
    };
    let mut names: Vec<String> = arms
        .iter()
        .filter_map(|arm| match graph.node_data(*arm).as_deref() {
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(name))) => {
                Some(name.to_string())
            }
            _ => None,
        })
        .collect();
    names.sort();
    assert_eq!(names, vec!["a".to_string(), "b".to_string()]);
    assert!(
        !read.result_is_partial && !read.cache_suppress,
        "the exact closed keyspace is a complete, admissible value"
    );

    // Open program `{ a: number, ...T }` → the DEFERRED `KeyOf` carrier
    // (re-dispatchable when the operand closes), never a bare
    // warm-admittable `Opaque(Miss)`.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let open_program = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let read = keyof(open_program);
    let QueryResult::Value(node) = read.value else {
        panic!("expected a keyof carrier, got {:?}", read.value)
    };
    let data = graph.node_data(node).expect("keyof node interned");
    assert!(
        matches!(
            &*data,
            SemanticNodeData::KeyOf { base } if *base == open_program
        ),
        "an open program's keyof is the deferred carrier, not a bare Miss; observed {:?}",
        *data
    );
}

#[test]
fn typeinfo_join_collapses_dual_spelling_members() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let boolean = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    // `{...({1: string} | {"1": number}), z: boolean}` — the typeinfo
    // join must publish ONE row for the dual-spelled property with the
    // unioned value (never two mis-optionaled strict-key rows).
    let numeric_arm = object(graph, [numeric_surface_member(1, string)]);
    let string_arm = object(graph, [surface_member("1", number, false)]);
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from([
        numeric_arm,
        string_arm,
    ])));
    let root = program(
        graph,
        [
            ObjectConstructionEffect::Spread(union),
            property("z", boolean, false),
        ],
    );
    let surface = host
        .project_shallow_surface_graph_only(
            &host,
            &dispatch,
            root,
            Arc::from([]),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        )
        .expect("a correlated program joins a typeinfo surface");
    assert!(
        !surface.members_complete,
        "the correlated join never claims completeness"
    );
    let colliding: Vec<&crate::typeinfo::surface::TypeInfoSurfaceMember> = surface
        .members
        .iter()
        .filter(|member| {
            member
                .key
                .cloned_known()
                .is_some_and(|key| key.element_access_collides(&numeric_property_key(1)))
        })
        .collect();
    assert_eq!(
        colliding.len(),
        1,
        "one JS property — one joined row; observed {} members {:?}",
        surface.members.len(),
        surface
            .members
            .iter()
            .map(|member| member.key.as_string().map(str::to_string))
            .collect::<Vec<_>>()
    );
    let value_data = graph.node_data(colliding[0].value).expect("value interned");
    let SemanticNodeData::Union(arms) = &*value_data else {
        panic!(
            "the joined value unions both spellings' values; observed {:?}",
            *value_data
        )
    };
    assert!(
        arms.contains(&string) && arms.contains(&number),
        "both spellings' values union: {arms:?}"
    );
    assert!(
        !colliding[0].optional,
        "present in every alternative — the joined row is required"
    );
    let z = surface
        .members
        .iter()
        .find(|member| member.key.as_string() == Some("z"))
        .expect("z publishes");
    assert!(!z.optional);
}

#[test]
fn selector_local_closed_domain_proves_only_the_declared_keys() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));

    // (a) `{x: number, y: string}` under `Key("x")`: the closed domain is
    // SELECTOR-LOCAL — `lookup("y")` has NO answer (never a forged
    // AbsentProven), `keyof` stays sealed, `lookup("x")` proves presence.
    let two_props = program(
        graph,
        [property("x", number, false), property("y", string, false)],
    );
    let formula = project(
        &dispatch,
        two_props,
        ObjectProjectionSelector::Key(key("x")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula
        .closed()
        .expect("a fully-materialised selector evaluation is closed");
    let alternative = closed.alternatives().next().expect("one alternative");
    assert!(
        matches!(
            alternative.lookup(&key("x")),
            Some(crate::semantic_query::ClosedKeyLookup::Present(fact))
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "the selected key is proven inside the declared set"
    );
    assert_eq!(
        alternative.lookup(&key("y")),
        None,
        "a key outside the selector's declared set has NO verdict — never a forged AbsentProven"
    );
    assert_eq!(
        closed.keyof(),
        None,
        "a selector-local closed domain does not yield a formula keyof"
    );
    assert_eq!(
        alternative.is_empty(),
        None,
        "domain emptiness is a whole-domain operation"
    );

    // (b) `{...T, x: 1}` under `Key("x")`: liveness prunes the spread and
    // records no residual, so the alternative mints closed — but
    // selector-local: the pruned spread may contribute OTHER keys, so the
    // domain stays sealed while `x` itself is proven (dominance rule).
    let generic = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let pruned = program(
        graph,
        [
            ObjectConstructionEffect::Spread(generic),
            property("x", number, false),
        ],
    );
    let formula = project(
        &dispatch,
        pruned,
        ObjectProjectionSelector::Key(key("x")),
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula
        .closed()
        .expect("liveness pruning leaves no residual — the alternative is closed");
    let alternative = closed.alternatives().next().expect("one alternative");
    assert!(
        matches!(
            alternative.lookup(&key("x")),
            Some(crate::semantic_query::ClosedKeyLookup::Present(fact))
                if fact.value() == &ProjectionEvidence::Proven(number)
        ),
        "the dominated selected key is proven despite the pruned spread"
    );
    assert_eq!(
        alternative.lookup(&key("y")),
        None,
        "the pruned spread may hold `y` — no absence verdict exists"
    );
    assert_eq!(closed.keyof(), None);

    // The SAME program under the whole-program Surface selector keeps the
    // residual and stays OPEN — whole-domain honesty is preserved.
    let formula = project(
        &dispatch,
        pruned,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    assert!(
        formula.closed().is_none(),
        "a Surface evaluation over `{{...T, x: 1}}` keeps the open residual"
    );
}

#[test]
fn whole_domain_closed_witness_still_answers_domain_operations() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    // Surface selector over a closed program: the whole-domain witness
    // keeps keyof / is_empty / lookup-for-any-key.
    let root = program(
        graph,
        [
            property("x", number, false),
            ObjectConstructionEffect::Spread(object(graph, [surface_member("y", string, false)])),
        ],
    );
    let formula = project(
        &dispatch,
        root,
        ObjectProjectionSelector::Surface,
        ExactOptionalPropertyPolicy::Disabled,
    );
    let closed = formula.closed().expect("closed program");
    let alternative = closed.alternatives().next().expect("one alternative");
    assert_eq!(
        alternative.is_empty(),
        Some(false),
        "whole-domain emptiness answers"
    );
    assert!(
        matches!(
            alternative.lookup(&key("y")),
            Some(crate::semantic_query::ClosedKeyLookup::Present(_))
        ),
        "whole-domain lookup proves y"
    );
    assert!(
        matches!(
            alternative.lookup(&key("z")),
            Some(crate::semantic_query::ClosedKeyLookup::AbsentProven)
        ),
        "whole-domain lookup proves z's absence"
    );
    assert!(closed.keyof().is_some_and(|keyof| keyof.len() == 2));
}

#[test]
fn typeinfo_join_publishes_indeterminate_value_members_as_open_rows() {
    let host = host();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    // `{a: number, ...T}` — `a` is definitely PRESENT (a later spread
    // overwrites values, never removes keys) but its value is
    // Indeterminate. The typeinfo join must publish the row with the
    // honest open value (the walker's `Opaque(OpenSurface)` convention),
    // never drop it.
    let type_param = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let root = program(
        graph,
        [
            property("a", number, false),
            ObjectConstructionEffect::Spread(type_param),
        ],
    );
    let surface = host
        .project_shallow_surface_graph_only(
            &host,
            &dispatch,
            root,
            Arc::from([]),
            ProjectionReductionContext::published(ProjectionMode::Shallow),
            None,
        )
        .expect("an open program joins a typeinfo surface");
    assert!(!surface.members_complete);
    let a = surface
        .members
        .iter()
        .find(|member| member.key.as_string() == Some("a"))
        .expect("a definitely-present member with an indeterminate value still publishes");
    assert!(
        matches!(
            graph.node_data(a.value).as_deref(),
            Some(SemanticNodeData::Opaque(
                crate::semantic_query::QueryError::OpenSurface
            ))
        ),
        "the indeterminate value publishes as the honest open marker; observed {:?}",
        graph.node_data(a.value)
    );
}
