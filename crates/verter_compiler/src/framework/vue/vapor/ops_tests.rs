use super::{
    ops::{NameKind, Nav, Stmt},
    scope::{PendingNavRequest, VaporScope},
    test_support::with_inputs,
    VaporCompiler,
};
use crate::assembly::emit_plan::MapMode;
use crate::work_ledger::CompilerWorkLedger;
use verter_audit::observe::Uncaptured;

#[test]
fn a_reserved_slot_keeps_its_position() {
    with_inputs(
        "<template><div/></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, _| {
            let mut compiler = VaporCompiler::new(inputs);
            let mut scope = VaporScope {
                child_nav: vec![
                    Nav::Reserved,
                    Nav::At {
                        target: (NameKind::Path, 0),
                        container: 10,
                        index: 2,
                        text: false,
                    },
                ],
                ..VaporScope::default()
            };
            scope.pending_nav_requests.push(PendingNavRequest::OwnRef {
                own_ref: 5,
                dom_child_index: 0,
                nav_slot: 0,
            });
            compiler.resolve_pending_nav_requests(&mut scope).unwrap();
            assert_eq!(
                scope.child_nav,
                [
                    Nav::At {
                        target: (NameKind::Node, 5),
                        container: 0,
                        index: 0,
                        text: false,
                    },
                    Nav::At {
                        target: (NameKind::Path, 0),
                        container: 10,
                        index: 2,
                        text: false,
                    }
                ]
            );
        },
    );
}
#[test]
fn an_unresolved_slot_is_refused() {
    let mut state =
        super::state::VaporState::new(crate::emission::test_source("App.vue", ""), MapMode::NoMap);
    let mut scope = VaporScope::default();
    scope.child_statements.push(Stmt::Reserved);
    assert!(matches!(
        state.close_scope(scope, "  ", false, false, verter_span::Span::new(0, 0)),
        Err(
            crate::framework::vue::target::VueTargetRefusal::Unsupported {
                code: "unresolved-operation-slot",
                ..
            }
        )
    ));
}
#[test]
fn anchors_are_numbered_before_the_container_and_keep_their_positions() {
    with_inputs(
        "<template><div/></template>",
        MapMode::NoMap,
        false,
        &[],
        &mut CompilerWorkLedger::<Uncaptured>::new(),
        |inputs, _| {
            let mut compiler = VaporCompiler::new(inputs);
            let mut scope = VaporScope {
                child_nav: vec![Nav::Reserved, Nav::Reserved],
                child_statements: vec![Stmt::Reserved, Stmt::Reserved],
                ..VaporScope::default()
            };
            for slot in 0..2 {
                scope.pending_nav_requests.push(PendingNavRequest::Merge {
                    dom_child_index: slot as u32,
                    has_following: true,
                    nav_slot: Some(slot),
                    stmt_slot: slot,
                });
            }
            compiler.resolve_pending_nav_requests(&mut scope).unwrap();
            assert_eq!(scope.node_ref, Some(1));
            assert_eq!(
                scope.child_nav,
                [
                    Nav::At {
                        target: (NameKind::Node, 0),
                        container: 1,
                        index: 0,
                        text: false,
                    },
                    Nav::At {
                        target: (NameKind::Node, 2),
                        container: 1,
                        index: 1,
                        text: false,
                    }
                ]
            );
            assert_eq!(
                scope.child_statements[0],
                Stmt::InsertionState {
                    container: 1,
                    anchor: super::ops::InsertionAnchor::NodeRef(0)
                }
            );
            assert_eq!(
                scope.child_statements[1],
                Stmt::InsertionState {
                    container: 1,
                    anchor: super::ops::InsertionAnchor::NodeRef(2)
                }
            );
        },
    );
}
