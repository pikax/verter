//! The macro traversal driven one poll at a time.
//!
//! The traversal is one owned computation that its driver advances by poll.
//! With the step hook installed it yields at the start of each plan row,
//! after a typed row's payload read and between its runtime and TSC lanes,
//! and the hook samples the driving thread's observation depths before the
//! first poll and after every poll. These tests hold the stepped run to the
//! answer, the work and the thread state of the run driven in place.

use super::*;

use crate::typeinfo::vue_macro_codegen::VueMacroCodegenOutput;

const PROPS: &str = "/src/Props.vue";
const SIBLING: &str = "/src/InferenceBudgetSibling.vue";

/// One `defineProps` row with a type argument.
fn props_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert(
        &host,
        PROPS,
        r#"<script setup lang="ts">
type Nested = { child: { leaf: string } }
type EmitMap = { save: (value: number) => void }
defineProps<{
  title?: string
  count: number
  mixed: boolean | { nested: string }
  options: Nested
  onChange: EmitMap['save']
}>()
</script>"#,
    );
    host
}

/// A `defineProps` row whose payload inference exceeds its work budget,
/// followed by a complete `defineModel` row.
fn sibling_host() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    host.project_type_store().flow_slice().set_budget_for_test(
        verter_session_query::flow::peeker::FlowSliceBudget {
            max_selected_nodes: 4096,
            ..Default::default()
        },
    );
    let inferred = format!(
        "(true ? [{}] : 0) satisfies unknown",
        vec!["0"; 5_000].join(", ")
    );
    upsert(
        &host,
        SIBLING,
        &format!(
            r#"<script setup lang="ts">
class Payload {{ method() {{ return {inferred} }} }}
defineProps<{{ payload: Payload }}>()
defineModel<string>('selected')
</script>"#
        ),
    );
    host
}

/// Produce `demand` with the traversal stepped: the driving thread's
/// observation depths before the first poll and after every poll, and the
/// bundle.
fn produce_stepped(
    host: &VerterHost,
    canonical_id: &str,
    demand: VueMacroCodegenDemand,
) -> (Vec<[usize; 4]>, VueMacroCodegenOutput) {
    let observed = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let record = Arc::clone(&observed);
    *host.test_force.vue_macro_traversal_step_hook.0.lock() = Some(Arc::new(move || {
        record
            .lock()
            .push(verter_type_engine::request_context::thread_observation_depths_for_tests());
    }));
    let output = produce(host, canonical_id, demand);
    *host.test_force.vue_macro_traversal_step_hook.0.lock() = None;
    let observed = std::mem::take(&mut *observed.lock());
    (observed, output)
}

/// The stepped and in-place runs gave the same answer and did the same
/// work.
fn assert_same_answer(in_place: &VueMacroCodegenOutput, stepped: &VueMacroCodegenOutput) {
    assert_eq!(stepped.runtime, in_place.runtime, "runtime bundle");
    assert_eq!(stepped.tsc, in_place.tsc, "TSC bundle");
    assert_eq!(
        stepped.dependency_failures, in_place.dependency_failures,
        "dependency failures"
    );
    assert_eq!(stepped.completeness, in_place.completeness, "completeness");
    assert_eq!(
        stepped.transitive_canonicals, in_place.transitive_canonicals,
        "transitive canonicals"
    );
    assert_eq!(
        format!("{:?}", stepped.fact_footprint),
        format!("{:?}", in_place.fact_footprint),
        "fact footprint"
    );
    assert_eq!(stepped.counters, in_place.counters, "work counters");
}

#[test]
fn a_stepped_macro_traversal_does_the_work_of_one_driven_in_place() {
    let in_place = produce(&props_host(), PROPS, VueMacroCodegenDemand::Runtime);
    let (observations, stepped) =
        produce_stepped(&props_host(), PROPS, VueMacroCodegenDemand::Runtime);

    // Polls made: one suspension at each of the typed row's three points,
    // then the poll that answers. A traversal that began again after a
    // suspension would poll more.
    assert_eq!(
        observations.len() - 1,
        4,
        "the stepped traversal resumes where it suspended"
    );
    assert_eq!(stepped.counters.producer_invocations, 1);
    assert_eq!(stepped.counters.root_shallow_demands, 1);
    assert_eq!(stepped.counters.runtime_classifier_calls, 5);
    assert_eq!(stepped.counters.tsc_materializations, 0);
    assert_eq!(stepped.counters.scheduler_submissions, 1);
    assert_same_answer(&in_place, &stepped);
}

#[test]
fn the_macro_traversal_leaves_no_thread_scope_installed_between_steps() {
    let (observations, output) = produce_stepped(
        &sibling_host(),
        SIBLING,
        VueMacroCodegenDemand::RuntimeAndTsc,
    );

    // Two typed rows: three suspensions each, then the answering poll.
    assert_eq!(observations.len(), 8, "{observations:?}");
    let before = observations[0];
    for (poll, after) in observations.iter().enumerate().skip(1) {
        assert_eq!(
            *after, before,
            "after poll {poll} the thread's validity, completeness, sticky and tracer \
             depths are where they were before the first poll"
        );
    }
    assert_eq!(
        output
            .runtime
            .as_ref()
            .expect("runtime bundle")
            .entries
            .len(),
        2
    );
    assert_eq!(output.tsc.as_ref().expect("TSC bundle").entries.len(), 2);
}

#[test]
fn a_partial_macro_does_not_change_a_sibling_macros_outcome_across_steps() {
    let (observations, output) =
        produce_stepped(&sibling_host(), SIBLING, VueMacroCodegenDemand::Tsc);
    assert_eq!(observations.len(), 8, "{observations:?}");

    let bundle = output.tsc.as_ref().expect("TSC bundle");
    assert_eq!(bundle.entries.len(), 2);
    let MacroTscOutcome::Complete(MacroTscProjection::Props(props)) = &bundle.entries[0].outcome
    else {
        panic!("budgeted props projection must retain its typed declaration: {bundle:?}");
    };
    let payload = props
        .scope
        .dependency_declarations
        .iter()
        .find(|dependency| dependency.name == "Payload")
        .expect("Payload dependency");
    assert_eq!(
        payload.declaration_failure,
        Some(TscDeclarationFailureReason::SemanticInferenceUnavailable(
            TscSemanticInferenceUnavailableReason::WorkBudgetExceeded,
        ))
    );
    let MacroTscOutcome::Complete(MacroTscProjection::Model(model)) = &bundle.entries[1].outcome
    else {
        panic!("independent model projection must continue after a partial sibling: {bundle:?}");
    };
    assert_eq!(model.name, "selected");
    assert_eq!(model.value_type.as_str(), "string");
    assert!(output.completeness.is_partial());
    assert!(!output.facts_cacheable());

    let in_place = produce(&sibling_host(), SIBLING, VueMacroCodegenDemand::Tsc);
    assert_same_answer(&in_place, &output);
}
