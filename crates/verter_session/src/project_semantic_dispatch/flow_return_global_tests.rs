//! Reads through the program's GLOBAL declarations in the flow-return
//! lane: a bare type name no scope declares or imports names the merged
//! global declaration — every module's `declare global` contribution and
//! every script's file-scope interface, in declaration precedence order.
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

/// Global VALUE declarations — a `declare global { var … }` read bare or
/// through `globalThis` — fail closed: a value-space global declaration
/// has no routed body source, so the read is the typed unresolved value,
/// never a fabricated answer.
///
/// TypeScript 7.0.2: `readGlobalThisVar` (`globalThis.skVar`) and
/// `readBareVar` (`skVar`) are both `boolean`.
#[test]
fn global_value_reads_fail_closed() {
    let host = global_host();
    assert_unresolved(&host, READER, "readGlobalThisVar");
    assert_unresolved(&host, READER, "readBareVar");
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
