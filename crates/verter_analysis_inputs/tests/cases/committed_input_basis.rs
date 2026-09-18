//! F1: one committed InputBasis with a snapshot fence.
//!
//! Discriminates: consumer-local reads of unrecorded keys, torn publication
//! across two bases, mixed-revision commit, and identity that ignores probe
//! order.

use verter_analysis_inputs::{
    CommitError, DirectoryEntry, InputBasis, LoadWave, NegativeFact, Observation, ObserveError,
    RetryError, RetryOutcome, SnapshotFence, TornSnapshot,
};

fn file(canonical: &str, content: &str) -> Observation {
    Observation::file(canonical, content)
}

fn directory(canonical: &str, entries: Vec<DirectoryEntry>) -> Observation {
    Observation::directory(canonical, entries).expect("directory")
}

#[test]
fn commit_identity_is_stable_under_probe_order() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/b.ts", "/a.ts", "/d"]),
        [
            file("/b.ts", "b"),
            file("/a.ts", "a"),
            directory(
                "/d",
                vec![
                    DirectoryEntry::new("/d/b.ts", false),
                    DirectoryEntry::new("/d/a.ts", false),
                    DirectoryEntry::new("/d/a.ts", false),
                ],
            ),
        ],
        [],
    )
    .expect("commit");
    let second = InputBasis::commit(
        LoadWave::from_keys(["/d", "/a.ts", "/b.ts"]),
        [
            directory(
                "/d",
                vec![
                    DirectoryEntry::new("/d/a.ts", false),
                    DirectoryEntry::new("/d/b.ts", false),
                ],
            ),
            file("/a.ts", "a"),
            file("/b.ts", "b"),
        ],
        [],
    )
    .expect("commit");
    assert_eq!(first.id(), second.id());
    assert_eq!(
        first
            .observations()
            .map(Observation::canonical)
            .collect::<Vec<_>>(),
        vec!["/a.ts", "/b.ts", "/d"]
    );
    assert_eq!(first, second);
}

#[test]
fn distinct_content_mints_distinct_basis_ids() {
    let left = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "one")], [])
        .expect("commit");
    let right = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "two")], [])
        .expect("commit");
    assert_ne!(left.id(), right.id());
}

#[test]
fn mixed_revision_of_one_canonical_is_refused() {
    let error = InputBasis::commit(
        LoadWave::from_keys(["/a.ts"]),
        [file("/a.ts", "one"), file("/a.ts", "two")],
        [],
    )
    .expect_err("mixed revision");
    assert_eq!(
        error,
        CommitError::MixedRevision {
            canonical: "/a.ts".into()
        }
    );
}

#[test]
fn overlapping_positive_and_negative_is_refused() {
    let error = InputBasis::commit(
        LoadWave::from_keys(["/a.ts"]),
        [file("/a.ts", "one")],
        [NegativeFact::absent("/a.ts")],
    )
    .expect_err("overlap");
    assert_eq!(
        error,
        CommitError::OverlappingPositiveAndNegative {
            canonical: "/a.ts".into()
        }
    );
}

#[test]
fn observe_returns_committed_negative_not_an_unrecorded_fallback() {
    let basis = InputBasis::commit(
        LoadWave::from_keys(["/missing.ts"]),
        [],
        [NegativeFact::absent("/missing.ts")],
    )
    .expect("commit");
    match basis.observe("/missing.ts") {
        Err(ObserveError::Negative(fact)) => {
            assert_eq!(fact.canonical(), "/missing.ts");
        }
        other => panic!("expected recorded negative, got {other:?}"),
    }
}

#[test]
fn observe_unrecorded_key_is_not_a_filesystem_read() {
    let basis = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "a")], [])
        .expect("commit");
    assert_eq!(basis.observe("/other.ts"), Err(ObserveError::Unrecorded));
}

#[test]
fn fence_admits_the_bound_basis_and_refuses_a_foreign_one() {
    let bound = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "a")], [])
        .expect("commit");
    let foreign = InputBasis::commit(
        LoadWave::from_keys(["/a.ts"]),
        [file("/a.ts", "changed")],
        [],
    )
    .expect("commit");
    let fence = SnapshotFence::bind(&bound);
    assert_eq!(fence.admit(&bound), Ok(()));
    assert_eq!(fence.admit(&foreign), Err(TornSnapshot::BasisMismatch));
}

#[test]
fn identical_observations_commit_equal_to_a_fresh_commit() {
    let observations = [file("/a.ts", "a"), file("/b.ts", "b")];
    let negatives = [NegativeFact::absent("/c.ts")];
    let wave = LoadWave::from_keys(["/c.ts", "/a.ts", "/b.ts"]);
    let first =
        InputBasis::commit(wave.clone(), observations.clone(), negatives.clone()).expect("first");
    let second = InputBasis::commit(wave, observations, negatives).expect("second");
    assert_eq!(first.id(), second.id());
    assert_eq!(first, second);
}

#[test]
fn conflicting_directory_entries_for_one_path_are_refused() {
    let error = Observation::directory(
        "/d",
        vec![
            DirectoryEntry::new("/d/a", false),
            DirectoryEntry::new("/d/a", true),
        ],
    )
    .expect_err("conflict");
    assert_eq!(
        error,
        CommitError::ConflictingDirectoryEntry {
            directory: "/d".into(),
            path: "/d/a".into()
        }
    );
}

#[test]
fn commit_requires_every_wave_key_to_have_exactly_one_row() {
    let incomplete = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [], []).expect_err("gap");
    assert_eq!(
        incomplete,
        CommitError::IncompleteWave {
            canonical: "/a.ts".into()
        }
    );
    let extraneous = InputBasis::commit(
        LoadWave::from_keys(Vec::<&str>::new()),
        [file("/a.ts", "a")],
        [],
    )
    .expect_err("extra");
    assert_eq!(
        extraneous,
        CommitError::ExtraneousRow {
            canonical: "/a.ts".into()
        }
    );
}

/// F2: unrecorded keys become a sorted, deduplicated load wave. Recorded
/// positives and negatives are not rediscovered.
#[test]
fn discovery_wave_sorts_and_dedups_unrecorded_keys() {
    let basis = InputBasis::commit(
        LoadWave::from_keys(["/a.ts", "/missing.ts"]),
        [file("/a.ts", "a")],
        [NegativeFact::absent("/missing.ts")],
    )
    .expect("commit");
    let wave = basis.discovery_wave(["/c.ts", "/b.ts", "/a.ts", "/c.ts", "/missing.ts"]);
    assert_eq!(
        wave.keys().iter().map(|k| k.as_ref()).collect::<Vec<_>>(),
        vec!["/b.ts", "/c.ts"]
    );
}

/// F2-AC3: extending with discovered rows equals a fresh union commit.
#[test]
fn retry_extended_basis_equals_fresh_union_commit() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/a.ts", "/gone.ts"]),
        [file("/a.ts", "a")],
        [NegativeFact::absent("/gone.ts")],
    )
    .expect("first");
    let wave = first.discovery_wave(["/b.ts"]);
    let RetryOutcome::Extended(retried) =
        first.retry(wave, [file("/b.ts", "b")], []).expect("retry")
    else {
        panic!("expected extended basis");
    };
    let fresh = InputBasis::commit(
        LoadWave::from_keys(["/a.ts", "/gone.ts", "/b.ts"]),
        [file("/a.ts", "a"), file("/b.ts", "b")],
        [NegativeFact::absent("/gone.ts")],
    )
    .expect("fresh");
    assert_eq!(retried.id(), fresh.id());
    assert_eq!(retried, fresh);
    assert_ne!(retried.id(), first.id());
}

#[test]
fn retry_reuses_unprobed_negative_and_still_absent_identity() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/a.ts", "/gone.ts"]),
        [file("/a.ts", "a")],
        [NegativeFact::absent("/gone.ts")],
    )
    .expect("first");
    let reused = first
        .negatives()
        .find(|fact| fact.canonical() == "/gone.ts")
        .expect("gone")
        .clone();
    let RetryOutcome::Extended(extended) = first
        .retry(
            LoadWave::from_keys(["/gone.ts", "/b.ts"]),
            [file("/b.ts", "b")],
            [NegativeFact::absent("/gone.ts")],
        )
        .expect("retry")
    else {
        panic!("expected extended basis");
    };
    match extended.observe("/gone.ts") {
        Err(ObserveError::Negative(fact)) => assert_eq!(fact, &reused),
        other => panic!("expected reused negative, got {other:?}"),
    }
    assert_eq!(extended.observe("/b.ts").unwrap().file_content(), Some("b"));
}

#[test]
fn retry_drops_negative_when_the_key_becomes_present() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/gone.ts"]),
        [],
        [NegativeFact::absent("/gone.ts")],
    )
    .expect("first");
    let RetryOutcome::Extended(extended) = first
        .retry(
            LoadWave::from_keys(["/gone.ts"]),
            [file("/gone.ts", "now")],
            [],
        )
        .expect("retry")
    else {
        panic!("expected extended basis");
    };
    assert_eq!(
        extended.observe("/gone.ts").unwrap().file_content(),
        Some("now")
    );
}

/// Fail-closed: a directory revision that justified a child absence changed,
/// and the producer did not re-record that child.
#[test]
fn retry_refuses_stale_negative_after_parent_directory_revision_change() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/d", "/d/missing.ts"]),
        [directory("/d", vec![DirectoryEntry::new("/d/a.ts", false)])],
        [NegativeFact::absent("/d/missing.ts")],
    )
    .expect("first");
    let error = first
        .retry(
            LoadWave::from_keys(["/d"]),
            [directory(
                "/d",
                vec![
                    DirectoryEntry::new("/d/a.ts", false),
                    DirectoryEntry::new("/d/b.ts", false),
                ],
            )],
            [],
        )
        .expect_err("stale child negative");
    assert_eq!(
        error,
        RetryError::StaleNegative {
            canonical: "/d/missing.ts".into()
        }
    );
}

#[test]
fn retry_of_empty_or_already_recorded_wave_is_terminal() {
    let first = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "a")], [])
        .expect("first");
    assert_eq!(
        first
            .retry(LoadWave::from_keys(Vec::<&str>::new()), [], [])
            .expect("empty"),
        RetryOutcome::Terminal
    );
    assert_eq!(
        first
            .retry(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "a")], [])
            .expect("identical"),
        RetryOutcome::Terminal
    );
}

#[test]
fn prior_fence_refuses_the_retry_extended_basis() {
    let first = InputBasis::commit(LoadWave::from_keys(["/a.ts"]), [file("/a.ts", "a")], [])
        .expect("first");
    let fence = SnapshotFence::bind(&first);
    let RetryOutcome::Extended(extended) = first
        .retry(LoadWave::from_keys(["/b.ts"]), [file("/b.ts", "b")], [])
        .expect("retry")
    else {
        panic!("expected extended basis");
    };
    assert_eq!(fence.admit(&first), Ok(()));
    assert_eq!(fence.admit(&extended), Err(TornSnapshot::BasisMismatch));
    let next = SnapshotFence::bind(&extended);
    assert_eq!(next.admit(&first), Err(TornSnapshot::BasisMismatch));
}
