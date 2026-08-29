//! Compile-fail fixture: no external struct literal of the demand plan or
//! the obligation spec is possible — every field is private (E0451). If
//! any field were widened to `pub`, the corresponding literal would gain a
//! public field and this fixture's expectation would change.
//!
//! These legs live in their OWN fixture (separate from
//! `flow_solve_plan_and_spec_are_sealed.rs`) because a struct-literal
//! privacy error (E0451) on a type is suppressed when the same crate
//! already carries a field-access privacy error (E0616) on that type.
#![allow(dead_code, unreachable_code)]

use verter_session::for_tests::{FlowDemandPlan, FlowObligationSpec};

fn construct_plan() {
    let _ = FlowDemandPlan {
        basis: todo!(),
        subject: todo!(),
        structural_selection: todo!(),
        required_domains: todo!(),
        required_fact_families: todo!(),
        registry_closure: todo!(),
        coverage_obligations: todo!(),
        initial_obligations: todo!(),
        expanded_obligations: todo!(),
        work_order: todo!(),
        tie_break: todo!(),
        convergence: todo!(),
        resources: todo!(),
        obligation_specs: todo!(),
    };
}

fn construct_spec() {
    let _ = FlowObligationSpec {
        id: todo!(),
        requirement: todo!(),
        origin: todo!(),
        basis: todo!(),
        expected_dependencies: todo!(),
        expected_suboperations: todo!(),
    };
}

fn main() {}
