use crate::{CompletedAttempt, KernelAttempt};
use std::sync::Arc;
use verter_session_query::resolution::{
    AttemptFailure, AttemptOutcome, LoadSet, ResolutionBasis, ResolutionWorldBasis,
    ResolverObservationKind,
};

fn canonical(s: &str) -> Arc<str> {
    Arc::from(s)
}
fn basis(raw: u64) -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            verter_session_query::resolution::WorkspaceAuthorityId::test_only(raw),
            verter_session_query::resolution::ResolutionPopulation::Base,
            verter_session_query::resolution::ResolutionWorldId::test_only(raw),
            None,
        ),
        None,
    )
}

// `CompletedAttempt<T>` / `KernelAttempt<T>` pair a successful attempt's
// answer with the `AttemptOutput` it accumulated.

#[test]
fn completed_attempt_pairs_value_and_output() {
    let mut output = crate::AttemptOutput::new();
    output
        .record_ambient_dependency(canonical("consumer.ts"), canonical("virtual.d.ts"))
        .expect("within default budget");

    let completed = CompletedAttempt::new(42, output.clone());

    assert_eq!(completed.value, 42);
    assert_eq!(completed.output, output);
}

#[test]
fn kernel_attempt_complete_carries_both_value_and_output() {
    let mut output = crate::AttemptOutput::new();
    output
        .record_ambient_dependency(canonical("consumer.ts"), canonical("virtual.d.ts"))
        .expect("within default budget");

    let attempt: KernelAttempt<i32> = AttemptOutcome::Complete(CompletedAttempt::new(7, output));

    // Discriminates: extracting `.complete()` must yield the SAME
    // `CompletedAttempt` — a buggy top-level envelope that dropped the
    // output on the way through `complete()` (e.g. by re-wrapping just
    // the bare value) would fail this.
    let completed = attempt.complete().expect("Complete carries a value");
    assert_eq!(completed.value, 7);
    assert!(!completed.output.is_empty());
}

#[test]
fn kernel_attempt_need_inputs_and_terminal_carry_no_completed_attempt() {
    let need_inputs: KernelAttempt<i32> = AttemptOutcome::NeedInputs(LoadSet::empty(basis(1)));
    assert_eq!(need_inputs.complete(), None);

    let terminal: KernelAttempt<i32> =
        AttemptOutcome::Terminal(AttemptFailure::ObservationUnavailable {
            observation: ResolverObservationKind::ProjectGeneration,
        });
    assert_eq!(terminal.complete(), None);
}
