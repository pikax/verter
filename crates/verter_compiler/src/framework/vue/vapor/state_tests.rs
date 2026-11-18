use verter_audit::observe::Uncaptured;

use super::state::VaporState;
use crate::assembly::emit_plan::{EmitPlan, MapMode};
use crate::emission::test_source;
use crate::template::code_gen::shared::helpers::VaporHelper;
use crate::work_ledger::CompilerWorkLedger;

const TEMPLATE_SOURCE: &str = "<template><div/></template>";

fn state() -> VaporState<'static> {
    VaporState::new(test_source("App.vue", TEMPLATE_SOURCE), MapMode::NoMap)
}

fn text(plan: EmitPlan<'_>) -> String {
    plan.write(&mut CompilerWorkLedger::<Uncaptured>::new())
        .unwrap()
        .text()
        .to_owned()
}

#[test]
fn counters_start_at_zero_for_each_state() {
    let mut first = state();
    assert_eq!(first.counters.next_node(), 0);
    assert_eq!(first.counters.next_node(), 1);
    assert_eq!(first.counters.next_path(), 0);
    let mut second = state();
    assert_eq!(second.counters.next_node(), 0);
}

#[test]
fn templates_are_written_in_index_order_with_flags() {
    let mut state = state();
    let first = state.declare_template("<div>hi".into(), true, false);
    let second = state.declare_template("<p>".into(), false, true);
    assert_eq!((first, second), (0, 1));
    state.write_preamble();
    assert!(state.helpers().has(VaporHelper::Template));
    let (_, preamble) = state.finish().unwrap();
    assert_eq!(
        text(preamble.expect("templates were declared")),
        "const t0 = _template(\"<div>hi\", 1)\nconst t1 = _template(\"<p>\", 2)\n"
    );
}

#[test]
fn delegated_events_are_deduplicated_and_sorted() {
    let mut state = state();
    state.delegate_event("keydown");
    state.delegate_event("click");
    state.delegate_event("click");
    state.write_preamble();
    let (_, preamble) = state.finish().unwrap();
    assert_eq!(
        text(preamble.unwrap()),
        "_delegateEvents(\"click\", \"keydown\")\n"
    );
}

#[test]
fn an_unwritten_preamble_is_absent() {
    let mut state = state();
    state.write_preamble();
    let (render, preamble) = state.finish().unwrap();
    assert!(render.is_empty());
    assert!(preamble.is_none());
}
