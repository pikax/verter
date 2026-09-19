//! Committed-input-handoff discriminators.

use std::sync::Arc;

use super::{AcquiredFile, CommittedInputHandoff, HandoffObserve};
use crate::input_basis::{CommitError, RetryOutcome};

fn file(canonical: &str, content: &str) -> AcquiredFile {
    AcquiredFile {
        canonical: Arc::from(canonical),
        content: Arc::from(content),
    }
}

/// A requested file the acquisition wave never acquired observes as
/// typed `NeedInputs` naming the canonical — the demand for the next
/// asynchronous wave, not an absence and not an error. The handoff
/// holds no workspace handle, so no synchronous fetch can happen here.
#[test]
fn unacquired_key_observes_as_typed_need_inputs() {
    let handoff = CommittedInputHandoff::commit(
        [file("/src/App.vue", "<template><div/></template>")],
        [Arc::from("/src/missing.ts")],
    )
    .expect("one file plus one probed-missing key is a coherent wave");

    assert_eq!(
        handoff.observe("/src/never-acquired.ts"),
        HandoffObserve::NeedInputs {
            canonical: Arc::from("/src/never-acquired.ts")
        }
    );
}

/// A key the wave probed and recorded missing observes as a
/// complete-negative `Absent` — distinct from `NeedInputs` the way
/// complete-empty is distinct from partial, pending and failed.
#[test]
fn probed_missing_key_observes_as_complete_negative() {
    let handoff =
        CommittedInputHandoff::commit([file("/src/App.vue", "x")], [Arc::from("/src/gone.ts")])
            .expect("one file plus one probed-missing key is a coherent wave");

    assert_eq!(
        handoff.observe("/src/gone.ts"),
        HandoffObserve::Absent {
            canonical: Arc::from("/src/gone.ts")
        }
    );
    assert_ne!(
        handoff.observe("/src/gone.ts"),
        HandoffObserve::NeedInputs {
            canonical: Arc::from("/src/gone.ts")
        }
    );
}

/// Committed bytes observe byte-identically, and the handoff carries
/// the same committed identity for identical input rows — the
/// same-basis equivalence that native and browser executions of one
/// committed snapshot rely on.
#[test]
fn identical_rows_commit_to_identical_basis_identities() {
    let rows = || {
        [
            file("/lib/a.ts", "export const a = 1;"),
            file("/lib/b.ts", "export const b = 2;"),
        ]
    };
    let first = CommittedInputHandoff::commit(rows(), []).expect("coherent wave");
    let second = CommittedInputHandoff::commit(rows(), []).expect("coherent wave");

    assert_eq!(first.basis().id(), second.basis().id());
    assert_eq!(
        first.observe("/lib/a.ts"),
        HandoffObserve::File {
            canonical: Arc::from("/lib/a.ts"),
            content: Arc::from("export const a = 1;")
        }
    );
    // The publication fence admits exactly this committed identity.
    assert_eq!(first.binding().admit_publication(first.basis()), Ok(()));
}

/// Incoherent waves fail closed: a canonical both acquired and
/// probed-missing, and one acquired with two different contents, are
/// refused by the canonical constructor — the handoff adds no third
/// refusal vocabulary and no silent collapse.
#[test]
fn incoherent_waves_are_refused_closed() {
    let overlap = CommittedInputHandoff::commit([file("/x.ts", "1")], [Arc::from("/x.ts")]);
    assert_eq!(
        overlap.unwrap_err(),
        CommitError::OverlappingPositiveAndNegative {
            canonical: Arc::from("/x.ts")
        }
    );

    let mixed = CommittedInputHandoff::commit([file("/x.ts", "1"), file("/x.ts", "2")], []);
    assert_eq!(
        mixed.unwrap_err(),
        CommitError::MixedRevision {
            canonical: Arc::from("/x.ts")
        }
    );
}

/// The next acquisition wave demands exactly the unrecorded keys:
/// committed positives and probed-missing keys are not re-demanded, so
/// a cancelled or partial wave cannot silently re-read committed rows.
#[test]
fn next_acquisition_wave_demands_only_unrecorded_keys() {
    let handoff = CommittedInputHandoff::commit(
        [file("/lib/a.ts", "a"), file("/lib/b.ts", "b")],
        [Arc::from("/lib/gone.ts")],
    )
    .expect("coherent wave");

    let wave = handoff.next_acquisition_wave([
        Arc::from("/lib/a.ts"),
        Arc::from("/lib/gone.ts"),
        Arc::from("/lib/new.ts"),
    ]);
    assert_eq!(wave.keys(), &[Arc::from("/lib/new.ts")]);
    // An all-recorded request is terminal: no new acquisition is
    // demanded, so a resolver cannot manufacture work.
    assert_eq!(
        handoff
            .next_acquisition_wave([Arc::from("/lib/a.ts"), Arc::from("/lib/gone.ts")])
            .keys(),
        &[] as &[Arc<str>]
    );
}

/// Extending a committed handoff through the canonical retry machinery
/// preserves the already-committed rows: incremental extension and a
/// fresh commit of the same rows land on the same basis identity.
#[test]
fn incremental_extension_matches_fresh_commit_basis() {
    let first = CommittedInputHandoff::commit([file("/lib/a.ts", "a")], []).expect("coherent wave");
    let outcome = first
        .basis()
        .retry(
            first.basis().discovery_wave([Arc::from("/lib/b.ts")]),
            [crate::input_basis::Observation::file(
                Arc::from("/lib/b.ts"),
                Arc::from("b"),
            )],
            [],
        )
        .expect("retry of an unrecorded key extends");
    let RetryOutcome::Extended(extended) = outcome else {
        panic!("a new key must extend the basis");
    };
    let fresh = CommittedInputHandoff::commit([file("/lib/a.ts", "a"), file("/lib/b.ts", "b")], [])
        .expect("coherent wave");
    assert_eq!(extended.id(), fresh.basis().id());
    // The extended basis answers both keys while the original binding
    // still fences its own identity.
    assert!(extended.observe("/lib/b.ts").is_ok());
    assert_eq!(first.binding().admit_publication(first.basis()), Ok(()));
}
