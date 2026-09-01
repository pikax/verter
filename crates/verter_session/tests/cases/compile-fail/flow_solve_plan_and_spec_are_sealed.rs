//! Compile-fail fixture: the demand plan and the obligation spec of a flow
//! solve are SEALED — every field is private, construction is restricted to
//! the planner/runtime boundary, there is no mutable view, and the sealed
//! completion artifact is not cloneable. If any field were widened to
//! `pub`, a setter or mutable-slice accessor appeared, the spec
//! constructor escaped the boundary, or `SealedFlowCompletion` regained
//! `Clone`, the corresponding line would COMPILE and trybuild would turn
//! red.
//!
//! The no-struct-literal legs live in their OWN fixture
//! (`flow_solve_plan_and_spec_no_struct_literal.rs`): a struct-literal
//! privacy error (E0451) on a type is suppressed when the same crate
//! already carries a field-access privacy error (E0616) on that type, so
//! the two legs cannot share a fixture.
#![allow(dead_code, unreachable_code)]

use verter_session::for_tests::{FlowDemandPlan, FlowObligationSpec, SealedFlowCompletion};

// No field write and no field read: even with a plan in hand, its fields
// are unreachable (E0616).
fn mutate_plan(plan: &mut FlowDemandPlan) {
    plan.basis = todo!();
    plan.work_order = todo!();
    let _ = &plan.subject;
}

// No mutable slice or caller-supplied work order: no such accessor exists
// (E0599).
fn mutable_views(plan: &mut FlowDemandPlan) {
    let _: &mut [_] = plan.obligation_specs_mut();
    let _: &mut [_] = plan.work_order_mut();
}

// No boundary constructor: `new` is sealed to the planner/runtime
// boundary (E0624).
fn construct_spec() {
    let _ = FlowObligationSpec::new(todo!(), todo!(), todo!(), todo!(), todo!(), todo!());
}

// No field write and no field read on the evidence contract (E0616).
fn mutate_spec(spec: &mut FlowObligationSpec) {
    spec.basis = todo!();
    spec.expected_dependencies = todo!();
    let _ = &spec.expected_suboperations;
}

// The sealed completion artifact is one-shot and non-cloneable (E0277).
fn completion_is_not_cloneable() {
    fn assert_clone<T: Clone>() {}
    assert_clone::<SealedFlowCompletion>();
}

fn main() {}
