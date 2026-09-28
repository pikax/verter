//! `ReturnType` / `Parameters` over a GENERIC signature read TypeScript's
//! BASE signature: each type parameter at its constraint (`unknown` when it
//! has none; a default never applies), sibling parameters inside a
//! constraint replaced by their constraints for `N - 1` rounds, anything
//! still referenced erased to `any`, and a circular constraint (TS2313)
//! dropped. The rule holds on both routes a signature utility reaches a
//! callee through: the whole-result utility and the flow-return member
//! lookup (`ReturnType<typeof f>['k']`).
//!
//! Every expected print below is TypeScript 7.0.2's, measured on this exact
//! fixture through the corpus's two-step wrapper (`declare const v:
//! <probe>; export const s: null = v;`, `--noEmit --strict`) and read off
//! the TS2322 message.

use std::sync::Arc;

use super::{resolve_decl_key, ProjectSemanticDispatch};
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryResult, SemanticNodeData, SemanticQueryApi,
    SemanticQueryKey, SemanticQueryOutput,
};
use crate::{CompileErrorPolicy, FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_type_expr::{LiteralValue, ObjectMember, TypeExpr};

const CANONICAL: &str = "/w/base_signature.ts";

const SOURCE: &str = "\
export declare function cstr<T extends string>(v: T): T;
export declare function dflt<T extends string = 'x'>(v: T): T;
export declare function pickA<T extends { a: 1 }>(v: T): T['a'];
export declare function chain<A extends string, B extends A>(a: A, b: B): [A, B];
export declare function chainRev<B extends A, A extends number>(a: A, b: B): [A, B];
export declare function circ<A extends B, B extends A>(a: A, b: B): [A, B];
export declare function circUnion<A extends B | string, B extends A>(a: A, b: B): [A, B];
export declare function selfRef<T extends { next: T }>(v: T): T;
export declare function circ3<A extends B[], B extends C[], C extends A[]>(a: A): [A, B, C];
export function member<T extends { k: number }>(v: T) { return { k: v }; }
export declare function withPromise<T extends Promise<number>>(v: T): T;
export type RC = ReturnType<typeof cstr>;
export type PC = Parameters<typeof cstr>;
export type RDflt = ReturnType<typeof dflt>;
export type RPick = ReturnType<typeof pickA>;
export type PPick = Parameters<typeof pickA>;
export type RChain = ReturnType<typeof chain>;
export type PChain = Parameters<typeof chain>;
export type RChainRev = ReturnType<typeof chainRev>;
export type RCirc = ReturnType<typeof circ>;
export type RCircUnion = ReturnType<typeof circUnion>;
export type RSelf = ReturnType<typeof selfRef>;
export type RCirc3 = ReturnType<typeof circ3>;
export type MK = ReturnType<typeof member>['k'];
export type RPromise = ReturnType<typeof withPromise>;
export type BarePromise = Promise<number>;
";

fn host_with_fixture() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig {
        dev_mode: false,
        compile_error_policy: CompileErrorPolicy::StrictError,
        ..HostConfig::default()
    });
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: CANONICAL.to_string(),
            source: Arc::from(SOURCE),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert the fixture");
    host
}

/// The fixture alias `name`, resolved `Expanded` and projected to a
/// `TypeExpr`, printed in the checker's own spelling.
fn print_alias(host: &VerterHost, name: &str) -> String {
    let (outcome, _record) = host
        .resolve_named_symbol_with_audit(CANONICAL, name, Some(ProjectionMode::Expanded))
        .into_parts();
    let node = outcome
        .ok()
        .flatten()
        .unwrap_or_else(|| panic!("{name} must resolve"));
    let expr = host
        .project_node_to_type_expr_for_test(node)
        .unwrap_or_else(|| panic!("{name} must project to a TypeExpr"));
    print(&expr)
}

/// The checker's print of the shapes this fixture produces.
fn print(expr: &TypeExpr) -> String {
    match expr {
        TypeExpr::Primitive(name) => name.as_str().to_owned(),
        TypeExpr::Literal(LiteralValue::Number(n)) => n.to_string(),
        TypeExpr::Array { element, .. } => format!("{}[]", print(element)),
        TypeExpr::Tuple { elements, .. } => format!(
            "[{}]",
            elements
                .iter()
                .map(|element| match &element.label {
                    Some(label) => format!("{label}: {}", print(&element.ty)),
                    None => print(&element.ty),
                })
                .collect::<Vec<_>>()
                .join(", ")
        ),
        TypeExpr::Object(object) => format!(
            "{{ {}}}",
            object
                .properties
                .iter()
                .map(|member| match member {
                    ObjectMember::Property(p) => format!(
                        "{}: {}; ",
                        p.string_name().expect("string-key fixture"),
                        print(&p.ty)
                    ),
                    other => format!("{other:?}; "),
                })
                .collect::<String>()
        ),
        TypeExpr::Ref {
            name,
            type_arguments,
        } if !type_arguments.is_empty() => format!(
            "{name}<{}>",
            type_arguments
                .iter()
                .map(print)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => format!("{other:?}"),
    }
}

/// The whole-result utility route reads the base signature: constrained,
/// chained in either declaration order, self-referential, mutually
/// referential through arrays (erased to `any` after `N - 1` rounds), and
/// circular (dropped, so `unknown`) — with a declared default ignored.
#[test]
fn signature_utilities_read_the_base_signature_of_a_generic_callee() {
    let host = host_with_fixture();
    for (alias, measured) in [
        ("RC", "string"),
        ("PC", "[v: string]"),
        ("RDflt", "string"),
        ("RPick", "1"),
        ("PPick", "[v: { a: 1; }]"),
        ("RChain", "[string, string]"),
        ("PChain", "[a: string, b: string]"),
        ("RChainRev", "[number, number]"),
        ("RCirc", "[unknown, unknown]"),
        ("RCircUnion", "[unknown, unknown]"),
        ("RSelf", "{ next: any; }"),
        ("RCirc3", "[any[][][], any[][][], any[][][]]"),
        ("RPromise", "Promise<number>"),
    ] {
        assert_eq!(
            print_alias(&host, alias),
            measured,
            "{alias}: TypeScript 7.0.2 prints `{measured}`"
        );
    }
}

/// The flow-return MEMBER route applies the same base signature: the
/// callee's body-derived return `{ k: T }` read one segment deeper is the
/// constraint `{ k: number; }`, never `unknown`.
#[test]
fn a_return_type_member_lookup_reads_the_base_signature() {
    let host = host_with_fixture();
    assert_eq!(
        print_alias(&host, "MK"),
        "{ k: number; }",
        "ReturnType<typeof member>['k']: TypeScript 7.0.2 prints `{{ k: number; }}`"
    );
}

/// A builtin runtime nominal's application is its own resolved type: the
/// production structural-fact demand settles on the `Promise<number>`
/// carrier, complete, instead of instantiating it into a miss.
#[test]
fn a_builtin_nominal_application_settles_on_its_carrier() {
    let host = host_with_fixture();
    let dispatch = ProjectSemanticDispatch::new(&host);
    let alias = match dispatch.execute_type_node(SemanticQueryKey::ResolveDecl(resolve_decl_key(
        CANONICAL,
        verter_type_expr::TopLevelOwnerId::ordinary_file(),
        "BarePromise",
    ))) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("BarePromise resolves, got {other:?}"),
    };
    let settled = dispatch
        .normalize_node_for_structural_fact_demand(
            alias,
            ProjectionReductionContext::published(ProjectionMode::Expanded),
        )
        .into_complete_node()
        .expect("the demand completes");
    let graph = host.project_type_store().semantic_graph();
    match graph.node_data(settled).as_deref() {
        Some(SemanticNodeData::InstantiationRef { base, args }) => {
            assert_eq!(base.canonical_id.as_ref(), "__builtin__");
            assert_eq!(base.decl_name.as_ref(), "Promise");
            assert_eq!(args.len(), 1, "Promise<number> keeps its argument");
        }
        other => panic!("Promise<number> settles on its carrier, got {other:?}"),
    }
}
