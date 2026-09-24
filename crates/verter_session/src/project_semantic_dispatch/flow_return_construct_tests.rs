//! `new` expressions in the flow-return lane: a construction resolves
//! through the constructor's construct signatures — the shared signature
//! list's construct bucket, resolved by the call executor exactly as a
//! call resolves over its call signatures.
//!
//! Every expected answer was measured on the pinned TypeScript 7.0.2
//! checker (`tsc --declaration --emitDeclarationOnly --strict`) and is
//! quoted beside its row as the emitted `.d.ts` return type. Every row
//! pins the typed degradation and the family memo's candidate count
//! (1 = clean and warm, 0 = a degraded success that admits nothing).

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TopLevelOwnerId, TypeExpr};

const SHARED: &str = "/ctor/shared.ts";
const SHARED_SRC: &str = r#"
export interface Box<T> {
  value: T;
}
export interface Pair<A, B> { left: A; right: B }
"#;

/// Constructor VALUES: declared construct-signature types, reached by name,
/// through a parameter, and through a generic parameter.
const VALUES: &str = "/ctor/values.ts";
const VALUES_SRC: &str = r#"
import { Box, Pair } from "./shared";
declare const make0: (new () => Box<number>) & (new () => Pair<string, string>);
export function witnessNew0() { return new make0(); }

declare const single: new (a: number) => { q: string };
export function newSingle() { return new single(1); }

declare const overloaded: { new (a: string): Box<string>; new (a: number): Pair<number, number> };
export function newOverloadedNumber() { return new overloaded(1); }
export function newOverloadedString() { return new overloaded("s"); }

declare const unionCtor: (new () => Box<number>) | (new () => Pair<string, string>);
export function newUnion() { return new unionCtor(); }

interface CtorIface { new (x: number): Box<number> }
declare const ctorIface: CtorIface;
export function newInterface() { return new ctorIface(1); }

declare const genCtor: new <T>(x: T) => Box<T>;
export function newGenericCtorExplicit() { return new genCtor<string>("a"); }
export function newGenericCtorInferred() { return new genCtor(1); }

export function newParam(c: new () => Box<number>) { return new c(); }
export function newGenericParam<T>(c: new () => T) { return new c(); }

declare const anyCtor: any;
export function newAny() { return new anyCtor(); }

declare function plainFn(): number;
export function newCallOnly() { return new plainFn(); }
"#;

/// CLASS declarations: plain, generic (inferred, explicit, defaulted,
/// constrained), derived, and abstract.
const CLASSES: &str = "/ctor/classes.ts";
const CLASSES_SRC: &str = r#"
export class Plain { constructor(public v: number) {} }
export function newPlain() { return new Plain(1); }

export class NoArgs {}
export function newNoArgsBare() { return new NoArgs; }

export class Gen<T> { constructor(public v: T) {} }
export function newGenInferred() { return new Gen(1); }
export function newGenInferredString() { return new Gen("s"); }
export function newGenExplicit() { return new Gen<string>("s"); }

export class GenDefault<T = boolean> { v?: T }
export function newGenDefault() { return new GenDefault(); }

export class Constrained<T extends string> { constructor(public v: T) {} }
export function newConstrained() { return new Constrained("lit"); }
export function newConstrainedWide(s: string) { return new Constrained(s); }

export class Gen2<T> { constructor(public v: T) {} }
export class Derived<U> extends Gen2<U[]> {}
export function newDerivedGeneric() { return new Derived([1]); }
export class DerivedFixed extends Gen2<string> {}
export function newDerivedFixed() { return new DerivedFixed("s"); }

export abstract class Abs { abstract m(): void }
export function newAbstract() { return new Abs(); }
const AbsAlias = Abs;
export function newAbstractAlias() { return new AbsAlias(); }
export function newAbstractParam(c: typeof Abs) { return new c(); }
declare const absFactory: new () => Abs;
export function newAbstractFactory() { return new absFactory(); }
export abstract class AbsGen<T> { constructor(public v: T) {} }
export function newAbstractGeneric() { return new AbsGen(1); }
export class Concrete extends Abs { m() {} }
export function newConcreteSub() { return new Concrete(); }
export abstract class AbsMid extends Abs {}
export function newAbstractMid() { return new AbsMid(); }
"#;

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(canonical.to_string()),
        input_id: canonical.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(canonical)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

fn host_with(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (canonical, source) in files {
        upsert(&host, canonical, source);
    }
    host
}

fn ctor_host() -> Arc<VerterHost> {
    host_with(&[
        (SHARED, SHARED_SRC),
        (VALUES, VALUES_SRC),
        (CLASSES, CLASSES_SRC),
    ])
}

/// One evaluated function's public outcome.
#[derive(Debug, PartialEq)]
struct Outcome {
    ty: TypeExpr,
    degradation: Option<FlowReturnDegradation>,
    candidates: usize,
}

#[track_caller]
fn eval(host: &Arc<VerterHost>, canonical: &str, name: &str) -> Outcome {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            TopLevelOwnerId::ordinary_file(),
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
fn assert_clean_warm(host: &Arc<VerterHost>, canonical: &str, name: &str, expected: TypeExpr) {
    assert_eq!(
        eval(host, canonical, name),
        Outcome {
            ty: expected,
            degradation: None,
            candidates: 1,
        },
        "{name}"
    );
}

/// The construction is REFUSED: a degraded success carrying the typed
/// unresolved marker, admitting nothing.
#[track_caller]
fn assert_refused(host: &Arc<VerterHost>, canonical: &str, name: &str) {
    let outcome = eval(host, canonical, name);
    assert_eq!(
        outcome.degradation,
        Some(FlowReturnDegradation::UnrepresentableCallee),
        "{name} degradation (got {outcome:?})"
    );
    assert_eq!(outcome.candidates, 0, "{name} admits nothing");
}

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

fn applied(name: &str, args: Vec<TypeExpr>) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(args.into_boxed_slice()),
    }
}

fn named(name: &str) -> TypeExpr {
    applied(name, Vec::new())
}

/// A construction over a declared constructor VALUE takes the result of
/// the construct signature the arguments select.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// witnessNew0             new make0()               Box<number>
/// newSingle               new single(1)             { q: string; }
/// newOverloadedNumber     new overloaded(1)         Pair<number, number>
/// newOverloadedString     new overloaded("s")       Box<string>
/// newUnion                new unionCtor()           Box<number> | Pair<string, string>
/// newInterface            new ctorIface(1)          Box<number>
/// newGenericCtorExplicit  new genCtor<string>("a")  Box<string>
/// newGenericCtorInferred  new genCtor(1)            Box<number>
/// newParam                new c()                   Box<number>
/// newGenericParam         new c()                   T
/// newAny                  new anyCtor()             any
/// ```
///
/// `make0` is an intersection of two construct signatures that differ only
/// in their result: the first one applies. The union member order is the
/// stable canonical order, so the union row compares members as a set.
///
/// Mutation: removing the content half's `NewExpression` arm returns every
/// row to the unmodelled-position marker (`UnmodeledPosition`, 0
/// candidates); resolving the construction over the CALL bucket makes every
/// row a refused callee.
#[test]
fn a_construction_over_a_constructor_value_takes_the_selected_construct_signature() {
    let host = ctor_host();
    assert_clean_warm(
        &host,
        VALUES,
        "witnessNew0",
        applied("Box", vec![primitive(PrimitiveName::Number)]),
    );
    let single = eval(&host, VALUES, "newSingle");
    assert_eq!((single.degradation, single.candidates), (None, 1));
    let TypeExpr::Object(object) = &single.ty else {
        panic!("newSingle must be the signature's object result, got {single:?}");
    };
    assert!(
        matches!(
            object.properties.as_slice(),
            [verter_type_expr::ObjectMember::Property(property)]
                if property.key.as_string() == Some("q")
                    && property.ty == primitive(PrimitiveName::String)
        ),
        "newSingle is `{{ q: string }}`, got {single:?}"
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newOverloadedNumber",
        applied(
            "Pair",
            vec![
                primitive(PrimitiveName::Number),
                primitive(PrimitiveName::Number),
            ],
        ),
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newOverloadedString",
        applied("Box", vec![primitive(PrimitiveName::String)]),
    );
    let union = eval(&host, VALUES, "newUnion");
    assert_eq!((union.degradation, union.candidates), (None, 1));
    let TypeExpr::Union(members) = &union.ty else {
        panic!("newUnion must be a union, got {union:?}");
    };
    assert_eq!(members.len(), 2, "newUnion: {union:?}");
    for expected in [
        applied("Box", vec![primitive(PrimitiveName::Number)]),
        applied(
            "Pair",
            vec![
                primitive(PrimitiveName::String),
                primitive(PrimitiveName::String),
            ],
        ),
    ] {
        assert!(members.contains(&expected), "newUnion lacks {expected:?}");
    }
    assert_clean_warm(
        &host,
        VALUES,
        "newInterface",
        applied("Box", vec![primitive(PrimitiveName::Number)]),
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newGenericCtorExplicit",
        applied("Box", vec![primitive(PrimitiveName::String)]),
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newGenericCtorInferred",
        applied("Box", vec![primitive(PrimitiveName::Number)]),
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newParam",
        applied("Box", vec![primitive(PrimitiveName::Number)]),
    );
    assert_clean_warm(
        &host,
        VALUES,
        "newGenericParam",
        TypeExpr::TypeParameter(verter_type_expr::TypeParam {
            name: "T".to_string(),
            constraint: None,
            default: None,
            is_const: false,
        }),
    );
    assert_clean_warm(&host, VALUES, "newAny", primitive(PrimitiveName::Any));
}

/// A construction over a value with NO construct signature is refused.
///
/// TypeScript 7.0.2 reports TS7009 ("'new' expression, whose target lacks a
/// construct signature, implicitly has an 'any' type") and answers the
/// error type `any` for `newCallOnly`. A refusal is the lane's typed answer
/// for a checker error, never the error type published as a clean `any`.
#[test]
fn a_construction_without_a_construct_signature_is_refused() {
    let host = ctor_host();
    assert_refused(&host, VALUES, "newCallOnly");
}

/// A construction over a CLASS declaration is the class instance, and a
/// generic class infers its type parameters from the constructor arguments
/// — the class's construct signatures carry the class's type parameters as
/// their own clause.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// newPlain              new Plain(1)              Plain
/// newNoArgsBare         new NoArgs                NoArgs
/// newGenInferred        new Gen(1)                Gen<number>
/// newGenInferredString  new Gen("s")              Gen<string>
/// newGenExplicit        new Gen<string>("s")      Gen<string>
/// newGenDefault         new GenDefault()          GenDefault<boolean>
/// newConstrained        new Constrained("lit")    Constrained<"lit">
/// newConstrainedWide    new Constrained(s)        Constrained<string>
/// newDerivedGeneric     new Derived([1])          Derived<number>
/// newDerivedFixed       new DerivedFixed("s")     DerivedFixed
/// newConcreteSub        new Concrete()            Concrete
/// ```
///
/// `Derived<U>` declares no constructor, so it inherits `Gen2<U[]>`'s
/// parameter under its own clause (`new <U>(v: U[]) => Derived<U>`).
///
/// Mutation: leaving an unapplied generic class's construct signatures
/// without the class clause turns every generic row into a refused callee
/// (the argument cannot relate to the class's free binder).
#[test]
fn a_construction_over_a_class_is_the_instance_inferred_from_the_arguments() {
    let host = ctor_host();
    assert_clean_warm(&host, CLASSES, "newPlain", named("Plain"));
    assert_clean_warm(&host, CLASSES, "newNoArgsBare", named("NoArgs"));
    assert_clean_warm(
        &host,
        CLASSES,
        "newGenInferred",
        applied("Gen", vec![primitive(PrimitiveName::Number)]),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newGenInferredString",
        applied("Gen", vec![primitive(PrimitiveName::String)]),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newGenExplicit",
        applied("Gen", vec![primitive(PrimitiveName::String)]),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newGenDefault",
        applied("GenDefault", vec![primitive(PrimitiveName::Boolean)]),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newConstrained",
        applied(
            "Constrained",
            vec![TypeExpr::Literal(verter_type_expr::LiteralValue::String(
                "lit".to_string(),
            ))],
        ),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newConstrainedWide",
        applied("Constrained", vec![primitive(PrimitiveName::String)]),
    );
    assert_clean_warm(
        &host,
        CLASSES,
        "newDerivedGeneric",
        applied("Derived", vec![primitive(PrimitiveName::Number)]),
    );
    assert_clean_warm(&host, CLASSES, "newDerivedFixed", named("DerivedFixed"));
    assert_clean_warm(&host, CLASSES, "newConcreteSub", named("Concrete"));
}

/// A construction over an ABSTRACT class's own construct signatures is
/// refused, however the class is reached; a construct signature TYPED over
/// the abstract class is not the class's own and stays constructible.
///
/// TypeScript 7.0.2 reports TS2511 ("Cannot create an instance of an
/// abstract class") for every refused row below and answers the error type
/// (`.d.ts`: `any`):
///
/// ```text
/// newAbstract         new Abs()          any    (TS2511)
/// newAbstractAlias    new AbsAlias()     any    (TS2511)
/// newAbstractParam    new c()            any    (TS2511)
/// newAbstractGeneric  new AbsGen(1)      any    (TS2511)
/// newAbstractMid      new AbsMid()       any    (TS2511)
/// newAbstractFactory  new absFactory()   Abs
/// ```
///
/// Mutation: dropping the executor's abstract-class refusal publishes the
/// instance (`Abs`, `AbsGen<number>`, `AbsMid`) clean and warm for every
/// refused row.
#[test]
fn a_construction_over_an_abstract_class_is_refused_like_the_checker() {
    let host = ctor_host();
    for name in [
        "newAbstract",
        "newAbstractAlias",
        "newAbstractParam",
        "newAbstractGeneric",
        "newAbstractMid",
    ] {
        assert_refused(&host, CLASSES, name);
    }
    assert_clean_warm(&host, CLASSES, "newAbstractFactory", named("Abs"));
}

/// Making a constructed class abstract misses the warm construction in a
/// file that imports it.
///
/// TypeScript 7.0.2: `new K()` is `K` over `export class K {}` and TS2511
/// (`any`) over `export abstract class K {}`.
///
/// Mutation: the same as the abstract refusal above — without it the
/// second read publishes `K` again.
#[test]
fn an_edit_that_makes_a_class_abstract_misses_its_warm_construction() {
    const LIB: &str = "/ctor/edit/lib.ts";
    const USE: &str = "/ctor/edit/use.ts";
    let host = host_with(&[
        (LIB, "export class K {}\n"),
        (
            USE,
            "import { K } from \"./lib\";\nexport function build() { return new K(); }\n",
        ),
    ]);
    assert_clean_warm(&host, USE, "build", named("K"));
    upsert(&host, LIB, "export abstract class K {}\n");
    // The slot still holds the pre-edit candidate; the read must not serve
    // it.
    let edited = eval(&host, USE, "build");
    assert_eq!(
        edited.degradation,
        Some(FlowReturnDegradation::UnrepresentableCallee),
        "the construction over the edited class is refused (got {edited:?})"
    );
}
