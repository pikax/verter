//! Accessor reads in the flow-return lane: an accessor is a PROPERTY to
//! every reader, so reading it reads its value type — the getter's return,
//! else the setter's parameter — never the accessor function.
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

const ACCESSORS: &str = "/accessors/a.ts";
const ACCESSORS_SRC: &str = r#"
export class GetBox { get label() { return "x" } }
export function readGetter(b: GetBox) { return b.label; }
export function readGetterObj(b: GetBox) { return { label: b.label, n: 1 }; }
export function readGetterNew() { const b = new GetBox(); return { label: b.label, n: 1 }; }
export class Annotated { get label(): string { return "x" } }
export function readAnnotated(b: Annotated) { return b.label; }
export class SetOnly { set v(x: number) {} }
export function readSetOnly(b: SetOnly) { return b.v; }
export class Pair { get v(): string { return "" } set v(x: string | number) {} }
export function readPair(b: Pair) { return b.v; }
export class SetFirst { set v(x: string | number) {} get v(): string { return "" } }
export function readSetFirst(b: SetFirst) { return b.v; }
export class Static { static get s() { return 1 } static set t(x: boolean) {} }
export function readStatic() { return Static.s; }
export function readStaticSet() { return Static.t; }
export class Derived extends GetBox {}
export function readInherited(d: Derived) { return d.label; }
export class DerivedPair extends Pair { own = 1 }
export function readInheritedPair(d: DerivedPair) { return d.v; }
export class LitGet { get lit(): "a" { return "a" } }
export function readLitGet(b: LitGet) { return b.lit; }
export class Plain { m(): string { return "" } }
export function readMethod(p: Plain) { return p.m; }
"#;

fn host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(ACCESSORS.to_string()),
        input_id: ACCESSORS.to_string(),
        source: Arc::from(ACCESSORS_SRC),
        file_language: crate::LanguageRegistry::global()
            .classify_static(ACCESSORS)
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
            Arc::from(ACCESSORS),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(ACCESSORS),
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

/// The published `{ label, n }` object of the two object rows.
#[track_caller]
fn assert_label_n_object(host: &Arc<VerterHost>, name: &str) {
    let outcome = eval(host, name);
    assert_eq!(
        (outcome.degradation, outcome.candidates),
        (None, 1),
        "{name}"
    );
    let TypeExpr::Object(shape) = &outcome.ty else {
        panic!("{name} must be an object, got {outcome:?}");
    };
    let member = |key: &str| {
        shape
            .properties
            .iter()
            .find_map(|member| match member {
                verter_type_expr::ObjectMember::Property(property)
                    if property.key.as_string() == Some(key) =>
                {
                    Some(property.ty.clone())
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("{name}: member `{key}` missing in {outcome:?}"))
    };
    assert_eq!(member("label"), primitive(PrimitiveName::String), "{name}");
    assert_eq!(member("n"), primitive(PrimitiveName::Number), "{name}");
}

/// Reading an accessor through an instance, a static side, or a derived
/// class reads its VALUE type.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// readGetter         b.label              string
/// readGetterObj      { label: b.label }   { label: string; n: number; }
/// readGetterNew      (local instance)     { label: string; n: number; }
/// readAnnotated      b.label              string
/// readSetOnly        b.v                  number     (set-only: the parameter)
/// readPair           b.v                  string     (get/set: the getter)
/// readSetFirst       b.v                  string     (setter declared first)
/// readStatic         Static.s             number
/// readStaticSet      Static.t             boolean
/// readInherited      d.label              string     (through `extends`)
/// readInheritedPair  d.v                  string
/// readLitGet         b.lit                "a"
/// ```
///
/// Mutation: dropping the accessor arm of the member read publishes the
/// accessor FUNCTION (`() => string`, `(x: number) => void`) for every
/// row.
#[test]
fn an_accessor_read_is_its_value_type() {
    let host = host();
    assert_clean_warm(&host, "readGetter", primitive(PrimitiveName::String));
    assert_label_n_object(&host, "readGetterObj");
    assert_label_n_object(&host, "readGetterNew");
    assert_clean_warm(&host, "readAnnotated", primitive(PrimitiveName::String));
    assert_clean_warm(&host, "readSetOnly", primitive(PrimitiveName::Number));
    assert_clean_warm(&host, "readPair", primitive(PrimitiveName::String));
    assert_clean_warm(&host, "readSetFirst", primitive(PrimitiveName::String));
    assert_clean_warm(&host, "readStatic", primitive(PrimitiveName::Number));
    assert_clean_warm(&host, "readStaticSet", primitive(PrimitiveName::Boolean));
    assert_clean_warm(&host, "readInherited", primitive(PrimitiveName::String));
    assert_clean_warm(&host, "readInheritedPair", primitive(PrimitiveName::String));
    assert_clean_warm(
        &host,
        "readLitGet",
        TypeExpr::Literal(LiteralValue::String("a".to_string())),
    );
}

/// The discriminator: an ordinary METHOD read stays the method's function
/// type.
///
/// TypeScript 7.0.2: `readMethod` (`p.m`) is `() => string`.
#[test]
fn a_method_read_stays_the_method() {
    let host = host();
    let outcome = eval(&host, "readMethod");
    assert_eq!((outcome.degradation, outcome.candidates), (None, 1));
    let TypeExpr::Function(function) = &outcome.ty else {
        panic!("readMethod must be a function, got {outcome:?}");
    };
    assert!(function.parameters.is_empty());
    assert_eq!(
        function.return_type.as_deref(),
        Some(&primitive(PrimitiveName::String))
    );
}
