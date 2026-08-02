//! `WholeFunctionFlowIrNode` contract: the lazy per-function body IR
//! reborrows the retained parse snapshot once and lowers the whole
//! function with explicit control semantics (sequential regions, `if`
//! reachability, terminal return/throw, return-transparent vs
//! return-bearing loops, typed-unsupported constructs), lowers parameters
//! and simple local reaching definitions into explicit carriers, marks
//! direct same-slot recursion, rides the symbolic `ReturnType<typeof …>`
//! call carrier otherwise, and memoizes per function program key. Locator
//! misses are typed `None`s, never panics.

use std::sync::Arc;

use verter_semantic::analysis::function_program::{
    FunctionDescentStep, FunctionProgramEntry, FunctionProgramIndex,
};
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, ObjectMember, PrimitiveName, TypeExpr};

use crate::decl_body_memo::DeclBodyMemo;
use crate::flow_ir::{
    FlowIrBindingKind, FlowIrExpr, FlowIrStatement, FlowIrUnsupported, WholeFunctionFlowIrNode,
};

fn memo_for(source: &str) -> Arc<DeclBodyMemo> {
    let (state, _provenance) =
        crate::resolver_core::ShallowFileState::service_backed_with_provenance_for_test(
            "/ws/flow_ir.ts",
            source,
        );
    Arc::clone(state.decl_bodies())
}

fn entry_of<'a>(index: &'a FunctionProgramIndex, name: &str) -> &'a FunctionProgramEntry {
    index
        .entries
        .iter()
        .find(|entry| entry.key.declaration.name.as_ref() == name)
        .unwrap_or_else(|| panic!("{name} must be indexed"))
}

fn member_entry_of<'a>(
    index: &'a FunctionProgramIndex,
    class_name: &str,
    ordinal: u32,
) -> &'a FunctionProgramEntry {
    index
        .entries
        .iter()
        .find(|entry| {
            entry.key.declaration.name.as_ref() == class_name
                && matches!(&entry.key.part, FunctionPartIdentity::Member { member_path } if member_path.contains(&ordinal))
        })
        .unwrap_or_else(|| panic!("{class_name} member {ordinal} must be indexed"))
}

fn flow_ir_for(source: &str, name: &str) -> Arc<WholeFunctionFlowIrNode> {
    let memo = memo_for(source);
    let index = memo.function_program_index();
    memo.whole_function_flow_ir(entry_of(&index, name))
        .expect("flow IR must build for an indexed function")
}

/// @ai-generated - block-bodied function with if/else returns: region tree + no fall-through
#[test]
fn if_else_returns_build_region_tree_without_fallthrough() {
    let node = flow_ir_for(
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
    let FlowIrStatement::If {
        test,
        consequent,
        alternate,
    } = &node.body.statements[0]
    else {
        panic!("the single statement must be an if");
    };
    assert!(
        matches!(test, FlowIrExpr::Param { ordinal: 0 }),
        "the test lowers the parameter reference"
    );
    assert!(!consequent.can_fall_through, "the then arm returns");
    assert_eq!(consequent.statements.len(), 1);
    assert!(
        matches!(
            &consequent.statements[0],
            FlowIrStatement::Return {
                argument: Some(FlowIrExpr::Type(TypeExpr::Primitive(PrimitiveName::Number)))
            }
        ),
        "the then arm returns a widened numeric literal"
    );
    let alternate = alternate.as_ref().expect("an else arm exists");
    assert!(!alternate.can_fall_through, "the else arm returns");
    assert_eq!(alternate.statements.len(), 1);
    assert!(
        matches!(
            &alternate.statements[0],
            FlowIrStatement::Return {
                argument: Some(FlowIrExpr::Type(TypeExpr::Primitive(PrimitiveName::String)))
            }
        ),
        "the else arm returns a widened string literal"
    );
}

/// @ai-generated - if without else falls through: one return in the arm
#[test]
fn if_without_else_falls_through() {
    let node = flow_ir_for(
        "function pick(flag: boolean) {\n\
         \x20 if (flag) {\n\
         \x20   return 1;\n\
         \x20 }\n\
         }\n",
        "pick",
    );
    assert!(node.can_fall_through, "no else arm: fall-through");
    assert_eq!(node.body.statements.len(), 1);
    let FlowIrStatement::If {
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
        FlowIrStatement::Return { argument: Some(_) }
    ));
}

/// @ai-generated - bare return carries no argument and terminates the region
#[test]
fn bare_return_carries_no_argument() {
    let node = flow_ir_for("function done() { return; }\n", "done");
    assert!(!node.can_fall_through);
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Return { argument: None }],
    );
}

/// @ai-generated - return-free loop is fall-through transparent before a return
#[test]
fn return_free_loop_is_transparent() {
    let node = flow_ir_for(
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
        FlowIrStatement::TransparentLoop
    ));
    assert!(matches!(
        &node.body.statements[1],
        FlowIrStatement::Return { argument: Some(_) }
    ));
}

/// @ai-generated - return-bearing loop is typed-unsupported and stops the region
#[test]
fn return_bearing_loop_is_unsupported() {
    let node = flow_ir_for(
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
        &[FlowIrStatement::Unsupported(FlowIrUnsupported::Loop)],
        "the region stops at the unsupported marker; the trailing return is dropped"
    );
    assert!(!node.can_fall_through);
}

/// @ai-generated - switch is typed-unsupported
#[test]
fn switch_is_unsupported() {
    let node = flow_ir_for(
        "function pick(x: number) {\n\
         \x20 switch (x) {\n\
         \x20   case 1:\n\
         \x20     return 1;\n\
         \x20 }\n\
         \x20 return 2;\n\
         }\n",
        "pick",
    );
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Unsupported(FlowIrUnsupported::Switch)],
    );
}

/// @ai-generated - try is typed-unsupported
#[test]
fn try_is_unsupported() {
    let node = flow_ir_for(
        "function attempt() {\n\
         \x20 try {\n\
         \x20   return 1;\n\
         \x20 } catch {\n\
         \x20 }\n\
         }\n",
        "attempt",
    );
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Unsupported(FlowIrUnsupported::Try)],
    );
}

/// @ai-generated - return of a parameter lowers to the Param carrier with its annotation
#[test]
fn return_of_parameter_is_param_carrier() {
    let node = flow_ir_for("function id(a: number) { return a; }\n", "id");
    assert_eq!(node.params.len(), 1);
    assert_eq!(node.params[0].name.as_deref(), Some("a"));
    assert!(!node.params[0].optional);
    assert!(!node.params[0].rest);
    assert_eq!(
        node.params[0].ty,
        TypeExpr::Primitive(PrimitiveName::Number),
        "the authored annotation lowers"
    );
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Return {
            argument: Some(FlowIrExpr::Param { ordinal: 0 }),
        }],
    );
}

/// @ai-generated - optional and rest parameter flags and types lower
#[test]
fn optional_and_rest_params_lower() {
    let node = flow_ir_for(
        "function collect(a?: string, ...rest: boolean[]) { return; }\n",
        "collect",
    );
    assert_eq!(node.params.len(), 2);
    assert_eq!(node.params[0].name.as_deref(), Some("a"));
    assert!(node.params[0].optional);
    assert!(!node.params[0].rest);
    assert_eq!(
        node.params[0].ty,
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
        matches!(&node.params[1].ty, TypeExpr::Array { .. }),
        "the rest annotation lowers: {:?}",
        node.params[1].ty
    );
}

/// @ai-generated - local const binding is a Binding statement and its reference a Local carrier
#[test]
fn local_reaching_definition_is_binding_and_local() {
    let node = flow_ir_for(
        "function make() {\n\
         \x20 const x = 1;\n\
         \x20 return x;\n\
         }\n",
        "make",
    );
    assert_eq!(node.body.statements.len(), 2);
    let FlowIrStatement::Binding { name, kind, init } = &node.body.statements[0] else {
        panic!("the first statement must be the const binding");
    };
    assert_eq!(name.as_ref(), "x");
    assert_eq!(*kind, FlowIrBindingKind::Const);
    assert!(
        matches!(
            init,
            Some(FlowIrExpr::Type(TypeExpr::Literal(LiteralValue::Number(_))))
        ),
        "a const initializer keeps its literal: {init:?}"
    );
    assert_eq!(
        node.body.statements[1],
        FlowIrStatement::Return {
            argument: Some(FlowIrExpr::Local {
                name: Arc::from("x"),
            }),
        },
    );
}

/// @ai-generated - bare-identifier call to the function itself is the recursion hold
#[test]
fn direct_self_call_is_recursion_hold() {
    let node = flow_ir_for("function recur() { return recur(); }\n", "recur");
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Return {
            argument: Some(FlowIrExpr::DirectSelfCall),
        }],
    );
}

/// @ai-generated - an exact same-file served callee is a Flow obligation edge; unresolved / member calls ride the symbolic carrier or `any`
#[test]
fn symbolic_and_unrepresentable_calls() {
    let node = flow_ir_for(
        "function helper() { return 1; }\n\
         function run() { return helper(); }\n",
        "run",
    );
    let [FlowIrStatement::Return {
        argument: Some(FlowIrExpr::DirectCall(target)),
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
    let node = memo
        .whole_function_flow_ir(member_entry_of(&index, "Service", 1))
        .expect("the class method flow IR must build");
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Return {
            argument: Some(FlowIrExpr::Any),
        }],
        "an unrepresentable callee falls back to any"
    );
}

/// @ai-generated - object return keeps the spread member for later spread projection
#[test]
fn object_return_rides_spread_member() {
    let node = flow_ir_for(
        "function merge(base: { a: number }) {\n\
         \x20 return { ...base, x: 1 };\n\
         }\n",
        "merge",
    );
    let [FlowIrStatement::Return {
        argument: Some(FlowIrExpr::Type(TypeExpr::Object(object))),
    }] = node.body.statements.as_ref()
    else {
        panic!(
            "merge must return a lowered object: {:?}",
            node.body.statements
        );
    };
    assert!(
        matches!(&object.properties[0], ObjectMember::Spread(_)),
        "the spread member rides unfolded: {:?}",
        object.properties
    );
    assert!(
        matches!(&object.properties[1], ObjectMember::Property(_)),
        "the direct member follows in source order"
    );
}

/// @ai-generated - arrow expression body lowers to a single return of the expression
#[test]
fn arrow_expression_body_is_single_return() {
    let node = flow_ir_for("export const double = (x: number) => x * 2;\n", "double");
    assert_eq!(node.params.len(), 1);
    assert!(!node.can_fall_through, "an expression body always returns");
    assert_eq!(
        node.body.statements.as_ref(),
        &[FlowIrStatement::Return {
            argument: Some(FlowIrExpr::Any),
        }],
        "a binary expression is the scanner's any fallback"
    );
}

/// @ai-generated - whole_function_flow_ir memoizes per function key
#[test]
fn whole_function_flow_ir_memoizes_per_key() {
    let memo = memo_for("function id(a: number) { return a; }\n");
    let index = memo.function_program_index();
    let entry = entry_of(&index, "id");
    let first = memo.whole_function_flow_ir(entry).expect("builds");
    let second = memo.whole_function_flow_ir(entry).expect("builds");
    assert!(
        Arc::ptr_eq(&first, &second),
        "the same entry reuses the memoized node"
    );
}

/// @ai-generated - locator miss is a typed None, never a panic
#[test]
fn locator_miss_is_typed_none() {
    let memo = memo_for("function id(a: number) { return a; }\n");
    let index = memo.function_program_index();
    let entry = entry_of(&index, "id");

    let mut missing_contributor = entry.clone();
    missing_contributor.locator.contributor.contributor_index = 9999;
    assert!(
        memo.whole_function_flow_ir(&missing_contributor).is_none(),
        "an out-of-range contributor is a typed miss"
    );

    let mut bad_descent = entry.clone();
    bad_descent.locator.descent = Arc::from([FunctionDescentStep::VariableInitializer {
        declarator_ordinal: 99,
    }]);
    assert!(
        memo.whole_function_flow_ir(&bad_descent).is_none(),
        "a mismatched descent is a typed miss"
    );
}
