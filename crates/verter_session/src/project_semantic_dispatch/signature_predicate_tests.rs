//! Type-predicate and assertion signatures, end to end: the lowered
//! signature carries TypeScript's `TypePredicate` beside a `boolean` /
//! `void` return, the kernel's declared result exposes it as the effect
//! half of a result read, instantiation substitutes it, the relation
//! relates it, and display prints it.
//!
//! Every expected answer below is measured on the pinned TypeScript 7.0.2
//! (`tsc --declaration --emitDeclarationOnly --strict`).

use std::sync::Arc;

use super::call_resolve_tests::occurrence;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    FunctionParam, LiteralValue, PredicateSubject, PrimitiveKind, ProjectionMode,
    ProjectionReductionContext, QueryOutcome, Ready, SemanticContextId, SemanticNodeData,
    SemanticNodeId, SignatureKind, SignaturePredicate, SignatureReturnCarrier, TypeParamDecl,
    CONTEXT_FREE_EVALUATION,
};
use crate::signature_kernel::{
    BorrowedSet, CallSubstitution, PredicateEffect, ResultDemand, SemanticReadView,
    SignatureCandidate, SignatureStore,
};

const PROBE_FILE: &str = "/wb/signature_predicate_probe.ts";

/// Answer `probe` in TYPE position over a module of `source` through the
/// public audited flow-return boundary, reduce it to the altitude the
/// checker prints (named declarations kept by name) and hand the reduced
/// node to `read` — the signature corpus's own observation lane.
fn with_probe<R>(
    source: &str,
    probe: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::make_audit_host;
    let host = make_audit_host();
    let module = format!(
        "{source}\nexport function __predicate_probe() {{ \
            const __probe: {probe} = null as any; \
            return __probe; \
        }}\n"
    );
    crate::u6_flow_shape_corpus_tests::upsert(
        &host,
        PROBE_FILE,
        &crate::u6_flow_shape_corpus_tests::module_script(&module),
        crate::FileLanguage::script_ts(),
    );
    let identity = verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(PROBE_FILE),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from("__predicate_probe"),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    };
    let carrier = host.get_flow_return_type_with_audit(
        &identity,
        crate::semantic_query::ReturnProjectionDemand::whole_return(),
    );
    let result = carrier
        .as_result()
        .unwrap_or_else(|_| panic!("the probe `{probe}` produced no flow-return result"));
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let node = dispatch
        .normalize_node_keeping_declaration_refs_for_tests(
            result.return_type(),
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
        .unwrap_or_else(|| panic!("the probe `{probe}` reduced to a partial demand"));
    read(&dispatch, node)
}

fn describe(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> String {
    crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(dispatch, node, 0)
}

const PREDICATES: &str = "interface Foo { kind: 'foo'; n: number }\n\
interface Bar { kind: 'bar' }\n\
export function isFoo(x: unknown): x is Foo { return true; }\n\
export function assertBar(x: unknown): asserts x is Bar { }\n\
export function assertTruthy(x: unknown): asserts x { }\n\
export const isT = <T>(x: unknown): x is T => true;\n\
export class Box {\n\
  isFoo(): this is Foo { return true; }\n\
  assertFoo(): asserts this is Foo { }\n\
}";

/// `ReturnType` reads the checker's return of a predicate signature:
/// `boolean` for a type predicate and `void` for an assertion, through
/// function declarations, a targetless assertion, a generic instantiation
/// expression, and receiver predicates on class methods. Measured on
/// 7.0.2: `boolean`, `void`, `void`, `boolean`, `boolean`, `void`.
#[test]
fn return_type_of_a_predicate_signature_is_boolean_and_of_an_assertion_void() {
    for (probe, expected) in [
        ("ReturnType<typeof isFoo>", PrimitiveKind::Boolean),
        ("ReturnType<typeof assertBar>", PrimitiveKind::Void),
        ("ReturnType<typeof assertTruthy>", PrimitiveKind::Void),
        ("ReturnType<typeof isT<string>>", PrimitiveKind::Boolean),
        ("ReturnType<Box['isFoo']>", PrimitiveKind::Boolean),
        ("ReturnType<Box['assertFoo']>", PrimitiveKind::Void),
    ] {
        with_probe(PREDICATES, probe, |dispatch, node| {
            assert_eq!(
                dispatch.graph().node_data(node).as_deref(),
                Some(&SemanticNodeData::Primitive(expected)),
                "`{probe}` is `{expected:?}` on 7.0.2; measured `{}`",
                describe(dispatch, node)
            );
        });
    }
}

/// The lone call signature of a callable surface.
fn only_call_signature(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Object(surface)) => match surface.call_signatures.as_ref() {
            [signature] => *signature,
            other => panic!("expected one call signature, got {other:?}"),
        },
        Some(SemanticNodeData::Signature { .. }) => node,
        other => panic!("expected a callable, got {other:?}"),
    }
}

fn candidates(
    store: &SignatureStore,
    set: crate::signature_kernel::SignatureSetRef,
) -> Vec<SignatureCandidate> {
    match SemanticReadView::pin(store)
        .read_set(set)
        .expect("live set")
    {
        BorrowedSet::Empty => Vec::new(),
        BorrowedSet::One { candidate, .. } => vec![candidate],
        BorrowedSet::Many(list) => list.to_vec(),
    }
}

/// The effect half of one candidate's result read under `call`.
fn read_effects(
    dispatch: &ProjectSemanticDispatch<'_>,
    candidate: SignatureCandidate,
    call: crate::signature_kernel::CallSubstitutionId,
) -> Option<PredicateEffect> {
    let store = dispatch.graph().signature_store();
    match dispatch.read_signature_result(
        store,
        candidate.signature,
        call,
        ResultDemand::Effects,
        CONTEXT_FREE_EVALUATION,
        SemanticContextId::production(),
    ) {
        QueryOutcome::Ready(Ready { value, .. }) => {
            store.applied_result(value).expect("applied result").effects
        }
        QueryOutcome::Incomplete(reason) => panic!("an effects read is incomplete: {reason:?}"),
    }
}

fn identity_call(
    store: &SignatureStore,
    candidate: SignatureCandidate,
) -> crate::signature_kernel::CallSubstitutionId {
    let space = SemanticReadView::pin(store)
        .descriptor(candidate.signature)
        .expect("live descriptor")
        .residual_binders;
    store
        .intern_substitution(CallSubstitution::identity(space), None)
        .expect("identity substitution")
}

/// `typeof isT<string>` over `const isT = <T>(x: unknown): x is T => …`
/// instantiates the predicate with the signature: 7.0.2 prints `isT<string>`
/// as `(x: unknown) => x is string`. The kernel's effects read of that
/// signature reports the same predicate about parameter 0, and display
/// prints it as the checker does.
#[test]
fn an_instantiated_generic_predicate_narrows_to_its_argument() {
    with_probe(PREDICATES, "typeof isT<string>", |dispatch, node| {
        let signature = only_call_signature(dispatch, node);
        let graph = dispatch.graph();
        let is_string = |node: SemanticNodeId| {
            graph.node_data(node).as_deref()
                == Some(&SemanticNodeData::Primitive(PrimitiveKind::String))
        };
        match graph.node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                return_type,
                predicate,
                ..
            }) => {
                assert_eq!(
                    graph.node_data(*return_type).as_deref(),
                    Some(&SemanticNodeData::Primitive(PrimitiveKind::Boolean))
                );
                let predicate = predicate.unwrap_or_else(|| {
                    panic!("no predicate; measured `{}`", describe(dispatch, node))
                });
                assert_eq!(
                    (predicate.subject, predicate.asserts),
                    (PredicateSubject::Parameter(0), false)
                );
                assert!(
                    predicate.ty.is_some_and(is_string),
                    "the predicate target instantiates to `string`; measured `{}`",
                    describe(dispatch, node)
                );
            }
            other => panic!("expected the instantiated signature, got {other:?}"),
        }

        let store = graph.signature_store();
        let set = match dispatch.signatures_of_type(
            store,
            signature,
            SignatureKind::Call,
            SemanticContextId::production(),
        ) {
            QueryOutcome::Ready(Ready { value, .. }) => value,
            QueryOutcome::Incomplete(reason) => panic!("discovery is incomplete: {reason:?}"),
        };
        let [candidate] = candidates(store, set)[..] else {
            panic!("one call signature");
        };
        let effect = read_effects(dispatch, candidate, identity_call(store, candidate))
            .expect("the declared predicate is the result's effect");
        assert_eq!(effect.subject, PredicateSubject::Parameter(0));
        assert!(!effect.asserts);
        assert!(
            effect
                .ty
                .map(|token| store.type_token_node(token).expect("live token"))
                .is_some_and(is_string),
            "the effect target is the instantiated `string`"
        );

        let shown = crate::semantic_query::display::display(
            graph,
            &crate::semantic_query::SemanticQueryValue::TypeNode(signature),
            crate::semantic_query::demand::DisplayNeeds::empty(),
        );
        assert_eq!(shown.to_string(), "(x: unknown) => x is string");
    });
}

/// A generic predicate signature published straight to the kernel: the
/// declared recipe carries the predicate, and an effects read composes the
/// declaration and call maps over its target exactly like the return —
/// `x is T` read under `T := number` is `x is number`, and under the
/// identity map it stays the authored binder. A targetless assertion keeps
/// `asserts` with no target.
#[test]
fn kernel_effects_read_the_declared_predicate_under_the_call_map() {
    let host = Arc::new(crate::VerterHost::new_standalone(
        crate::HostConfig::default(),
    ));
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let prim = |kind| graph.intern_node(SemanticNodeData::Primitive(kind));
    let (unknown, boolean, number, void) = (
        prim(PrimitiveKind::Unknown),
        prim(PrimitiveKind::Boolean),
        prim(PrimitiveKind::Number),
        prim(PrimitiveKind::Void),
    );
    let t = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("isT"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    });
    let signature = |name: &str, type_parameters: Vec<TypeParamDecl>, ret, predicate| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: SignatureKind::Call,
            params: Arc::from(
                vec![FunctionParam::synthetic(
                    Some(Arc::from("x")),
                    unknown,
                    false,
                    false,
                )]
                .into_boxed_slice(),
            ),
            return_type: ret,
            type_parameters: Arc::from(type_parameters.into_boxed_slice()),
            occurrence: Some(occurrence(name, 0)),
            return_carrier: SignatureReturnCarrier::Declared(ret),
            signature_span: None,
            return_type_span: None,
            predicate: Some(predicate),
            is_abstract: false,
        })
    };
    let store = graph.signature_store();
    let discover = |node| match dispatch.signatures_of_type(
        store,
        node,
        SignatureKind::Call,
        SemanticContextId::production(),
    ) {
        QueryOutcome::Ready(Ready { value, .. }) => candidates(store, value),
        QueryOutcome::Incomplete(reason) => panic!("discovery is incomplete: {reason:?}"),
    };

    let generic = signature(
        "isT",
        vec![TypeParamDecl {
            name: Arc::from("T"),
            param: t,
            constraint: None,
            default: None,
            is_const: false,
        }],
        boolean,
        SignaturePredicate {
            subject: PredicateSubject::Parameter(0),
            asserts: false,
            ty: Some(t),
        },
    );
    let [candidate] = discover(generic)[..] else {
        panic!("one call signature");
    };
    let space = SemanticReadView::pin(store)
        .descriptor(candidate.signature)
        .unwrap()
        .residual_binders;
    let token = store.binder_token_for(space, 0).unwrap();
    let mapped = store
        .intern_substitution(
            CallSubstitution::map(
                space,
                crate::semantic_query::CanonicalTypeSubstitution::new(vec![(token, number)]),
            ),
            None,
        )
        .unwrap();
    let effect = read_effects(&dispatch, candidate, mapped).expect("declared predicate");
    assert_eq!(
        (effect.subject, effect.asserts),
        (PredicateSubject::Parameter(0), false)
    );
    assert_eq!(
        effect.ty.map(|token| store.type_token_node(token).unwrap()),
        Some(number),
        "the call map instantiates the predicate target"
    );
    let open = read_effects(&dispatch, candidate, identity_call(store, candidate))
        .expect("declared predicate");
    assert_eq!(
        open.ty.map(|token| store.type_token_node(token).unwrap()),
        Some(t),
        "an unmapped binder is the authored binder, not a token"
    );

    let assertion = signature(
        "assertTruthy",
        Vec::new(),
        void,
        SignaturePredicate {
            subject: PredicateSubject::Parameter(0),
            asserts: true,
            ty: None,
        },
    );
    let [candidate] = discover(assertion)[..] else {
        panic!("one call signature");
    };
    assert_eq!(
        read_effects(&dispatch, candidate, identity_call(store, candidate)),
        Some(PredicateEffect {
            subject: PredicateSubject::Parameter(0),
            asserts: true,
            ty: None,
        })
    );
}

/// A target carrying a TYPE predicate relates predicates, not returns.
/// Measured on 7.0.2 (each `A extends B ? 1 : 0`):
/// `(x: unknown) => boolean` against `(x: unknown) => x is string` is `0`;
/// `(x: unknown) => x is string` against `(x: unknown) => boolean` is `1`;
/// against `(x: unknown) => x is string | number` is `1`;
/// `(x: unknown) => x is number` against `(x: unknown) => x is string` is
/// `0`; a predicate about the OTHER parameter is `0`; an assertion source
/// against a type predicate is `0`; and `typeof isFoo extends (x: any) => x
/// is infer U ? U : never` infers `Foo`.
#[test]
fn a_type_predicate_target_relates_predicates_not_returns() {
    for (probe, expected) in [
        (
            "((x: unknown) => boolean) extends ((x: unknown) => x is string) ? 1 : 0",
            0.0,
        ),
        (
            "((x: unknown) => x is string) extends ((x: unknown) => boolean) ? 1 : 0",
            1.0,
        ),
        (
            "((x: unknown) => x is string) extends ((x: unknown) => x is string | number) ? 1 : 0",
            1.0,
        ),
        (
            "((x: unknown) => x is number) extends ((x: unknown) => x is string) ? 1 : 0",
            0.0,
        ),
        (
            "((x: unknown, y: unknown) => y is string) extends ((x: unknown, y: unknown) => x is string) ? 1 : 0",
            0.0,
        ),
        (
            "((x: unknown) => asserts x is string) extends ((x: unknown) => x is string) ? 1 : 0",
            0.0,
        ),
    ] {
        with_probe(PREDICATES, probe, |dispatch, node| {
            assert_eq!(
                dispatch.graph().node_data(node).as_deref(),
                Some(&SemanticNodeData::Literal(LiteralValue::Number(expected))),
                "`{probe}` is `{expected}` on 7.0.2; measured `{}`",
                describe(dispatch, node)
            );
        });
    }
    with_probe(
        PREDICATES,
        "typeof isFoo extends (x: any) => x is infer U ? U : never",
        |dispatch, node| match dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::DeclRef { identity }) => {
                assert_eq!(identity.decl_name.as_ref(), "Foo");
            }
            other => panic!(
                "`x is infer U` infers `Foo` on 7.0.2; measured {other:?} (`{}`)",
                describe(dispatch, node)
            ),
        },
    );
}

/// A target signature whose result is exactly `void` or `any` accepts any
/// source result: neither the source's return nor its predicate is
/// compared, while the parameters still are. Measured on 7.0.2 (each
/// `A extends B ? 1 : 0`): `() => number` against `() => void`, `() => any`,
/// `() => unknown`, an alias of `void` and an alias of `any` is `1`;
/// against `() => undefined` and `() => void | undefined` it is `0`; a type
/// guard and a plain `boolean` function against an assertion are `1`; a
/// plain `boolean` function against a type guard stays `0`; a parameter
/// mismatch against a `void` result stays `0`; a construct signature, an
/// async function, an object's call signature, `never` and `undefined`
/// results against `void` are `1`; and `() => void` against
/// `() => number` is `0`.
#[test]
fn a_void_or_any_target_result_accepts_any_source_result() {
    const ALIASES: &str = "type V = void;\ntype A = any;\ntype VU = void | undefined;";
    for (probe, expected) in [
        ("(() => number) extends (() => void) ? 1 : 0", 1.0),
        ("(() => number) extends (() => any) ? 1 : 0", 1.0),
        ("(() => number) extends (() => unknown) ? 1 : 0", 1.0),
        ("(() => number) extends (() => V) ? 1 : 0", 1.0),
        ("(() => number) extends (() => A) ? 1 : 0", 1.0),
        ("(() => number) extends (() => undefined) ? 1 : 0", 0.0),
        (
            "(() => number) extends (() => void | undefined) ? 1 : 0",
            0.0,
        ),
        ("(() => number) extends (() => VU) ? 1 : 0", 0.0),
        (
            "((x: unknown) => x is string) extends ((x: unknown) => asserts x is string) ? 1 : 0",
            1.0,
        ),
        (
            "((x: unknown) => boolean) extends ((x: unknown) => asserts x is string) ? 1 : 0",
            1.0,
        ),
        (
            "((x: unknown) => boolean) extends ((x: unknown) => x is string) ? 1 : 0",
            0.0,
        ),
        (
            "((x: string) => number) extends ((x: number) => void) ? 1 : 0",
            0.0,
        ),
        (
            "((x: number) => number) extends ((x: number) => void) ? 1 : 0",
            1.0,
        ),
        ("(new () => { a: 1 }) extends (new () => void) ? 1 : 0", 1.0),
        ("(() => Promise<number>) extends (() => void) ? 1 : 0", 1.0),
        ("{ (): number } extends { (): void } ? 1 : 0", 1.0),
        ("(() => never) extends (() => void) ? 1 : 0", 1.0),
        ("(() => undefined) extends (() => void) ? 1 : 0", 1.0),
        ("(() => void) extends (() => number) ? 1 : 0", 0.0),
    ] {
        with_probe(ALIASES, probe, |dispatch, node| {
            assert_eq!(
                dispatch.graph().node_data(node).as_deref(),
                Some(&SemanticNodeData::Literal(LiteralValue::Number(expected))),
                "`{probe}` is `{expected}` on 7.0.2; measured `{}`",
                describe(dispatch, node)
            );
        });
    }
}

/// The predicate is part of a signature's identity: a type predicate, an
/// assertion, a receiver predicate and a predicate-less `boolean` signature
/// over the same parameters intern apart and key apart under
/// `VerterStableV1`, and display prints each the way 7.0.2 does.
#[test]
fn predicates_participate_in_identity_ordering_and_display() {
    let host = Arc::new(crate::VerterHost::new_standalone(
        crate::HostConfig::default(),
    ));
    let dispatch = ProjectSemanticDispatch::new(host.as_ref());
    let graph = dispatch.graph();
    let prim = |kind| graph.intern_node(SemanticNodeData::Primitive(kind));
    let (unknown, boolean, void, string) = (
        prim(PrimitiveKind::Unknown),
        prim(PrimitiveKind::Boolean),
        prim(PrimitiveKind::Void),
        prim(PrimitiveKind::String),
    );
    let signature = |ret, predicate| {
        graph.intern_node(SemanticNodeData::Signature {
            kind: SignatureKind::Call,
            params: Arc::from(
                vec![FunctionParam::synthetic(
                    Some(Arc::from("x")),
                    unknown,
                    false,
                    false,
                )]
                .into_boxed_slice(),
            ),
            return_type: ret,
            type_parameters: Arc::from(Vec::new().into_boxed_slice()),
            occurrence: None,
            return_carrier: SignatureReturnCarrier::Declared(ret),
            signature_span: None,
            return_type_span: None,
            predicate,
            is_abstract: false,
        })
    };
    let predicate = |subject, asserts, ty| {
        Some(SignaturePredicate {
            subject,
            asserts,
            ty,
        })
    };
    let x = PredicateSubject::Parameter(0);
    let cases = [
        (signature(boolean, None), "(x: unknown) => boolean"),
        (
            signature(boolean, predicate(x, false, Some(string))),
            "(x: unknown) => x is string",
        ),
        (
            signature(void, predicate(x, true, Some(string))),
            "(x: unknown) => asserts x is string",
        ),
        (
            signature(void, predicate(x, true, None)),
            "(x: unknown) => asserts x",
        ),
        (
            signature(
                boolean,
                predicate(PredicateSubject::This, false, Some(string)),
            ),
            "(x: unknown) => this is string",
        ),
    ];
    for (index, (node, printed)) in cases.iter().enumerate() {
        let shown = crate::semantic_query::display::display(
            graph,
            &crate::semantic_query::SemanticQueryValue::TypeNode(*node),
            crate::semantic_query::demand::DisplayNeeds::empty(),
        );
        assert_eq!(shown.to_string(), *printed);
        for (other, _) in &cases[index + 1..] {
            assert_ne!(node, other, "`{printed}` interns apart");
            assert_ne!(
                crate::semantic_query::stable_key::stable_key_for_node(graph, *node),
                crate::semantic_query::stable_key::stable_key_for_node(graph, *other),
                "`{printed}` keys apart under VerterStableV1"
            );
        }
    }
}

/// A function value returned from a body keeps its authored predicate: a
/// guard factory's return is a guard. Measured on 7.0.2:
/// `ReturnType<typeof makeGuard>` is `(x: unknown) => x is Foo` and
/// `ReturnType<typeof makeAssert>` is `(v: unknown) => asserts v is Foo`.
#[test]
fn a_returned_function_value_keeps_its_authored_predicate() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    let source = "interface Foo { kind: 'foo' }\n\
export function makeGuard() { return (x: unknown): x is Foo => true; }\n\
export function makeAssert() { return function (v: unknown): asserts v is Foo { }; }";
    for (probe, printed) in [
        ("ReturnType<typeof makeGuard>", "(x: unknown) => x is Foo"),
        (
            "ReturnType<typeof makeAssert>",
            "(v: unknown) => asserts v is Foo",
        ),
    ] {
        let expected = checker_syntax::parse(printed).expect("checker print parses");
        with_probe(source, probe, |dispatch, node| {
            assert!(
                checker_syntax::matches_node(dispatch, node, &expected, 0),
                "`{probe}` is `{printed}` on 7.0.2; measured `{}`",
                describe(dispatch, node)
            );
        });
    }
}

/// Guards whose union and intersection signatures the composite-signature
/// tests read.
const COMPOSITES: &str = "interface A { a: 1 }\n\
interface B { b: 1 }\n\
declare const u: ((x: unknown) => x is A) | ((x: unknown) => x is B);\n\
declare const uf: ((x: unknown) => x is A) | ((x: unknown) => false);\n\
declare const u3: ((x: unknown) => x is A) | ((x: unknown) => x is B) | ((x: unknown) => false);\n\
declare const uu: ((x: unknown) => x is A) | ((y: unknown) => y is B);\n\
declare const ub: ((x: unknown) => x is A) | ((x: unknown) => boolean);\n\
declare const ut: ((x: unknown) => x is A) | ((x: unknown) => true);\n\
declare const uidx: ((x: unknown, y: unknown) => x is A) | ((x: unknown, y: unknown) => y is B);\n\
declare const ua: ((x: unknown) => asserts x is A) | ((x: unknown) => asserts x is B);\n\
declare const uab: ((x: unknown) => asserts x is A) | ((x: unknown) => x is B);\n\
declare const i: ((x: unknown) => x is A) & ((x: unknown) => x is B);\n\
declare const ib: ((x: unknown) => boolean) & ((x: unknown) => x is B);";

/// One shared call signature of a probe: its graph node and the effect
/// its kernel candidate reads under the identity call map.
type SharedCallSignature = (SemanticNodeId, Option<PredicateEffect>);

/// The call signatures `SignaturesOfType` shares for `probe`.
fn shared_call_signatures(
    probe: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, Vec<SharedCallSignature>),
) {
    with_probe(COMPOSITES, probe, |dispatch, node| {
        let store = dispatch.graph().signature_store();
        let set = match dispatch.signatures_of_type(
            store,
            node,
            SignatureKind::Call,
            SemanticContextId::production(),
        ) {
            QueryOutcome::Ready(Ready { value, .. }) => value,
            QueryOutcome::Incomplete(reason) => panic!("`{probe}` is incomplete: {reason:?}"),
        };
        let effects: Vec<Option<PredicateEffect>> = candidates(store, set)
            .into_iter()
            .map(|candidate| read_effects(dispatch, candidate, identity_call(store, candidate)))
            .collect();
        let nodes = match dispatch.shared_signature_nodes(node, SignatureKind::Call) {
            super::signature_discovery::SharedSignatureNodes::Nodes(nodes) => nodes,
            super::signature_discovery::SharedSignatureNodes::Incomplete(reason) => {
                panic!("`{probe}`'s signature nodes are incomplete: {reason:?}")
            }
        };
        assert_eq!(nodes.len(), effects.len());
        read(dispatch, nodes.into_iter().zip(effects).collect());
    });
}

fn shown(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> String {
    crate::semantic_query::display::display(
        dispatch.graph(),
        &crate::semantic_query::SemanticQueryValue::TypeNode(node),
        crate::semantic_query::demand::DisplayNeeds::empty(),
    )
    .to_string()
}

/// Whether a live node is the checker's `printed` type.
fn prints_as(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId, printed: &str) -> bool {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    let expected = checker_syntax::parse(printed).expect("checker print parses");
    checker_syntax::matches_node(dispatch, node, &expected, 0)
}

/// A call through a UNION of guards takes the checker's composite rule
/// (`getUnionOrIntersectionTypePredicate`), never an OR of the members:
/// every member must carry a type predicate of the same kind about the
/// same parameter, and the union signature narrows to the union of their
/// targets; a member returning exactly `false` is admitted and adds
/// nothing. Measured on 7.0.2 (`if (g(v)) return v; return null;`): `u`
/// narrows `v` to `A | B`, `uf` to `A`, a three-member union with a
/// `false` member to `A | B`, members naming the parameter differently to
/// `A | B`; a `boolean` or `true` member, predicates about different
/// parameters, assertions, and an assertion beside a predicate narrow
/// nothing (`unknown`).
#[test]
fn a_union_signature_carries_the_checker_composite_predicate() {
    for (probe, printed) in [
        ("typeof u", Some("(x: unknown) => x is A | B")),
        ("typeof uf", Some("(x: unknown) => x is A")),
        ("typeof u3", Some("(x: unknown) => x is A | B")),
        ("typeof uu", Some("(x: unknown) => x is A | B")),
        ("typeof ub", None),
        ("typeof ut", None),
        ("typeof uidx", None),
        ("typeof ua", None),
        ("typeof uab", None),
    ] {
        shared_call_signatures(probe, |dispatch, signatures| {
            let [(node, effect)] = signatures.as_slice() else {
                panic!("`{probe}` has one union signature; measured {signatures:?}");
            };
            let predicate = match dispatch.graph().node_data(*node).as_deref() {
                Some(SemanticNodeData::Signature { predicate, .. }) => *predicate,
                other => panic!("`{probe}`'s union signature is a signature node: {other:?}"),
            };
            match printed {
                Some(printed) => {
                    assert!(
                        prints_as(dispatch, *node, printed),
                        "`{probe}`'s union signature is `{printed}` on 7.0.2; measured `{}`",
                        shown(dispatch, *node)
                    );
                    let effect = effect.expect("the union signature's effect");
                    let predicate = predicate.expect("the composite node carries it");
                    assert_eq!((effect.subject, effect.asserts), (predicate.subject, false));
                    assert_eq!(
                        effect.ty.map(|token| {
                            dispatch
                                .graph()
                                .signature_store()
                                .type_token_node(token)
                                .expect("live token")
                        }),
                        predicate.ty,
                        "the effect read and the node form agree"
                    );
                }
                None => {
                    assert_eq!(
                        (predicate, *effect),
                        (None, None),
                        "`{probe}` narrows nothing on 7.0.2; measured `{}`",
                        shown(dispatch, *node)
                    );
                }
            }
        });
    }
}

/// An INTERSECTION of guards is an overload list, not a composite: its
/// signatures keep their own predicates in member order, so a call
/// resolves to the first applicable one. Measured on 7.0.2: `if (i(v))`
/// narrows `v` to `A` (not `A & B`, not `A | B`), and `if (ib(v))`, whose
/// first member returns `boolean`, narrows nothing.
#[test]
fn an_intersection_of_guards_keeps_each_overload_predicate_in_order() {
    for (probe, printed) in [
        (
            "typeof i",
            ["(x: unknown) => x is A", "(x: unknown) => x is B"],
        ),
        (
            "typeof ib",
            ["(x: unknown) => boolean", "(x: unknown) => x is B"],
        ),
    ] {
        shared_call_signatures(probe, |dispatch, signatures| {
            assert_eq!(signatures.len(), 2, "`{probe}` keeps both overloads");
            for ((node, effect), printed) in signatures.iter().zip(printed) {
                assert!(
                    prints_as(dispatch, *node, printed),
                    "`{probe}` overload is `{printed}`; measured `{}`",
                    shown(dispatch, *node)
                );
                assert_eq!(
                    effect.is_some(),
                    printed.contains(" is "),
                    "each overload's effect is its own predicate"
                );
            }
        });
    }
}

/// Conditional inference from an overloaded source reads its LAST
/// signature — the checker's `inferFromSignatures` — for a predicate as
/// for a return, while assignability still accepts any overload.
/// Measured on 7.0.2: `typeof i extends (x: any) => x is infer U ? U :
/// never` is `B`, and so is the `ib` twin whose first overload returns
/// `boolean`; `((() => A) & (() => B)) extends () => infer R ? R : never`
/// is `B`; over the union `u` inference collects one candidate per member,
/// `A | B`; `typeof i` extends both `(x: any) => x is A` and `(x: any) =>
/// x is B` (`1`).
#[test]
fn conditional_inference_reads_the_last_overload_predicate() {
    for (probe, printed) in [
        ("typeof i extends (x: any) => x is infer U ? U : never", "B"),
        (
            "typeof ib extends (x: any) => x is infer U ? U : never",
            "B",
        ),
        (
            "((() => A) & (() => B)) extends () => infer R ? R : never",
            "B",
        ),
        (
            "typeof u extends (x: any) => x is infer U ? U : never",
            "A | B",
        ),
        ("typeof i extends (x: any) => x is A ? 1 : 0", "1"),
        ("typeof i extends (x: any) => x is B ? 1 : 0", "1"),
    ] {
        with_probe(COMPOSITES, probe, |dispatch, node| {
            assert!(
                prints_as(dispatch, node, printed),
                "`{probe}` is `{printed}` on 7.0.2; measured `{}`",
                shown(dispatch, node)
            );
        });
    }
}

/// A body-derived signature's effect is the predicate the checker infers
/// from its body: the effects read forces the same body obligation the
/// return read does. Measured on 7.0.2: `function isStr(x: unknown) {
/// return typeof x === "string"; }` is `(x: unknown) => x is string`, and
/// `function multi(x: unknown) { if (x) return typeof x === "string";
/// return false; }` is `(x: unknown) => boolean`.
#[test]
fn a_body_signature_effect_is_its_inferred_predicate() {
    let source = "export function isStr(x: unknown) { return typeof x === \"string\"; }\n\
export function multi(x: unknown) { if (x) return typeof x === \"string\"; return false; }";
    for (probe, inferred) in [("typeof isStr", true), ("typeof multi", false)] {
        with_probe(source, probe, |dispatch, node| {
            let store = dispatch.graph().signature_store();
            let set = match dispatch.signatures_of_type(
                store,
                node,
                SignatureKind::Call,
                SemanticContextId::production(),
            ) {
                QueryOutcome::Ready(Ready { value, .. }) => value,
                QueryOutcome::Incomplete(reason) => panic!("`{probe}` is incomplete: {reason:?}"),
            };
            let [candidate] = candidates(store, set)[..] else {
                panic!("`{probe}` has one call signature");
            };
            let is_body = {
                let view = SemanticReadView::pin(store);
                let descriptor = view
                    .descriptor(candidate.signature)
                    .expect("live descriptor");
                let template = view.template(descriptor.template).expect("live template");
                matches!(
                    view.recipe(template.result_recipe).expect("live recipe"),
                    crate::signature_kernel::SignatureResultRecipe::Body { .. }
                )
            };
            assert!(is_body, "`{probe}` publishes a body recipe");
            let effect = read_effects(dispatch, candidate, identity_call(store, candidate));
            if inferred {
                let effect = effect.expect("the inferred predicate is the body's effect");
                assert_eq!(
                    (effect.subject, effect.asserts),
                    (PredicateSubject::Parameter(0), false)
                );
                assert_eq!(
                    effect
                        .ty
                        .and_then(|token| store.type_token_node(token).ok())
                        .and_then(|target| dispatch.graph().node_data(target).as_deref().cloned()),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                );
            } else {
                assert_eq!(effect, None, "`{probe}` infers no predicate on 7.0.2");
            }
        });
    }
}
