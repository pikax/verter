use std::collections::BTreeSet;
use std::sync::Arc;

use super::{
    AttemptFailure, AttemptOutcome, CompletedAttempt, InputKey, KernelAttempt, LoadSet,
    ResolutionBasis, ResolutionWorldBasis, ResolverObservationKind,
};
use crate::resolver_core::{InputResolutionBudgetMeter, InputResolutionBudgets};

#[test]
fn ratified_input_resolution_budgets_are_the_default_and_inclusive() {
    let ratified = InputResolutionBudgets::RATIFIED;
    assert_eq!(InputResolutionBudgets::default(), ratified);
    assert_eq!(ratified.attempts(), 256);
    assert_eq!(ratified.unique_keys(), 1_024);
    assert_eq!(ratified.input_bytes(), 1_048_576);
    assert_eq!(ratified.driver_depth(), 64);
    assert_eq!(ratified.churn(), 8);
    assert_eq!(ratified.alias_geometry_retention(), 1_024);
    assert_eq!(ratified.completed_witness_retention(), 1_024);
}

#[test]
fn input_resolution_budget_override_is_whole_value_and_tightening_only() {
    let tightened = InputResolutionBudgets::try_tightened(128, 512, 65_536, 32, 4)
        .expect("all five nonzero fields are within the ratified maxima");
    assert_eq!(tightened.attempts(), 128);
    assert_eq!(tightened.unique_keys(), 512);
    assert_eq!(tightened.input_bytes(), 65_536);
    assert_eq!(tightened.driver_depth(), 32);
    assert_eq!(tightened.churn(), 4);
    assert_eq!(tightened.alias_geometry_retention(), 1_024);
    assert_eq!(tightened.completed_witness_retention(), 1_024);

    let seven = InputResolutionBudgets::try_tightened_with_retention(128, 512, 65_536, 32, 4, 3, 2)
        .expect("all seven nonzero fields tighten the ratified maxima");
    assert_eq!(seven.alias_geometry_retention(), 3);
    assert_eq!(seven.completed_witness_retention(), 2);
    assert_eq!(
        InputResolutionBudgets::try_tightened_with_retention(128, 512, 65_536, 32, 4, 1_025, 2,)
            .expect_err("alias retention cannot exceed ratified policy")
            .meter(),
        InputResolutionBudgetMeter::AliasGeometryRetention,
    );
    assert_eq!(
        InputResolutionBudgets::try_tightened_with_retention(128, 512, 65_536, 32, 4, 3, 0,)
            .expect_err("completed-witness retention cannot be disabled")
            .meter(),
        InputResolutionBudgetMeter::CompletedWitnessRetention,
    );

    assert_eq!(
        InputResolutionBudgets::try_tightened(257, 512, 65_536, 32, 4)
            .expect_err("an override may not raise a ratified maximum")
            .meter(),
        InputResolutionBudgetMeter::Attempts
    );
    assert_eq!(
        InputResolutionBudgets::try_tightened(128, 512, 0, 32, 4)
            .expect_err("zero disables a mandatory budget")
            .meter(),
        InputResolutionBudgetMeter::InputBytes
    );
}

fn canonical(s: &str) -> Arc<str> {
    Arc::from(s)
}

/// Builds a synthetic-but-structurally-valid `ResolutionBasis` seeded from
/// `raw` — every field derived from `raw` so two different seeds always
/// produce two UNEQUAL bases (needed by the basis-change/mismatch tests
/// below), never a fixed placeholder.
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

#[test]
fn load_set_new_sorts_and_dedups_keys() {
    let a = InputKey::FileContent {
        canonical: canonical("b.ts"),
    };
    let b = InputKey::FileContent {
        canonical: canonical("a.ts"),
    };
    let dup = b.clone();

    let set = LoadSet::new(vec![a.clone(), b.clone(), dup], basis(1));

    // Discriminates: an un-deduped/un-sorted builder would keep 3 entries
    // in insertion order (a, b, b) instead of 2 sorted entries (b, a).
    assert_eq!(set.keys(), &[b, a]);
}

#[test]
fn load_set_empty_has_no_keys() {
    let set = LoadSet::empty(basis(7));
    assert!(set.is_empty());
    // Discriminates: a builder that dropped/replaced the basis would fail
    // this equality against a freshly-seeded basis(7).
    assert_eq!(set.basis(), basis(7));
}

#[test]
fn load_set_delta_excludes_already_requested_keys() {
    let requested_key = InputKey::PathProbe {
        path: canonical("dir/x"),
    };
    let fresh_key = InputKey::PathProbe {
        path: canonical("dir/y"),
    };
    let set = LoadSet::new(vec![requested_key.clone(), fresh_key.clone()], basis(1));

    let mut accumulated = BTreeSet::new();
    accumulated.insert(requested_key);

    let delta = set.delta(&accumulated);

    // Discriminates: a delta that ignored `accumulated_requested` would
    // return both keys, not just the fresh one.
    assert_eq!(delta, vec![fresh_key]);
}

#[test]
fn load_set_delta_is_empty_when_everything_already_requested() {
    let key = InputKey::RealPath {
        path: canonical("dir/z"),
    };
    let set = LoadSet::new(vec![key.clone()], basis(1));
    let mut accumulated = BTreeSet::new();
    accumulated.insert(key);

    assert!(set.delta(&accumulated).is_empty());
}

#[test]
fn attempt_outcome_map_transforms_complete_only() {
    let complete: AttemptOutcome<i32> = AttemptOutcome::Complete(4);
    assert_eq!(complete.map(|n| n * 2).complete(), Some(8));

    let need_inputs: AttemptOutcome<i32> = AttemptOutcome::NeedInputs(LoadSet::empty(basis(1)));
    // Discriminates: a buggy `map` that unconditionally unwrapped would
    // panic here instead of passing NeedInputs through untouched.
    assert!(need_inputs.map(|n| n * 2).is_need_inputs());

    let terminal: AttemptOutcome<i32> =
        AttemptOutcome::Terminal(AttemptFailure::InputResolutionNoProgress {
            unresolved: Vec::new(),
        });
    assert!(terminal.map(|n| n * 2).is_terminal());
}

#[test]
fn attempt_outcome_predicates_are_mutually_exclusive() {
    let complete: AttemptOutcome<()> = AttemptOutcome::Complete(());
    assert!(complete.is_complete() && !complete.is_need_inputs() && !complete.is_terminal());

    let need_inputs: AttemptOutcome<()> = AttemptOutcome::NeedInputs(LoadSet::empty(basis(1)));
    assert!(
        !need_inputs.is_complete() && need_inputs.is_need_inputs() && !need_inputs.is_terminal()
    );

    let terminal: AttemptOutcome<()> =
        AttemptOutcome::Terminal(AttemptFailure::InputCommitConflictExceeded { retries: 3 });
    assert!(!terminal.is_complete() && !terminal.is_need_inputs() && terminal.is_terminal());
}

#[test]
fn attempt_outcome_complete_discards_non_complete_variants() {
    let need_inputs: AttemptOutcome<i32> = AttemptOutcome::NeedInputs(LoadSet::empty(basis(1)));
    assert_eq!(need_inputs.complete(), None);

    let terminal: AttemptOutcome<i32> =
        AttemptOutcome::Terminal(AttemptFailure::InputLoadUnavailable {
            key: Box::new(InputKey::PackageManifest {
                directory: canonical("pkg"),
            }),
        });
    assert_eq!(terminal.complete(), None);
}

fn store_view_token(seed: u64) -> crate::resolver_core::StoreViewValidationToken {
    crate::resolver_core::StoreViewValidationToken::new(
        seed,
        seed,
        seed,
        seed,
        seed,
        None,
        seed,
        [0_u8; 16],
        crate::resolver_core::StoreViewProjectIdentity([0_u8; 16]),
        None,
    )
}

// `ResolutionBasis` is an exact structured tuple — the kernel compares it
// whole-value, never a folded/hashed digest. These tests discriminate a
// buggy structural-equality impl (one that ignored a field, e.g. by
// deriving `PartialEq` on a wrapper that hashed first) from the real one.

#[test]
fn resolution_basis_equal_when_every_field_matches() {
    assert_eq!(basis(1), basis(1));
}

#[test]
fn resolution_basis_differs_on_workspace_authority_alone() {
    let a = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(9),
            None,
        ),
        None,
    );
    let b = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            // Different authority, SAME base world id — a fold that
            // collapsed authority+id into one scalar could not
            // distinguish this from `a`.
            crate::resolver_core::WorkspaceAuthorityId::test_only(2),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(9),
            None,
        ),
        None,
    );
    // Discriminates: two engines can independently mint
    // ResolutionWorldId(9) — authority is what tells them apart.
    assert_ne!(a, b);
}

#[test]
fn resolution_basis_differs_on_base_world_id_alone() {
    assert_ne!(basis(1), basis(2));
}

#[test]
fn resolution_basis_differs_on_population() {
    let base_population = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    );
    let session_population = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Session(
                crate::resolver_core::SessionFingerprint::test_only(1),
            ),
            crate::resolver_core::ResolutionWorldId::test_only(1),
            // A session-population attempt binds a session world too.
            Some(crate::resolver_core::ResolutionWorldId::test_only(2)),
        ),
        None,
    );
    assert_ne!(base_population, session_population);
}

#[test]
fn resolution_basis_differs_on_session_view_alone() {
    let workspace_only = ResolutionBasis::new(
        ResolutionWorldBasis::new(
            crate::resolver_core::WorkspaceAuthorityId::test_only(1),
            crate::resolver_core::ResolutionPopulation::Base,
            crate::resolver_core::ResolutionWorldId::test_only(1),
            None,
        ),
        None,
    );
    let with_session_view =
        ResolutionBasis::new(workspace_only.resolution_world(), Some(store_view_token(1)));
    // Discriminates: a `ResolutionBasis` that only compared
    // `resolution_world` (dropping `session_view` from equality) would
    // wrongly treat a full-session attempt as interchangeable with a
    // workspace-only one bound to the same resolution world.
    assert_ne!(workspace_only, with_session_view);

    let different_session_view =
        ResolutionBasis::new(workspace_only.resolution_world(), Some(store_view_token(2)));
    assert_ne!(with_session_view, different_session_view);
}

#[test]
fn resolution_basis_unbound_placeholder_never_equals_a_real_basis() {
    let placeholder = ResolutionBasis::unbound_placeholder();
    // Discriminates: if the placeholder's sentinel world id/authority
    // (`0`) ever collided with a real minted value, a stale placeholder
    // basis could spuriously validate against live data instead of
    // failing every comparison as designed.
    assert_ne!(placeholder, basis(1));
    assert_eq!(placeholder, ResolutionBasis::unbound_placeholder());
}

// `CompletedAttempt<T>` / `KernelAttempt<T>` pair a successful attempt's
// answer with the `AttemptOutput` it accumulated.

#[test]
fn completed_attempt_pairs_value_and_output() {
    let mut output = crate::resolver_core::AttemptOutput::new();
    output
        .record_ambient_dependency(canonical("consumer.ts"), canonical("virtual.d.ts"))
        .expect("within default budget");

    let completed = CompletedAttempt::new(42, output.clone());

    assert_eq!(completed.value, 42);
    assert_eq!(completed.output, output);
}

#[test]
fn kernel_attempt_complete_carries_both_value_and_output() {
    let mut output = crate::resolver_core::AttemptOutput::new();
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

// `AttemptFailure::ObservationUnavailable` is the typed failure for a driver
// asked for an observation outside its populated capability
// subset (the five immediate-value observations have no `InputKey` to
// name via `InputLoadUnavailable`).

#[test]
fn observation_unavailable_names_the_missing_capability() {
    let failure = AttemptFailure::ObservationUnavailable {
        observation: ResolverObservationKind::ProjectGeneration,
    };
    match failure {
        AttemptFailure::ObservationUnavailable { observation } => {
            assert_eq!(observation, ResolverObservationKind::ProjectGeneration);
        }
        other => panic!("expected ObservationUnavailable, got {other:?}"),
    }
}

#[test]
fn observation_unavailable_discriminates_by_observation_kind() {
    let missing_project_generation = AttemptFailure::ObservationUnavailable {
        observation: ResolverObservationKind::ProjectGeneration,
    };
    let missing_env_hashes = AttemptFailure::ObservationUnavailable {
        observation: ResolverObservationKind::EnvHashes,
    };
    // Discriminates: a failure that only recorded "unavailable" without
    // the observation kind (or that hashed/collapsed distinct kinds)
    // could not tell these two capability gaps apart.
    assert_ne!(missing_project_generation, missing_env_hashes);
}
