//! Slot-root flags, derived from Vue 3.6.0-rc.15's `markSlotRootOperations`
//! (`compiler-vapor/src/generators/block.ts`): when a slot block has no
//! stable root, each returned `if` gets SLOT_ROOT (128, `genIfFlags`), each
//! `for` SLOT_ROOT (32, `genForFlags`), each dynamic component SLOT_ROOT (4,
//! `genDynamicComponentFlags`) and each outlet FORWARDED (4, `genSlotFlags`),
//! and the blocks a conditional or a loop renders are marked in turn. A slot
//! outlet's fallback inside a slot block is marked the same way
//! (`genSlotOutlet`). Without the flags runtime-vapor never tells the slot
//! boundary that the root's content changed, so a fallback stays beside it.
use super::test_support::render;

fn render_code(source: &str) -> String {
    render(source).code
}

#[test]
fn a_conditional_beside_a_comment_root_notifies_the_slot_boundary() {
    let code =
        render_code("<template><Panel><!-- c --><p v-if=\"ok\">ready</p></Panel></template>");
    assert!(code.contains("}, { _: 1 /* NON_STABLE */ })"), "{code}");
    assert!(
        code.contains("}, null, 129 /* TRUE_SINGLE_ROOT, SLOT_ROOT */)"),
        "{code}"
    );
}

#[test]
fn a_conditional_beside_a_stable_root_is_not_a_slot_root() {
    let code = render_code("<template><Panel><p/><b v-if=\"ok\">ready</b></Panel></template>");
    assert!(!code.contains("SLOT_ROOT"), "{code}");
    assert!(!code.contains("NON_STABLE"), "{code}");
}

#[test]
fn a_loop_slot_root_and_the_conditional_its_row_returns_are_marked() {
    // The row of `<template v-for>` returns the conditional, which is forced
    // MULTI_ROOT on both sides (2 | 2 << 2) and SLOT_ROOT (128); the loop is
    // IS_FRAGMENT (16), SLOT_ROOT (32) and WRAPPED_ROWS (64).
    let code = render_code(
        "<template><Outer><template v-for=\"item in items\"><p v-if=\"item.ok\">{{ item.name }}</p></template></Outer></template>",
    );
    assert!(
        code.contains("}, undefined, 112 /* IS_FRAGMENT, SLOT_ROOT, WRAPPED_ROWS */)"),
        "{code}"
    );
    assert!(
        code.contains("}, null, 138 /* TRUE_MULTI_ROOT, FALSE_MULTI_ROOT, SLOT_ROOT */)"),
        "{code}"
    );
}

#[test]
fn a_loop_of_elements_is_a_single_node_slot_root() {
    let code = render_code(
        "<template><Outer><p v-for=\"item in items\">{{ item }}</p></Outer></template>",
    );
    assert!(
        code.contains("}, undefined, 40 /* IS_SINGLE_NODE, SLOT_ROOT */)"),
        "{code}"
    );
}

#[test]
fn every_dynamic_component_root_of_an_unstable_slot_is_a_slot_root() {
    let code = render_code(
        "<template><Outer><component :is=\"a\"/><component :is=\"b\"/></Outer></template>",
    );
    assert_eq!(
        code.matches(", null, null, 4 /* SLOT_ROOT */)").count(),
        2,
        "{code}"
    );
    // A sole dynamic component keeps its SLOT_SINGLE_ROOT shape.
    let code = render_code("<template><Outer><component :is=\"view\"/></Outer></template>");
    assert!(
        code.contains(", null, null, 36 /* SLOT_SINGLE_ROOT, SLOT_ROOT */)"),
        "{code}"
    );
    // A stable sibling leaves the dynamic component unmarked.
    let code = render_code("<template><Outer><p/><component :is=\"a\"/></Outer></template>");
    assert!(!code.contains("SLOT_ROOT"), "{code}");
}

#[test]
fn an_outlet_is_forwarded_only_as_a_root_of_an_unstable_slot() {
    let code = render_code("<template><Outer><p/><slot/></Outer></template>");
    assert!(code.contains("_createSlot()"), "{code}");
    assert!(!code.contains("FORWARDED"), "{code}");
    // The conditional's branch returns the outlet, which is marked with it.
    let code = render_code("<template><Outer><slot v-if=\"ok\"/></Outer></template>");
    assert!(code.contains(", SLOT_ROOT */)"), "{code}");
    assert!(
        code.contains("_createSlot(\"default\", null, null, 4 /* FORWARDED */)"),
        "{code}"
    );
}

#[test]
fn a_fallback_inside_a_slot_block_marks_its_roots() {
    let code = render_code("<template><Outer><slot><b v-if=\"ok\">x</b></slot></Outer></template>");
    assert!(
        code.contains("}, null, 129 /* TRUE_SINGLE_ROOT, SLOT_ROOT */)"),
        "{code}"
    );
    assert!(code.contains(", 4 /* FORWARDED */)"), "{code}");
    // Outside any slot block the fallback is not a slot's content.
    let code = render_code("<template><slot><b v-if=\"ok\">x</b></slot></template>");
    assert!(!code.contains("SLOT_ROOT"), "{code}");
}
