//! Graph-facing signature discovery: subjects, precedence, provenance,
//! optionality, unions, intersections, and demand-driven results.

use std::sync::Arc;

use super::call_resolve_tests::{occurrence, signature};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    CanonicalTypeSubstitution, FunctionParam, IncompleteReason, PrimitiveKind, QueryError,
    QueryOutcome, Ready, SemanticContext, SemanticContextId, SemanticNodeData, SemanticNodeId,
    SignatureKind, SignatureReturnCarrier, TupleElement, TypeParamDecl, CONTEXT_FREE_EVALUATION,
};
use crate::signature_kernel::{
    BorrowedSet, CallSubstitution, ResultDemand, SemanticReadView, SignatureResultRecipe,
    SignatureSetRef, SignatureStore,
};
use crate::types::UpsertRequest;
use crate::{HostConfig, VerterHost};

const CANONICAL: &str = "/ws/signature-discovery.ts";

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(CANONICAL.to_string()),
        input_id: CANONICAL.to_string(),
        source: Arc::from("export {};\n"),
        file_language: crate::LanguageRegistry::global()
            .classify_static(CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn prim(d: &ProjectSemanticDispatch<'_>, kind: PrimitiveKind) -> SemanticNodeId {
    d.graph().intern_node(SemanticNodeData::Primitive(kind))
}

fn param(ty: SemanticNodeId) -> FunctionParam {
    FunctionParam::synthetic(None, ty, false, false)
}

fn callable(
    d: &ProjectSemanticDispatch<'_>,
    calls: Vec<SemanticNodeId>,
    constructs: Vec<SemanticNodeId>,
) -> SemanticNodeId {
    d.graph()
        .intern_node(SemanticNodeData::Object(crate::test_surface_view! {
            members: Arc::from(Vec::new().into_boxed_slice()),
            call_signatures: Arc::from(calls.into_boxed_slice()),
            construct_signatures: Arc::from(constructs.into_boxed_slice()),
            index_signatures: Arc::from(Vec::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
}

fn discover(
    d: &ProjectSemanticDispatch<'_>,
    subject: SemanticNodeId,
    kind: SignatureKind,
) -> QueryOutcome<SignatureSetRef> {
    d.signatures_of_type(
        d.graph().signature_store(),
        subject,
        kind,
        SemanticContextId::production(),
    )
}

fn ready(outcome: QueryOutcome<SignatureSetRef>) -> SignatureSetRef {
    match outcome {
        QueryOutcome::Ready(Ready { value, .. }) => value,
        QueryOutcome::Incomplete(reason) => panic!("expected ready, got incomplete {reason:?}"),
    }
}

fn count(set: SignatureSetRef, store: &SignatureStore) -> usize {
    match SemanticReadView::pin(store)
        .read_set(set)
        .expect("live set")
    {
        BorrowedSet::Empty => 0,
        BorrowedSet::One { .. } => 1,
        BorrowedSet::Many(list) => list.len(),
    }
}

fn candidates(
    set: SignatureSetRef,
    store: &SignatureStore,
) -> Vec<crate::signature_kernel::SignatureCandidate> {
    match SemanticReadView::pin(store)
        .read_set(set)
        .expect("live set")
    {
        BorrowedSet::Empty => Vec::new(),
        BorrowedSet::One { candidate, .. } => vec![candidate],
        BorrowedSet::Many(list) => list.to_vec(),
    }
}

fn recipe_of(
    store: &SignatureStore,
    candidate: crate::signature_kernel::SignatureCandidate,
) -> SignatureResultRecipe {
    let view = SemanticReadView::pin(store);
    let descriptor = view.descriptor(candidate.signature).unwrap();
    let template = view.template(descriptor.template).unwrap();
    *view.recipe(template.result_recipe).unwrap()
}

fn identity_substitution(
    store: &SignatureStore,
    candidate: crate::signature_kernel::SignatureCandidate,
) -> crate::signature_kernel::CallSubstitutionId {
    let view = SemanticReadView::pin(store);
    let space = view
        .descriptor(candidate.signature)
        .unwrap()
        .residual_binders;
    store
        .intern_substitution(CallSubstitution::identity(space), None)
        .unwrap()
}

fn read_return(
    d: &ProjectSemanticDispatch<'_>,
    candidate: crate::signature_kernel::SignatureCandidate,
    call: crate::signature_kernel::CallSubstitutionId,
) -> Result<SemanticNodeId, IncompleteReason> {
    let store = d.graph().signature_store();
    match d.read_signature_result(
        store,
        candidate.signature,
        call,
        ResultDemand::Return,
        CONTEXT_FREE_EVALUATION,
        SemanticContextId::production(),
    ) {
        QueryOutcome::Ready(Ready { value, .. }) => {
            let record = store.applied_result(value).unwrap();
            Ok(store
                .type_token_node(record.return_type.expect("return demanded"))
                .unwrap())
        }
        QueryOutcome::Incomplete(reason) => Err(reason),
    }
}

/// A direct signature is One only when its stored kind matches; the other
/// kind is a COMPLETE negative, distinct from every incomplete reason.
#[test]
fn direct_signature_matches_only_its_own_kind() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let call = signature(
        &d,
        "f",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        string,
    );
    let construct = signature(&d, "C", 0, SignatureKind::Construct, vec![], vec![], string);
    let store = d.graph().signature_store();

    assert_eq!(
        count(ready(discover(&d, call, SignatureKind::Call)), store),
        1
    );
    assert_eq!(
        ready(discover(&d, call, SignatureKind::Construct)),
        SignatureSetRef::Empty
    );
    assert_eq!(
        count(
            ready(discover(&d, construct, SignatureKind::Construct)),
            store
        ),
        1
    );
    assert_eq!(
        ready(discover(&d, construct, SignatureKind::Call)),
        SignatureSetRef::Empty
    );
}

/// An empty candidate set is a complete negative; an unsettled subject is an
/// explicit incomplete outcome, never Empty.
#[test]
fn empty_is_a_complete_negative_distinct_from_incomplete() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let object = callable(&d, vec![], vec![]);
    assert_eq!(
        ready(discover(&d, object, SignatureKind::Call)),
        SignatureSetRef::Empty
    );
    let unconstrained = d.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("Free"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("Free"),
    });
    assert_eq!(
        ready(discover(&d, unconstrained, SignatureKind::Call)),
        SignatureSetRef::Empty
    );
    let refused = d
        .graph()
        .intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    assert_eq!(
        discover(&d, refused, SignatureKind::Call),
        QueryOutcome::Incomplete(IncompleteReason::UnsettledInput)
    );
}

/// Authored order and overload ordinals survive: the candidate list follows
/// the surface's declared order and each provenance carries its own ordinal.
#[test]
fn object_surface_keeps_authored_order_and_overload_ordinals() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let boolean = prim(&d, PrimitiveKind::Boolean);
    let a = signature(
        &d,
        "o",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        string,
    );
    let b = signature(
        &d,
        "o",
        1,
        SignatureKind::Call,
        vec![param(number)],
        vec![],
        number,
    );
    let c = signature(
        &d,
        "o",
        2,
        SignatureKind::Call,
        vec![param(boolean)],
        vec![],
        boolean,
    );
    let object = callable(&d, vec![b, c, a], vec![]);
    let store = d.graph().signature_store();
    let list = candidates(ready(discover(&d, object, SignatureKind::Call)), store);
    assert_eq!(list.len(), 3);
    let view = SemanticReadView::pin(store);
    let ordinals: Vec<u32> = list
        .iter()
        .map(|c| view.provenance(c.provenance).unwrap().overload_ordinal)
        .collect();
    assert_eq!(ordinals, vec![0, 1, 2]);
    let source_ordinals: Vec<u32> = list
        .iter()
        .map(|c| view.provenance(c.provenance).unwrap().source_ordinal)
        .collect();
    assert_eq!(
        source_ordinals,
        vec![1, 2, 0],
        "authored occurrence ordinals ride along"
    );
}

/// Aliases and constrained type parameters settle through to their
/// signature-bearing shape.
#[test]
fn alias_and_constraint_settle_to_the_signature_shape() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let f = signature(
        &d,
        "g",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        string,
    );
    let object = callable(&d, vec![f], vec![]);
    let alias = d.graph().intern_node(SemanticNodeData::Alias(object));
    let constrained = d.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("Fn"),
        param_index: 0,
        constraint: Some(alias),
        default: None,
        display_name: Arc::from("Fn"),
    });
    let store = d.graph().signature_store();
    assert_eq!(
        count(ready(discover(&d, alias, SignatureKind::Call)), store),
        1
    );
    assert_eq!(
        count(ready(discover(&d, constrained, SignatureKind::Call)), store),
        1
    );
}

/// Optionality is an explicit effective fact: an optional parameter adds
/// `undefined` only under strictNullChecks, a fixed tuple rest flattens into
/// ordinary positions, and an array-rest tail keeps its required positions.
#[test]
fn optionality_and_tuple_rest_flatten_under_the_active_context() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let tuple = d.graph().intern_node(SemanticNodeData::Tuple {
        elements: Arc::from(
            vec![
                TupleElement {
                    label: Some(Arc::from("a")),
                    value: string,
                    optional: false,
                    rest: false,
                },
                TupleElement {
                    label: Some(Arc::from("b")),
                    value: number,
                    optional: true,
                    rest: false,
                },
            ]
            .into_boxed_slice(),
        ),
        readonly: false,
    });
    let sig = signature(
        &d,
        "h",
        0,
        SignatureKind::Call,
        vec![
            FunctionParam::synthetic(None, string, true, false),
            FunctionParam::synthetic(Some(Arc::from("rest")), tuple, false, true),
        ],
        vec![],
        string,
    );
    let store = d.graph().signature_store();
    let layout_of = |context: SemanticContextId| {
        let set = ready(d.signatures_of_type(store, sig, SignatureKind::Call, context));
        let candidate = candidates(set, store).remove(0);
        let view = SemanticReadView::pin(store);
        let descriptor = view.descriptor(candidate.signature).unwrap();
        let template = view.template(descriptor.template).unwrap();
        let shape = view.shape(template.input_shape).unwrap();
        view.layout(shape.parameter_layout).unwrap().clone()
    };
    let strict = layout_of(SemanticContextId::production());
    assert_eq!(
        strict.parameters.len(),
        3,
        "optional + flattened tuple positions"
    );
    assert!(strict.rest.is_none());
    assert!(strict.parameters[0].optionality.includes_undefined);
    assert!(strict.parameters[2].optionality.declared_optional);

    let mut options = SemanticContext::production();
    options.effective_semantic_options.strict_null_checks = false;
    let loose = layout_of(options.intern());
    assert!(loose.parameters[0].optionality.declared_optional);
    assert!(!loose.parameters[0].optionality.includes_undefined);
    assert_ne!(strict, loose, "the effective option is part of the shape");
}

/// Enumeration forces zero bodies; only a result read demanding the return
/// forces one, and an effects-only demand never does.
#[test]
fn enumeration_forces_no_body_and_effects_only_reads_stay_shape_only() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let node = d.graph().intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from(vec![param(string)].into_boxed_slice()),
        return_type: string,
        type_parameters: Arc::from(Vec::new().into_boxed_slice()),
        occurrence: Some(occurrence("body", 0)),
        return_carrier: SignatureReturnCarrier::Function(
            verter_type_expr::facts::FunctionReturnSource::Absent,
        ),
        signature_span: None,
        return_type_span: None,
    });
    let object = callable(&d, vec![node], vec![]);
    let store = d.graph().signature_store();
    let set = ready(discover(&d, object, SignatureKind::Call));
    let candidate = candidates(set, store).remove(0);
    assert!(matches!(
        recipe_of(store, candidate),
        SignatureResultRecipe::Body { .. }
    ));
    assert_eq!(store.bodies_forced(), 0, "enumeration inspected no body");

    let call = identity_substitution(store, candidate);
    let effects = d.read_signature_result(
        store,
        candidate.signature,
        call,
        ResultDemand::Effects,
        CONTEXT_FREE_EVALUATION,
        SemanticContextId::production(),
    );
    assert!(matches!(effects, QueryOutcome::Ready(_)));
    assert_eq!(
        store.bodies_forced(),
        0,
        "an effects-only demand forced no body"
    );

    assert_eq!(
        read_return(&d, candidate, call),
        Err(IncompleteReason::UnresolvedObligation),
        "a body without a recoverable carrier is an explicit unresolved obligation"
    );
    assert_eq!(store.bodies_forced(), 1);
}

/// A declared generic return applies the composed environment-then-call map
/// exactly once, and an unmapped binder stays the authored binder: no
/// residual-space token escapes into the graph.
#[test]
fn declared_result_applies_the_call_map_once_and_leaks_no_binder_token() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let number = prim(&d, PrimitiveKind::Number);
    let t = d.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("Ret"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let ret = d.graph().intern_node(SemanticNodeData::Array {
        element: t,
        readonly: false,
    });
    let sig = signature(
        &d,
        "wrap",
        0,
        SignatureKind::Call,
        vec![param(t)],
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        ret,
    );
    let store = d.graph().signature_store();
    let candidate = candidates(ready(discover(&d, sig, SignatureKind::Call)), store).remove(0);
    let view = SemanticReadView::pin(store);
    let space = view
        .descriptor(candidate.signature)
        .unwrap()
        .residual_binders;
    drop(view);
    let token = store.binder_token_for(space, 0).unwrap();

    let mapped = store
        .intern_substitution(
            CallSubstitution::map(space, CanonicalTypeSubstitution::new(vec![(token, number)])),
            None,
        )
        .unwrap();
    let node = read_return(&d, candidate, mapped).expect("declared return reads");
    assert_eq!(
        d.graph().node_data(node).as_deref(),
        Some(&SemanticNodeData::Array {
            element: number,
            readonly: false
        })
    );

    let open = identity_substitution(store, candidate);
    let node = read_return(&d, candidate, open).expect("declared return reads");
    assert_eq!(
        node, ret,
        "an unmapped binder is the authored binder, not a token"
    );
    assert!(!SignatureStore::is_binder_token(node));
}

/// Union arms whose signatures match (parameters equal, returns ignored)
/// form one common-match candidate; its result is the union of the arm
/// returns, read through mapped constituent edges.
#[test]
fn union_common_match_builds_one_candidate_with_mapped_constituents() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let boolean = prim(&d, PrimitiveKind::Boolean);
    let a = signature(
        &d,
        "ua",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        number,
    );
    let b = signature(
        &d,
        "ub",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        boolean,
    );
    let union = d.intern_normalized_union_or_intersection(
        &[callable(&d, vec![a], vec![]), callable(&d, vec![b], vec![])],
        true,
    );
    let store = d.graph().signature_store();
    let set = ready(discover(&d, union, SignatureKind::Call));
    let list = candidates(set, store);
    assert_eq!(list.len(), 1);
    let recipe = recipe_of(store, list[0]);
    let SignatureResultRecipe::UnionCommon { constituents, .. } = recipe else {
        panic!("expected a common-match composite, got {recipe:?}");
    };
    let view = SemanticReadView::pin(store);
    assert_eq!(view.sequence(constituents).unwrap().edges.len(), 2);
    drop(view);
    let call = identity_substitution(store, list[0]);
    let node = read_return(&d, list[0], call).expect("union return reads");
    let expected = d.intern_normalized_union_or_intersection(&[number, boolean], true);
    assert_eq!(node, expected);
}

/// Only when no common match exists does restricted synthesis run: distinct
/// parameter types intersect, and an arm with no signatures makes the whole
/// union have none.
#[test]
fn union_synthesis_is_restricted_and_absent_arm_yields_no_signatures() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let a = signature(
        &d,
        "sa",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        number,
    );
    let b = signature(
        &d,
        "sb",
        0,
        SignatureKind::Call,
        vec![param(number), param(number)],
        vec![],
        string,
    );
    let union = d.intern_normalized_union_or_intersection(
        &[callable(&d, vec![a], vec![]), callable(&d, vec![b], vec![])],
        true,
    );
    let store = d.graph().signature_store();
    let list = candidates(ready(discover(&d, union, SignatureKind::Call)), store);
    assert_eq!(list.len(), 1);
    assert!(matches!(
        recipe_of(store, list[0]),
        SignatureResultRecipe::UnionSynthesized { .. }
    ));
    let view = SemanticReadView::pin(store);
    let descriptor = view.descriptor(list[0].signature).unwrap();
    let template = view.template(descriptor.template).unwrap();
    let shape = view.shape(template.input_shape).unwrap();
    let layout = view.layout(shape.parameter_layout).unwrap();
    assert_eq!(layout.parameters.len(), 2, "the longer parameter list wins");
    assert_eq!(shape.declared_minimum, 2, "the larger minimum wins");
    drop(view);

    let with_missing = d.intern_normalized_union_or_intersection(
        &[callable(&d, vec![a], vec![]), callable(&d, vec![], vec![])],
        true,
    );
    assert_eq!(
        ready(discover(&d, with_missing, SignatureKind::Call)),
        SignatureSetRef::Empty
    );
}

/// Intersections keep authored order and dedup by signature equivalence
/// (returns included), retaining the first representative.
#[test]
fn intersection_dedups_by_signature_equivalence_in_authored_order() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let first = signature(
        &d,
        "ia",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        number,
    );
    let duplicate = signature(
        &d,
        "ib",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        number,
    );
    let other = signature(
        &d,
        "ic",
        0,
        SignatureKind::Call,
        vec![param(number)],
        vec![],
        string,
    );
    let members = [
        callable(&d, vec![first], vec![]),
        callable(&d, vec![duplicate], vec![]),
        callable(&d, vec![other], vec![]),
    ];
    let intersection = d.graph().intern_node(SemanticNodeData::Intersection(
        crate::semantic_query::composite::CompositeList::test_fixture(Arc::from(
            members.to_vec().into_boxed_slice(),
        )),
    ));
    let store = d.graph().signature_store();
    let list = candidates(
        ready(discover(&d, intersection, SignatureKind::Call)),
        store,
    );
    assert_eq!(list.len(), 2, "the equivalent signature is deduplicated");
    let view = SemanticReadView::pin(store);
    let ordinals: Vec<u32> = list
        .iter()
        .map(|c| view.provenance(c.provenance).unwrap().source_ordinal)
        .collect();
    assert_eq!(ordinals, vec![0, 0]);
    let first_source = store.descriptor_source(list[0].signature).unwrap().unwrap();
    assert_eq!(store.type_token_node(first_source).unwrap(), first);
}

/// A mixin constructor composes the other members' construct signatures
/// with the mixin's instance type.
#[test]
fn mixin_constructor_intersection_composes_instance_types() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let any = prim(&d, PrimitiveKind::Any);
    let mixin_instance = d.graph().intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("mixin".into()),
    ));
    let base_instance = d.graph().intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("base".into()),
    ));
    let any_array = d.graph().intern_node(SemanticNodeData::Array {
        element: any,
        readonly: false,
    });
    let mixin = signature(
        &d,
        "Mixin",
        0,
        SignatureKind::Construct,
        vec![FunctionParam::synthetic(
            Some(Arc::from("args")),
            any_array,
            false,
            true,
        )],
        vec![],
        mixin_instance,
    );
    let base = signature(
        &d,
        "Base",
        0,
        SignatureKind::Construct,
        vec![param(string)],
        vec![],
        base_instance,
    );
    let members = [
        callable(&d, vec![], vec![base]),
        callable(&d, vec![], vec![mixin]),
    ];
    let intersection = d.graph().intern_node(SemanticNodeData::Intersection(
        crate::semantic_query::composite::CompositeList::test_fixture(Arc::from(
            members.to_vec().into_boxed_slice(),
        )),
    ));
    let store = d.graph().signature_store();
    let list = candidates(
        ready(discover(&d, intersection, SignatureKind::Construct)),
        store,
    );
    assert_eq!(list.len(), 2, "each member's constructor is composed");
    assert!(matches!(
        recipe_of(store, list[0]),
        SignatureResultRecipe::IntersectionConstruct { .. }
    ));
    let call = identity_substitution(store, list[0]);
    let node = read_return(&d, list[0], call).expect("constructor result reads");
    let expected =
        d.intern_normalized_union_or_intersection(&[base_instance, mixin_instance], false);
    assert_eq!(
        node, expected,
        "own instance first, then the mixin instance"
    );
}

fn signature_set_of(
    d: &ProjectSemanticDispatch<'_>,
    subject: SemanticNodeId,
    context: SemanticContextId,
) -> SignatureSetRef {
    use crate::semantic_query::{
        QueryResult, SemanticQueryApi, SemanticQueryKey, SemanticQueryValue,
    };
    match d.execute(SemanticQueryKey::SignaturesOfType {
        subject,
        kind: SignatureKind::Call,
        context,
    }) {
        QueryResult::Value(output) => match output.value {
            SemanticQueryValue::SignatureSet(value) => value.set,
            other => panic!("expected a signature set, got {other:?}"),
        },
        other => panic!("expected a value, got {other:?}"),
    }
}

/// The query rides the family memo: an identical demand is warm, and two
/// semantic contexts (here differing only in `strictNullChecks`) are two
/// families that never serve one another.
#[test]
fn signatures_of_type_do_not_warm_hit_across_semantic_contexts() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let sig = signature(
        &d,
        "warm",
        0,
        SignatureKind::Call,
        vec![FunctionParam::synthetic(None, string, true, false)],
        vec![],
        string,
    );
    let store = d.graph().signature_store();
    let production = SemanticContextId::production();
    let mut options = SemanticContext::production();
    options.effective_semantic_options.strict_null_checks = false;
    let loose = options.intern();

    let before = d.graph().stats_snapshot();
    let first = signature_set_of(&d, sig, production);
    let after_cold = d.graph().stats_snapshot();
    assert_eq!(after_cold.misses, before.misses + 1);
    let second = signature_set_of(&d, sig, production);
    let after_warm = d.graph().stats_snapshot();
    assert_eq!(
        first, second,
        "an identical demand reuses the published set"
    );
    assert_eq!(
        after_warm.hits,
        after_cold.hits + 1,
        "identical demand is a warm hit"
    );
    assert_eq!(after_warm.misses, after_cold.misses);

    let other = signature_set_of(&d, sig, loose);
    let after_other = d.graph().stats_snapshot();
    assert_eq!(
        after_other.misses,
        after_warm.misses + 1,
        "a different semantic context is a cold build, never a warm hit"
    );
    assert_ne!(
        first, other,
        "the effective options are part of the candidate's shape"
    );
    let strict_first = candidates(first, store).remove(0);
    let loose_first = candidates(other, store).remove(0);
    assert_ne!(strict_first.signature, loose_first.signature);
}

/// Two demands on one descriptor that differ only in projection are two
/// keys and two cold builds; an identical demand is warm.
#[test]
fn read_signature_result_distinct_demands_do_not_alias() {
    use crate::semantic_query::{QueryResult, SemanticQueryApi, SemanticQueryKey};
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let sig = signature(
        &d,
        "demand",
        0,
        SignatureKind::Call,
        vec![param(string)],
        vec![],
        string,
    );
    let store = d.graph().signature_store();
    let candidate = candidates(ready(discover(&d, sig, SignatureKind::Call)), store).remove(0);
    let call = identity_substitution(store, candidate);
    let key = |projection| crate::signature_kernel::ReadSignatureResultKey {
        descriptor: candidate.signature,
        call_substitution: call,
        projection,
        evaluation: CONTEXT_FREE_EVALUATION,
        semantic_context: SemanticContextId::production(),
    };
    assert_ne!(key(ResultDemand::Return), key(ResultDemand::Both));
    let run = |projection| {
        let result = d.execute(SemanticQueryKey::ReadSignatureResult(key(projection)));
        assert!(matches!(result, QueryResult::Value(_)), "got {result:?}");
    };
    let before = d.graph().stats_snapshot();
    run(ResultDemand::Return);
    run(ResultDemand::Both);
    let cold = d.graph().stats_snapshot();
    assert_eq!(cold.misses, before.misses + 2);
    run(ResultDemand::Return);
    let warm = d.graph().stats_snapshot();
    assert_eq!(warm.hits, cold.hits + 1);
    assert_eq!(warm.misses, cold.misses);
}

fn generic_identity(
    d: &ProjectSemanticDispatch<'_>,
    name: &str,
    constraint: Option<SemanticNodeId>,
) -> SemanticNodeId {
    let t = d.graph().intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic(name),
        param_index: 0,
        constraint,
        default: None,
        display_name: Arc::from(name),
    });
    signature(
        d,
        name,
        0,
        SignatureKind::Call,
        vec![param(t)],
        vec![TypeParamDecl {
            name: Arc::from(name),
            param: t,
            constraint,
            default: None,
            is_const: false,
        }],
        t,
    )
}

/// Generic signatures union only on an EXACT match under the positional
/// binder correspondence: `<T>(x: T) => T` and `<U>(x: U) => U` are one
/// candidate; a differently constrained binder is not the same signature.
#[test]
fn generic_union_members_match_exactly_under_binder_correspondence() {
    let host = host();
    let d = ProjectSemanticDispatch::new(host.as_ref());
    let string = prim(&d, PrimitiveKind::String);
    let store = d.graph().signature_store();

    let t = generic_identity(&d, "T", None);
    let u = generic_identity(&d, "U", None);
    let same = d.intern_normalized_union_or_intersection(
        &[callable(&d, vec![t], vec![]), callable(&d, vec![u], vec![])],
        true,
    );
    let list = candidates(ready(discover(&d, same, SignatureKind::Call)), store);
    assert_eq!(list.len(), 1);
    assert!(matches!(
        recipe_of(store, list[0]),
        SignatureResultRecipe::UnionCommon { .. }
    ));

    let constrained = generic_identity(&d, "S", Some(string));
    let differ = d.intern_normalized_union_or_intersection(
        &[
            callable(&d, vec![t], vec![]),
            callable(&d, vec![constrained], vec![]),
        ],
        true,
    );
    assert_eq!(
        ready(discover(&d, differ, SignatureKind::Call)),
        SignatureSetRef::Empty,
        "non-identical generic binders neither match nor synthesize"
    );
}

const AMBIENT_LIB: &str = r#"
interface String {
  (radix: number): boolean;
  readonly length: number;
}
interface Number {
  toFixed(): string;
}
"#;

fn project_config(root: &str) -> verter_workspace::VfsProjectConfig {
    verter_workspace::VfsProjectConfig {
        root: root.to_string(),
        rank: verter_workspace::ProjectRank::Explicit,
        tsconfig_path: Some(format!("{root}/tsconfig.json")),
        root_files: vec![],
        extensions: vec![".ts".into(), ".d.ts".into()],
        workspace_root: root.to_string(),
        workspace_aliases: vec![],
        compiler_options: verter_semantic::resolver_core::IdeProjectCompilerOptions::default(),
        references: vec![],
        membership: verter_workspace::configured_membership_match_all_under_root(
            &verter_workspace::CanonicalPath::new(root),
        ),
    }
}

fn upsert_ts(host: &VerterHost, canonical: &str, source: &str, ambient: bool) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: if ambient {
                None
            } else {
                Some(canonical.to_string())
            },
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("the fixture serves");
}

fn project_host(ambient_lib: Option<&str>) -> Arc<VerterHost> {
    let workspace = Arc::new(verter_workspace::MemoryWorkspace::new(
        verter_workspace::MemoryOptions::default(),
    ));
    workspace.set_project_graph(verter_workspace::ProjectGraph::from_configs(vec![
        project_config("/ws"),
    ]));
    let mut virtual_id = None;
    if let Some(lib) = ambient_lib {
        verter_workspace::WorkspaceAccess::register_ambient_lib(
            workspace.as_ref(),
            verter_workspace::AmbientLibSpec {
                project_id: Some(verter_workspace::workspace_snapshot::ProjectId(0)),
                canonical_id: Arc::from("lib.d.ts"),
                source: Arc::from(lib),
            },
        )
        .expect("the ambient corpus registers");
        let key = verter_workspace::WorkspaceRead::project_stable_key(
            workspace.as_ref(),
            verter_workspace::workspace_snapshot::ProjectId(0),
        )
        .expect("project key");
        virtual_id = Some(verter_workspace::ambient_virtual_canonical_id(
            key, "lib.d.ts",
        ));
    }
    let access: Arc<dyn verter_workspace::WorkspaceAccess> = workspace;
    let host = Arc::new(VerterHost::new(HostConfig::default(), access));
    if let (Some(id), Some(lib)) = (virtual_id, ambient_lib) {
        upsert_ts(&host, &id, lib, true);
    }
    upsert_ts(&host, "/ws/main.ts", "export const w = 1;\n", false);
    host
}

/// Apparent primitives read the resolved global population: a call
/// signature merged into the global `String` is a `string` candidate, a
/// wrapper that declares none is a complete negative, and an absent
/// population (`noLib`) is the checker's empty apparent type — never a
/// fabricated signature and never an incomplete outcome.
#[test]
fn apparent_primitives_read_the_resolved_global_population() {
    let with_lib = project_host(Some(AMBIENT_LIB));
    let store_view = with_lib.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::new(&with_lib, &store_view, overlay);
    let d = ProjectSemanticDispatch::new(&ctx);
    let _scope =
        super::LexicalDemandScopeGuard::push(&d.lexical_demand_scope, Arc::from("/ws/main.ts"));
    let store = d.graph().signature_store();
    let string = prim(&d, PrimitiveKind::String);
    let number = prim(&d, PrimitiveKind::Number);
    let literal = d.graph().intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String("x".into()),
    ));

    assert_eq!(
        count(ready(discover(&d, string, SignatureKind::Call)), store),
        1
    );
    assert_eq!(
        count(ready(discover(&d, literal, SignatureKind::Call)), store),
        1
    );
    assert_eq!(
        ready(discover(&d, string, SignatureKind::Construct)),
        SignatureSetRef::Empty
    );
    assert_eq!(
        ready(discover(&d, number, SignatureKind::Call)),
        SignatureSetRef::Empty
    );

    let without_lib = project_host(None);
    let store_view = without_lib.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let ctx = crate::resolver_core::HostResolverContext::new(&without_lib, &store_view, overlay);
    let d = ProjectSemanticDispatch::new(&ctx);
    let _scope =
        super::LexicalDemandScopeGuard::push(&d.lexical_demand_scope, Arc::from("/ws/main.ts"));
    let string = prim(&d, PrimitiveKind::String);
    assert_eq!(
        ready(discover(&d, string, SignatureKind::Call)),
        SignatureSetRef::Empty
    );

    // No request or demand scope at all: the population cannot be consulted,
    // which is an incomplete outcome and never an Empty.
    drop(_scope);
    assert_eq!(
        discover(&d, string, SignatureKind::Call),
        QueryOutcome::Incomplete(IncompleteReason::UnsettledInput)
    );
}

/// The logical identity of a candidate — binder-space key, provenance group,
/// parent, ordinals and locator — is a function of the authored program, not
/// of which nodes happened to be interned first.
#[test]
fn candidate_identity_is_independent_of_intern_order() {
    let observe = |warm_first: bool| {
        let host = host();
        let d = ProjectSemanticDispatch::new(host.as_ref());
        if warm_first {
            // Unrelated nodes and signatures interned before the subject.
            for kind in [
                PrimitiveKind::Boolean,
                PrimitiveKind::Symbol,
                PrimitiveKind::BigInt,
            ] {
                let t = prim(&d, kind);
                let _ = signature(
                    &d,
                    "noise",
                    7,
                    SignatureKind::Call,
                    vec![param(t)],
                    vec![],
                    t,
                );
            }
        }
        let string = prim(&d, PrimitiveKind::String);
        let number = prim(&d, PrimitiveKind::Number);
        let a = signature(
            &d,
            "stable",
            0,
            SignatureKind::Call,
            vec![param(string)],
            vec![],
            number,
        );
        let b = signature(
            &d,
            "stable",
            1,
            SignatureKind::Call,
            vec![param(number)],
            vec![],
            string,
        );
        let object = callable(&d, vec![a, b], vec![]);
        let store = d.graph().signature_store();
        let list = candidates(ready(discover(&d, object, SignatureKind::Call)), store);
        let view = SemanticReadView::pin(store);
        list.iter()
            .map(|c| {
                let descriptor = view.descriptor(c.signature).unwrap();
                let space = view.space(descriptor.residual_binders).unwrap();
                let provenance = view.provenance(c.provenance).unwrap();
                (
                    space.key,
                    provenance.declaration_group,
                    provenance.declaration_parent,
                    provenance.source_ordinal,
                    provenance.overload_ordinal,
                    provenance.source_locator,
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(observe(false), observe(true));
}
