//! Tagged templates in the flow-return lane: `` tag`a${x}b` `` is a CALL of
//! its tag whose first argument is the template strings (a value of the
//! GLOBAL `TemplateStringsArray` type) and whose remaining arguments are the
//! substitutions, resolved through the same call executor an ordinary call
//! takes — overload choice, argument inference, explicit type arguments and
//! the call-boundary literal widening included.
//!
//! Every expected answer was measured on the pinned TypeScript 7.0.2
//! checker (`tsc --declaration --emitDeclarationOnly --strict`) and is
//! quoted beside its row as the emitted `.d.ts` return type. The standalone
//! host carries no lib, so a script declares the global
//! `TemplateStringsArray` the lib would.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr};

/// The global the lib declares: a script's file-scope interface.
const LIB: &str = "/tag/lib.ts";
const LIB_SRC: &str = r#"
interface TemplateStringsArray { readonly raw: readonly string[]; readonly length: number }
"#;

const TAGS: &str = "/tag/tags.ts";
const TAGS_SRC: &str = r#"
export class Box { readonly tag = "box" }
declare function box(strings: TemplateStringsArray): Box;
export function tagBare() { return box`b`; }
declare function tagNums(strings: TemplateStringsArray, ...v: number[]): boolean;
export function tagSubs() { return tagNums`a${1}b${2}c`; }
declare function gt<T>(s: TemplateStringsArray, v: T): T;
export function tagGeneric() { return gt`${1}`; }
export function tagGenericString() { return gt`${"s"}`; }
declare function gtArr<T>(s: TemplateStringsArray, ...v: T[]): T[];
export function tagGenericRest() { return gtArr`${1}${2}`; }
export function tagGenericRestStrings() { return gtArr`${"a"}${"b"}`; }
declare function ot(s: TemplateStringsArray, v: string): "S";
declare function ot(s: TemplateStringsArray, v: number): "N";
export function tagOverloadNumber() { return ot`${1}`; }
export function tagOverloadString() { return ot`${"x"}`; }
declare function ut(s: TemplateStringsArray): string | number;
export function tagUnion() { return ut`u`; }
declare function ro(s: readonly string[]): "ro";
export function tagReadonlyArray() { return ro`r`; }
declare const objTag: { t(s: TemplateStringsArray): "member" };
export function tagMember() { return objTag.t`m`; }
export function tagParam(f: (s: TemplateStringsArray) => number) { return f`p`; }
declare function tsaRet(s: TemplateStringsArray): TemplateStringsArray;
export function tagReturnsStrings() { return tsaRet`z`; }
export function tagRawField(s: TemplateStringsArray) { return s.raw; }
declare function gtS<T>(s: TemplateStringsArray): T;
export function tagGenericNoEvidence() { return gtS`n`; }
export function tagExplicit() { return gtS<string>`n`; }
export function tagInObject() { return { label: "x", made: box`b` }; }
declare function strict(s: TemplateStringsArray, v: string): 1;
export function tagMismatch() { return strict`${1}`; }
export function tagStatement() { box`s`; return 1; }
export function tagSequence() { return (0, box`q`); }
export function tagLocalBinding() { const b = box`l`; return b; }
export function tagLocalTag() { const t = box; return t`x`; }
export function tagConditional(f: boolean) { return f ? box`c` : 1; }
export function tagTemplateInTemplate() { return `${box`t`}`; }
declare const thisObj: { m(this: { k: 1 }, s: TemplateStringsArray): string };
export function tagThisMismatch() { return thisObj.m`x`; }
declare function gtLit<T extends string>(s: TemplateStringsArray, v: T): T;
export function tagConstrainedLiteral() { return gtLit`${"lit"}`; }
declare function nested(s: TemplateStringsArray): (s: TemplateStringsArray) => 7;
export function tagCurried() { return nested`a``b`; }
export const moduleTag = box`m`;
"#;

/// A tag never narrows: the checker binds no call flow node for a tagged
/// template, whatever its tag's signature.
const NARROW: &str = "/tag/narrow.ts";
const NARROW_SRC: &str = r#"
declare function assertStr(s: TemplateStringsArray, v: unknown): asserts v is string;
declare function isStr(s: TemplateStringsArray, v: unknown): v is string;
export function discardedAssert(x: unknown) { assertStr`${x}`; return x; }
export function controlPredicate(x: unknown) { if (isStr`${x}`) { return x; } return 0; }
export function ternaryPredicate(x: unknown) { return isStr`${x}` ? x : 0; }
declare function log(s: TemplateStringsArray, ...v: unknown[]): void;
export function statementTag(x: string) { log`x=${x}`; return x; }
"#;

/// A module-local `TemplateStringsArray` does not rename the strings' type.
const SHADOW: &str = "/tag/shadow.ts";
const SHADOW_SRC: &str = r#"
interface TemplateStringsArray { local: true }
declare function sel(s: TemplateStringsArray): "local";
declare function sel(s: { readonly raw: readonly string[] }): "global";
export function tagShadow() { return sel`x`; }
"#;

/// Several fresh candidates for one binder — the arguments of a rest
/// parameter — widen at the call boundary exactly as one does.
const REST: &str = "/tag/rest.ts";
const REST_SRC: &str = r#"
declare function restArr<T>(...v: T[]): T[];
declare function restPick<T>(...v: T[]): T;
declare function restBox<T>(...v: T[]): { v: T };
declare const two: 2;
export function restArrLits() { return restArr(1, 2); }
export function restArrMixed() { return restArr(1, two); }
export function restArrSame() { return restArr(1, 1); }
export function restPickLits() { return restPick(1, 2); }
export function restPickObj() { return { p: restPick(1, 2) }; }
export function restBoxLits() { return restBox("a", "b"); }
"#;

fn host_with(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (canonical, source) in files {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some((*canonical).to_string()),
            input_id: (*canonical).to_string(),
            source: Arc::from(*source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        });
    }
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

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

fn string_lit(value: &str) -> TypeExpr {
    TypeExpr::Literal(LiteralValue::String(value.to_string()))
}

fn number_lit(value: f64) -> TypeExpr {
    TypeExpr::Literal(LiteralValue::Number(value))
}

fn named(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    }
}

fn array_of(element: TypeExpr, readonly: bool) -> TypeExpr {
    TypeExpr::Array {
        element: Arc::new(element),
        readonly,
    }
}

/// The type of one member of a published object.
#[track_caller]
fn member(ty: &TypeExpr, key: &str) -> TypeExpr {
    let TypeExpr::Object(shape) = ty else {
        panic!("expected an object, got {ty:?}");
    };
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
        .unwrap_or_else(|| panic!("member `{key}` missing in {ty:?}"))
}

/// A tagged template resolves as a call of its tag, clean and warm.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// tagBare                box`b`                   Box
/// tagSubs                tagNums`a${1}b${2}c`     boolean
/// tagGeneric             gt`${1}`                 number
/// tagGenericString       gt`${"s"}`               string
/// tagGenericRest         gtArr`${1}${2}`          number[]
/// tagGenericRestStrings  gtArr`${"a"}${"b"}`      string[]
/// tagOverloadNumber      ot`${1}`                 "N"
/// tagOverloadString      ot`${"x"}`               "S"
/// tagUnion               ut`u`                    string | number
/// tagReadonlyArray       ro`r`                    "ro"
/// tagMember              objTag.t`m`              "member"
/// tagParam               f`p`                     number
/// tagReturnsStrings      tsaRet`z`                TemplateStringsArray
/// tagRawField            s.raw                    readonly string[]
/// tagGenericNoEvidence   gtS`n`                   unknown
/// tagExplicit            gtS<string>`n`           string
/// tagInObject            { made: box`b` }         { label: string; made: Box; }
/// tagMismatch            strict`${1}`             1          (TS2345; lone candidate)
/// tagStatement           box`s`; return 1         number
/// tagSequence            (0, box`q`)              Box
/// tagLocalBinding        const b = box`l`         Box
/// tagLocalTag            const t = box; t`x`      Box
/// tagConditional         f ? box`c` : 1           1 | Box
/// tagTemplateInTemplate  `${box`t`}`              string
/// tagThisMismatch        thisObj.m`x`             string     (TS2741; lone candidate)
/// tagConstrainedLiteral  gtLit`${"lit"}`          "lit"
/// tagCurried             nested`a``b`             7
/// ```
///
/// Mutation: dropping the content half's tagged-template arm fails every
/// row closed as an unmodelled call position; dropping the tagged templates
/// from the program index's call-site map leaves the executor rows
/// (generic, overloaded, explicit) without their authored call and degrades
/// them.
#[test]
fn a_tagged_template_is_a_call_of_its_tag() {
    use PrimitiveName::{Boolean, Number, String, Unknown};
    let host = host_with(&[(LIB, LIB_SRC), (TAGS, TAGS_SRC)]);
    for (name, expected) in [
        ("tagBare", named("Box")),
        ("tagSubs", primitive(Boolean)),
        ("tagGeneric", primitive(Number)),
        ("tagGenericString", primitive(String)),
        ("tagGenericRest", array_of(primitive(Number), false)),
        ("tagGenericRestStrings", array_of(primitive(String), false)),
        ("tagOverloadNumber", string_lit("N")),
        ("tagOverloadString", string_lit("S")),
        (
            "tagUnion",
            TypeExpr::union(vec![primitive(Number), primitive(String)]),
        ),
        ("tagReadonlyArray", string_lit("ro")),
        ("tagMember", string_lit("member")),
        ("tagParam", primitive(Number)),
        ("tagReturnsStrings", named("TemplateStringsArray")),
        ("tagRawField", array_of(primitive(String), true)),
        ("tagGenericNoEvidence", primitive(Unknown)),
        ("tagExplicit", primitive(String)),
        ("tagMismatch", number_lit(1.0)),
        ("tagStatement", primitive(Number)),
        ("tagSequence", named("Box")),
        ("tagLocalBinding", named("Box")),
        ("tagLocalTag", named("Box")),
        (
            "tagConditional",
            TypeExpr::union(vec![named("Box"), number_lit(1.0)]),
        ),
        ("tagTemplateInTemplate", primitive(String)),
        ("tagThisMismatch", primitive(String)),
        ("tagConstrainedLiteral", string_lit("lit")),
        ("tagCurried", number_lit(7.0)),
    ] {
        assert_clean_warm(&host, TAGS, name, expected);
    }
    let object = eval(&host, TAGS, "tagInObject");
    assert_eq!(
        (object.degradation, object.candidates),
        (None, 1),
        "tagInObject"
    );
    assert_eq!(member(&object.ty, "label"), primitive(String));
    assert_eq!(member(&object.ty, "made"), named("Box"));
}

/// A tag never narrows, in a statement, a test or a discarded position:
/// the checker binds no call flow node for a tagged template, so neither an
/// `asserts` tag nor a type-predicate tag narrows the read that follows.
///
/// TypeScript 7.0.2: `discardedAssert`, `controlPredicate` and
/// `ternaryPredicate` are `unknown`; `statementTag` is `string`.
///
/// Mutation: certifying a result-independent tagged template like an
/// unproven call (a tag the flow half cannot show non-narrowing) flags every
/// row's typed guard-narrowing gap and nothing warms.
#[test]
fn a_tagged_template_never_narrows() {
    let host = host_with(&[(LIB, LIB_SRC), (NARROW, NARROW_SRC)]);
    for name in ["discardedAssert", "controlPredicate", "ternaryPredicate"] {
        assert_clean_warm(&host, NARROW, name, primitive(PrimitiveName::Unknown));
    }
    assert_clean_warm(
        &host,
        NARROW,
        "statementTag",
        primitive(PrimitiveName::String),
    );
}

/// The template strings are the GLOBAL `TemplateStringsArray`, whatever the
/// call's own scope declares under that name.
///
/// TypeScript 7.0.2: `tagShadow` is `"global"` — the module-local
/// `TemplateStringsArray { local: true }` types only the first overload's
/// parameter, which the global strings do not satisfy.
///
/// With no global declaration at all, the shadowed scope has no name for
/// the strings' type: an overloaded tag then has no argument evidence to
/// choose by and degrades typed, never picking the local reading.
///
/// Mutation: typing the strings by the call scope's own reading of the name
/// selects the first overload (`"local"`).
#[test]
fn the_template_strings_are_the_global_type() {
    let host = host_with(&[(LIB, LIB_SRC), (SHADOW, SHADOW_SRC)]);
    assert_clean_warm(&host, SHADOW, "tagShadow", string_lit("global"));

    let host = host_with(&[(SHADOW, SHADOW_SRC)]);
    let outcome = eval(&host, SHADOW, "tagShadow");
    assert_eq!(
        (outcome.degradation, outcome.candidates),
        (Some(FlowReturnDegradation::UnrepresentableCallee), 0),
        "no global strings type: the overloaded tag fails closed, got {outcome:?}"
    );
}

/// Several fresh candidates for one binder — the arguments of a rest
/// parameter — widen at the call boundary like one: each fresh literal arm
/// widens unless the binder is kept at the return's top level, and a kept
/// union stays fresh for a caller's member position.
///
/// TypeScript 7.0.2:
///
/// ```text
/// restArrLits    restArr(1, 2)             number[]
/// restArrMixed   restArr(1, two)           number[]     (two: 2)
/// restArrSame    restArr(1, 1)             number[]
/// restPickLits   restPick(1, 2)            1 | 2        (binder kept at top level)
/// restPickObj    { p: restPick(1, 2) }     { p: number; }
/// restBoxLits    restBox("a", "b")         { v: string; }
/// ```
///
/// Mutation: widening only a lone literal candidate publishes `(2 | 1)[]`
/// for `restArrLits`; tracking only a lone fresh return publishes
/// `{ p: 2 | 1 }` for `restPickObj`.
#[test]
fn rest_argument_literals_widen_like_one_argument() {
    use PrimitiveName::{Number, String};
    let host = host_with(&[(REST, REST_SRC)]);
    for name in ["restArrLits", "restArrMixed", "restArrSame"] {
        assert_clean_warm(&host, REST, name, array_of(primitive(Number), false));
    }
    let kept = eval(&host, REST, "restPickLits");
    assert_eq!((kept.degradation, kept.candidates), (None, 1));
    let TypeExpr::Union(arms) = &kept.ty else {
        panic!("restPickLits must be a union, got {kept:?}");
    };
    let mut arms: Vec<TypeExpr> = arms.to_vec();
    arms.sort_by(|left, right| format!("{left:?}").cmp(&format!("{right:?}")));
    assert_eq!(arms, vec![number_lit(1.0), number_lit(2.0)]);
    let object = eval(&host, REST, "restPickObj");
    assert_eq!((object.degradation, object.candidates), (None, 1));
    assert_eq!(member(&object.ty, "p"), primitive(Number));
    let boxed = eval(&host, REST, "restBoxLits");
    assert_eq!((boxed.degradation, boxed.candidates), (None, 1));
    assert_eq!(member(&boxed.ty, "v"), primitive(String));
}

/// A module `const` initialized by a tagged template is typed by the tag's
/// return, like one initialized by a call: its value derives from a call,
/// so it takes the indexed call rail instead of value inference.
///
/// TypeScript 7.0.2: `moduleTag` is `Box`.
///
/// Mutation: letting value inference answer a tagged template leaves
/// `typeof moduleTag` a miss.
#[test]
fn a_module_const_bound_to_a_tagged_template_is_the_tag_return() {
    let host = host_with(&[(LIB, LIB_SRC), (TAGS, TAGS_SRC)]);
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let env = host.host_view_env_hashes_for(TAGS);
    let project_identity = host.host_view_project_identity_for(TAGS).fold_u32();
    let node = match dispatch.execute_type_node(SemanticQueryKey::TypeOf {
        value_root: crate::semantic_query::ValueRootSlotIdentity::new(
            crate::semantic_query::ValueRootKey {
                scope: crate::semantic_query::ScopeId::file(
                    Arc::from(TAGS),
                    TopLevelOwnerId::ordinary_file(),
                ),
                name: Arc::from("moduleTag"),
            },
            project_identity,
            env.type_env_hash,
            env.lib_env_hash,
        ),
        path: Arc::from([]),
        context: crate::semantic_query::TypeOfContext::new(
            crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Expanded,
            ),
            env.resolve_env_hash,
        ),
    }) {
        QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
        other => panic!("`typeof moduleTag` must resolve, got {other:?}"),
    };
    assert_eq!(
        host.project_node_to_type_expr_for_test(node),
        Some(named("Box"))
    );
}
