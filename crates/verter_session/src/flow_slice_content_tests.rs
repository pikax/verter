//! `SliceContent` contract: the slice-gated content lowering reborrows
//! the retained parse snapshot once per demand and lowers ONLY the
//! demanded slice's selected content, with explicit control semantics
//! (sequential regions, `if` reachability, terminal return/throw,
//! return-transparent vs return-bearing loops, typed-unsupported
//! constructs), parameter / simple local reaching-definition carriers,
//! direct same-slot recursion holds, and the symbolic
//! `ReturnType<typeof …>` call carrier. Content outside the selection is
//! omitted (an unread binding) or rides the typed `Elided` carrier (a
//! sibling member value). Locator misses are typed `None`s, never
//! panics.

use std::sync::Arc;

use verter_semantic::analysis::flow::flow_graph::build_function_flow_graph;
use verter_semantic::analysis::flow::lower::lower_slice_plan;
use verter_semantic::analysis::flow::peeker::{FlowSliceBudget, ReturnPathPeeker, SliceDemand};
use verter_semantic::analysis::function_program::{
    FunctionDescentStep, FunctionProgramEntry, FunctionProgramIndex,
};
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TypeExpr};

use crate::decl_body_memo::DeclBodyMemo;
use crate::flow_slice_content::{
    FlowSliceSelection, SliceBindingKind, SliceCall, SliceCallSite, SliceContent, SliceExpr,
    SliceGuard, SliceObjectEntry, SliceObjectMember, SliceRegion, SliceStatement, SliceUnsupported,
};

/// The MEMBER entries of a structural object literal, in authored order.
fn object_members(entries: &[SliceObjectEntry]) -> Vec<&SliceObjectMember> {
    entries
        .iter()
        .filter_map(|entry| match entry {
            SliceObjectEntry::Member(member) => Some(member.as_ref()),
            SliceObjectEntry::Spread { .. } => None,
        })
        .collect()
}

fn memo_for(source: &str) -> Arc<DeclBodyMemo> {
    let (state, _provenance) =
        crate::resolver_core::ShallowFileState::service_backed_with_provenance_for_test(
            "/ws/flow_slice_content.ts",
            source,
        );
    Arc::clone(state.decl_bodies())
}

fn entry_of<'a>(index: &'a FunctionProgramIndex, name: &'a str) -> &'a FunctionProgramEntry {
    index
        .matches_named(name)
        .next()
        .map(|matched| matched.entry())
        .unwrap_or_else(|| panic!("{name} must be indexed"))
}

fn member_entry_of<'a>(
    index: &'a FunctionProgramIndex,
    class_name: &'a str,
    ordinal: u32,
) -> &'a FunctionProgramEntry {
    index
        .matches_named(class_name)
        .find(|matched| {
            matches!(&matched.key().part, FunctionPartIdentity::Member { member_path } if member_path.contains(&ordinal))
        })
        .map(|matched| matched.entry())
        .unwrap_or_else(|| panic!("{class_name} member {ordinal} must be indexed"))
}

/// The REAL demand selection for `entry` under the return-projection
/// `path` (empty = whole return): skeleton → graph → plan → lowered
/// slice → selection — the exact pipeline the flow evaluator runs.
fn selection_for(
    memo: &DeclBodyMemo,
    entry: &FunctionProgramEntry,
    path: &[Arc<str>],
) -> (
    FlowSliceSelection,
    Arc<verter_semantic::analysis::flow::FunctionBodySkeleton>,
) {
    let skeleton = memo
        .function_body_skeleton(entry)
        .expect("the skeleton must build for an indexed function");
    let graph = build_function_flow_graph(&skeleton);
    let demand = SliceDemand::for_return_projection(&skeleton, path);
    let plan = ReturnPathPeeker::new(&graph)
        .plan(&demand, &FlowSliceBudget::default())
        .expect("the default budget admits these fixtures");
    let ir = lower_slice_plan(&plan, &graph, &skeleton);
    (FlowSliceSelection::from_slice_ir(&ir), Arc::new(skeleton))
}

fn content_for_path(source: &str, name: &str, path: &[Arc<str>]) -> Arc<SliceContent> {
    let memo = memo_for(source);
    let index = memo.function_program_index();
    let entry = entry_of(&index, name);
    let (selection, skeleton) = selection_for(&memo, entry, path);
    memo.flow_slice_content(entry, selection, skeleton)
        .expect("slice content must build for an indexed function")
}

fn content_for(source: &str, name: &str) -> Arc<SliceContent> {
    content_for_path(source, name, &[])
}

#[test]
fn optional_chain_after_active_type_predicate_lowers_to_gap() {
    let node = content_for(
        "export {};\n\
         type T = { length: number };\n\
         function isT(x: any): x is T { return true }\n\
         function makeProps(x: any) { if (isT(x)) return x?.length; return 0 }",
        "makeProps",
    );
    let consequent = node
        .body
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::If { consequent, .. } => Some(consequent),
            _ => None,
        })
        .expect("the body must contain the guarded arm");
    assert!(
        matches!(
            &consequent.statements[0],
            SliceStatement::Return {
                argument: Some(SliceExpr::Gap(
                    crate::semantic_query::FlowGap::UnmodeledExpression
                )),
                ..
            }
        ),
        "an optional chain rooted at an actively narrowed `any` must not be lowered as OptionalAnyChain"
    );
}

#[test]
fn optional_chain_with_effectful_call_argument_lowers_to_gap() {
    let node = content_for(
        "function makeProps(a: any, x: string | number) { return a?.b(x = \"s\") }",
        "makeProps",
    );
    assert!(matches!(
        &node.body.statements[0],
        SliceStatement::Return {
            argument: Some(SliceExpr::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression
            )),
            ..
        }
    ));
}

#[test]
fn optional_chain_with_effectful_computed_key_lowers_to_gap() {
    let node = content_for(
        "function makeProps(a: any, x: string | number) { return a?.[x = 2] }",
        "makeProps",
    );
    assert!(matches!(
        &node.body.statements[0],
        SliceStatement::Return {
            argument: Some(SliceExpr::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression
            )),
            ..
        }
    ));
}

#[test]
fn optional_chain_with_nested_syntactic_effect_lowers_to_gap() {
    for source in [
        "function makeProps(a: any, x: number) { return a?.b(1 + (x = 2)) }",
        "function makeProps(a: any, x: number) { return a?.[x++] }",
        "function makeProps(a: any, x: { p?: number }) { return a?.b(delete x.p) }",
        "async function makeProps(a: any, x: Promise<number>) { return a?.b(await x) }",
        "function* makeProps(a: any) { return a?.b(yield 1) }",
    ] {
        let node = content_for(source, "makeProps");
        assert!(
            matches!(
                &node.body.statements[0],
                SliceStatement::Return {
                    argument: Some(SliceExpr::Gap(
                        crate::semantic_query::FlowGap::UnmodeledExpression
                    )),
                    ..
                }
            ),
            "discarding a nested assignment/update/delete/await/yield must fail closed: {source}"
        );
    }
}

/// @ai-generated - block-bodied function with if/else returns: region tree + no fall-through
#[test]
fn if_else_returns_build_region_tree_without_fallthrough() {
    let node = content_for(
        "function pick(flag: boolean) {\n\
         \x20 if (flag) {\n\
         \x20   return 1;\n\
         \x20 } else {\n\
         \x20   return \"two\";\n\
         \x20 }\n\
         }\n",
        "pick",
    );
    assert!(!node.can_fall_through, "both arms return");
    assert_eq!(node.body.statements.len(), 1, "one if statement");
    assert!(node.body.can_fall_through == node.can_fall_through);
    let SliceStatement::If {
        consequent,
        alternate,
        ..
    } = &node.body.statements[0]
    else {
        panic!("the single statement must be an if");
    };
    assert!(!consequent.can_fall_through, "the then arm returns");
    assert_eq!(consequent.statements.len(), 1);
    assert!(
        matches!(
            &consequent.statements[0],
            SliceStatement::Return {
                argument: Some(SliceExpr::Type(leaf)),
                widening_literal: true,
            } if matches!(leaf.ty(), TypeExpr::Literal(LiteralValue::Number(_)))
        ),
        "a return argument PRESERVES its fresh literal and flags it: tsc \
         widens only a lone contributor, so the return join owns the \
         decision (`pick` is `\"two\" | 1`, not `string | number`)"
    );
    let alternate = alternate.as_ref().expect("an else arm exists");
    assert!(!alternate.can_fall_through, "the else arm returns");
    assert_eq!(alternate.statements.len(), 1);
    assert!(
        matches!(
            &alternate.statements[0],
            SliceStatement::Return {
                argument: Some(SliceExpr::Type(leaf)),
                widening_literal: true,
            } if matches!(leaf.ty(), TypeExpr::Literal(LiteralValue::String(_)))
        ),
        "the else arm likewise preserves its fresh literal"
    );
}

/// @ai-generated - if without else falls through: one return in the arm
#[test]
fn if_without_else_falls_through() {
    let node = content_for(
        "function pick(flag: boolean) {\n\
         \x20 if (flag) {\n\
         \x20   return 1;\n\
         \x20 }\n\
         }\n",
        "pick",
    );
    assert!(node.can_fall_through, "no else arm: fall-through");
    assert_eq!(node.body.statements.len(), 1);
    let SliceStatement::If {
        consequent,
        alternate,
        ..
    } = &node.body.statements[0]
    else {
        panic!("the single statement must be an if");
    };
    assert!(alternate.is_none());
    assert!(!consequent.can_fall_through);
    assert_eq!(consequent.statements.len(), 1);
    assert!(matches!(
        &consequent.statements[0],
        SliceStatement::Return {
            argument: Some(_),
            ..
        }
    ));
}

/// @ai-generated - bare return carries no argument and terminates the region
#[test]
fn bare_return_carries_no_argument() {
    let node = content_for("function done() { return; }\n", "done");
    assert!(!node.can_fall_through);
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Return {
            argument: None,
            widening_literal: false,
        }],
    );
}

/// @ai-generated - return-free loop is fall-through transparent before a return
#[test]
fn return_free_loop_is_transparent() {
    let node = content_for(
        "function count() {\n\
         \x20 for (let i = 0; i < 3; i++) {\n\
         \x20 }\n\
         \x20 return 1;\n\
         }\n",
        "count",
    );
    assert!(!node.can_fall_through);
    assert_eq!(node.body.statements.len(), 2);
    assert!(matches!(
        node.body.statements[0],
        SliceStatement::TransparentLoop
    ));
    assert!(matches!(
        &node.body.statements[1],
        SliceStatement::Return {
            argument: Some(_),
            ..
        }
    ));
}

/// @ai-generated - a loop whose transfer depends on a downstream-selected
/// binding must use the typed loop refusal until fixed-point semantics exist.
#[test]
fn selected_loop_transfers_are_unsupported_but_inert_loops_stay_transparent() {
    for source in [
        "function makeProps(x: \"s\" | 0) { while (typeof x === \"string\") { } return x }",
        "declare function assertNumber(v: unknown): asserts v is number\nfunction makeProps(x: string | number) { do { assertNumber(x); break } while (true); return x }",
        "function makeProps(x: \"a\" | \"b\") { exit: while (true) { if (x === \"a\") break exit; throw 0 } return x }",
    ] {
        let node = content_for(source, "makeProps");
        let unsupported = node.body.statements.iter().any(|statement| match statement {
            SliceStatement::Unsupported(SliceUnsupported::Loop) => true,
            SliceStatement::Labeled { body, .. } => matches!(
                body.statements.first(),
                Some(SliceStatement::Unsupported(SliceUnsupported::Loop))
            ),
            _ => false,
        });
        assert!(
            unsupported,
            "a loop transfer involving the selected return binding must refuse: {source}"
        );
    }

    let invoked = content_for(
        "function makeProps() { let x: \"a\" | \"b\" = \"a\"; do { (() => { x = \"b\" })() } while (false); return x }",
        "makeProps",
    );
    assert!(invoked.body.statements.iter().any(|statement| matches!(
        statement,
        SliceStatement::Unsupported(SliceUnsupported::InvokedClosureEffect)
    )));

    let inert = content_for(
        "declare function opaque(): boolean\nfunction makeProps(x: string | number) { while (opaque()) { } return x }",
        "makeProps",
    );
    assert!(
        matches!(
            inert.body.statements.first(),
            Some(SliceStatement::TransparentLoop)
        ),
        "a loop with no selected capture keeps the existing transparent path"
    );

    let arithmetic = content_for(
        "function makeProps(x: number) { while (x + 1) { break } return x }",
        "makeProps",
    );
    assert!(
        matches!(
            arithmetic.body.statements.first(),
            Some(SliceStatement::TransparentLoop)
        ),
        "an inert arithmetic control read must not turn the loop into a transfer"
    );
}

/// @ai-generated - unknown logical operands stay explicit so the evaluator
/// can derive the positive and negative edges asymmetrically.
#[test]
fn logical_guards_preserve_unmodelled_operands_for_both_edge_readings() {
    let and_node = content_for(
        "export {};\ndeclare function opaque(): boolean\nfunction makeProps(x: string | number) { if (typeof x === \"string\" && opaque()) throw 0; return { v: x } }",
        "makeProps",
    );
    let Some(guard) = and_node
        .body
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::If { guard, .. } => Some(guard),
            _ => None,
        })
    else {
        panic!("the conjunction fixture must lower an if guard");
    };
    assert!(
        matches!(guard, SliceGuard::And(parts) if parts.iter().any(|part| matches!(part, SliceGuard::None))),
        "the unmodelled conjunction must remain explicit for the ambiguous false edge: {guard:?}"
    );

    let or_node = content_for(
        "export {};\ndeclare function opaque(): boolean\nfunction makeProps(x: string | number) { if (typeof x === \"string\" || opaque()) throw 0; return { v: x } }",
        "makeProps",
    );
    let Some(guard) = or_node
        .body
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::If { guard, .. } => Some(guard),
            _ => None,
        })
    else {
        panic!("the disjunction fixture must lower an if guard");
    };
    assert!(
        matches!(guard, SliceGuard::Or(parts) if parts.iter().any(|part| matches!(part, SliceGuard::None))),
        "the unmodelled disjunction must remain explicit so its false edge negates every modelled disjunct: {guard:?}"
    );

    let negated = content_for(
        "export {};\nfunction isStr(v: string | number): v is string { return typeof v === \"string\" }\nfunction makeProps(x: string | number, n: number) { if (!(isStr(x) && n > 0)) throw 0; return { v: x } }",
        "makeProps",
    );
    let Some(guard) = negated
        .body
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::If { guard, .. } => Some(guard),
            _ => None,
        })
    else {
        panic!("the negated-conjunction fixture must lower an if guard");
    };
    assert!(
        matches!(guard, SliceGuard::Or(parts) if parts.iter().any(|part| matches!(part, SliceGuard::TypePredicate { .. }))),
        "negating a conjunction must retain the predicate leaf for the opposite edge: {guard:?}"
    );
}

/// The guard channel and the control-call certification serve ONLY a
/// callee whose checker-visible declaration set is provably the one
/// same-file declaration: the file must be module-scoped (top-level
/// module syntax — a script's top-level functions are globals merging
/// with every other script's and every `declare global` block's
/// same-name declarations) and the binding must not be exported by any
/// spelling (an exported binding is augmentable through `declare module`).
/// Anything else lowers the test to an unmodelled guard behind the typed
/// `GuardNarrowing` gap — never a locally selected predicate target, never
/// a certified call.
#[test]
fn control_callee_certification_requires_provable_module_local_closure() {
    const PREDICATE: &str =
        "function isStr(v: string | number): v is string { return typeof v === \"string\" }";
    const BOOLEAN: &str = "function check(v: string | number): boolean { return true }";
    const BODY: &str =
        "function makeProps(x: string | number) { if (isStr(x)) return x; if (check(x)) return x; return 0 }";
    let guarded_if_count = |node: &SliceContent| {
        node.body
            .statements
            .iter()
            .filter(|statement| {
                matches!(
                    statement,
                    SliceStatement::If {
                        guard: SliceGuard::TypePredicate { .. },
                        ..
                    }
                )
            })
            .count()
    };
    let gap_count = |node: &SliceContent| {
        node.body
            .statements
            .iter()
            .filter(|statement| {
                matches!(
                    statement,
                    SliceStatement::Gap(crate::semantic_query::FlowGap::GuardNarrowing)
                )
            })
            .count()
    };

    // Module-scoped, unexported callees: the closure is provable — the
    // predicate mints its guard and the annotated boolean call is
    // certified (no gap at all).
    let closed = content_for(
        &format!("export {{}};\n{PREDICATE}\n{BOOLEAN}\n{BODY}"),
        "makeProps",
    );
    assert_eq!(
        guarded_if_count(&closed),
        1,
        "the unexported module-local predicate mints its guard: {closed:?}"
    );
    assert_eq!(
        gap_count(&closed),
        0,
        "the unexported module-local boolean callee is certified: {closed:?}"
    );
    assert_eq!(
        closed.decided_above_call_spans.len(),
        1,
        "exactly the certified boolean call is decided above: {closed:?}"
    );

    // A SCRIPT (no module syntax) with the same declarations: neither
    // closure is provable — the predicate call lowers as an unmodelled
    // guard and the boolean call is not certified; each control test
    // drains its own gap.
    let script = content_for(&format!("{PREDICATE}\n{BOOLEAN}\n{BODY}"), "makeProps");
    assert_eq!(
        guarded_if_count(&script),
        0,
        "a script's top-level predicate is a global whose declaration set the \
         file cannot enumerate: {script:?}"
    );
    assert_eq!(
        gap_count(&script),
        2,
        "both control tests gap in a script: {script:?}"
    );
    assert!(
        script.decided_above_call_spans.is_empty(),
        "nothing is certified in a script: {script:?}"
    );

    // A module whose callees are EXPORTED (declaration spelling and
    // specifier spelling): augmentable, so neither closure is provable.
    for exported in [
        format!("export {PREDICATE}\nexport {BOOLEAN}\n{BODY}"),
        format!("{PREDICATE}\n{BOOLEAN}\nexport {{ isStr, check as verify }};\n{BODY}"),
        format!("{PREDICATE}\n{BOOLEAN}\nexport default isStr;\nexport {{ check }};\n{BODY}"),
    ] {
        let node = content_for(&exported, "makeProps");
        assert_eq!(
            guarded_if_count(&node),
            0,
            "an exported predicate is never locally selected: {exported}"
        );
        assert_eq!(
            gap_count(&node),
            2,
            "both control tests gap for exported callees: {exported}"
        );
        assert!(
            node.decided_above_call_spans.is_empty(),
            "nothing is certified for exported callees: {exported}"
        );
    }
}

/// A NAMESPACE-OWNED call site binds its bare callee through the
/// enclosing block's own scope before the top level: a block-local
/// declaration of the name shadows the module-scope one, exactly as the
/// function index binds `N.check` over the file-global `check` for a
/// direct call. The closure gate enumerates only TOP-LEVEL declarations,
/// so inside a `namespace` / `module` block it can neither certify the
/// top-level non-predicate (the shadowing block-local callee IS a
/// predicate the checker applies) nor mint the top-level predicate's
/// target (the block-local predicate narrows to a DIFFERENT type). Both
/// directions refuse: the test lowers behind the typed `GuardNarrowing`
/// gap, nothing is certified, no guard is minted. The top-level call site
/// of the same file keeps its provable closure — the refusal is about the
/// CALL SITE's scope, not the mere presence of a namespace in the file.
#[test]
fn namespace_owned_call_site_never_resolves_callee_closure_at_top_level() {
    const BODY: &str = "function make(x: string | number) { if (check(x)) return x; return false }";
    let predicate_guard_count = |node: &SliceContent| {
        node.body
            .statements
            .iter()
            .filter(|statement| {
                matches!(
                    statement,
                    SliceStatement::If {
                        guard: SliceGuard::TypePredicate { .. },
                        ..
                    }
                )
            })
            .count()
    };
    let gap_count = |node: &SliceContent| {
        node.body
            .statements
            .iter()
            .filter(|statement| {
                matches!(
                    statement,
                    SliceStatement::Gap(crate::semantic_query::FlowGap::GuardNarrowing)
                )
            })
            .count()
    };
    for (top_level, block_local) in [("boolean", "x is number"), ("x is string", "x is number")] {
        let source = format!(
            "export {{}};\n\
             function check(x: string | number): {top_level} {{ return true }}\n\
             namespace N {{\n\
             function check(x: string | number): {block_local} {{ return true }}\n\
             export {BODY}\n\
             }}\n\
             {BODY}"
        );

        let owned = content_for(&source, "N.make");
        assert_eq!(
            predicate_guard_count(&owned),
            0,
            "a namespace-owned call site never mints the top-level `{top_level}` \
             declaration's guard: {owned:?}"
        );
        assert_eq!(
            gap_count(&owned),
            1,
            "a namespace-owned control test gaps (top-level `{top_level}`): {owned:?}"
        );
        assert!(
            owned.decided_above_call_spans.is_empty(),
            "a namespace-owned control call is never certified (top-level \
             `{top_level}`): {owned:?}"
        );

        // The top-level call site binds the top-level declaration, whose
        // closure IS provable: the single unexported module-local `check`.
        let top = content_for(&source, "make");
        let top_level_is_predicate = top_level != "boolean";
        assert_eq!(
            predicate_guard_count(&top),
            usize::from(top_level_is_predicate),
            "the top-level call site keeps its provable closure (top-level \
             `{top_level}`): {top:?}"
        );
        assert_eq!(
            gap_count(&top),
            0,
            "the top-level control test never gaps (top-level `{top_level}`): {top:?}"
        );
        assert_eq!(
            top.decided_above_call_spans.len(),
            usize::from(!top_level_is_predicate),
            "exactly the non-predicate top-level call is certified (top-level \
             `{top_level}`): {top:?}"
        );
    }
}

/// The number of `if` statements guarded by a predicate fact.
fn predicate_guard_count(node: &SliceContent) -> usize {
    node.body
        .statements
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                SliceStatement::If {
                    guard: SliceGuard::TypePredicate { .. },
                    ..
                }
            )
        })
        .count()
}

/// The number of `if` statements guarded by an `instanceof` fact.
fn instanceof_guard_count(node: &SliceContent) -> usize {
    node.body
        .statements
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                SliceStatement::If {
                    guard: SliceGuard::Instanceof { .. },
                    ..
                }
            )
        })
        .count()
}

/// The number of typed guard-narrowing gaps in the body region.
fn guard_gap_count(node: &SliceContent) -> usize {
    node.body
        .statements
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                SliceStatement::Gap(crate::semantic_query::FlowGap::GuardNarrowing)
            )
        })
        .count()
}

/// A same-file predicate whose TARGET names a binding of the CALLEE's own
/// declaration — its type-parameter clause (`function isSame<T>(x: T): x
/// is T`) or a formal parameter (`x is typeof y`) — is instantiated by the
/// CALL: the checker binds `T` to the argument's `string | number` (a
/// tautology here), never to the owner scope's unrelated `type T =
/// number`. This half performs no call-site inference, and the caller's
/// environment cannot resolve such a target, so the predicate channel
/// refuses it: the control test lowers behind the typed gap and nothing
/// is certified, for the guard spelling and the `asserts x is T`
/// statement spelling alike. A generic callee whose target is closed over
/// the module scope (`x is string`) keeps its guard.
#[test]
fn predicate_target_over_callee_bindings_never_resolves_in_the_caller_environment() {
    const CALLER: &str = "function f(x: string | number) { if (isSame(x)) return x; return false }";
    for callee in [
        "function isSame<T>(x: T): x is T { return true }",
        "function isSame(x: unknown, y: string): x is typeof y { return true }",
        "function isSame<T>(x: T): x is T[] { return true }",
    ] {
        let source = format!("export {{}};\ntype T = number;\n{callee}\n{CALLER}");
        let node = content_for(&source, "f");
        assert_eq!(
            predicate_guard_count(&node),
            0,
            "a call-instantiated predicate target is never minted as a guard \
             (`{callee}`): {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "the control test gaps (`{callee}`): {node:?}"
        );
        assert!(
            node.decided_above_call_spans.is_empty(),
            "the predicate call is never certified (`{callee}`): {node:?}"
        );
    }

    let asserting = content_for(
        "export {};\n\
         type T = number;\n\
         function assertSame<T>(x: T): asserts x is T {}\n\
         function g(x: string | number) { assertSame(x); return x }",
        "g",
    );
    assert!(
        !asserting
            .body
            .statements
            .iter()
            .any(|statement| matches!(statement, SliceStatement::Assertion { .. })),
        "a call-instantiated assertion target is never minted: {asserting:?}"
    );
    assert_eq!(
        guard_gap_count(&asserting),
        1,
        "the assertion statement gaps: {asserting:?}"
    );

    let closed = content_for(
        &format!(
            "export {{}};\ntype T = number;\n\
             function isSame<T>(x: T): x is string {{ return true }}\n{CALLER}"
        ),
        "f",
    );
    assert_eq!(
        predicate_guard_count(&closed),
        1,
        "a generic callee whose target is closed over the module scope keeps its guard: {closed:?}"
    );
    assert_eq!(guard_gap_count(&closed), 0, "{closed:?}");
}

/// `x instanceof A` narrows by the VALUE `A` denotes at the test, and the
/// frame's lexical authority binds that name first: a parameter `A:
/// typeof B` shadows the owner-scope `class A`, and the checker narrows to
/// `B`. The evaluator lowers the constructor as an owner-scope type
/// reference, which is exactly the class's instance type ONLY when the
/// bare name is provably the module's single same-file `class`
/// declaration at this call site. Every other right-hand side — a
/// frame-bound name, a namespace-owned call site (the block scope binds
/// first), a non-class top-level value, an import — lowers behind the
/// typed gap rather than as a guard over the wrong binding.
#[test]
fn instanceof_constructor_binds_through_the_frame_before_owner_scope() {
    const BODY: &str = "{ if (x instanceof A) return x; return false }";
    let refused = [
        (
            "a parameter shadows the class",
            format!(
                "export {{}};\nclass A {{ a = 1 }}\nclass B {{ b = 1 }}\n\
                 function f(x: A | B, A: typeof B) {BODY}"
            ),
            "f",
        ),
        (
            "a body-local shadows the class",
            "export {};\nclass A { a = 1 }\nclass B { b = 1 }\n\
             function f(x: A | B) { const A = B; if (x instanceof A) return x; return false }"
                .to_string(),
            "f",
        ),
        (
            "a non-class top-level value",
            format!(
                "export {{}};\nclass B {{ b = 1 }}\nconst A = B;\n\
                 function f(x: B | string) {BODY}"
            ),
            "f",
        ),
        (
            "an imported constructor",
            format!(
                "import {{ A }} from \"./a\";\nclass B {{ b = 1 }}\n\
                 function f(x: A | B) {BODY}"
            ),
            "f",
        ),
        (
            "a namespace-owned call site",
            format!(
                "export {{}};\nclass A {{ a = 1 }}\nclass B {{ b = 1 }}\n\
                 namespace N {{ class A {{ n = 1 }}\nexport function f(x: A | B) {BODY} }}"
            ),
            "N.f",
        ),
        (
            "a script file",
            format!("class A {{ a = 1 }}\nclass B {{ b = 1 }}\nfunction f(x: A | B) {BODY}"),
            "f",
        ),
    ];
    for (case, source, name) in &refused {
        let node = content_for(source, name);
        assert_eq!(
            instanceof_guard_count(&node),
            0,
            "{case}: no guard over an unprovable constructor binding: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 1, "{case}: the test gaps: {node:?}");
    }

    let closed = content_for(
        &format!(
            "export {{}};\nclass A {{ a = 1 }}\nclass B {{ b = 1 }}\nfunction f(x: A | B) {BODY}"
        ),
        "f",
    );
    assert_eq!(
        instanceof_guard_count(&closed),
        1,
        "the module's single same-file class declaration keeps its guard: {closed:?}"
    );
    assert_eq!(guard_gap_count(&closed), 0, "{closed:?}");
}

/// The body region of the ONE nested function value `name` returns.
fn returned_nested_body(node: &SliceContent) -> &SliceRegion {
    node.body
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::Return {
                argument: Some(SliceExpr::NestedFunctionValue { body, .. }),
                ..
            } => Some(body),
            _ => None,
        })
        .expect("the function returns a nested function value")
}

/// The number of `if` statements of `region` guarded by a predicate fact.
fn region_predicate_guard_count(region: &SliceRegion) -> usize {
    region
        .statements
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                SliceStatement::If {
                    guard: SliceGuard::TypePredicate { .. },
                    ..
                }
            )
        })
        .count()
}

/// The number of typed guard-narrowing gaps of `region`.
fn region_guard_gap_count(region: &SliceRegion) -> usize {
    region
        .statements
        .iter()
        .filter(|statement| {
            matches!(
                statement,
                SliceStatement::Gap(crate::semantic_query::FlowGap::GuardNarrowing)
            )
        })
        .count()
}

/// A same-file predicate's TARGET is authored in the CALLEE's declaration
/// scope: `x is T` on a top-level `isNum` names the MODULE's `T`. The
/// fact is consumed inside the CALLER's frame, whose binder environment
/// interns the caller's own type parameters and whose lexical authority
/// binds its body-local declarations first — so a target naming anything
/// the CALLER binds (its own or an enclosing frame's `<T>`, a body-local
/// `type T` / `class T`, a local value under a `typeof y` root) would
/// rebind to the WRONG declaration: `isNum(x): x is T` over `type T =
/// number` narrows `number | string` to `number` in the checker, and to
/// the caller's `T extends number` here. The channel refuses every such
/// target — guard and `asserts` spellings alike — and the position takes
/// the typed gap. A caller binding an UNRELATED name, or a value-only
/// local of the same name (a `const T` never shadows a type), keeps the
/// guard.
#[test]
fn predicate_target_over_caller_bindings_never_resolves_in_the_caller_environment() {
    const PRELUDE: &str = "export {};\ntype T = number;\nconst y = 1;\n\
                           function isNum(x: unknown): x is T { return true }\n\
                           function isY(x: unknown): x is typeof y { return true }\n";
    let refused = [
        (
            "the caller's own type parameter",
            "function f<T extends number>(x: number | string, _t: T) { if (isNum(x)) return x; return false }",
        ),
        (
            "a body-local type alias",
            "function f(x: number | string) { type T = string; if (isNum(x)) return x; return false }",
        ),
        (
            "a body-local class",
            "function f(x: number | string) { class T { t = 1 }; if (isNum(x)) return x; return false }",
        ),
        (
            "a body-local value under the target's `typeof` root",
            "function f(x: number | string) { const y = \"s\"; if (isY(x)) return x; return false }",
        ),
        (
            "a parameter under the target's `typeof` root",
            "function f(x: number | string, y: string) { if (isY(x)) return x; return false }",
        ),
    ];
    for (case, caller) in refused {
        let node = content_for(&format!("{PRELUDE}{caller}"), "f");
        assert_eq!(
            predicate_guard_count(&node),
            0,
            "{case}: a target the caller frame rebinds is never minted as a guard: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the control test gaps: {node:?}"
        );
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: the predicate call is never certified: {node:?}"
        );
    }

    let enclosing = content_for(
        &format!(
            "{PRELUDE}function outer<T extends number>() {{ \
             return (x: number | string) => {{ if (isNum(x)) return x; return false }} }}"
        ),
        "outer",
    );
    let nested = returned_nested_body(&enclosing);
    assert_eq!(
        region_predicate_guard_count(nested),
        0,
        "an ENCLOSING frame's type parameter rebinds the target too: {enclosing:?}"
    );
    assert_eq!(
        region_guard_gap_count(nested),
        1,
        "the nested control test gaps: {enclosing:?}"
    );
    assert!(
        enclosing.decided_above_call_spans.is_empty(),
        "the nested predicate call is never certified: {enclosing:?}"
    );

    let asserting = content_for(
        &format!(
            "{PRELUDE}function assertNum(x: unknown): asserts x is T {{}}\n\
             function g<T extends number>(x: number | string, _t: T) {{ assertNum(x); return x }}"
        ),
        "g",
    );
    assert!(
        !asserting
            .body
            .statements
            .iter()
            .any(|statement| matches!(statement, SliceStatement::Assertion { .. })),
        "a caller-rebound assertion target is never minted: {asserting:?}"
    );
    assert_eq!(
        guard_gap_count(&asserting),
        1,
        "the assertion statement gaps: {asserting:?}"
    );

    for (case, caller) in [
        (
            "an unrelated caller binder",
            "function f<U>(x: number | string, _u: U) { if (isNum(x)) return x; return false }",
        ),
        (
            "a value-only local of the target's name",
            "function f(x: number | string) { const T = 1; if (isNum(x)) return x; return false }",
        ),
    ] {
        let node = content_for(&format!("{PRELUDE}{caller}"), "f");
        assert_eq!(
            predicate_guard_count(&node),
            1,
            "{case}: a target closed over the module scope keeps its guard: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// `x instanceof A` narrows to the class's INSTANCE type only when the
/// constructor's static side has no `[Symbol.hasInstance]`: a static
/// method of that key whose return is a type predicate makes the checker
/// narrow to the PREDICATE's target instead (`x is B` on `A` narrows `A |
/// B` to `B`, not `A`), and the static side is INHERITED through the
/// heritage chain. The evaluator's owner-scope type reference always
/// yields the instance type, so the gate refuses a class that declares a
/// static computed key (any computed key can spell `Symbol.hasInstance`
/// through an alias), and a class whose heritage it cannot prove clean
/// (a superclass that is not itself a provably closed same-file class).
/// A plain class, a class with instance-side computed keys only, and a
/// class extending a provably clean same-file class keep the guard.
#[test]
fn instanceof_constructor_with_symbol_has_instance_never_narrows_to_the_class_instance() {
    const BODY: &str = "function f(x: A | B) { if (x instanceof A) return x; return false }";
    const B: &str = "class B { b = 1 }\n";
    let refused = [
        (
            "a direct static [Symbol.hasInstance]",
            format!(
                "export {{}};\n{B}class A {{ static [Symbol.hasInstance](x: unknown): x is B {{ return true }} }}\n{BODY}"
            ),
        ),
        (
            "an inherited static [Symbol.hasInstance]",
            format!(
                "export {{}};\n{B}class Base {{ static [Symbol.hasInstance](x: unknown): x is B {{ return true }} }}\n\
                 class A extends Base {{}}\n{BODY}"
            ),
        ),
        (
            "an aliased static computed key",
            format!(
                "export {{}};\n{B}const key = Symbol.hasInstance;\n\
                 class A {{ static [key](x: unknown): x is B {{ return true }} }}\n{BODY}"
            ),
        ),
        (
            "a static computed property",
            format!(
                "export {{}};\n{B}class A {{ static [Symbol.hasInstance] = (x: unknown): x is B => true }}\n{BODY}"
            ),
        ),
        (
            "an imported superclass",
            format!("import {{ Base }} from \"./base\";\n{B}class A extends Base {{}}\n{BODY}"),
        ),
        (
            "a superclass expression",
            format!(
                "export {{}};\n{B}declare function mixin(): new () => object;\n\
                 class A extends mixin() {{}}\n{BODY}"
            ),
        ),
        (
            "a superclass shadowed by a same-file value",
            format!(
                "export {{}};\n{B}class Real {{ static [Symbol.hasInstance](x: unknown): x is B {{ return true }} }}\n\
                 const Base = Real;\nclass A extends Base {{}}\n{BODY}"
            ),
        ),
    ];
    for (case, source) in &refused {
        let node = content_for(source, "f");
        assert_eq!(
            instanceof_guard_count(&node),
            0,
            "{case}: no guard over a constructor whose static side may carry \
             `Symbol.hasInstance`: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 1, "{case}: the test gaps: {node:?}");
    }

    let accepted = [
        (
            "an instance-side computed key only",
            format!(
                "export {{}};\n{B}class A {{ [Symbol.hasInstance](x: unknown): x is B {{ return true }}; static n = 1 }}\n{BODY}"
            ),
        ),
        (
            "a provably clean same-file superclass",
            format!("export {{}};\n{B}class Base {{ base = 1 }}\nclass A extends Base {{}}\n{BODY}"),
        ),
    ];
    for (case, source) in &accepted {
        let node = content_for(source, "f");
        assert_eq!(
            instanceof_guard_count(&node),
            1,
            "{case}: the class's instance type IS the narrowing target: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// A sequence's DISCARDED operands never feed its value, but a call among
/// them still runs — and an ASSERTION call narrows everything after it
/// (`(assertString(x), x)` is `string` in the checker). Blanket-certifying
/// those calls decided-above dropped the narrowing while the superset
/// completed clean. A discarded call is certified only when it provably
/// establishes no narrowing: a `new` construct, or a provably closed
/// same-file `function` declaration whose return annotation is not an
/// `asserts` predicate (an unannotated declaration cannot be an assertion
/// — assertion signatures are never inferred). Every other discarded call
/// is unprovable: never certified, and the statement takes the typed gap.
#[test]
fn discarded_sequence_operand_calls_are_never_blanket_certified() {
    let refused = [
        (
            "a closed same-file assertion",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return (assertString(x), x) }",
        ),
        (
            "an imported callee",
            "import { touch } from \"./touch\";\n\
             function f(x: string | number) { return (touch(), x) }",
        ),
        (
            "an exported same-file callee",
            "export function touch() {}\n\
             function f(x: string | number) { return (touch(), x) }",
        ),
        (
            "a member callee",
            "export {};\ndeclare const o: { touch(): void };\n\
             function f(x: string | number) { return (o.touch(), x) }",
        ),
        (
            "a closed assertion nested in a ternary arm",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(c: boolean, x: string | number) { return c ? (assertString(x), x) : x }",
        ),
    ];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable discarded call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    // A provably non-narrowing OUTER call is certified on its own
    // account; the unprovable assertion nested in its argument is not,
    // and still gaps the statement.
    let nested_source = "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
                         function touch(_v: unknown) {}\n\
                         function f(x: string | number) { return (touch(assertString(x)), x) }";
    let nested = content_for(nested_source, "f");
    let certified_texts: Vec<&str> = nested
        .decided_above_call_spans
        .iter()
        .map(|span| &nested_source[span.start as usize..span.end as usize])
        .collect();
    assert_eq!(
        certified_texts,
        ["touch(assertString(x))"],
        "only the outer non-narrowing call is certified: {nested:?}"
    );
    assert_eq!(
        guard_gap_count(&nested),
        1,
        "the nested assertion gaps the statement: {nested:?}"
    );

    let certified = [
        (
            "a closed unannotated same-file callee",
            "export {};\nfunction touch() {}\n\
             function f(x: string | number) { return (touch(), x) }",
            1,
        ),
        (
            "a closed non-predicate-annotated same-file callee",
            "export {};\nfunction touch(): void {}\n\
             function f(x: string | number) { return (touch(), x) }",
            1,
        ),
        (
            "a construct",
            "export {};\nclass Probe {}\n\
             function f(x: string | number) { return (new Probe(), x) }",
            1,
        ),
    ];
    for (case, source, spans) in certified {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            spans,
            "{case}: a provably non-narrowing discarded call is certified: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// A type carrier folds its operand into ONE shallow-pass answer WITHOUT
/// visiting it (`(…, 0) as number` is `number`), so a sequence under the
/// carrier never reaches the direct-sequence arm and its discarded
/// operands' calls escape to the leaf's blanket certification. An
/// assertion call there still narrows every read that follows, so it takes
/// the SAME discarded-operand discipline the direct arm applies: certified
/// only when the callee provably establishes no narrowing, otherwise the
/// statement takes the typed gap.
#[test]
fn type_carrier_wrapped_sequence_calls_are_never_blanket_certified() {
    let refused = [
        (
            "a closed same-file assertion",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return ((assertString(x), 0) as number) }",
        ),
        (
            "a closed same-file assertion under nested parens",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return (((assertString(x), 0)) as number) }",
        ),
        (
            "a closed same-file assertion under an angle-bracket assertion",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return <number>(assertString(x), 0) }",
        ),
        (
            "an imported callee",
            "import { touch } from \"./touch\";\n\
             function f(x: string | number) { return ((touch(), 0) as number) }",
        ),
        (
            "an exported same-file callee",
            "export function touch() {}\n\
             function f(x: string | number) { return ((touch(), 0) as number) }",
        ),
    ];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable wrapped discarded call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    // A provably non-narrowing wrapped discarded call keeps its
    // certification, and a carrier over a plain non-call leaf mints no
    // gap — the unwrapped-sequence discipline (covered by the direct-arm
    // test above) is unchanged.
    let certified = [
        (
            "a closed unannotated same-file callee",
            "export {};\nfunction touch() {}\n\
             function f(x: string | number) { return ((touch(), 0) as number) }",
            1,
        ),
        (
            "a closed non-predicate-annotated same-file callee",
            "export {};\nfunction touch(): void {}\n\
             function f(x: string | number) { return ((touch(), 0) as number) }",
            1,
        ),
        (
            "a construct",
            "export {};\nclass Probe {}\n\
             function f(x: string | number) { return ((new Probe(), 0) as number) }",
            1,
        ),
        (
            "no call at all",
            "export {};\n\
             function f(x: string | number) { return (0 as number) }",
            0,
        ),
    ];
    for (case, source, spans) in certified {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            spans,
            "{case}: a provably non-narrowing wrapped discarded call is certified: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// A class's decorators and `super_class` heritage expression evaluate in
/// the ENCLOSING frame — before the class body exists — so a sequence
/// among them takes the SAME discarded-operand discipline as any other
/// same-frame sequence. Treating the WHOLE class as a nested frame
/// skipped that split: `((class extends (assertString(x), Base) {}) as
/// typeof Base)` blanket-certified the heritage assertion decided-above
/// while the checker narrows `x` to `string`, and no call obligation
/// exists for the class expression, so the unnarrowed superset would
/// publish complete and warm. A heritage-sequence call is certified only
/// when the callee provably establishes no narrowing; the assertion is
/// unprovable, so the statement takes the typed gap.
#[test]
fn class_heritage_sequence_calls_are_never_blanket_certified() {
    let refused = [(
        "a closed same-file assertion in the heritage sequence",
        "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
         class Base {}\n\
         function f(x: string | number) { return ((class extends (assertString(x), Base) {}) as typeof Base) }",
    )];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable heritage call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    // A plain heritage carries no call and stays gap-free.
    let plain = content_for(
        "export {};\nclass Base {}\n\
         function f(x: string | number) { return ((class extends Base {}) as typeof Base) }",
        "f",
    );
    assert!(
        plain.decided_above_call_spans.is_empty(),
        "a plain heritage has no call to record: {plain:?}"
    );
    assert_eq!(
        guard_gap_count(&plain),
        0,
        "a plain heritage mints no gap: {plain:?}"
    );

    // The class BODY stays its own frame: a sequence inside a method
    // body does not split, and its calls keep the blanket decided-above
    // treatment they always had.
    let body = content_for(
        "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
         function f(x: string | number) { return ((class { m() { return (assertString(x), x) } }) as object) }",
        "f",
    );
    assert_eq!(
        body.decided_above_call_spans.len(),
        1,
        "a class-body call keeps the nested-frame blanket certification: {body:?}"
    );
    assert_eq!(
        guard_gap_count(&body),
        0,
        "a class-body sequence does not split: {body:?}"
    );
}

/// A CONTROL position nested inside a leaf-lowered expression is still a
/// control position: the checker binds a predicate call in a ternary test
/// or a `&&` / `||` left operand into the branch narrowing even when the
/// WHOLE form folds into one shallow-pass leaf answer (`[isString(x) ? x
/// : false]` is `(string | boolean)[]` in the checker). Blanket-certifying
/// that call decided-above dropped the narrowing while the unnarrowed
/// superset completed clean. A call inside a leaf takes the SAME
/// per-callee certification the statement-level control arm applies:
/// certified only when the callee provably establishes no narrowing,
/// otherwise the enclosing statement takes the typed gap.
#[test]
fn leaf_nested_control_position_calls_are_never_blanket_certified() {
    let refused = [
        (
            "a closed same-file predicate in a ternary test",
            "export {};\nfunction isString(x: unknown): x is string { return typeof x === \"string\" }\n\
             function f(x: string | number) { return [isString(x) ? x : false] }",
        ),
        (
            "a closed same-file predicate in a `&&` left operand folded by a carrier",
            "export {};\nfunction isString(x: unknown): x is string { return typeof x === \"string\" }\n\
             function f(x: string | number) { const y = ((isString(x) && x) as unknown); return { y, x } }",
        ),
        (
            "an imported callee in a ternary test",
            "import { isString } from \"./is\";\n\
             function f(x: string | number) { return [isString(x) ? x : false] }",
        ),
        (
            "a closed UNANNOTATED same-file callee in a ternary test",
            "export {};\nfunction check(x: string | number) { return true }\n\
             function f(x: string | number) { return [check(x) ? x : false] }",
        ),
    ];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable nested control call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    let certified = [
        (
            "a closed non-predicate-annotated callee in a ternary test",
            "export {};\nfunction check(x: string | number): boolean { return true }\n\
             function f(x: string | number) { return [check(x) ? x : false] }",
        ),
        (
            "a closed non-predicate-annotated callee in a `&&` left operand folded by a carrier",
            "export {};\nfunction check(x: string | number): boolean { return true }\n\
             function f(x: string | number) { const y = ((check(x) && x) as unknown); return { y, x } }",
        ),
    ];
    for (case, source) in certified {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            1,
            "{case}: a provably non-narrowing nested control call is certified: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// An assertion call hides in a leaf-lowered VALUE position whenever a
/// type carrier folds the whole form without visiting it: `(assertString(x)
/// as void)` is `void` to the shallow pass, and the carrier arm keeps the
/// whole-carrier leaf lowering for a non-object operand — so the call never
/// reaches a structural arm and would be blanket-certified decided-above
/// while the checker narrows every read that follows. The rightmost operand
/// of a folded sequence is the same leak one position over: `((0,
/// assertString(x)) as unknown)` discards nothing syntactically, but the
/// assertion still runs. Both take the per-callee certification: only a
/// provably non-asserting callee certifies; anything else gaps the
/// enclosing statement.
#[test]
fn leaf_carrier_wrapped_assertion_calls_are_never_blanket_certified() {
    let refused = [
        (
            "a closed same-file assertion under a carrier",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { const y = (assertString(x) as void); return { y, x } }",
        ),
        (
            "a closed same-file assertion as a folded sequence's LAST operand",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { const y = ((0, assertString(x)) as unknown); return { y, x } }",
        ),
        (
            "an imported callee under a carrier",
            "import { touch } from \"./touch\";\n\
             function f(x: string | number) { const y = (touch() as void); return { y, x } }",
        ),
    ];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable leaf value-position call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    let certified = [
        (
            "a closed unannotated same-file callee under a carrier",
            "export {};\nfunction touch() {}\n\
             function f(x: string | number) { const y = (touch() as void); return { y, x } }",
        ),
        (
            "a closed non-assertion predicate as a folded sequence's last operand",
            "export {};\nfunction check(x: unknown): x is string { return true }\n\
             function f(x: string | number) { const y = ((0, check(x)) as unknown); return { y, x } }",
        ),
    ];
    for (case, source) in certified {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            1,
            "{case}: a provably non-narrowing leaf value-position call is certified: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }
}

/// A class STATIC BLOCK runs at class evaluation — enclosing-frame
/// immediate, unlike the deferred bodies around it (methods run when
/// called, property initializers at construction). Guarding the whole
/// class body as a nested frame skipped the sequence split for the block:
/// `(class { static { (assertString(x), 0); } } as object)` blanket-certified
/// the assertion decided-above while the checker narrows `x` for every
/// read after the class. A static block's calls take the SAME same-frame
/// discipline the enclosing statement would: certified only when the
/// callee provably establishes no narrowing, otherwise the typed gap.
/// Deferred bodies keep the nested-frame blanket treatment.
#[test]
fn class_static_block_calls_take_enclosing_frame_discipline() {
    let refused = [
        (
            "a closed same-file assertion in a static block's discarded operand",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return (class { static { (assertString(x), 0); } } as object) }",
        ),
        (
            "an imported callee in a static block",
            "import { touch } from \"./touch\";\n\
             function f(x: string | number) { return (class { static { touch(); } } as object) }",
        ),
        (
            "a closed same-file assertion in a static property initializer",
            "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
             function f(x: string | number) { return (class { static p = (assertString(x), 0); } as object) }",
        ),
    ];
    for (case, source) in refused {
        let node = content_for(source, "f");
        assert!(
            node.decided_above_call_spans.is_empty(),
            "{case}: an unprovable static-block call is never certified: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            1,
            "{case}: the statement takes the typed gap: {node:?}"
        );
    }

    let certified = [(
        "a closed unannotated same-file callee in a static block",
        "export {};\nfunction touch() {}\n\
         function f(x: string | number) { return (class { static { touch(); } } as object) }",
    )];
    for (case, source) in certified {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            1,
            "{case}: a provably non-narrowing static-block call is certified: {node:?}"
        );
        assert_eq!(guard_gap_count(&node), 0, "{case}: {node:?}");
    }

    // Deferred bodies stay nested frames: a property initializer runs at
    // CONSTRUCTION, not at class evaluation, so its calls keep the
    // nested-frame blanket certification.
    let deferred = [(
        "a property initializer",
        "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
         function f(x: string | number) { return (class { p = (assertString(x), 0) } as object) }",
    )];
    for (case, source) in deferred {
        let node = content_for(source, "f");
        assert_eq!(
            node.decided_above_call_spans.len(),
            1,
            "{case}: a deferred body keeps the nested-frame blanket certification: {node:?}"
        );
        assert_eq!(
            guard_gap_count(&node),
            0,
            "{case}: a deferred body mints no gap: {node:?}"
        );
    }
}

/// A semantically complete `any` leaf answer (`as any`) is warm-admissible
/// by design — but the leaf still RUNS, and a class static block executes
/// at class evaluation, so the assertion in `(class { static {
/// assertString(x); } } as any)` narrows `x` to `string` for every read
/// that follows in the checker. Answering `SemanticAny` BEFORE the leaf
/// call scanner ran skipped the same-frame discipline entirely: the
/// skeleton mints no call obligation for a class body, so nothing
/// certified the call and nothing gapped — the enclosing `{ a, x }` could
/// seal warm with `x` unnarrowed. The `SemanticAny` arm takes the SAME
/// per-callee certification every other leaf arm applies: an unprovable
/// static-block call flags the statement's typed gap and is never
/// certified, while a plain call-free `as any` leaf stays clean.
#[test]
fn semantic_any_leaf_still_scans_immediate_static_block_calls() {
    let node = content_for(
        "export {};\nfunction assertString(x: unknown): asserts x is string {}\n\
         function f(x: string | number) { const a = (class { static { assertString(x); } } as any); return { a, x } }",
        "f",
    );
    assert!(
        node.decided_above_call_spans.is_empty(),
        "an unprovable static-block call under `as any` is never certified: {node:?}"
    );
    assert_eq!(
        guard_gap_count(&node),
        1,
        "the statement takes the typed gap: {node:?}"
    );

    // Positive control: a call-free `as any` leaf certifies nothing and
    // mints no gap.
    let clean = content_for(
        "export {};\n\
         function g(x: string | number) { const a = ({ p: 1 } as any); return { a, x } }",
        "g",
    );
    assert!(
        clean.decided_above_call_spans.is_empty(),
        "a call-free leaf certifies nothing: {clean:?}"
    );
    assert_eq!(
        guard_gap_count(&clean),
        0,
        "a call-free `as any` leaf stays clean: {clean:?}"
    );
}

/// @ai-generated - return-bearing loop is typed-unsupported and stops the region
#[test]
fn return_bearing_loop_is_unsupported() {
    let node = content_for(
        "function spin() {\n\
         \x20 while (true) {\n\
         \x20   return 1;\n\
         \x20 }\n\
         \x20 return 2;\n\
         }\n",
        "spin",
    );
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Unsupported(SliceUnsupported::Loop)],
        "the region stops at the unsupported marker; the trailing return is dropped"
    );
    assert!(!node.can_fall_through);
}

/// @ai-generated - a switch lowers each case clause as its own region
#[test]
fn switch_lowers_case_regions() {
    let node = content_for(
        "function pick(x: number) {\n\
         \x20 switch (x) {\n\
         \x20   case 1:\n\
         \x20     return 1;\n\
         \x20 }\n\
         \x20 return 2;\n\
         }\n",
        "pick",
    );
    // No `default` and no `break`: the no-matching-case path reaches the
    // trailing return, so the body still falls through the switch.
    let [SliceStatement::Switch {
        cases, has_default, ..
    }, trailing] = node.body.statements.as_ref()
    else {
        panic!("expected a Switch statement followed by the trailing return");
    };
    assert!(!has_default);
    assert_eq!(cases.len(), 1);
    assert!(!cases[0].breaks);
    assert!(!cases[0].region.can_fall_through);
    assert!(matches!(
        cases[0].region.statements.as_ref(),
        [SliceStatement::Return {
            argument: Some(_),
            ..
        }]
    ));
    assert!(matches!(
        trailing,
        SliceStatement::Return {
            argument: Some(_),
            ..
        }
    ));
    assert!(!node.can_fall_through);
}

/// @ai-generated - a try lowers each clause as its own region
#[test]
fn try_lowers_clause_regions() {
    let node = content_for(
        "function attempt() {\n\
         \x20 try {\n\
         \x20   return 1;\n\
         \x20 } catch {\n\
         \x20 }\n\
         }\n",
        "attempt",
    );
    let [SliceStatement::Try {
        block,
        catch,
        finally,
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!("expected a single Try statement");
    };
    assert!(!block.can_fall_through);
    assert!(matches!(
        block.statements.as_ref(),
        [SliceStatement::Return {
            argument: Some(_),
            ..
        }]
    ));
    let catch = catch.as_ref().expect("the catch clause lowers");
    assert!(catch.param.is_none());
    assert!(catch.region.can_fall_through);
    assert!(catch.region.statements.is_empty());
    assert!(finally.is_none());
    // The catch falls through, so the try does — and the body has no
    // trailing return.
    assert!(node.can_fall_through);
}

fn pending_break_undefined_flag(region: &SliceRegion) -> Option<bool> {
    region
        .statements
        .iter()
        .find_map(|statement| match statement {
            SliceStatement::Try {
                pending_break_contributes_undefined,
                ..
            } => Some(*pending_break_contributes_undefined),
            SliceStatement::Labeled { body, .. } => pending_break_undefined_flag(body),
            _ => None,
        })
}

#[test]
fn labelled_break_undefined_uses_a_guaranteed_return_suffix() {
    let wrapped = content_for(
        "function makeProps() { OUT: INNER: { try { break OUT; } finally { return \"a\" as const; } } }",
        "makeProps",
    );
    assert_eq!(pending_break_undefined_flag(&wrapped.body), Some(true));

    let conditional = content_for(
        "function makeProps(flag: boolean) { L: try { break L; } finally { return \"a\" as const; } if (flag) return \"b\" as const; }",
        "makeProps",
    );
    assert_eq!(
        pending_break_undefined_flag(&conditional.body),
        Some(true),
        "a conditional suffix return leaves the function-end path alive"
    );

    let direct = content_for(
        "function makeProps() { L: try { break L; } finally { return \"a\" as const; } return \"b\" as const; }",
        "makeProps",
    );
    assert_eq!(
        pending_break_undefined_flag(&direct.body),
        Some(false),
        "a guaranteed suffix return consumes the function-end path"
    );
}

/// @ai-generated - return of a parameter lowers to the Param carrier with its annotation
#[test]
fn return_of_parameter_is_param_carrier() {
    let node = content_for("function id(a: number) { return a; }\n", "id");
    assert_eq!(node.params.len(), 1);
    assert_eq!(node.params[0].name.as_deref(), Some("a"));
    assert!(!node.params[0].optional);
    assert!(!node.params[0].rest);
    assert_eq!(
        *node.params[0].ty.ty(),
        TypeExpr::Primitive(PrimitiveName::Number),
        "the authored annotation lowers"
    );
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Return {
            argument: Some(SliceExpr::Param { ordinal: 0 }),
            widening_literal: false,
        }],
    );
}

/// @ai-generated - optional and rest parameter flags and types lower
#[test]
fn optional_and_rest_params_lower() {
    let node = content_for(
        "function collect(a?: string, ...rest: boolean[]) { return; }\n",
        "collect",
    );
    assert_eq!(node.params.len(), 2);
    assert_eq!(node.params[0].name.as_deref(), Some("a"));
    assert!(node.params[0].optional);
    assert!(!node.params[0].rest);
    assert_eq!(
        *node.params[0].ty.ty(),
        TypeExpr::union(vec![
            TypeExpr::Primitive(PrimitiveName::String),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]),
        "an optional parameter is T | undefined inside the body"
    );
    assert_eq!(node.params[1].name.as_deref(), Some("rest"));
    assert!(!node.params[1].optional);
    assert!(node.params[1].rest);
    assert!(
        matches!(node.params[1].ty.ty(), TypeExpr::Array { .. }),
        "the rest annotation lowers: {:?}",
        node.params[1].ty.ty()
    );
}

/// @ai-generated - local const binding is a Binding statement and its reference a Local carrier
#[test]
fn local_reaching_definition_is_binding_and_local() {
    let node = content_for(
        "function make() {\n\
         \x20 const x = 1;\n\
         \x20 return x;\n\
         }\n",
        "make",
    );
    assert_eq!(node.body.statements.len(), 2);
    let SliceStatement::Binding {
        name,
        kind,
        init,
        declared,
        widening_literal,
    } = &node.body.statements[0]
    else {
        panic!("the first statement must be the const binding");
    };
    assert_eq!(name.as_ref(), "x");
    assert_eq!(*kind, SliceBindingKind::Const);
    assert!(
        declared.is_none(),
        "an unannotated declarator carries no declared type"
    );
    assert!(
        matches!(
            init,
            Some(SliceExpr::Type(leaf)) if matches!(leaf.ty(), TypeExpr::Literal(LiteralValue::Number(_)))
        ),
        "a const initializer keeps its literal: {init:?}"
    );
    assert!(
        *widening_literal,
        "an unannotated bare-literal const is a WIDENING literal binding"
    );
    assert_eq!(
        node.body.statements[1],
        SliceStatement::Return {
            argument: Some(SliceExpr::Local {
                name: Arc::from("x"),
                param: None,
                captured: false,
            }),
            widening_literal: false,
        },
    );
}

/// @ai-generated - bare-identifier call to the function itself is the recursion hold
#[test]
fn direct_self_call_is_recursion_hold() {
    let node = content_for("function recur() { return recur(); }\n", "recur");
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Return {
            argument: Some(SliceExpr::Call(
                SliceCall::DirectSelf,
                SliceCallSite::new(0, false, false, verter_span::Span::new(26, 33)),
            )),
            widening_literal: false,
        }],
    );
}

/// @ai-generated - an exact same-file served callee is a Flow obligation edge; unresolved / member calls ride the symbolic carrier or `any`
#[test]
fn symbolic_and_unrepresentable_calls() {
    let node = content_for(
        "function helper() { return 1; }\n\
         function run() { return helper(); }\n",
        "run",
    );
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Call(SliceCall::Direct(target), _)),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "run's exact same-file call is a direct flow edge: {:?}",
            node.body.statements
        );
    };
    assert_eq!(target.declaration.name.as_ref(), "helper");
    assert_eq!(
        target.part,
        verter_type_expr::facts::FunctionPartIdentity::DeclarationBody
    );
    assert_eq!(target.overload_ordinal, 0);

    let memo = memo_for(
        "class Service {\n\
         \x20 helper() { return 1; }\n\
         \x20 run() { return this.helper(); }\n\
         }\n",
    );
    let index = memo.function_program_index();
    let entry = member_entry_of(&index, "Service", 1);
    let (selection, skeleton) = selection_for(&memo, entry, &[]);
    let node = memo
        .flow_slice_content(entry, selection, skeleton)
        .expect("the class method slice content must build");
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Return {
            argument: Some(SliceExpr::UnreducedCallValue),
            widening_literal: false,
        }],
        "a `this` receiver is not modeled, so the call has no structural \
         arm and fails closed rather than fabricating `any`"
    );
}

/// A SPREAD entry lowers STRUCTURALLY — its source is an ordinary
/// selected value position, lowered by whatever arm its own form takes,
/// and the direct members around it keep their own lowered values.
///
/// The spread operand here is a FREE name, so its own arm is the leaf
/// lowering, which resolves `typeof base` in the file's OWNER SCOPE —
/// exactly where a free name belongs.
///
/// Mutation recipe: abandoning the structural lowering when an entry is a
/// spread (the pre-change behaviour) folds the whole literal into ONE
/// `SliceExpr::Type` leaf and fails the entry destructure — which is what
/// made `{ ...base(), n: 1 }` an unmodelled position at the root.
#[test]
fn object_return_lowers_a_spread_entry_structurally() {
    let node = content_for(
        "declare const base: { a: number };\n\
         function merge() {\n\
         \x20 return { ...base, x: 1 };\n\
         }\n",
        "merge",
    );
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Object { entries }),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "merge must return a STRUCTURAL object: {:?}",
            node.body.statements
        );
    };
    let [SliceObjectEntry::Spread { source }, SliceObjectEntry::Member(member)] = entries.as_ref()
    else {
        panic!("the spread entry precedes the direct member in source order: {entries:?}");
    };
    assert!(
        matches!(source, SliceExpr::Type(leaf) if matches!(leaf.ty(), TypeExpr::TypeOf(_))),
        "a FREE spread operand takes the owner-scope leaf lowering: {source:?}"
    );
    assert_eq!(member.key.static_name(), Some("x"));
    assert!(
        matches!(member.value, SliceExpr::Type(_)),
        "the direct member keeps its own lowered value: {:?}",
        member.value
    );
}

/// The DEMAND PLANNER selects a spread SOURCE's expression site, and the
/// content half reaches it — under a projection demand for a key ONLY the
/// spread can provide.
///
/// This is the planner/content agreement the shared entry classifier
/// exists to hold. The whole pipeline runs for real here (skeleton →
/// graph → plan → IR → selection), so a content half that descended into
/// a spread the PLANNER did not open a site for would lower its source as
/// the typed `Elided` carrier and lose it — the failure mode the earlier
/// conditional-expression asymmetry produced.
///
/// The DIRECT sibling is the discrimination in the other direction: a
/// demand for `a` cannot be satisfied by the static `x` write, so `x`
/// stays elided while the spread — an unknown-key write — stays a
/// candidate provider.
///
/// Mutation recipe: dropping `SkeletonObjectEntry::Spread`'s
/// `open_site` (or the graph's spread `PathWrite` edge) elides the spread
/// SOURCE instead, which is the exact under-selection that publishes an
/// object missing the keys the spread contributes.
#[test]
fn member_demand_selects_the_spread_source_and_elides_the_unrelated_sibling() {
    let source = "declare const base: { a: number };\n\
                  function merge() {\n\
                  \x20 return { ...base, x: 1 };\n\
                  }\n";

    let node = content_for_path(source, "merge", &[Arc::from("a")]);
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Object { entries }),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "merge must return a structural object: {:?}",
            node.body.statements
        );
    };
    let [SliceObjectEntry::Spread { source: spread }, SliceObjectEntry::Member(member)] =
        entries.as_ref()
    else {
        panic!("the spread entry precedes the direct member: {entries:?}");
    };
    assert!(
        !matches!(spread, SliceExpr::Elided),
        "a demand for `a` reaches the SPREAD source (the only entry that can \
         provision it): {spread:?}"
    );
    assert_eq!(member.key.static_name(), Some("x"));
    assert!(
        matches!(member.value, SliceExpr::Elided),
        "the statically-unrelated sibling `x` stays elided: {:?}",
        member.value
    );

    // A WHOLE-return demand selects both.
    let node = content_for(source, "merge");
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Object { entries }),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "merge must return a structural object: {:?}",
            node.body.statements
        );
    };
    let [SliceObjectEntry::Spread { source: spread }, SliceObjectEntry::Member(member)] =
        entries.as_ref()
    else {
        panic!("the spread entry precedes the direct member: {entries:?}");
    };
    assert!(
        !matches!(spread, SliceExpr::Elided),
        "a whole-return demand reaches the spread source: {spread:?}"
    );
    assert!(
        !matches!(member.value, SliceExpr::Elided),
        "and the direct member: {:?}",
        member.value
    );
}

/// A spread of a FRAME binding resolves through the frame's own lexical
/// authority — the parameter carrier, never an owner-scope leaf.
///
/// Before the structural spread lowering this was an owner-scope LEAK:
/// the whole-literal leaf emitted `...typeof base`, which resolves
/// against the file's module scope, so `{ ...base, x: 1 }` published an
/// unrelated module-scope `base`'s members. The root-identifier gate
/// caught it and failed the whole return closed. Now there is nothing to
/// catch: the operand never reaches owner scope at all.
///
/// Mutation recipe: leaf-lowering a spread-bearing literal republishes
/// `SliceExpr::Type`/`FrameShadowed` over `typeof base` and fails the
/// `Param` assertion — the bait `{ a: string }` is what the evaluator
/// would then answer with.
#[test]
fn object_return_spread_of_a_frame_binding_reads_the_frame_binding() {
    let node = content_for(
        "declare const base: { a: string };\n\
         function merge(base: { a: number }) {\n\
         \x20 return { ...base, x: 1 };\n\
         }\n",
        "merge",
    );
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Object { entries }),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "a parameter-rooted spread must not lower as a bare owner-scope leaf: {:?}",
            node.body.statements
        );
    };
    let [SliceObjectEntry::Spread { source }, SliceObjectEntry::Member(member)] = entries.as_ref()
    else {
        panic!("the spread entry precedes the direct member in source order: {entries:?}");
    };
    assert_eq!(
        source,
        &SliceExpr::Param { ordinal: 0 },
        "the frame-owned spread operand IS the parameter"
    );
    assert_eq!(member.key.static_name(), Some("x"));
}

/// @ai-generated - arrow expression body lowers to a single return of the expression
#[test]
fn arrow_expression_body_is_single_return() {
    let node = content_for("export const double = (x: number) => x * 2;\n", "double");
    assert_eq!(node.params.len(), 1);
    assert!(!node.can_fall_through, "an expression body always returns");
    assert_eq!(
        node.body.statements.as_ref(),
        &[SliceStatement::Return {
            argument: Some(SliceExpr::Gap(
                crate::semantic_query::FlowGap::UnmodeledExpression
            )),
            widening_literal: false,
        }],
        "a binary expression is an unmodelled leaf, not semantic any"
    );
}

/// An UNREAD binding is outside the whole-return slice's value-selected
/// slots: the content omits the whole declaration (its initializer never
/// lowers), while the read binding stays. Mutation recipe: lowering every
/// declarator regardless of selection re-materialises the unread
/// initializer.
#[test]
fn unread_binding_is_omitted_from_content() {
    let node = content_for(
        "function make() {\n\
         \x20 const unused = [1, 2, 3];\n\
         \x20 const x = 1;\n\
         \x20 return x;\n\
         }\n",
        "make",
    );
    assert_eq!(
        node.body.statements.len(),
        2,
        "only the read binding and the return remain: {:?}",
        node.body.statements
    );
    assert!(
        matches!(
            &node.body.statements[0],
            SliceStatement::Binding { name, .. } if name.as_ref() == "x"
        ),
        "the read binding lowers"
    );
    assert!(matches!(
        &node.body.statements[1],
        SliceStatement::Return {
            argument: Some(_),
            ..
        }
    ));
}

/// A single-member return-projection demand elides the sibling member's
/// VALUE (the member list stays complete for static missing-member
/// detection) and keeps the demanded member's content. Mutation recipe:
/// lowering every member value regardless of selection re-materialises
/// the sibling.
#[test]
fn member_demand_elides_sibling_member_values() {
    let node = content_for_path(
        "function pair() {\n\
         \x20 return { a: \"heavy\", b: 1 };\n\
         }\n",
        "pair",
        &[Arc::from("b")],
    );
    let [SliceStatement::Return {
        argument: Some(SliceExpr::Object { entries }),
        ..
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "pair must return a structural object: {:?}",
            node.body.statements
        );
    };
    let members = object_members(entries);
    assert_eq!(members.len(), 2, "the member LIST stays complete");
    assert_eq!(members[0].key.static_name(), Some("a"));
    assert!(
        matches!(members[0].value, SliceExpr::Elided),
        "the sibling's value is the typed Elided carrier: {:?}",
        members[0].value
    );
    assert_eq!(members[1].key.static_name(), Some("b"));
    assert!(
        matches!(members[1].value, SliceExpr::Type(_)),
        "the demanded member's value lowers: {:?}",
        members[1].value
    );
}

/// @ai-generated - locator miss is a typed None, never a panic
#[test]
fn locator_miss_is_typed_none() {
    let memo = memo_for("function id(a: number) { return a; }\n");
    let index = memo.function_program_index();
    let entry = entry_of(&index, "id");
    let (selection, skeleton) = selection_for(&memo, entry, &[]);

    let mut missing_contributor = entry.clone();
    missing_contributor.locator.contributor.contributor_index = 9999;
    assert!(
        memo.flow_slice_content(
            &missing_contributor,
            selection.clone(),
            Arc::clone(&skeleton)
        )
        .is_none(),
        "an out-of-range contributor is a typed miss"
    );

    let mut bad_descent = entry.clone();
    bad_descent.locator.descent = Arc::from([FunctionDescentStep::VariableInitializer {
        declarator_ordinal: 99,
    }]);
    assert!(
        memo.flow_slice_content(&bad_descent, selection, skeleton)
            .is_none(),
        "a mismatched descent is a typed miss"
    );
}
