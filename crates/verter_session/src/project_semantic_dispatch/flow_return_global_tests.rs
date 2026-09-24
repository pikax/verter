//! Reads through the program's GLOBAL declarations in the flow-return
//! lane: a bare type name no scope declares or imports names the merged
//! global declaration — every module's `declare global` contribution and
//! every script's file-scope interface, in declaration precedence order.
//! A bare VALUE name no scope binds, and a `globalThis` member path, read
//! the global value declaration the same way: a module's `declare global`
//! `var` / `let` / `const` / `function`, or a script's file-scope value.
//!
//! Every expected answer was measured on the pinned TypeScript 7.0.2
//! checker (`tsc --declaration --emitDeclarationOnly --strict` over the
//! same files) and is quoted beside its row as the emitted `.d.ts` return
//! type. Every row pins the typed degradation and the family memo's
//! candidate count.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TopLevelOwnerId, TypeExpr};

const AUGMENT: &str = "/g/augment.ts";
const AUGMENT_SRC: &str = "declare global { interface SkGlobal { tag: string } }\nexport {};\n";

const AUGMENT2: &str = "/g/augment2.ts";
const AUGMENT2_SRC: &str = r#"declare global {
  interface SkGlobal { extra: number }
  interface SkBox<T> { value: T }
  var skVar: boolean;
}
export {};
"#;

/// A module that contributes to the global AND reads it.
const SELF: &str = "/g/self.ts";
const SELF_SRC: &str = r#"declare global { interface SkGlobal { own: boolean } }
export function readOwn(g: SkGlobal) { return g.own; }
export function readOthersFromSelf(g: SkGlobal) { return g.extra; }
export function readTagFromSelf(g: SkGlobal) { return g.tag; }
"#;

/// A SCRIPT (no module syntax): its `declare global` binds nothing — not
/// the name it alone declares, and not its member of a global other
/// modules declare — and its file-scope interface IS global.
const SCRIPT_AUG: &str = "/g/aug-script.ts";
const SCRIPT_AUG_SRC: &str =
    "declare global { interface SkScript { tag: string } interface SkGlobal { fromScript: string } }\n";
const SCRIPT_IFACE: &str = "/g/iface-script.ts";
const SCRIPT_IFACE_SRC: &str = "interface SkPlain { tag: string }\n";

const READER: &str = "/g/m0.ts";
const READER_SRC: &str = r#"export function witnessGlobal0(g: SkGlobal) { return g.tag; }
export function readExtra(g: SkGlobal) { return g.extra; }
export function readOwnCross(g: SkGlobal) { return g.own; }
export function readWhole(g: SkGlobal) { return g; }
export function readBoxValue(b: SkBox<number>) { return b.value; }
export function readScriptAug(g: SkScript) { return g.tag; }
export function readScriptPlain(g: SkPlain) { return g.tag; }
export function readFromScript(g: SkGlobal) { return g.fromScript; }
export function readGlobalThisVar() { return globalThis.skVar; }
export function readBareVar() { return skVar; }
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

fn global_host() -> Arc<VerterHost> {
    host_with(&[
        (AUGMENT, AUGMENT_SRC),
        (AUGMENT2, AUGMENT2_SRC),
        (SELF, SELF_SRC),
        (SCRIPT_AUG, SCRIPT_AUG_SRC),
        (SCRIPT_IFACE, SCRIPT_IFACE_SRC),
        (READER, READER_SRC),
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

/// The read fails closed: a degraded success with the typed unresolved
/// value, admitting nothing.
#[track_caller]
fn assert_unresolved(host: &Arc<VerterHost>, canonical: &str, name: &str) {
    let outcome = eval(host, canonical, name);
    assert_eq!(
        outcome.degradation,
        Some(FlowReturnDegradation::UnresolvedValue),
        "{name} degradation (got {outcome:?})"
    );
    assert_eq!(outcome.candidates, 0, "{name} admits nothing");
}

fn primitive(name: PrimitiveName) -> TypeExpr {
    TypeExpr::Primitive(name)
}

/// A member read through a global interface sees EVERY declaration of it,
/// from a file that declares none of them.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// witnessGlobal0  g.tag       string     (augment.ts)
/// readExtra       g.extra     number     (augment2.ts)
/// readOwnCross    g.own       boolean    (self.ts)
/// readWhole       g           SkGlobal
/// readBoxValue    b.value     number     (SkBox<number>, augment2.ts)
/// ```
///
/// `readWhole` keeps the global's NAME: the reference names the merged
/// global through its first declaration rather than dissolving it into
/// the merged structure.
///
/// Mutation: without the bare-name global fallback every row reads through
/// the unresolved `BareRef` and degrades (`UnresolvedValue`, 0
/// candidates); without the global fold in `Instantiate`, `readExtra` and
/// `readOwnCross` — members of declarations other than the one the
/// reference names — degrade the same way.
#[test]
fn a_global_member_read_resolves_through_every_global_declaration() {
    let host = global_host();
    assert_clean_warm(
        &host,
        READER,
        "witnessGlobal0",
        primitive(PrimitiveName::String),
    );
    assert_clean_warm(&host, READER, "readExtra", primitive(PrimitiveName::Number));
    assert_clean_warm(
        &host,
        READER,
        "readOwnCross",
        primitive(PrimitiveName::Boolean),
    );
    assert_clean_warm(
        &host,
        READER,
        "readWhole",
        TypeExpr::Ref {
            name: Arc::from("SkGlobal"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    );
    assert_clean_warm(
        &host,
        READER,
        "readBoxValue",
        primitive(PrimitiveName::Number),
    );
}

/// A module that contributes a `declare global` interface and reads it
/// sees the OTHER modules' declarations too.
///
/// TypeScript 7.0.2 (`.d.ts` return types of `self.ts`):
///
/// ```text
/// readOwn             g.own     boolean
/// readOthersFromSelf  g.extra   number
/// readTagFromSelf     g.tag     string
/// ```
///
/// Mutation: without the global fold in `Instantiate` the same-file read
/// sees only its own declaration, and the two cross-file rows degrade.
#[test]
fn a_global_contributor_reads_every_other_global_declaration() {
    let host = global_host();
    assert_clean_warm(&host, SELF, "readOwn", primitive(PrimitiveName::Boolean));
    assert_clean_warm(
        &host,
        SELF,
        "readOthersFromSelf",
        primitive(PrimitiveName::Number),
    );
    assert_clean_warm(
        &host,
        SELF,
        "readTagFromSelf",
        primitive(PrimitiveName::String),
    );
}

/// Edits to a global declaration — the declaration the reference names
/// and one folded in beside it — and a declaration added later all miss
/// the warm read.
///
/// TypeScript 7.0.2: with `augment.ts` (`tag: string`) and `augment2.ts`
/// (`extra: number`), `g.tag` is `string` and `g.extra` is `number`; with
/// `extra: string` in `augment2.ts` `g.extra` is `string`; with `tag:
/// number` in `augment.ts` `g.tag` is `number`; once `self.ts` joins,
/// `g.own` is `boolean`.
///
/// Mutation: without the global fold in `Instantiate`, `readExtra` never
/// resolves (`augment2.ts` is not the declaration the reference names).
#[test]
fn an_edit_to_a_global_declaration_misses_the_warm_read() {
    let host = host_with(&[
        (AUGMENT, AUGMENT_SRC),
        (
            AUGMENT2,
            "declare global { interface SkGlobal { extra: number } }\nexport {};\n",
        ),
        (READER, READER_SRC),
    ]);
    assert_clean_warm(
        &host,
        READER,
        "witnessGlobal0",
        primitive(PrimitiveName::String),
    );
    assert_clean_warm(&host, READER, "readExtra", primitive(PrimitiveName::Number));
    upsert(
        &host,
        AUGMENT2,
        "declare global { interface SkGlobal { extra: string } }\nexport {};\n",
    );
    let folded = eval(&host, READER, "readExtra");
    assert_eq!(
        (folded.ty, folded.degradation),
        (primitive(PrimitiveName::String), None),
        "the edited folded-in declaration's member type"
    );
    upsert(
        &host,
        AUGMENT,
        "declare global { interface SkGlobal { tag: number } }\nexport {};\n",
    );
    let named = eval(&host, READER, "witnessGlobal0");
    assert_eq!(
        (named.ty, named.degradation),
        (primitive(PrimitiveName::Number), None),
        "the edited named declaration's member type"
    );
    upsert(&host, SELF, SELF_SRC);
    let added = eval(&host, READER, "readOwnCross");
    assert_eq!(
        (added.ty, added.degradation),
        (primitive(PrimitiveName::Boolean), None),
        "the added declaration's member"
    );
}

/// `declare global` augments the global scope only from a MODULE; a
/// script's own file-scope interface is global.
///
/// TypeScript 7.0.2: `aug-script.ts` is an error (TS2669, "Augmentations
/// for the global scope can only be directly nested in external modules
/// or ambient module declarations") that binds nothing — `SkScript` is
/// unknown to the reader (TS2304, `readScriptAug` is the error type, `.d.ts`
/// `any`), and `SkGlobal` has no `fromScript` (TS2339, `readFromScript`
/// is `any`). `readScriptPlain` is `string`.
///
/// Mutation: letting a script's `declare global` name the global makes
/// `readScriptAug` resolve to `string`; folding it into the global makes
/// `readFromScript` resolve to `string`.
#[test]
fn declare_global_contributes_only_from_a_module() {
    let host = global_host();
    assert_unresolved(&host, READER, "readScriptAug");
    assert_unresolved(&host, READER, "readFromScript");
    assert_clean_warm(
        &host,
        READER,
        "readScriptPlain",
        primitive(PrimitiveName::String),
    );
}

/// A global VALUE declaration read bare or through `globalThis` resolves
/// through the declaration.
///
/// TypeScript 7.0.2: `readGlobalThisVar` (`globalThis.skVar`) and
/// `readBareVar` (`skVar`) are both `boolean`.
///
/// Mutation: without the global value root in `TypeOf` both rows degrade
/// (`UnresolvedValue`); without the global-augmentation fallback of the
/// prepared value declaration they degrade the same way.
#[test]
fn a_global_value_reads_through_its_declaration() {
    let host = global_host();
    assert_clean_warm(
        &host,
        READER,
        "readGlobalThisVar",
        primitive(PrimitiveName::Boolean),
    );
    assert_clean_warm(
        &host,
        READER,
        "readBareVar",
        primitive(PrimitiveName::Boolean),
    );
}

const GV_AUGMENT: &str = "/gv/augment.ts";
const GV_AUGMENT_SRC: &str = r#"declare global {
  interface SkGlobal { extra: number }
  var skVar: boolean;
  function skFn(): number;
  function skFn(x: string): string;
  let skLet: string;
  const skConst: 42;
  var skObj: { deep: { v: "d" } };
  namespace skNs { const inner: number; }
}
export {};
"#;

/// A SCRIPT: every file-scope value is global by name; its `var`s and
/// functions are also properties of the global object.
const GV_SCRIPT: &str = "/gv/script.ts";
const GV_SCRIPT_SRC: &str = r#"declare var scriptVar: number;
declare function scriptFn(): string;
var plainScriptVar = "s";
let scriptLet = 1;
function scriptDeclFn() { return true; }
function scriptReadsGlobalThis() { return globalThis.skVar; }
function scriptReadsOwnVar() { return globalThis.scriptVar; }
function scriptReadsBare() { return skVar; }
"#;

const GV_READER: &str = "/gv/m0.ts";
const GV_READER_SRC: &str = r#"export function readGlobalThisVar() { return globalThis.skVar; }
export function readBareVar() { return skVar; }
export function readGlobalFn() { return skFn(); }
export function readGlobalFnOverload() { return skFn("x"); }
export function readGlobalThisFn() { return globalThis.skFn(); }
export function readGlobalFnValue() { return skFn; }
export function readBareLet() { return skLet; }
export function readBareConst() { return skConst; }
export function readDeep() { return skObj.deep.v; }
export function readGlobalThisDeep() { return globalThis.skObj.deep; }
export function readNsInner() { return skNs.inner; }
export function readScriptVar() { return scriptVar; }
export function readGlobalThisScriptVar() { return globalThis.scriptVar; }
export function readScriptFn() { return scriptFn(); }
export function readPlainScriptVar() { return plainScriptVar; }
export function readScriptLet() { return scriptLet; }
export function readScriptDeclFn() { return scriptDeclFn(); }
export function readGlobalThisWhole() { return globalThis; }
export function readTypeofGlobalThisParam(g: typeof globalThis) { return g.skVar; }
export function readGlobalThisGlobalThis() { return globalThis.globalThis.skVar; }
export function readGlobalThisInObject() { return { v: globalThis.skVar, n: 1 }; }
export function readGlobalThisPlainScript() { return globalThis.plainScriptVar; }
"#;

/// A module whose own `skVar` shadows the global by NAME, never on the
/// global object.
const GV_SHADOW: &str = "/gv/m1.ts";
const GV_SHADOW_SRC: &str = r#"const skVar = 1;
export function readShadowedGlobalThis() { return globalThis.skVar; }
export function localGlobalThis() { const globalThis = { skVar: "local" }; return globalThis.skVar; }
"#;

/// Names that are NOT properties of the global object: a block-scoped
/// global and an undeclared one (the checker's TS2339 / TS7017).
const GV_MISSING: &str = "/gv/m2.ts";
const GV_MISSING_SRC: &str = r#"export function readGlobalThisLet() { return globalThis.skLet; }
export function readGlobalThisScriptLet() { return globalThis.scriptLet; }
export function readGlobalThisMissing() { return globalThis.nothingHere; }
"#;

fn global_value_host() -> Arc<VerterHost> {
    host_with(&[
        (GV_AUGMENT, GV_AUGMENT_SRC),
        (GV_SCRIPT, GV_SCRIPT_SRC),
        (GV_READER, GV_READER_SRC),
        (GV_SHADOW, GV_SHADOW_SRC),
        (GV_MISSING, GV_MISSING_SRC),
    ])
}

fn literal(value: verter_type_expr::LiteralValue) -> TypeExpr {
    TypeExpr::Literal(value)
}

/// The type of one member of a published object.
#[track_caller]
fn member_of(ty: &TypeExpr, key: &str) -> TypeExpr {
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

/// Every global value form, read bare, through `globalThis`, through a
/// `typeof globalThis` parameter and from a script, resolves as the
/// checker resolves it.
///
/// TypeScript 7.0.2 (`.d.ts` return types):
///
/// ```text
/// readGlobalThisVar          globalThis.skVar            boolean
/// readBareVar                skVar                       boolean
/// readGlobalFn               skFn()                      number
/// readGlobalFnOverload       skFn("x")                   string
/// readGlobalThisFn           globalThis.skFn()           number
/// readBareLet                skLet                       string
/// readBareConst              skConst                     42
/// readDeep                   skObj.deep.v                "d"
/// readGlobalThisDeep         globalThis.skObj.deep       { v: "d"; }
/// readScriptVar              scriptVar                   number
/// readGlobalThisScriptVar    globalThis.scriptVar        number
/// readScriptFn               scriptFn()                  string
/// readPlainScriptVar         plainScriptVar              string
/// readScriptLet              scriptLet                   number
/// readScriptDeclFn           scriptDeclFn()              boolean
/// readGlobalThisWhole        globalThis                  typeof globalThis
/// readTypeofGlobalThisParam  (g: typeof globalThis) g.skVar   boolean
/// readGlobalThisGlobalThis   globalThis.globalThis.skVar boolean
/// readGlobalThisInObject     { v: globalThis.skVar }     { v: boolean; n: number; }
/// readGlobalThisPlainScript  globalThis.plainScriptVar   string
/// readShadowedGlobalThis     globalThis.skVar (module `const skVar = 1`)  boolean
/// localGlobalThis            (local `globalThis` object) string
/// scriptReadsGlobalThis      globalThis.skVar in a script     boolean
/// scriptReadsOwnVar          globalThis.scriptVar in a script number
/// scriptReadsBare            skVar in a script                boolean
/// ```
///
/// Mutation: without the global value root every bare and `globalThis`
/// row degrades (`UnresolvedValue`, a call row `UnrepresentableCallee`);
/// without the script's file-scope value contributions (or without
/// ingesting a script that holds only values) the script rows degrade;
/// without the walker's member read through a deferred carrier,
/// `readTypeofGlobalThisParam` degrades.
#[test]
fn every_global_value_form_resolves_like_the_checker() {
    use PrimitiveName::{Boolean, Number, String};
    let host = global_value_host();
    for (canonical, name, expected) in [
        (GV_READER, "readGlobalThisVar", primitive(Boolean)),
        (GV_READER, "readBareVar", primitive(Boolean)),
        (GV_READER, "readGlobalFn", primitive(Number)),
        (GV_READER, "readGlobalFnOverload", primitive(String)),
        (GV_READER, "readGlobalThisFn", primitive(Number)),
        (GV_READER, "readBareLet", primitive(String)),
        (
            GV_READER,
            "readBareConst",
            literal(verter_type_expr::LiteralValue::Number(42.0)),
        ),
        (
            GV_READER,
            "readDeep",
            literal(verter_type_expr::LiteralValue::String("d".to_string())),
        ),
        (GV_READER, "readScriptVar", primitive(Number)),
        (GV_READER, "readGlobalThisScriptVar", primitive(Number)),
        (GV_READER, "readScriptFn", primitive(String)),
        (GV_READER, "readPlainScriptVar", primitive(String)),
        (GV_READER, "readScriptLet", primitive(Number)),
        (GV_READER, "readScriptDeclFn", primitive(Boolean)),
        (
            GV_READER,
            "readGlobalThisWhole",
            TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec!["globalThis".to_string()],
                type_args: Vec::new(),
            }),
        ),
        (GV_READER, "readTypeofGlobalThisParam", primitive(Boolean)),
        (GV_READER, "readGlobalThisGlobalThis", primitive(Boolean)),
        (GV_READER, "readGlobalThisPlainScript", primitive(String)),
        (GV_SHADOW, "readShadowedGlobalThis", primitive(Boolean)),
        (GV_SHADOW, "localGlobalThis", primitive(String)),
        (GV_SCRIPT, "scriptReadsGlobalThis", primitive(Boolean)),
        (GV_SCRIPT, "scriptReadsOwnVar", primitive(Number)),
        (GV_SCRIPT, "scriptReadsBare", primitive(Boolean)),
    ] {
        assert_clean_warm(&host, canonical, name, expected);
    }
    for (name, key, expected) in [
        (
            "readGlobalThisDeep",
            "v",
            literal(verter_type_expr::LiteralValue::String("d".to_string())),
        ),
        ("readGlobalThisInObject", "v", primitive(Boolean)),
    ] {
        let outcome = eval(&host, GV_READER, name);
        assert_eq!(
            (outcome.degradation, outcome.candidates),
            (None, 1),
            "{name}"
        );
        assert_eq!(member_of(&outcome.ty, key), expected, "{name}");
    }
}

/// A global function read as a VALUE is its whole overload surface.
///
/// TypeScript 7.0.2: `readGlobalFnValue` (`skFn`) prints `typeof skFn` —
/// the two declared overloads, `() => number` and `(x: string) => string`,
/// which the published surface spells structurally, in declaration order.
#[test]
fn a_global_function_value_is_its_overload_surface() {
    let host = global_value_host();
    let outcome = eval(&host, GV_READER, "readGlobalFnValue");
    assert_eq!((outcome.degradation, outcome.candidates), (None, 1));
    let TypeExpr::Object(shape) = &outcome.ty else {
        panic!("readGlobalFnValue must be a callable surface, got {outcome:?}");
    };
    let returns: Vec<Option<TypeExpr>> = shape
        .properties
        .iter()
        .filter_map(|member| match member {
            verter_type_expr::ObjectMember::CallSignature(signature) => {
                Some(signature.return_type.as_deref().cloned())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        returns,
        vec![
            Some(primitive(PrimitiveName::Number)),
            Some(primitive(PrimitiveName::String)),
        ]
    );
}

/// What is NOT on the global object stays unresolved: a block-scoped
/// global (`let`) through `globalThis`, an undeclared name, and a
/// namespace member, whose value surface this lane does not route.
///
/// TypeScript 7.0.2: `globalThis.skLet` and `globalThis.scriptLet` are
/// TS2339 and `globalThis.nothingHere` TS7017 — the checker's error type,
/// recovery for a program that does not type-check. `skNs.inner` is
/// `number`: a global namespace's members are not declarations this lane
/// resolves a value root to.
///
/// Mutation: admitting every global value kind as a property of the global
/// object resolves `readGlobalThisLet` to `string` and
/// `readGlobalThisScriptLet` to `number`.
#[test]
fn a_name_off_the_global_object_stays_unresolved() {
    let host = global_value_host();
    for name in [
        "readGlobalThisLet",
        "readGlobalThisScriptLet",
        "readGlobalThisMissing",
    ] {
        assert_unresolved(&host, GV_MISSING, name);
    }
    assert_unresolved(&host, GV_READER, "readNsInner");
}

const GV_MULTI_SCRIPT: &str = "/gv/multi-a.ts";
const GV_MULTI_SCRIPT_SRC: &str = r#"declare var dupVar: { a: 1 };
declare function splitFn(): number;
"#;
const GV_MULTI_MODULE: &str = "/gv/multi-b.ts";
const GV_MULTI_MODULE_SRC: &str = r#"declare global { var dupVar: { a: 1 }; function splitFn(x: string): string; }
export {};
"#;
const GV_MULTI_READER: &str = "/gv/multi-r.ts";
const GV_MULTI_READER_SRC: &str = r#"export function readDupVar() { return dupVar.a; }
export function readSplitFnNone() { return splitFn(); }
export function readSplitFnString() { return splitFn("x"); }
"#;

/// A global declared in more than one file: a `var` redeclared elsewhere
/// is ONE variable typed by its first declaration; a function whose
/// overloads span files is not merged here, and its calls fail closed.
///
/// TypeScript 7.0.2: `readDupVar` is `1`; `readSplitFnNone` is `number`
/// and `readSplitFnString` `string` — one overload set merged across the
/// two files, which this lane does not assemble.
///
/// Mutation: refusing every multi-file global degrades `readDupVar`;
/// admitting the first file's declaration for a function answers
/// `readSplitFnString` with the first file's lone overload.
#[test]
fn a_global_declared_in_several_files() {
    let host = host_with(&[
        (GV_MULTI_SCRIPT, GV_MULTI_SCRIPT_SRC),
        (GV_MULTI_MODULE, GV_MULTI_MODULE_SRC),
        (GV_MULTI_READER, GV_MULTI_READER_SRC),
    ]);
    assert_clean_warm(
        &host,
        GV_MULTI_READER,
        "readDupVar",
        literal(verter_type_expr::LiteralValue::Number(1.0)),
    );
    for name in ["readSplitFnNone", "readSplitFnString"] {
        let outcome = eval(&host, GV_MULTI_READER, name);
        assert_eq!(
            (outcome.degradation, outcome.candidates),
            (Some(FlowReturnDegradation::UnrepresentableCallee), 0),
            "{name}: {outcome:?}"
        );
    }
}

/// An edit that touches no declaration of the global keeps the warm read:
/// the read's observation of the name's contributors validates unchanged.
///
/// Mutation: validating the observation over fewer symbol spaces than the
/// reader observed misses every warm global value read.
#[test]
fn an_unrelated_edit_keeps_the_warm_global_value_read() {
    let host = global_value_host();
    for name in ["readBareVar", "readGlobalThisScriptVar"] {
        let _ = eval(&host, GV_READER, name);
    }
    upsert(&host, "/gv/unrelated.ts", "export const unrelated = 1;\n");
    assert_clean_warm(
        &host,
        GV_READER,
        "readBareVar",
        primitive(PrimitiveName::Boolean),
    );
    assert_clean_warm(
        &host,
        GV_READER,
        "readGlobalThisScriptVar",
        primitive(PrimitiveName::Number),
    );
}

/// A declaration file holding only file-scope values declares globals.
///
/// TypeScript 7.0.2: `readEnvVar` is `string`, `readEnvFn` `number`.
///
/// Mutation: ingesting a declaration file only for its `declare global` /
/// `declare module` blocks leaves both reads unresolved.
#[test]
fn a_declaration_file_of_values_declares_globals() {
    const ENV: &str = "/gv/env.d.ts";
    const ENV_READER: &str = "/gv/env-r.ts";
    let host = host_with(&[
        (
            ENV,
            "declare var envVar: string;\ndeclare function envFn(): number;\n",
        ),
        (
            ENV_READER,
            "export function readEnvVar() { return envVar; }\nexport function readEnvFn() { return globalThis.envFn(); }\n",
        ),
    ]);
    assert_clean_warm(
        &host,
        ENV_READER,
        "readEnvVar",
        primitive(PrimitiveName::String),
    );
    assert_clean_warm(
        &host,
        ENV_READER,
        "readEnvFn",
        primitive(PrimitiveName::Number),
    );
}

/// A module declaring a global value beside a module-scope value of the
/// same name: the identity `(module, name)` names the module's own value,
/// so the global is not addressed and its reads fail closed.
///
/// TypeScript 7.0.2: `readBoth` and `readBothViaGlobalThis` are `boolean`
/// (the global); `readOwnBoth` inside the declaring module is `number`.
///
/// Mutation: addressing the global through the declaring module answers
/// both reads with the module's own `const`.
#[test]
fn a_global_beside_a_same_name_module_value_fails_closed() {
    const BOTH: &str = "/gv/both.ts";
    const BOTH_READER: &str = "/gv/both-r.ts";
    let host = host_with(&[
        (
            BOTH,
            "declare global { var bothVar: boolean; }\nconst bothVar = 1;\nexport function readOwnBoth() { return bothVar; }\n",
        ),
        (
            BOTH_READER,
            "export function readBoth() { return bothVar; }\nexport function readBothViaGlobalThis() { return globalThis.bothVar; }\n",
        ),
    ]);
    for name in ["readBoth", "readBothViaGlobalThis"] {
        assert_unresolved(&host, BOTH_READER, name);
    }
}

/// An edit to a global value's declaration, and a declaration added later,
/// both miss the warm read.
///
/// TypeScript 7.0.2: `skVar` is `boolean` while `augment.ts` declares
/// `var skVar: boolean`, and `string` once it declares `var skVar:
/// string`; `lateVar` is `number` once a script declares it. `lateFn()`
/// is `number` with one declaration and stays `number` once another
/// script adds an overload — a merge across files this lane fails closed
/// on.
///
/// Mutation: validating the population while an upserted contributor
/// waits for ingestion keeps the first warm `readLateFn` answer after the
/// added overload; observing only the type space's contributors keeps the
/// second once another read has ingested it.
#[test]
fn an_edit_to_a_global_value_misses_the_warm_read() {
    const LATE_READER: &str = "/gv/late-r.ts";
    let host = host_with(&[
        (
            GV_AUGMENT,
            "declare global { var skVar: boolean; }\nexport {};\n",
        ),
        ("/gv/fn-a.ts", "declare function lateFn(): number;\n"),
        (
            LATE_READER,
            "export function readBare() { return skVar; }\nexport function readLate() { return lateVar; }\nexport function readLateFn() { return lateFn(); }\n",
        ),
    ]);
    assert_clean_warm(
        &host,
        LATE_READER,
        "readBare",
        primitive(PrimitiveName::Boolean),
    );
    assert_clean_warm(
        &host,
        LATE_READER,
        "readLateFn",
        primitive(PrimitiveName::Number),
    );
    assert_unresolved(&host, LATE_READER, "readLate");
    upsert(
        &host,
        GV_AUGMENT,
        "declare global { var skVar: string; }\nexport {};\n",
    );
    let edited = eval(&host, LATE_READER, "readBare");
    assert_eq!(
        (edited.ty, edited.degradation),
        (primitive(PrimitiveName::String), None),
        "the edited declaration's type"
    );
    upsert(&host, "/gv/late.ts", "declare var lateVar: number;\n");
    let added = eval(&host, LATE_READER, "readLate");
    assert_eq!(
        (added.ty, added.degradation),
        (primitive(PrimitiveName::Number), None),
        "the added declaration's type"
    );
    // The overload arrives in a script that waits for ingestion: the warm
    // read may not vouch for the population until it is ingested.
    let fn_b = "declare function lateFn(x: string): string;\n";
    upsert(&host, "/gv/fn-b.ts", fn_b);
    let pending = eval(&host, LATE_READER, "readLateFn");
    assert_eq!(
        pending.degradation,
        Some(FlowReturnDegradation::UnrepresentableCallee),
        "overloads spread over files fail closed: {pending:?}"
    );
    // Once another read has ingested an added overload, the warm read
    // still sees it through the name's value contributors.
    let host = host_with(&[
        ("/gv/fn-a.ts", "declare function lateFn(): number;\n"),
        (
            LATE_READER,
            "export function readBare() { return skVar; }\nexport function readLateFn() { return lateFn(); }\n",
        ),
    ]);
    assert_clean_warm(
        &host,
        LATE_READER,
        "readLateFn",
        primitive(PrimitiveName::Number),
    );
    upsert(&host, "/gv/fn-b.ts", fn_b);
    let _ = eval(&host, LATE_READER, "readBare");
    let merged = eval(&host, LATE_READER, "readLateFn");
    assert_eq!(
        merged.degradation,
        Some(FlowReturnDegradation::UnrepresentableCallee),
        "overloads spread over files fail closed: {merged:?}"
    );
}

/// The declaring file a whole-value read of the global names, read off the
/// published graph node.
fn declaring_file(host: &Arc<VerterHost>, canonical: &str, name: &str) -> Option<Arc<str>> {
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
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
    else {
        return None;
    };
    match dispatch.graph().node_data(result.return_type()).as_deref() {
        Some(SemanticNodeData::DeclRef { identity }) => Some(Arc::clone(&identity.canonical_id)),
        _ => None,
    }
}

/// The global a bare name reaches is named by its FIRST declaration in
/// declaration precedence order, whatever order the declaring files were
/// ingested in — the global contributor population's precedence, not
/// discovery order.
///
/// TypeScript 7.0.2: `readWhole` is `SkGlobal` and `g.extra` is `number`
/// under either file order.
///
/// Mutation: picking the LAST declaration instead of the first names
/// `augment2.ts` under both orders.
#[test]
fn the_global_identity_does_not_follow_ingest_order() {
    let augment2 = "declare global { interface SkGlobal { extra: number } }\nexport {};\n";
    let forward = host_with(&[
        (AUGMENT, AUGMENT_SRC),
        (AUGMENT2, augment2),
        (READER, READER_SRC),
    ]);
    let reverse = host_with(&[
        (AUGMENT2, augment2),
        (AUGMENT, AUGMENT_SRC),
        (READER, READER_SRC),
    ]);
    for host in [&forward, &reverse] {
        assert_eq!(
            declaring_file(host, READER, "readWhole").as_deref(),
            Some(AUGMENT),
            "the global is named by its first declaration"
        );
        assert_clean_warm(host, READER, "readExtra", primitive(PrimitiveName::Number));
    }
}
