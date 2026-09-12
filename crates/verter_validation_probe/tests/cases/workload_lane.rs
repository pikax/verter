//! The workload lane against the pinned external corpus.
//!
//! Gated behind `external-corpus` in its entirety: the canonical hermetic run
//! neither reads nor requires the checkout. Only the dedicated probe workflow
//! provisions it and enables this feature.
//!
//! The lane is ONE table-driven test per lane, never one per fixture: which
//! cases run, what they are expected to do, and who owns that expectation are
//! all manifest data.
#![cfg(feature = "external-corpus")]

use verter_validation_probe::lane;
use verter_validation_probe::runner::PhaseDeadlines;
use verter_validation_probe::summary::Lane;

/// Which lane this run drives. The workflow's smoke job leaves it unset; the
/// main job sets it to `main`.
fn lane_from_env() -> Lane {
    match std::env::var("VALIDATION_PROBE_LANE").as_deref() {
        Ok("main") => Lane::Main,
        _ => Lane::Smoke,
    }
}

/// Drive the selected lane, publish the summary artifact, and fail on a gate
/// regression.
///
/// The test asserts the lane's OWN disposition, which is the same one
/// `validation-probe-summary --dispose` computes from the published artifact.
/// Both must agree, and both are computed from gate cells alone: a canary or
/// known-fail never blocks here either.
#[test]
fn workload_lane_runs_the_pinned_slice_and_publishes_its_summary() {
    let lane = lane_from_env();
    let summary = match lane::run(lane, PhaseDeadlines::default()) {
        Ok(summary) => summary,
        Err(error) => panic!("the validation probe lane could not run: {error}"),
    };
    let path = lane::write_summary(&summary).expect("the summary artifact is writable");

    // Non-zero work, proven from the document rather than from this test's own
    // idea of what it selected.
    let vue = summary
        .frameworks
        .iter()
        .find(|block| block.framework == verter_validation_probe::Framework::Vue)
        .expect("the lane drove the vue framework");
    assert!(
        vue.counters.selected > 0,
        "the lane selected no case; {} would otherwise be a green summary of nothing",
        path.display(),
    );
    assert_eq!(
        vue.counters.attempted, vue.counters.selected,
        "the lane attempted {} of {} selected cases",
        vue.counters.attempted, vue.counters.selected,
    );
    if lane == Lane::Smoke {
        assert!(
            vue.counters.selected <= verter_validation_probe::MAX_SMOKE_CASES,
            "the smoke slice drove {} cases",
            vue.counters.selected,
        );
    }

    // Determinism: the same slice, re-planned, is the same case set in the
    // same order. A slice that shuffled between runs would make every
    // comparison of two summaries meaningless.
    let manifest = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the manifest is valid");
    assert_eq!(
        lane::selection(&manifest, lane),
        lane::selection(&manifest, lane),
        "the selection is not deterministic",
    );

    let disposition = summary.disposition();
    assert_eq!(
        disposition,
        verter_validation_probe::Disposition::Clean,
        "the lane reported {} gate regression(s); see {}",
        summary.totals.gated_regressions,
        path.display(),
    );
}
