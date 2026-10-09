//! A class declaration's `extends` value and static blocks run at its
//! statement, once, in source order, and the enclosing flow applies what
//! they do: a write retypes every later read, an entered assertion narrows
//! it, and a static block that cannot complete leaves the code after the
//! class unreachable (its reads see the declared type). Positions outside
//! the supported shape keep the typed gap and never publish Complete.
//!
//! Every expected answer is TypeScript 7.0.2's (`tsc --declaration
//! --emitDeclarationOnly`, target ES2022), identical under `--strict`,
//! `strictNullChecks` off, `noImplicitAny` off and both off.

use std::sync::Arc;

use super::*;
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_engine::semantic_query::{
    FlowReturnResult, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::TopLevelOwnerId;

const EFFECTS: &str = r#"
declare function assertString(x: unknown): asserts x is string;
declare const xs: string[];
class B {}
export function staticWrite(x: string | number) { class C { static { x = "s"; } } return x; }
export function heritageWrite(x: string | number) { class C extends (x = "s", B) {} return x; }
export function staticThrow(x: string | number) { class C { static { throw 0; } } return x; }
export function throwAfterWrite(x: string | number) { x = 1; class C { static { x = "s"; throw 0; } } return x; }
export function staticOrder(x: string | number) { class C { static { x = 1; } static { x = "s"; } } return x; }
export function heritageThenStatic(x: string | number | boolean) { class C extends (x = true, B) { static { x = 1; } } return x; }
export function dependentRead(x: string | number) { let y: string | number = 0; class C { static { x = "s"; y = x; } } return y; }
export function staticAssertion(x: string | number) { class C { static { assertString(x); } } return x; }
export function branchThrow(x: string | number | boolean, c: boolean) { class C { static { if (c) { x = "s"; throw 0; } x = 1; } } return x; }
export function labeledBreak(x: string | number | boolean) { class C { static { blk: { x = 1; break blk; } } } return x; }
export function tryCatch(x: string | number | boolean) { class C { static { try { x = 1; } catch { x = true; } } } return x; }
export function readBefore(x: string | number) { const r = x; class C { static { x = "s"; } } return r; }
export function heritageOnce(x: string | number) { class C extends (x = "s", B) {} x = 1; const c = new C(); return { c, x }; }
export function staticLocal(x: string | number) { class C { static { let y = 0; y = 1; } } return x; }
export function throwOnly(x: string | number) { class C { static { throw 0; } } }
export function forOf(x: string | number) { class C { static { for (x of xs) {} } } return x; }
export function receiverWrite(x: string | number, y: number) { class C { static n = 1; static { this.n = 2; x = "s"; } } return { x, y }; }
export function expressionWrite(x: string | number) { const K = class { static { x = "s"; } }; return x; }
export function forOfPlain(x: string | number) { for (x of xs) {} return x; }
"#;

/// The rows whose class-evaluation positions all run inline.
const APPLIED: &[(&str, &str)] = &[
    ("staticWrite", "string"),
    ("heritageWrite", "string"),
    ("staticThrow", "string | number"),
    ("throwAfterWrite", "string | number"),
    ("staticOrder", "string"),
    ("heritageThenStatic", "number"),
    ("dependentRead", "string"),
    ("staticAssertion", "string"),
    ("branchThrow", "number"),
    ("labeledBreak", "number"),
    ("tryCatch", "number | true"),
    ("readBefore", "string | number"),
    ("throwOnly", "void"),
];

#[test]
fn class_evaluation_effects_apply_once_in_source_order() {
    let names: Vec<&str> = APPLIED.iter().map(|(name, _)| *name).collect();
    let mut failures = super::flow_return_tests::flow_reads_without_proof(EFFECTS, &names);
    failures.extend(super::differential_harness_tests::Matrix::new(EFFECTS).returns(APPLIED));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The `extends` value runs once, at the class statement: constructing the
/// class later reads its value and never re-applies its write, so `x` is
/// the `1` written after the class. (A result carrying a local class value
/// keeps the capture family's typed gap, so this row is the answer, not
/// the proof.)
#[test]
fn heritage_effects_never_reapply_when_the_class_value_is_read() {
    let failures = super::differential_harness_tests::Matrix::new(EFFECTS)
        .returns(&[("heritageOnce", "{ c: C; x: number; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A static block that reads the class receiver or declares a local
/// (`staticLocal`: no outer write, still unmodelled), and a class EXPRESSION's
/// static block, run code the enclosing frame does not model: the checker
/// answers `{ x: string; y: number; }` and `string`, and the lane never
/// publishes either Complete — neither drops the write nor certifies the
/// sibling `y` beside an unapplied one. A static block applies what the
/// frame applies and no more: its `for (x of xs)` head write stays the
/// unapplied write it is directly in the frame (both `string | number`).
#[test]
fn unsupported_class_evaluation_effects_never_publish_complete() {
    let names = [
        "receiverWrite",
        "expressionWrite",
        "forOf",
        "forOfPlain",
        "staticLocal",
    ];
    let unproven = super::flow_return_tests::flow_reads_without_proof(EFFECTS, &names);
    assert_eq!(
        unproven.len(),
        names.len(),
        "each unmodelled class-evaluation write keeps its read partial: {unproven:?}"
    );
}

const FILE: &str = "/ws/class-evaluation/edit.ts";

fn upsert(host: &VerterHost, source: &str) {
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(FILE.to_string()),
        input_id: FILE.to_string(),
        source: Arc::from(source),
        file_language: crate::LanguageRegistry::global()
            .classify_static(FILE)
            .static_resolution(),
        aliases: Vec::new(),
    });
}

/// The whole-return flow of `name`: its projected return, whether it is
/// clean, and how many warm candidates its slot holds after the read.
fn observe(host: &Arc<VerterHost>, name: &str) -> (verter_type_expr::TypeExpr, bool, usize) {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = verter_type_engine::semantic_query::FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(FILE),
            TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(FILE),
        demand: verter_type_engine::semantic_query::ReturnProjectionDemand::whole_return(),
        input: verter_type_engine::semantic_query::FlowInputContext::empty(),
        result_contract: super::flow_solve::flow_return_result_contract_id(),
    };
    let QueryResult::Value(SemanticQueryOutput {
        value: SemanticQueryValue::FlowReturn(result),
        ..
    }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
    else {
        panic!("{name} must produce a value");
    };
    let result: Arc<FlowReturnResult> = result;
    let projected = host
        .project_node_to_type_expr_for_test(result.return_type())
        .unwrap_or_else(|| panic!("{name} must project"));
    let candidates = dispatch
        .graph()
        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
    (projected, result.degradation().is_none(), candidates)
}

/// A warm read and a read after editing a static block's write equal a
/// fresh computation over the same source.
#[test]
fn class_evaluation_effects_warm_and_edited_equal_fresh() {
    const BEFORE: &str = "export function f(x: string | number | boolean) { class C extends (x = true, class {}) { static { x = \"s\"; } } return x; }\n";
    const AFTER: &str = "export function f(x: string | number | boolean) { class C extends (x = true, class {}) { static { x = 1; } } return x; }\n";
    let fresh = |source: &str| {
        let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
        upsert(&host, source);
        observe(&host, "f")
    };

    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    upsert(&host, BEFORE);
    let cold = observe(&host, "f");
    assert_eq!(
        cold.0,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::String),
        "the static block's write follows the heritage write"
    );
    assert!(cold.1, "the applied effects publish clean");
    assert_eq!(cold.2, 1, "the clean result is admitted warm");
    let warm = observe(&host, "f");
    assert_eq!(warm, cold, "the warm read equals the cold read");
    assert_eq!(
        warm,
        fresh(BEFORE),
        "the warm read equals a fresh computation"
    );
    assert!(
        warm.2 == 1,
        "the warm read reuses the one admitted candidate"
    );

    upsert(&host, AFTER);
    let edited = observe(&host, "f");
    assert_eq!(
        edited.0,
        verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number),
        "the edited static block's write is applied, never the stale one"
    );
    let fresh_after = fresh(AFTER);
    assert_eq!(
        (&edited.0, edited.1),
        (&fresh_after.0, fresh_after.1),
        "the edited read equals a fresh computation"
    );
}
