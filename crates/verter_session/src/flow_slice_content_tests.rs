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
            SliceObjectEntry::Member(member) => Some(member),
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
        "function makeProps() { let x: \"a\" | \"b\" = \"a\"; do { (() => { x = \"b\" })() } while (false); return x }",
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
        "declare function opaque(): boolean\nfunction makeProps(x: string | number) { if (typeof x === \"string\" && opaque()) throw 0; return { v: x } }",
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
        "declare function opaque(): boolean\nfunction makeProps(x: string | number) { if (typeof x === \"string\" || opaque()) throw 0; return { v: x } }",
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
        "function isStr(v: string | number): v is string { return typeof v === \"string\" }\nfunction makeProps(x: string | number, n: number) { if (!(isStr(x) && n > 0)) throw 0; return { v: x } }",
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
            argument: Some(SliceExpr::Any),
            widening_literal: false,
        }],
        "a binary expression is the leaf lowering's any fallback"
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
