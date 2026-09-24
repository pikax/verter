//! Class expressions in a function body: the flow lane composes
//! TypeScript's class rules over every class form the checker types —
//! the class's own type parameters, an `extends` value of any form,
//! computed keys, index signatures, `accessor` properties, overloads, a
//! non-public constructor, member types read off method, accessor and
//! initializer bodies — and names every reference to the class the way
//! the checker prints it.
//!
//! Every expected print is TypeScript 7.0.2's, measured on the fixture
//! through the corpus's two-step wrapper (`declare const v:
//! ReturnType<typeof probe>; export const s: null = v;`, `--noEmit
//! --strict`) and read off the TS2322 message, unless a row says it was
//! measured another way.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnResult, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::TopLevelOwnerId;

const FILE: &str = "/ws/cls/classes.ts";

fn host_with(source: &str) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
    host
}

fn with_dispatch<R>(source: &str, f: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R) -> R {
    let host = host_with(source);
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    f(&dispatch)
}

/// The whole-return flow key of the fixture position `name` at `part`.
fn flow_key(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    part: FunctionPartIdentity,
) -> crate::semantic_query::FlowReturnKey {
    crate::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(FILE),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            part,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(FILE),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    }
}

/// The flow result of the fixture position `name` at `part`, which must
/// evaluate clean.
#[track_caller]
fn clean_result(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    part: FunctionPartIdentity,
) -> Arc<FlowReturnResult> {
    let key = flow_key(dispatch, name, part);
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
    else {
        panic!("{name} must produce a value");
    };
    assert_eq!(result.degradation(), None, "{name} must evaluate clean");
    result
}

/// One probe function's clean return, normalized the way the signature
/// corpus compares a probe: deferred utility applications reduced, named
/// declarations and class expressions kept by name.
#[track_caller]
fn with_probe<R>(
    source: &str,
    name: &str,
    check: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_dispatch(source, |dispatch| {
        let result = clean_result(dispatch, name, FunctionPartIdentity::DeclarationBody);
        let normalized = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(
                result.return_type(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            )
            .into_complete_node()
            .unwrap_or_else(|| panic!("{name}: the probe's structural-fact demand completes"));
        check(dispatch, normalized)
    })
}

/// `node` structurally equals the checker print `expected`, through the
/// shared checker-syntax projection.
#[track_caller]
fn assert_matches(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    expected: &str,
    what: &str,
) {
    let parsed = checker_syntax::parse(expected)
        .unwrap_or_else(|error| panic!("`{expected}` must parse: {error}"));
    assert!(
        checker_syntax::matches_node(dispatch, node, &parsed, 0),
        "{what}: expected `{expected}`, measured `{}`",
        render_node(dispatch, node, 0)
    );
}

/// Each `(probe, checker print)` pair holds on `source`.
#[track_caller]
fn assert_probes(source: &str, rows: &[(&str, &str)]) {
    for (name, expected) in rows {
        with_probe(source, name, |dispatch, node| {
            assert_matches(dispatch, node, expected, name);
        });
    }
}

/// One probe is a parameter tuple: each element's label, its optionality,
/// and its value's checker print.
#[track_caller]
fn assert_tuple_probe(source: &str, name: &str, expected: &[(&str, bool, &str)]) {
    with_probe(source, name, |dispatch, node| {
        let Some(SemanticNodeData::Tuple { elements, .. }) =
            dispatch.graph().node_data(node).as_deref().cloned()
        else {
            panic!(
                "{name}: expected a parameter tuple, measured `{}`",
                render_node(dispatch, node, 0)
            );
        };
        assert_eq!(elements.len(), expected.len(), "{name}: arity");
        for (element, (label, optional, value)) in elements.iter().zip(expected) {
            assert_eq!(element.label.as_deref(), Some(*label), "{name}: label");
            assert_eq!(element.optional, *optional, "{name}: {label} optionality");
            assert_matches(dispatch, element.value, value, name);
        }
    });
}

/// The class expression a flow result's constructor type constructs: the
/// constructor surface and the instance node its first construct
/// signature returns.
fn constructor_and_instance(
    dispatch: &ProjectSemanticDispatch<'_>,
    constructor: SemanticNodeId,
) -> (crate::semantic_query::SurfaceView, SemanticNodeId) {
    let graph = dispatch.graph();
    let Some(SemanticNodeData::Object(view)) = graph.node_data(constructor).as_deref().cloned()
    else {
        panic!(
            "expected a constructor surface, got `{}`",
            render_node(dispatch, constructor, 0)
        );
    };
    let signature = *view
        .construct_signatures
        .first()
        .expect("a class constructor has a construct signature");
    let Some(SemanticNodeData::Signature { return_type, .. }) =
        graph.node_data(signature).as_deref().cloned()
    else {
        panic!("a construct signature");
    };
    (view, return_type)
}

/// The own (non-inherited) surface of a class-expression instance.
fn instance_surface(
    dispatch: &ProjectSemanticDispatch<'_>,
    instance: SemanticNodeId,
) -> crate::semantic_query::SurfaceView {
    let graph = dispatch.graph();
    let Some(SemanticNodeData::ClassExpressionInstance { surface, .. }) =
        graph.node_data(instance).as_deref().cloned()
    else {
        panic!(
            "expected a class-expression instance, got `{}`",
            render_node(dispatch, instance, 0)
        );
    };
    match graph.node_data(surface).as_deref().cloned() {
        Some(SemanticNodeData::Object(view)) => view,
        _ => panic!(
            "expected an own-member surface, got `{}`",
            render_node(dispatch, surface, 0)
        ),
    }
}

/// The printed name of the class-expression instance `node`.
fn printed_name(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> String {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::ClassExpressionInstance {
            identity,
            type_arguments,
            ..
        }) => identity.printed_name_in(dispatch.graph(), type_arguments),
        _ => panic!(
            "expected a class-expression instance, got `{}`",
            render_node(dispatch, node, 0)
        ),
    }
}

// ──────────────────────────────────────────────────────────────────────
// The class's own type parameters
// ──────────────────────────────────────────────────────────────────────

const GENERICS: &str = r#"
export function generic() { return class<T> { value!: T; get(): T { return this.value; } }; }
export function constrained() { return class<T extends string = "d"> { constructor(public v: T) {} }; }
export function outer<U>() { return class<T> { t!: T; u!: U }; }
export function genericInstance() { const x: InstanceType<ReturnType<typeof generic>> = null as any; return x; }
export function genericValue() { const x: InstanceType<ReturnType<typeof generic>>['value'] = null as any; return x; }
export function genericGet() { const x: InstanceType<ReturnType<typeof generic>>['get'] = null as any; return x; }
export function genericPrototype() { const G = generic(); return G.prototype; }
export function constrainedInstance() { const x: InstanceType<ReturnType<typeof constrained>> = null as any; return x; }
export function constrainedParams() { const x: ConstructorParameters<ReturnType<typeof constrained>> = null as any; return x; }
export function constrainedMember() { const x: InstanceType<ReturnType<typeof constrained>>['v'] = null as any; return x; }
export function outerInstance() { const x: InstanceType<ReturnType<typeof outer<number>>> = null as any; return x; }
export function outerMember() { const x: InstanceType<ReturnType<typeof outer<number>>>['u'] = null as any; return x; }
"#;

/// A class expression's own type parameters make each of its construct
/// signatures generic over them (`new <T>() => …`), and a reference to
/// the instance carries its arguments after the name.
///
/// Oracle (tsc 7.0.2): declaration emit spells `generic()` as `{ new
/// <T>(): { value: T; get(): T; }; }` and `constrained()` as `{ new <T
/// extends string = "d">(v: T): { v: T; }; }`. `InstanceType` reads the
/// construct signature's base signature — each parameter at its
/// constraint, `unknown` without one, never its default:
/// `genericInstance` is `(Anonymous class)<unknown>` (`value` `unknown`,
/// `get` `() => unknown`), `constrainedInstance` is `(Anonymous
/// class)<string>` (`v` `string`, `ConstructorParameters` `[v: string]`).
/// A class's prototype is its instance over `any`: `genericPrototype` is
/// `(Anonymous class)<any>`. Outer and own arguments print together:
/// `outerInstance` is `outer.(Anonymous class)<unknown>`, its `u`
/// `number`.
#[test]
fn class_expression_type_parameters_make_its_construct_signatures_generic() {
    assert_probes(
        GENERICS,
        &[
            ("genericInstance", "(Anonymous class)<unknown>"),
            ("genericValue", "unknown"),
            ("genericGet", "() => unknown"),
            ("genericPrototype", "(Anonymous class)<any>"),
            ("constrainedInstance", "(Anonymous class)<string>"),
            ("constrainedMember", "string"),
            ("outerInstance", "outer.(Anonymous class)<unknown>"),
            ("outerMember", "number"),
        ],
    );
    assert_tuple_probe(GENERICS, "constrainedParams", &[("v", false, "string")]);
    // The construct signature itself declares the class's clause.
    with_dispatch(GENERICS, |dispatch| {
        let result = clean_result(dispatch, "generic", FunctionPartIdentity::DeclarationBody);
        let (view, _) = constructor_and_instance(dispatch, result.return_type());
        let signature = view.construct_signatures[0];
        let Some(SemanticNodeData::Signature {
            type_parameters, ..
        }) = dispatch.graph().node_data(signature).as_deref().cloned()
        else {
            panic!("a construct signature");
        };
        assert_eq!(
            type_parameters
                .iter()
                .map(|parameter| parameter.name.to_string())
                .collect::<Vec<_>>(),
            ["T"],
            "`new <T>() => …`"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// `extends` values of every form
// ──────────────────────────────────────────────────────────────────────

const HERITAGE: &str = r#"
export class Named { n = 1; constructor(a: string, b?: number) {} static s = "s"; }
export class A1 { a = 1; }
export class B1 { b = "x"; }
export class C1 { a = 1; c = true; }
declare const cond: boolean;
declare function withMixed(b: typeof Named): typeof Named & (new (...args: any[]) => { mixed: boolean });
export function fromCall() { return class extends withMixed(Named) { own = 1; }; }
export function fromUnion() { return class extends (cond ? A1 : B1) { own = 1; }; }
export function fromSubtypes() { return class extends (cond ? A1 : C1) { own = 1; }; }
export function fromLocal() { const Base = A1; return class extends Base { own = 1; }; }
export function callInstance() { const x: InstanceType<ReturnType<typeof fromCall>> = null as any; return x; }
export function callMixed() { const x: InstanceType<ReturnType<typeof fromCall>>['mixed'] = null as any; return x; }
export function callInherited() { const x: InstanceType<ReturnType<typeof fromCall>>['n'] = null as any; return x; }
export function callOwn() { const x: InstanceType<ReturnType<typeof fromCall>>['own'] = null as any; return x; }
export function callParams() { const x: ConstructorParameters<ReturnType<typeof fromCall>> = null as any; return x; }
export function callStatic() { const x: ReturnType<typeof fromCall>['s'] = null as any; return x; }
export function unionOwn() { const x: InstanceType<ReturnType<typeof fromUnion>>['own'] = null as any; return x; }
export function unionParams() { const x: ConstructorParameters<ReturnType<typeof fromUnion>> = null as any; return x; }
export function subtypeInherited() { const x: InstanceType<ReturnType<typeof fromSubtypes>>['a'] = null as any; return x; }
export function localInherited() { const x: InstanceType<ReturnType<typeof fromLocal>>['a'] = null as any; return x; }
"#;

/// An `extends` value is any constructor-valued expression the frame
/// evaluates at class evaluation — a call, a conditional, a local.
///
/// Oracle (tsc 7.0.2): over `class extends withMixed(Named) { own = 1 }`
/// (`withMixed` returns `typeof Named & (new (...args: any[]) => {
/// mixed: boolean })`, a mixin intersection) the instance is `(Anonymous
/// class)` with `mixed` `boolean`, `n` `number`, `own` `number`;
/// `ConstructorParameters` is `[a: string, b?: number | undefined]`; the
/// static `s` is `string`. A union base constructor's signature returns
/// the SUBTYPE-reduced union of its members' results: `cond ? A1 : C1`
/// reduces to `A1` (`a` is `number`), while `cond ? A1 : B1` stays a
/// union, which is no valid base type — declaration emit spells
/// `fromUnion()` as `{ new (): { own: number; }; }` (keys `"own"`,
/// `ConstructorParameters` `[]`). A local `Base` reads through its
/// binding: `a` is `number`.
#[test]
fn class_expression_extends_any_constructor_valued_expression() {
    assert_probes(
        HERITAGE,
        &[
            ("callInstance", "(Anonymous class)"),
            ("callMixed", "boolean"),
            ("callInherited", "number"),
            ("callOwn", "number"),
            ("callStatic", "string"),
            ("unionOwn", "number"),
            ("subtypeInherited", "number"),
            ("localInherited", "number"),
        ],
    );
    assert_tuple_probe(
        HERITAGE,
        "callParams",
        &[("a", false, "string"), ("b", true, "number | undefined")],
    );
    assert_tuple_probe(HERITAGE, "unionParams", &[]);
    // The non-reducible union contributes no inherited member: the
    // instance is the class's own members alone.
    with_dispatch(HERITAGE, |dispatch| {
        let result = clean_result(dispatch, "fromUnion", FunctionPartIdentity::DeclarationBody);
        let (_, instance) = constructor_and_instance(dispatch, result.return_type());
        let Some(SemanticNodeData::ClassExpressionInstance { surface, .. }) =
            dispatch.graph().node_data(instance).as_deref().cloned()
        else {
            panic!("a class-expression instance");
        };
        assert_matches(
            dispatch,
            surface,
            "{ own: number; }",
            "fromUnion instance surface",
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// Member shapes
// ──────────────────────────────────────────────────────────────────────

const SHAPES: &str = r#"
declare const sym: unique symbol;
declare const anyString: string;
declare const anyNumber: number;
export function computed() { const k = "key"; return class { [k] = 1; ["lit"] = "s"; [sym] = true; 1 = false; }; }
export function lateBound() { return class { [anyString] = 1; a = "x"; static [anyString] = true; static b = 1; }; }
export function lateBoundNumber() { return class { [anyNumber] = "x"; 1 = true; a = 1; }; }
export function indexed() { return class { [key: string]: number; x = 1; static [key: number]: string; }; }
export function accessors() { return class { accessor v = 1; accessor w: string | undefined; static accessor s = true; }; }
export function overloadedCtor() { return class { constructor(a: string); constructor(a: number, b: boolean); constructor(a: any, b?: any) {} }; }
export function overloadedMethod() { return class { m(a: string): string; m(a: number): number; m(a: any): any { return a; } }; }
export function privateCtor() { return class { private constructor(a: string) {} x = 1; }; }
export function protectedCtor() { return class { protected constructor() {} y = 1; }; }
export function computedKey() { const x: InstanceType<ReturnType<typeof computed>>['key'] = null as any; return x; }
export function computedLiteral() { const x: InstanceType<ReturnType<typeof computed>>['lit'] = null as any; return x; }
export function computedSymbol() { const x: InstanceType<ReturnType<typeof computed>>[typeof sym] = null as any; return x; }
export function computedNumber() { const x: InstanceType<ReturnType<typeof computed>>[1] = null as any; return x; }
export function accessorValue() { const x: InstanceType<ReturnType<typeof accessors>>['v'] = null as any; return x; }
export function accessorDeclared() { const x: InstanceType<ReturnType<typeof accessors>>['w'] = null as any; return x; }
export function accessorStatic() { const x: ReturnType<typeof accessors>['s'] = null as any; return x; }
export function overloadedCtorParams() { const x: ConstructorParameters<ReturnType<typeof overloadedCtor>> = null as any; return x; }
export function overloadedMethodReturn() { const x: ReturnType<InstanceType<ReturnType<typeof overloadedMethod>>['m']> = null as any; return x; }
export function overloadedMethodParams() { const x: Parameters<InstanceType<ReturnType<typeof overloadedMethod>>['m']> = null as any; return x; }
export function privateMember() { const x: ReturnType<typeof privateCtor>['prototype']['x'] = null as any; return x; }
export function protectedInstance() { const x: ReturnType<typeof protectedCtor>['prototype'] = null as any; return x; }
"#;

/// A computed key whose value is a string, number or unique-symbol
/// literal names its member — a key reading the frame's `const k =
/// "key"` included.
///
/// Oracle (tsc 7.0.2), over `const k = "key"; return class { [k] = 1;
/// ["lit"] = "s"; [sym] = true; 1 = false; }`: declaration emit spells the
/// instance `{ key: number; lit: string; [sym]: boolean; 1: boolean; }`,
/// and `['key']` is `number`, `['lit']` `string`, `[typeof sym]`
/// `boolean`, `[1]` `boolean`.
#[test]
fn class_expression_literal_computed_keys_name_their_members() {
    assert_probes(
        SHAPES,
        &[
            ("computedKey", "number"),
            ("computedLiteral", "string"),
            ("computedSymbol", "boolean"),
            ("computedNumber", "boolean"),
        ],
    );
}

/// A computed key whose value is no single literal is LATE-BOUND: it names
/// no member, and the side it sits on gets the implicit index signature
/// of its key kind, whose value is the union of every applicable member's
/// type — a string index reads every string- and number-named member (and
/// the constructor's `prototype`), a number index every numeric-named one
/// (`getIndexInfosOfIndexSymbol`).
///
/// Oracle (tsc 7.0.2): over `class { [anyString] = 1; a = "x"; static
/// [anyString] = true; static b = 1; }` (`anyString: string`) the
/// instance's `[string]` is `string | number` and the constructor's
/// `[string]` is `number | boolean | (Anonymous class)`; over `class {
/// [anyNumber] = "x"; 1 = true; a = 1; }` the instance's `[number]` is
/// `string | boolean`. (The shapes are read off the composed surfaces: an
/// indexed access through an index signature is not a projection this
/// lane answers for any declaration.)
#[test]
fn class_expression_late_bound_keys_index_their_side() {
    with_dispatch(SHAPES, |dispatch| {
        let index = |view: &crate::semantic_query::SurfaceView| {
            view.index_signatures
                .iter()
                .map(|signature| (signature.key_type, signature.value_type))
                .collect::<Vec<_>>()
        };
        let result = clean_result(dispatch, "lateBound", FunctionPartIdentity::DeclarationBody);
        let (constructor, instance) = constructor_and_instance(dispatch, result.return_type());
        let own = instance_surface(dispatch, instance);
        assert!(
            own.positive_members()
                .iter()
                .all(|member| member.key.as_string() == Some("a")),
            "a late-bound key names no member"
        );
        let [(key, value)] = index(&own)[..] else {
            panic!("one implicit instance index signature");
        };
        assert_matches(dispatch, key, "string", "instance index key");
        assert_matches(dispatch, value, "string | number", "instance index value");
        let [(key, value)] = index(&constructor)[..] else {
            panic!("one implicit static index signature");
        };
        assert_matches(dispatch, key, "string", "static index key");
        assert_matches(
            dispatch,
            value,
            "number | boolean | (Anonymous class)",
            "static index value",
        );

        let result = clean_result(
            dispatch,
            "lateBoundNumber",
            FunctionPartIdentity::DeclarationBody,
        );
        let (_, instance) = constructor_and_instance(dispatch, result.return_type());
        let own = instance_surface(dispatch, instance);
        let [(key, value)] = index(&own)[..] else {
            panic!("one implicit number index signature");
        };
        assert_matches(dispatch, key, "number", "number index key");
        assert_matches(dispatch, value, "string | boolean", "number index value");
    });
}

/// Declared index signatures sit on their side, and an `accessor`
/// property is a property of its annotation, else its initializer's
/// widened type.
///
/// Oracle (tsc 7.0.2): declaration emit spells `indexed()` as `{ new (): {
/// [key: string]: number; x: number; }; [key: number]: string; }` (its
/// `[string]` is `number`, the constructor's `[number]` `string`); over
/// `class { accessor v = 1; accessor w: string | undefined; static
/// accessor s = true; }` `v` is `number`, `w` `string | undefined`, the
/// static `s` `boolean`.
#[test]
fn class_expression_index_signatures_and_accessor_properties() {
    assert_probes(
        SHAPES,
        &[
            ("accessorValue", "number"),
            ("accessorDeclared", "string | undefined"),
            ("accessorStatic", "boolean"),
        ],
    );
    with_dispatch(SHAPES, |dispatch| {
        let result = clean_result(dispatch, "indexed", FunctionPartIdentity::DeclarationBody);
        let (constructor, instance) = constructor_and_instance(dispatch, result.return_type());
        let own = instance_surface(dispatch, instance);
        let [instance_index] = &own.index_signatures[..] else {
            panic!("one instance index signature");
        };
        assert_matches(
            dispatch,
            instance_index.key_type,
            "string",
            "instance index key",
        );
        assert_matches(
            dispatch,
            instance_index.value_type,
            "number",
            "instance index value",
        );
        let [static_index] = &constructor.index_signatures[..] else {
            panic!("one static index signature");
        };
        assert_matches(
            dispatch,
            static_index.key_type,
            "number",
            "static index key",
        );
        assert_matches(
            dispatch,
            static_index.value_type,
            "string",
            "static index value",
        );
    });
}

/// Overload signatures are the visible signatures — of the constructor
/// and of a method — and the implementation behind them is not.
///
/// Oracle (tsc 7.0.2): declaration emit spells `overloadedCtor()` as `{
/// new (a: string): {}; new (a: number, b: boolean): {}; }` and
/// `overloadedMethod()`'s instance as `{ m(a: string): string; m(a:
/// number): number; }`; `ConstructorParameters` reads the last overload
/// (`[a: number, b: boolean]`), and so do `ReturnType` (`number`) and
/// `Parameters` (`[a: number]`) of `m`.
#[test]
fn class_expression_overloads_are_its_visible_signatures() {
    assert_tuple_probe(
        SHAPES,
        "overloadedCtorParams",
        &[("a", false, "number"), ("b", false, "boolean")],
    );
    assert_probes(SHAPES, &[("overloadedMethodReturn", "number")]);
    assert_tuple_probe(SHAPES, "overloadedMethodParams", &[("a", false, "number")]);
    with_dispatch(SHAPES, |dispatch| {
        let result = clean_result(
            dispatch,
            "overloadedCtor",
            FunctionPartIdentity::DeclarationBody,
        );
        let (constructor, _) = constructor_and_instance(dispatch, result.return_type());
        assert_eq!(
            constructor.construct_signatures.len(),
            2,
            "the two overloads, never the implementation"
        );
        let result = clean_result(
            dispatch,
            "overloadedMethod",
            FunctionPartIdentity::DeclarationBody,
        );
        let (_, instance) = constructor_and_instance(dispatch, result.return_type());
        let own = instance_surface(dispatch, instance);
        let group: Vec<SemanticNodeId> = own
            .positive_members()
            .iter()
            .filter(|member| member.key.as_string() == Some("m"))
            .map(|member| member.value)
            .collect();
        let [first, second] = group[..] else {
            panic!("`m` is its two overload signatures");
        };
        assert_matches(dispatch, first, "(a: string) => string", "first overload");
        assert_matches(dispatch, second, "(a: number) => number", "second overload");
    });
}

/// A non-public constructor keeps the construct signature its parameters
/// declare: its accessibility is not part of the constructor type's shape.
///
/// Oracle (tsc 7.0.2): declaration emit spells `privateCtor()` as `{ new
/// (a: string): { x: number; }; }` and `protectedCtor()` as `{ new (): {
/// y: number; }; }`; `ReturnType<typeof privateCtor>['prototype']['x']`
/// is `number` and `ReturnType<typeof protectedCtor>['prototype']` is
/// `(Anonymous class)`.
#[test]
fn class_expression_non_public_constructor_keeps_its_construct_signature() {
    assert_probes(
        SHAPES,
        &[
            ("privateMember", "number"),
            ("protectedInstance", "(Anonymous class)"),
        ],
    );
    with_dispatch(SHAPES, |dispatch| {
        let result = clean_result(
            dispatch,
            "privateCtor",
            FunctionPartIdentity::DeclarationBody,
        );
        let (constructor, _) = constructor_and_instance(dispatch, result.return_type());
        let Some(SemanticNodeData::Signature { params, .. }) = dispatch
            .graph()
            .node_data(constructor.construct_signatures[0])
            .as_deref()
            .cloned()
        else {
            panic!("a construct signature");
        };
        assert_eq!(params.len(), 1, "`new (a: string)`");
        assert_matches(
            dispatch,
            params[0].ty,
            "string",
            "the private constructor's parameter",
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// Member types read off bodies and initializers
// ──────────────────────────────────────────────────────────────────────

const BODIES: &str = r#"
declare function make(): { k: number };
declare function id<T>(x: T): T;
export function bodies(p: string, n: number | undefined) {
  const local = 1;
  return class {
    literal() { return 1; }
    choose(x: string) { return x ? x : null; }
    param() { return p; }
    captured() { return local; }
    optional() { return n; }
    empty() {}
    fails() { throw 0; }
    get read() { return local; }
    get paired() { return 1; }
    set paired(v) {}
    static fromParam() { return p; }
    fromParam = p;
    fromLocal = local;
    readonly pinned = local;
    fromCall = make();
    fromGeneric = id(p);
    arrow = () => p;
    declared = n;
    static staticParam = p;
  };
}
export function narrowed(n: number | undefined) {
  if (n !== undefined) {
    const C = class { field = n; static staticField = n; };
    return C;
  }
  return null;
}
export function methodLiteral() { const x: InstanceType<ReturnType<typeof bodies>>['literal'] = null as any; return x; }
export function methodChoose() { const x: InstanceType<ReturnType<typeof bodies>>['choose'] = null as any; return x; }
export function methodParam() { const x: InstanceType<ReturnType<typeof bodies>>['param'] = null as any; return x; }
export function methodCaptured() { const x: InstanceType<ReturnType<typeof bodies>>['captured'] = null as any; return x; }
export function methodOptional() { const x: InstanceType<ReturnType<typeof bodies>>['optional'] = null as any; return x; }
export function methodEmpty() { const x: InstanceType<ReturnType<typeof bodies>>['empty'] = null as any; return x; }
export function methodThrows() { const x: InstanceType<ReturnType<typeof bodies>>['fails'] = null as any; return x; }
export function getterRead() { const x: InstanceType<ReturnType<typeof bodies>>['read'] = null as any; return x; }
export function getterPaired() { const x: InstanceType<ReturnType<typeof bodies>>['paired'] = null as any; return x; }
export function staticMethod() { const x: ReturnType<typeof bodies>['fromParam'] = null as any; return x; }
export function fieldParam() { const x: InstanceType<ReturnType<typeof bodies>>['fromParam'] = null as any; return x; }
export function fieldLocal() { const x: InstanceType<ReturnType<typeof bodies>>['fromLocal'] = null as any; return x; }
export function fieldPinned() { const x: InstanceType<ReturnType<typeof bodies>>['pinned'] = null as any; return x; }
export function fieldCall() { const x: InstanceType<ReturnType<typeof bodies>>['fromCall'] = null as any; return x; }
export function fieldGeneric() { const x: InstanceType<ReturnType<typeof bodies>>['fromGeneric'] = null as any; return x; }
export function fieldArrow() { const x: InstanceType<ReturnType<typeof bodies>>['arrow'] = null as any; return x; }
export function fieldDeclared() { const x: InstanceType<ReturnType<typeof bodies>>['declared'] = null as any; return x; }
export function staticField() { const x: ReturnType<typeof bodies>['staticParam'] = null as any; return x; }
"#;

/// A method's and a getter's type come from their bodies, through the flow
/// lane's own function-return inference — a class method's, whose body
/// that never completes is `void` where an object-literal method's is
/// `never`.
///
/// Oracle (tsc 7.0.2), over `bodies(p: string, n: number | undefined)`
/// with `const local = 1`: `literal` is `() => number`, `choose` `(x:
/// string) => string | null`, `param` `() => string`, `captured` `() =>
/// number`, `optional` `() => number | undefined`, `empty` `() => void`,
/// `fails` (`throw 0`) `() => void` (the same method on an object literal
/// is `() => never`), the getter `read` `number`, the pair `paired` (`get
/// paired() { return 1 }` over an unannotated setter) `number`, and the
/// static `fromParam` `() => string`.
#[test]
fn class_expression_method_and_getter_types_come_from_their_bodies() {
    assert_probes(
        BODIES,
        &[
            ("methodLiteral", "() => number"),
            ("methodChoose", "(x: string) => string | null"),
            ("methodParam", "() => string"),
            ("methodCaptured", "() => number"),
            ("methodOptional", "() => number | undefined"),
            ("methodEmpty", "() => void"),
            ("methodThrows", "() => void"),
            ("getterRead", "number"),
            ("getterPaired", "number"),
            ("staticMethod", "() => string"),
        ],
    );
}

/// A property initializer is typed in the frame, over the DECLARED type of
/// every binding it reads — a property declaration is its own flow
/// container, so no narrowing of the enclosing frame reaches it — and the
/// value widens unless the property is `readonly`.
///
/// Oracle (tsc 7.0.2), over the same `bodies`: `fromParam` is `string`,
/// `fromLocal` `number`, `fromCall` `{ k: number; }`, `fromGeneric`
/// `string`, `arrow` `() => string`, `declared` `number | undefined`, the
/// static `staticParam` `string`; the `readonly pinned = local` member is
/// `1` (measured directly as `InstanceType<ReturnType<typeof
/// bodies>>['pinned']`). Over `narrowed`, which authors the class under `if
/// (n !== undefined)`, both `field` and the static `staticField` are
/// `number | undefined` — the narrow does not reach either initializer.
#[test]
fn class_expression_initializers_read_the_frame_at_declared_types() {
    assert_probes(
        BODIES,
        &[
            ("fieldParam", "string"),
            ("fieldLocal", "number"),
            ("fieldPinned", "1"),
            ("fieldCall", "{ k: number; }"),
            ("fieldGeneric", "string"),
            ("fieldArrow", "() => string"),
            ("fieldDeclared", "number | undefined"),
            ("staticField", "string"),
        ],
    );
    with_dispatch(BODIES, |dispatch| {
        let result = clean_result(dispatch, "narrowed", FunctionPartIdentity::DeclarationBody);
        let constructor = match dispatch.graph().node_data(result.return_type()).as_deref() {
            Some(composite @ SemanticNodeData::Union(_)) => composite
                .composite_members()
                .expect("union arms")
                .iter()
                .copied()
                .find(|arm| {
                    matches!(
                        dispatch.graph().node_data(*arm).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    )
                })
                .expect("the class arm"),
            _ => panic!("`C | null`"),
        };
        let (view, instance) = constructor_and_instance(dispatch, constructor);
        let own = instance_surface(dispatch, instance);
        let field = own
            .positive_members()
            .iter()
            .find(|member| member.key.as_string() == Some("field"))
            .expect("the instance field");
        assert_matches(dispatch, field.value, "number | undefined", "field");
        let static_field = view
            .positive_members()
            .iter()
            .find(|member| member.key.as_string() == Some("staticField"))
            .expect("the static field");
        assert_matches(
            dispatch,
            static_field.value,
            "number | undefined",
            "staticField",
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// The name a reference prints with
// ──────────────────────────────────────────────────────────────────────

const NAMES: &str = r#"
export class Holder { make<T>() { return class { v!: T }; } static build<T>() { return class { v!: T }; } }
export class GHolder<H> { make() { return class { v!: H }; } make2<T>() { return class { v!: T; h!: H }; } }
export const arrow = <T,>() => class { v!: T };
export const fexpr = function <T>() { return class { v!: T }; };
export const named = function nm<T>() { return class { v!: T }; };
export const o = { m<T>() { return class { v!: T }; } };
namespace N { export function make<T>() { const C = class { v!: T }; return C.prototype; } }
export function viaLocalArrow<T>() { const inner = () => class { v!: T }; return inner(); }
export function nestedClause() { const inner = <T,>() => class { v!: T }; return inner<string>(); }
export function bothClauses<U>() { const inner = <T,>() => class { v!: T; u!: U }; return inner<string>(); }
export function viaIife<T>() { return (() => class { v!: T })(); }
export function unreferenced<T>() { return class { x = 1 }; }
export function inside<T>(x: T) { const C = class { v = x; }; return C; }
export function insidePrototype<T>(x: T) { const C = class { v = x; }; return C.prototype; }
export function inObject() { return { K: class { x = 1; } }; }
export function assigned() { let C; C = class { y = 1; }; return C; }
export function parenthesized() { const P = (class { z = 1; }); return P.prototype; }
export function classId() { const Q = class Inner { w = 1; }; return Q.prototype; }
export function method() { const x: InstanceType<ReturnType<typeof Holder.prototype.make<string>>> = null as any; return x; }
export function staticMethod() { const x: InstanceType<ReturnType<typeof Holder.build<string>>> = null as any; return x; }
export function arrowHeld() { const x: InstanceType<ReturnType<typeof arrow<string>>> = null as any; return x; }
export function functionExpression() { const x: InstanceType<ReturnType<typeof fexpr<string>>> = null as any; return x; }
export function namedFunctionExpression() { const x: InstanceType<ReturnType<typeof named<string>>> = null as any; return x; }
export function objectMethod() { const x: InstanceType<ReturnType<typeof o.m<string>>> = null as any; return x; }
export function outerOfNested() { const x: InstanceType<ReturnType<typeof viaLocalArrow<string>>> = null as any; return x; }
export function innerClause() { const x: InstanceType<ReturnType<typeof nestedClause>> = null as any; return x; }
export function twoClauses() { const x: InstanceType<ReturnType<typeof bothClauses<number>>> = null as any; return x; }
export function iife() { const x: InstanceType<ReturnType<typeof viaIife<string>>> = null as any; return x; }
export function unreferencedClause() { const x: InstanceType<ReturnType<typeof unreferenced<string>>> = null as any; return x; }
export function unreferencedBase() { const x: InstanceType<ReturnType<typeof unreferenced>> = null as any; return x; }
export function instantiatedInside() { return insidePrototype(1); }
export function objectKey() { const x: InstanceType<ReturnType<typeof inObject>['K']> = null as any; return x; }
export function assignedName() { const x: InstanceType<ReturnType<typeof assigned>> = null as any; return x; }
"#;

/// A reference to a class expression prints every enclosing clause it
/// instantiates, outermost first, each by the name the checker prints for
/// the clause's declaration: a function by its own name, never its
/// namespace's or its enclosing function's; a function or arrow
/// expression by its own name, else the variable holding it; a class
/// method as `Class.method`; an object-literal method as `holder.method`.
/// The class's own name is its binding identifier, else the name it is
/// assigned to, else `(Anonymous class)`.
///
/// Oracle (tsc 7.0.2): `method` is `Holder.make.(Anonymous class)`,
/// `staticMethod` `Holder.build.(Anonymous class)`, `arrowHeld`
/// `arrow.(Anonymous class)`, `functionExpression` `fexpr.(Anonymous
/// class)`, `namedFunctionExpression` `nm.(Anonymous class)`,
/// `objectMethod` `o.m.(Anonymous class)`; through a nested function
/// `outerOfNested` is `viaLocalArrow.(Anonymous class)`, `innerClause`
/// `inner.(Anonymous class)`, `twoClauses` `bothClauses.inner.(Anonymous
/// class)`, `iife` `viaIife.(Anonymous class)`; a clause the body never
/// references still qualifies (`unreferencedClause` and — its parameter
/// at `unknown` — `unreferencedBase` are `unreferenced.(Anonymous
/// class)`); `instantiatedInside` (`insidePrototype(1)`) is
/// `insidePrototype.C`; `objectKey` is `K`, `assignedName` `C`;
/// `parenthesized` stays `(Anonymous class)` and `classId` is `Inner`.
#[test]
fn class_expression_references_print_their_instantiated_clauses() {
    assert_probes(
        NAMES,
        &[
            ("method", "Holder.make.(Anonymous class)"),
            ("staticMethod", "Holder.build.(Anonymous class)"),
            ("arrowHeld", "arrow.(Anonymous class)"),
            ("functionExpression", "fexpr.(Anonymous class)"),
            ("namedFunctionExpression", "nm.(Anonymous class)"),
            ("objectMethod", "o.m.(Anonymous class)"),
            ("outerOfNested", "viaLocalArrow.(Anonymous class)"),
            ("innerClause", "inner.(Anonymous class)"),
            ("twoClauses", "bothClauses.inner.(Anonymous class)"),
            ("iife", "viaIife.(Anonymous class)"),
            ("unreferencedClause", "unreferenced.(Anonymous class)"),
            ("unreferencedBase", "unreferenced.(Anonymous class)"),
            ("instantiatedInside", "insidePrototype.C"),
            ("objectKey", "K"),
            ("assignedName", "C"),
            ("parenthesized", "(Anonymous class)"),
            ("classId", "Inner"),
        ],
    );
}

/// A reference read inside the declaring body keeps every clause at its
/// own parameters and prints the bare name; a prototype read is the class
/// over `any` for every type parameter it has, outer ones included, so it
/// prints qualified.
///
/// Oracle (tsc 7.0.2): inside `inside<T>(x: T)`, `new C()` prints `C`
/// (`const i: null = new C()`), while `C.prototype` prints `inside.C`
/// (`const p: null = C.prototype`); inside `namespace N { function
/// make<T>() }`, `C.prototype` prints `make.C` — a namespace never
/// qualifies its member.
#[test]
fn class_expression_reference_in_its_own_body_prints_unqualified() {
    with_dispatch(NAMES, |dispatch| {
        let result = clean_result(dispatch, "inside", FunctionPartIdentity::DeclarationBody);
        let (_, instance) = constructor_and_instance(dispatch, result.return_type());
        assert_eq!(printed_name(dispatch, instance), "C");
        let result = clean_result(
            dispatch,
            "insidePrototype",
            FunctionPartIdentity::DeclarationBody,
        );
        assert_eq!(
            printed_name(dispatch, result.return_type()),
            "insidePrototype.C"
        );
        let result = clean_result(dispatch, "N.make", FunctionPartIdentity::DeclarationBody);
        assert_eq!(printed_name(dispatch, result.return_type()), "make.C");
    });
}

/// A class member's class clause precedes the member's own, and each
/// prints on its own: instantiating the class clause alone prints the
/// class, and instantiating both prints the class, then the member under
/// its class.
///
/// Oracle (tsc 7.0.2): `new GHolder<number>().make()`'s instance prints
/// `GHolder.(Anonymous class)`, and `new
/// GHolder<number>().make2<string>()`'s prints
/// `GHolder.GHolder.make2.(Anonymous class)`. The references are
/// instantiated here by substituting the served members' own binders: a
/// receiver's type arguments do not reach a body-derived member return on
/// any class (`GH<number>`'s `get() { return null as unknown as H; }`
/// reads `H`), which is not a class-expression rule.
#[test]
fn class_member_clauses_print_class_then_member() {
    with_dispatch(NAMES, |dispatch| {
        let graph = dispatch.graph();
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let instance_of = |ordinal: u32| {
            let result = clean_result(
                dispatch,
                "GHolder",
                FunctionPartIdentity::Member {
                    member_path: Arc::from(vec![ordinal].into_boxed_slice()),
                },
            );
            constructor_and_instance(dispatch, result.return_type()).1
        };
        let arguments = |instance: SemanticNodeId| match graph.node_data(instance).as_deref() {
            Some(SemanticNodeData::ClassExpressionInstance { type_arguments, .. }) => {
                type_arguments.to_vec()
            }
            _ => panic!("a class-expression instance"),
        };

        let make = instance_of(0);
        assert_eq!(printed_name(dispatch, make), "(Anonymous class)");
        let [holder] = arguments(make)[..] else {
            panic!("`make` sees the class clause alone");
        };
        let make = dispatch.substitute_semantic_type_param(make, holder, number);
        assert_eq!(printed_name(dispatch, make), "GHolder.(Anonymous class)");

        let make2 = instance_of(1);
        assert_eq!(printed_name(dispatch, make2), "(Anonymous class)");
        let [holder, own] = arguments(make2)[..] else {
            panic!("`make2` sees the class clause, then its own");
        };
        let make2 = dispatch.substitute_semantic_type_param(make2, holder, number);
        assert_eq!(printed_name(dispatch, make2), "GHolder.(Anonymous class)");
        let make2 = dispatch.substitute_semantic_type_param(make2, own, string);
        assert_eq!(
            printed_name(dispatch, make2),
            "GHolder.GHolder.make2.(Anonymous class)"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// Prototype reads and base constraints next to class expressions
// ──────────────────────────────────────────────────────────────────────

const PROTOTYPES: &str = r#"
interface Base { label: string }
export function Mixin<S extends new (...args: any[]) => Base>(Base: S) { return class extends Base { extra = 1; }; }
export function MixinS<S extends new (...args: any[]) => Base>(Base: S) { return class extends Base { s!: S; }; }
export function Plain<S>(s: S) { return class { v = s; }; }
export class Decl { x = 1; static s = ""; }
export declare class DDecl { y: number; }
export function declaredPrototype() { const x: (typeof Decl)['prototype']['x'] = null as any; return x; }
export function ambientPrototype() { const x: (typeof DDecl)['prototype']['y'] = null as any; return x; }
export function mixinBare() { const x: InstanceType<ReturnType<typeof Mixin>> = null as any; return x; }
export function mixinBareMember() { const x: InstanceType<ReturnType<typeof Mixin>>['label'] = null as any; return x; }
export function mixinConstraint() { const x: InstanceType<ReturnType<typeof MixinS>>['s'] = null as any; return x; }
export function unconstrained() { const x: InstanceType<ReturnType<typeof Plain>>['v'] = null as any; return x; }
export function mixinParams() { const x: ConstructorParameters<ReturnType<typeof Mixin>> = null as any; return x; }
"#;

/// A constructor's `prototype` reads the same in its element-access
/// spelling as in its member spelling, on a declared class too.
///
/// Oracle (tsc 7.0.2): `(typeof Decl)['prototype']` is `Decl` and
/// `(typeof DDecl)['prototype']` is `DDecl`, so `['prototype']['x']` is
/// `number` and `['prototype']['y']` is `number`.
#[test]
fn element_access_prototype_reads_a_declared_class_instance() {
    assert_probes(
        PROTOTYPES,
        &[
            ("declaredPrototype", "number"),
            ("ambientPrototype", "number"),
        ],
    );
}

/// `ReturnType` over a generic factory WITHOUT type arguments reads the
/// base signature: a constrained parameter at its constraint, an
/// unconstrained one at `unknown` — so the class a mixin factory returns
/// composes over the constraint's base.
///
/// Oracle (tsc 7.0.2): `InstanceType<ReturnType<typeof Mixin>>` is
/// `Mixin.(Anonymous class) & Base`, its `label` `string`;
/// `InstanceType<ReturnType<typeof MixinS>>['s']` is `new (...args: any[])
/// => Base`; `InstanceType<ReturnType<typeof Plain>>['v']` is `unknown`;
/// `ConstructorParameters<ReturnType<typeof Mixin>>` is `any[]`.
#[test]
fn bare_factory_return_reads_each_parameter_at_its_constraint() {
    assert_probes(
        PROTOTYPES,
        &[
            ("mixinBare", "Mixin.(Anonymous class) & Base"),
            ("mixinBareMember", "string"),
            ("unconstrained", "unknown"),
            ("mixinParams", "any[]"),
        ],
    );
    // `new (...args: any[]) => Base`, read off the construct signature.
    with_probe(PROTOTYPES, "mixinConstraint", |dispatch, node| {
        let Some(SemanticNodeData::Signature {
            kind,
            params,
            return_type,
            ..
        }) = dispatch.graph().node_data(node).as_deref().cloned()
        else {
            panic!(
                "mixinConstraint: expected a construct signature, measured `{}`",
                render_node(dispatch, node, 0)
            );
        };
        assert!(matches!(
            kind,
            crate::semantic_query::SignatureKind::Construct
        ));
        let [rest] = &params[..] else {
            panic!("mixinConstraint: one rest parameter");
        };
        assert!(rest.rest, "mixinConstraint: `...args`");
        assert_matches(dispatch, rest.ty, "any[]", "mixinConstraint parameter");
        assert_matches(dispatch, return_type, "Base", "mixinConstraint instance");
    });
}

// ──────────────────────────────────────────────────────────────────────
// The printed-name rule
// ──────────────────────────────────────────────────────────────────────

/// The checker's `typeReferenceToTypeNode` rule, clause by clause: a
/// clause prints its declaration exactly when one of its arguments is not
/// its own parameter.
///
/// Oracle (tsc 7.0.2), over `function outer3<U>() { function inner<T>()
/// { return class { … }; } }`: `inner<U>()` read inside `outer3` prints
/// `inner.(Anonymous class)` (only the inner clause is instantiated), and
/// `outer3<number>()`'s `inner<string>()` prints `outer3.inner.(Anonymous
/// class)`.
#[test]
fn printed_name_qualifies_exactly_the_instantiated_clauses() {
    let identity = crate::semantic_query::ClassExpressionIdentity {
        canonical_id: Arc::from(FILE),
        owner: TopLevelOwnerId::ordinary_file(),
        offset: 0,
        name: Arc::from("(Anonymous class)"),
        outer_clauses: Arc::from([
            crate::semantic_query::ClassExpressionClause {
                container: Arc::from("outer3"),
                parameters: Arc::from([Arc::from("U")]),
            },
            crate::semantic_query::ClassExpressionClause {
                container: Arc::from("inner"),
                parameters: Arc::from([Arc::from("T")]),
            },
        ]),
        own_arity: 0,
        constructor_visibility: None,
    };
    let (u, t, other) = (SemanticNodeId(1), SemanticNodeId(2), SemanticNodeId(3));
    let is_parameter = |argument: SemanticNodeId, name: &str| {
        (argument == u && name == "U") || (argument == t && name == "T")
    };
    assert_eq!(
        identity.printed_name(&[u, t], is_parameter),
        "(Anonymous class)"
    );
    assert_eq!(
        identity.printed_name(&[u, other], is_parameter),
        "inner.(Anonymous class)"
    );
    assert_eq!(
        identity.printed_name(&[other, other], is_parameter),
        "outer3.inner.(Anonymous class)"
    );
}
