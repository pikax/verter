use std::sync::Arc;

use super::priority_frontier;
use crate::resolver_core::{
    AttemptFailure, AttemptOutcome, AttemptOutput, CompletedAttempt, InputKey, KernelAttempt,
    LoadSet, ResolutionBasis, ResolutionWorldBasis,
};

fn canonical(s: &str) -> Arc<str> {
    Arc::from(s)
}

fn basis(raw: u64) -> ResolutionBasis {
    ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(raw),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(raw),
            None,
        ),
        None,
    )
}

fn path_probe_key(name: &str) -> InputKey {
    InputKey::PathProbe {
        path: canonical(name),
    }
}

fn output_naming(consumer: &str) -> AttemptOutput {
    let mut output = AttemptOutput::new();
    output
        .record_ambient_dependency(canonical(consumer), canonical("virtual"))
        .expect("within default budget");
    output
}

fn hit(value: &str, output: AttemptOutput) -> KernelAttempt<Option<String>> {
    AttemptOutcome::Complete(CompletedAttempt::new(Some(value.to_string()), output))
}

fn miss(output: AttemptOutput) -> KernelAttempt<Option<String>> {
    AttemptOutcome::Complete(CompletedAttempt::new(None, output))
}

fn blocked(key_name: &str, at_basis: ResolutionBasis) -> KernelAttempt<Option<String>> {
    AttemptOutcome::NeedInputs(LoadSet::new(vec![path_probe_key(key_name)], at_basis))
}

fn terminal() -> KernelAttempt<Option<String>> {
    AttemptOutcome::Terminal(AttemptFailure::InputResolutionNoProgress {
        unresolved: Vec::new(),
    })
}

/// Rule 10 + rule 1: an exhausted miss (every candidate misses, no block,
/// no terminal) merges every candidate's output IN CANDIDATE ORDER — the
/// complete ordered rejected-candidate witness.
#[test]
fn exhausted_miss_merges_output_in_candidate_order() {
    let candidates = vec!["a", "b", "c"];
    let outcome = priority_frontier(basis(1), candidates, |name| miss(output_naming(name)));

    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: None,
            output,
        }) => {
            let consumers: Vec<_> = output
                .ambient_dependencies()
                .iter()
                .map(|dep| dep.consumer_canonical.to_string())
                .collect();
            // Discriminates: an unordered accumulator (e.g. one that
            // pushed to a set) could pass with any permutation; this
            // requires the exact candidate order.
            assert_eq!(consumers, vec!["a", "b", "c"]);
        }
        other => panic!("expected Complete(None), got {other:?}"),
    }
}

/// Rule 2: a hit before any block merges the accumulated pre-hit misses'
/// output with the hit's own output, then returns.
#[test]
fn hit_before_any_block_merges_prior_misses_and_returns() {
    let outcome = priority_frontier(basis(1), vec![0, 1, 2], |i| match i {
        0 => miss(output_naming("rejected-0")),
        1 => hit("winner", output_naming("hit-1")),
        _ => panic!("must not evaluate candidates after a hit"),
    });

    match outcome {
        AttemptOutcome::Complete(CompletedAttempt {
            value: Some(value),
            output,
        }) => {
            assert_eq!(value, "winner");
            let consumers: Vec<_> = output
                .ambient_dependencies()
                .iter()
                .map(|dep| dep.consumer_canonical.to_string())
                .collect();
            // Discriminates: a hit that discarded the earlier rejected
            // candidates' witness (instead of merging it in) would leave
            // only "hit-1" here.
            assert_eq!(consumers, vec!["rejected-0", "hit-1"]);
        }
        other => panic!("expected Complete(Some(_)), got {other:?}"),
    }
}

/// Rules 3 + 4: the first block retains only its own `LoadSet`; a further
/// same-basis block UNIONS its keys in.
#[test]
fn same_basis_blocks_union_their_load_sets() {
    let outcome: KernelAttempt<Option<String>> =
        priority_frontier(basis(1), vec![0, 1, 2], |i| match i {
            0 => miss(AttemptOutput::new()),
            1 => blocked("first", basis(1)),
            2 => blocked("second", basis(1)),
            _ => unreachable!(),
        });

    match outcome {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(
                load_set.keys(),
                &[path_probe_key("first"), path_probe_key("second")]
            );
        }
        other => panic!("expected NeedInputs, got {other:?}"),
    }
}

/// Rule 5: a known lower-priority hit AFTER a higher block cannot win —
/// the frontier stops and returns the blocked set instead of the hit.
#[test]
fn lower_priority_hit_after_a_block_cannot_win() {
    let outcome = priority_frontier(basis(1), vec![0, 1], |i| match i {
        0 => blocked("higher", basis(1)),
        1 => hit("should-not-win", output_naming("ignored")),
        _ => unreachable!(),
    });

    match outcome {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(load_set.keys(), &[path_probe_key("higher")]);
        }
        // Discriminates: a buggy frontier that let a later hit win
        // regardless of priority would return Complete(Some("should-not-win"))
        // here instead.
        other => panic!("expected NeedInputs (blocked set wins), got {other:?}"),
    }
}

/// Rule 6: a terminal before any block propagates immediately.
#[test]
fn terminal_before_any_block_propagates() {
    let outcome: KernelAttempt<Option<String>> =
        priority_frontier(basis(1), vec![0, 1], |i| match i {
            0 => terminal(),
            1 => panic!("must not evaluate candidates after a propagated terminal"),
            _ => unreachable!(),
        });

    assert!(matches!(
        outcome,
        AttemptOutcome::Terminal(AttemptFailure::InputResolutionNoProgress { .. })
    ));
}

/// Rule 7: a terminal encountered only speculatively AFTER a
/// higher-priority block does NOT outrank that block — the frontier
/// returns the blocked set instead of the terminal.
#[test]
fn terminal_after_a_block_does_not_outrank_it() {
    let outcome = priority_frontier(basis(1), vec![0, 1], |i| match i {
        0 => blocked("higher", basis(1)),
        1 => terminal(),
        _ => unreachable!(),
    });

    match outcome {
        AttemptOutcome::NeedInputs(load_set) => {
            assert_eq!(load_set.keys(), &[path_probe_key("higher")]);
        }
        // Discriminates: a frontier that let the terminal outrank the
        // block would return Terminal(_) here instead.
        other => panic!("expected NeedInputs (blocked set wins over terminal), got {other:?}"),
    }
}

/// Rule 8: a basis mismatch is NOT unioned with other blocked keys —
/// return the mismatching `LoadSet` immediately, short-circuiting any
/// further union.
#[test]
fn basis_mismatch_short_circuits_without_unioning() {
    let outcome = priority_frontier(basis(1), vec![0, 1], |i| match i {
        0 => blocked("same-basis", basis(1)),
        // A different basis — as if the underlying observation moved
        // mid-attempt.
        1 => blocked("stale-basis", basis(2)),
        _ => unreachable!(),
    });

    match outcome {
        AttemptOutcome::NeedInputs(load_set) => {
            // Discriminates: a frontier that unioned across bases would
            // return both keys under `basis(1)`; the mismatch must win
            // ALONE, under its OWN (differing) basis.
            assert_eq!(load_set.keys(), &[path_probe_key("stale-basis")]);
            assert_eq!(load_set.basis(), basis(2));
        }
        other => panic!("expected NeedInputs (the mismatching set alone), got {other:?}"),
    }
}

/// Rule 9: every `NeedInputs`/`Terminal` return path discards all
/// accumulated branch/frontier output — structurally guaranteed by
/// `KernelAttempt`'s own shape (`NeedInputs`/`Terminal` never carry a
/// `CompletedAttempt`), but exercised here so a future signature change
/// that reintroduced an output channel on those arms would need an
/// explicit, reviewable decision rather than silently leaking evidence
/// from a rejected candidate.
#[test]
fn need_inputs_result_carries_no_completed_attempt() {
    let outcome: KernelAttempt<Option<String>> =
        priority_frontier(basis(1), vec![0, 1], |i| match i {
            0 => miss(output_naming("rejected-before-block")),
            1 => blocked("the-block", basis(1)),
            _ => unreachable!(),
        });

    assert!(outcome.is_need_inputs());
    // `.complete()` is `None` on every non-`Complete` arm — the pre-block
    // miss's output has nowhere to leak into.
    assert_eq!(outcome.complete(), None);
}
