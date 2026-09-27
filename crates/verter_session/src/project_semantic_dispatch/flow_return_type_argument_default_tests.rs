//! Type-parameter DEFAULTS that name an earlier type parameter, in calls
//! and constructions: the checker instantiates such a default with the
//! earlier parameter's INFERRED type — after literal widening — so the
//! default follows the widened inference, never the fresh literal.
//!
//! Every expected answer was measured on the pinned TypeScript 7.0.2
//! checker (`tsc --declaration --emitDeclarationOnly --strict`) and is
//! quoted beside its row as the emitted `.d.ts` return type.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr};

const DEFAULTS: &str = "/defaults/d.ts";
const DEFAULTS_SRC: &str = r#"
declare function f2<A, B = A[]>(a: A, b?: B): [A, B];
export function callDefaultFromPrior() { return f2(1); }
export function callDefaultFromPriorString() { return f2("s"); }
export function callFreshArgOnly(a: number) { return f2(a); }
declare function f3<A, B = A>(a: A): B;
export function callDefaultIdentity() { return f3(1); }
declare function f4<A extends string, B = A[]>(a: A): B;
export function callDefaultConstrained() { return f4("x"); }
declare function f5<A, B = { v: A }>(a: A): B;
export function callDefaultObject() { return f5(true); }
declare function f6<const A, B = A[]>(a: A): B;
export function callDefaultConst() { return f6("k"); }
declare function k<A, B = A[]>(a: A): A | B;
export function callDefaultKeptTopLevel() { return k(1); }
export function callDefaultKeptTopLevelMember() { return { v: k(1) }; }
export class Ctor2<A, B = A[]> { constructor(public a: A, public b?: B) {} }
export function newDefaultFromPrior() { return new Ctor2(1); }
export function newDefaultFromPriorExplicitB() { return new Ctor2(1, ["s"]); }
export class Ctor3<A, B = A> { constructor(public a: A) {} }
export function newDefaultIdentity() { return new Ctor3("s"); }
"#;

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(DEFAULTS.to_string()),
        input_id: DEFAULTS.to_string(),
        source: Arc::from(DEFAULTS_SRC),
        file_language: crate::LanguageRegistry::global()
            .classify_static(DEFAULTS)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

/// One evaluated function's public outcome.
#[derive(Debug, PartialEq)]
struct Outcome {
    ty: TypeExpr,
    degradation: Option<FlowReturnDegradation>,
    candidates: usize,
}

#[track_caller]
fn eval(host: &Arc<VerterHost>, name: &str) -> Outcome {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(DEFAULTS),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(DEFAULTS),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => Outcome {
            ty: host
                .project_node_to_type_expr_for_test(result.return_type())
                .unwrap_or_else(|| panic!("{name}: the value did not project")),
            degradation: result.degradation(),
            candidates: dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
        },
        other => panic!("{name} must produce a value, got {other:?}"),
    }
}

#[track_caller]
fn assert_clean_warm(host: &Arc<VerterHost>, name: &str, expected: TypeExpr) {
    assert_eq!(
        eval(host, name),
        Outcome {
            ty: expected,
            degradation: None,
            candidates: 1,
        },
        "{name}"
    );
}

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

fn array(element: TypeExpr) -> TypeExpr {
    TypeExpr::Array {
        element: Arc::new(element),
        readonly: false,
    }
}

fn pair(first: TypeExpr, second: TypeExpr) -> TypeExpr {
    TypeExpr::Tuple {
        elements: [first, second]
            .into_iter()
            .map(|ty| verter_type_expr::TupleElement {
                label: None,
                ty,
                optional: false,
                rest: false,
            })
            .collect::<Vec<_>>()
            .into(),
        readonly: false,
    }
}

fn applied(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(args.into_boxed_slice()),
    }
}

/// A default naming an earlier parameter follows that parameter's WIDENED
/// inference, in a call.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// callDefaultFromPrior        f2(1)       [number, number[]]
/// callDefaultFromPriorString  f2("s")     [string, string[]]
/// callFreshArgOnly            f2(a)       [number, number[]]
/// callDefaultIdentity         f3(1)       number
/// callDefaultObject           f5(true)    { v: boolean; }
/// callDefaultConstrained      f4("x")     "x"[]
/// callDefaultConst            f6("k")     "k"[]
/// ```
///
/// `A` keeps its literal where the checker keeps it — a primitive
/// constraint (`f4`) or a `const` parameter (`f6`) — and the default
/// follows it there too.
///
/// Mutation: returning the widened substitution without re-deriving the
/// defaulted bindings answers `[number, 1[]]`, `[string, "s"[]]`, `1` and
/// `{ v: true }` for the widened rows.
#[test]
fn a_default_naming_an_earlier_parameter_follows_its_widened_inference_in_a_call() {
    let host = host();
    assert_clean_warm(
        &host,
        "callDefaultFromPrior",
        pair(
            primitive(PrimitiveName::Number),
            array(primitive(PrimitiveName::Number)),
        ),
    );
    assert_clean_warm(
        &host,
        "callDefaultFromPriorString",
        pair(
            primitive(PrimitiveName::String),
            array(primitive(PrimitiveName::String)),
        ),
    );
    assert_clean_warm(
        &host,
        "callFreshArgOnly",
        pair(
            primitive(PrimitiveName::Number),
            array(primitive(PrimitiveName::Number)),
        ),
    );
    assert_clean_warm(
        &host,
        "callDefaultIdentity",
        primitive(PrimitiveName::Number),
    );
    let object = eval(&host, "callDefaultObject");
    assert_eq!((object.degradation, object.candidates), (None, 1));
    let TypeExpr::Object(shape) = &object.ty else {
        panic!("callDefaultObject must be an object, got {object:?}");
    };
    assert!(
        matches!(
            shape.properties.as_slice(),
            [verter_type_expr::ObjectMember::Property(property)]
                if property.key.as_string() == Some("v")
                    && property.ty == primitive(PrimitiveName::Boolean)
        ),
        "callDefaultObject is `{{ v: boolean }}`, got {object:?}"
    );
    assert_clean_warm(
        &host,
        "callDefaultConstrained",
        array(TypeExpr::Literal(LiteralValue::String("x".to_string()))),
    );
    assert_clean_warm(
        &host,
        "callDefaultConst",
        array(TypeExpr::Literal(LiteralValue::String("k".to_string()))),
    );
}

/// Where the earlier parameter's literal is KEPT (it stands at top level of
/// the return), the default keeps it too.
///
/// TypeScript 7.0.2: `callDefaultKeptTopLevel` is `1 | 1[]`, and the
/// member read `{ v: k(1) }` widens the kept literal alone: `{ v: number
/// | 1[]; }`.
#[test]
fn a_default_follows_a_kept_literal_inference() {
    let host = host();
    let kept = eval(&host, "callDefaultKeptTopLevel");
    assert_eq!((kept.degradation, kept.candidates), (None, 1));
    let one = TypeExpr::Literal(LiteralValue::Number(1.0));
    let TypeExpr::Union(members) = &kept.ty else {
        panic!("callDefaultKeptTopLevel must be a union, got {kept:?}");
    };
    assert_eq!(members.len(), 2, "{kept:?}");
    assert!(members.contains(&one) && members.contains(&array(one.clone())));
    let member = eval(&host, "callDefaultKeptTopLevelMember");
    let TypeExpr::Object(shape) = &member.ty else {
        panic!("callDefaultKeptTopLevelMember must be an object, got {member:?}");
    };
    let [verter_type_expr::ObjectMember::Property(property)] = shape.properties.as_slice() else {
        panic!("one member expected, got {member:?}");
    };
    let TypeExpr::Union(members) = &property.ty else {
        panic!("`v` must be a union, got {member:?}");
    };
    assert_eq!(members.len(), 2, "{member:?}");
    assert!(
        members.contains(&primitive(PrimitiveName::Number)) && members.contains(&array(one)),
        "`v` is `number | 1[]`, got {member:?}"
    );
}

/// The same rule in a construction: a generic class's default names an
/// earlier class parameter.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// newDefaultFromPrior           new Ctor2(1)          Ctor2<number, number[]>
/// newDefaultFromPriorExplicitB  new Ctor2(1, ["s"])   Ctor2<number, string[]>
/// newDefaultIdentity            new Ctor3("s")        Ctor3<string, string>
/// ```
///
/// Mutation: the same as the call rows — without the re-derivation the
/// defaulted argument keeps the fresh literal (`Ctor2<number, 1[]>`,
/// `Ctor3<string, "s">`).
#[test]
fn a_default_naming_an_earlier_parameter_follows_its_widened_inference_in_a_construction() {
    let host = host();
    assert_clean_warm(
        &host,
        "newDefaultFromPrior",
        applied(
            "Ctor2",
            vec![
                primitive(PrimitiveName::Number),
                array(primitive(PrimitiveName::Number)),
            ],
        ),
    );
    assert_clean_warm(
        &host,
        "newDefaultFromPriorExplicitB",
        applied(
            "Ctor2",
            vec![
                primitive(PrimitiveName::Number),
                array(primitive(PrimitiveName::String)),
            ],
        ),
    );
    assert_clean_warm(
        &host,
        "newDefaultIdentity",
        applied(
            "Ctor3",
            vec![
                primitive(PrimitiveName::String),
                primitive(PrimitiveName::String),
            ],
        ),
    );
}
