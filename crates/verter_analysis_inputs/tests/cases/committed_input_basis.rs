//! F1: one committed InputBasis with a snapshot fence.
//!
//! Discriminates: consumer-local reads of unrecorded keys, torn publication
//! across two bases, mixed-revision commit, and identity that ignores probe
//! order.

use verter_analysis_inputs::{
    CommitError, InputBasis, LoadWave, NegativeFact, Observation, ObserveError, SnapshotFence,
    TornSnapshot,
};

fn file(canonical: &str, content: &str) -> Observation {
    Observation::file(canonical, content)
}

#[test]
fn commit_identity_is_stable_under_probe_order() {
    let first = InputBasis::commit(
        LoadWave::from_keys(["/b.ts", "/a.ts"]),
        [file("/b.ts", "b"), file("/a.ts", "a")],
        [],
    )
    .expect("commit");
    let second = InputBasis::commit(
        LoadWave::from_keys(["/a.ts", "/b.ts"]),
        [file("/a.ts", "a"), file("/b.ts", "b")],
        [],
    )
    .expect("commit");
    assert_eq!(first.id(), second.id());
    assert_eq!(
        first
            .observations()
            .map(Observation::canonical)
            .collect::<Vec<_>>(),
        vec!["/a.ts", "/b.ts"]
    );
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
