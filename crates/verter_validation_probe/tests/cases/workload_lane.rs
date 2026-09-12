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
use verter_validation_probe::outcome::{Dimension, ProbeOutcomeClass};
use verter_validation_probe::runner::{self, DriverCommand, PhaseDeadlines, PlannedCase};
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

    // Determinism: the slice is re-DERIVED from an independently loaded
    // manifest and must be the same case set in the same order, and the same
    // one the summary actually recorded. A slice that shuffled between runs
    // would make every comparison of two summaries meaningless. (Comparing one
    // `selection` call against another would prove nothing: it clones a field.)
    let manifest = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the manifest is valid");
    let reloaded = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the manifest is valid");
    assert_eq!(
        manifest.derived_smoke_slice(),
        reloaded.derived_smoke_slice(),
        "the smoke derivation is not deterministic",
    );
    let replanned: Vec<String> = lane::plan(&lane::selection(&reloaded, lane))
        .expect("the selection is loadable")
        .into_iter()
        .map(|case| case.case_id)
        .collect();
    assert_eq!(
        replanned, vue.selected_cases,
        "re-planning the lane selected a different case set than the summary recorded",
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

/// A CONSTRUCTED, supported single-file component reaches a `runtimeClient`
/// product through the real addon.
///
/// The corpus cases already exercise the route, but their outcomes depend on
/// what the upstream project happens to contain. This one does not: the source
/// is written here and is unambiguously supported, so the only ways it can
/// fail are the ones worth knowing about — the driver never reaching the
/// compiler at all (a `binding` or `host` failure, a spawn failure, an addon
/// that will not load). Without it, a lane whose driver silently stopped
/// calling the addon could still look like a lane of known failures.
#[test]
fn a_constructed_supported_component_reaches_a_runtime_client_product() {
    let manifest = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the manifest is valid");
    // Any inventoried case id will do as the carrier identity; what is
    // compiled is the source written here, which the driver never re-reads
    // from the corpus.
    let case_id = manifest
        .smoke
        .first()
        .expect("the smoke slice is non-empty")
        .clone();
    let relative_path = case_id
        .strip_prefix("vue/")
        .expect("a vue case id")
        .to_string();

    let planned = PlannedCase {
        case_id: case_id.clone(),
        relative_path,
        source: concat!(
            "<template>\n  <p :class=\"tone\">{{ label }}</p>\n</template>\n\n",
            "<script setup>\nconst label = 'probe'\nconst tone = 'plain'\n</script>\n",
        )
        .to_string(),
    };

    let driver = DriverCommand::committed(&verter_validation_probe::corpus::workspace_root());
    let results = runner::run_cases(
        &manifest,
        &driver,
        std::slice::from_ref(&planned),
        PhaseDeadlines::default(),
    )
    .unwrap_or_else(|error| panic!("the driver could not be run: {error}"));

    let observation = results
        .into_iter()
        .next()
        .expect("one planned case yields one result")
        .observation
        .unwrap_or_else(|error| panic!("the observation is representable: {error}"));

    assert_eq!(
        observation.terminal(Dimension::Route).class(),
        Some(ProbeOutcomeClass::Pass),
        "the public route did not answer a supported component: {:?}",
        observation.terminal(Dimension::Route),
    );
    assert_eq!(
        observation.terminal(Dimension::Compile).class(),
        Some(ProbeOutcomeClass::Pass),
        "a supported component did not reach a valid runtimeClient product: {:?}",
        observation.terminal(Dimension::Compile),
    );
}

/// A ZERO-SELECTION manifest fails the lane through the workflow's own entry
/// point rather than publishing a green summary of nothing.
///
/// The refusal is driven over a PLANTED manifest directory, not the committed
/// one: a lane whose re-scoping refusal could only be exercised by editing the
/// file it ships is a refusal nobody can test without breaking the lane.
#[test]
fn a_planted_zero_selection_manifest_fails_the_lane() {
    let committed = lane::manifest_dir().join("vue.toml");
    let text = std::fs::read_to_string(&committed)
        .unwrap_or_else(|error| panic!("reading {}: {error}", committed.display()));
    let manifest = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the committed manifest is valid");

    // Empty the slice, and prove the plant applied: a mutation that silently
    // did not match would leave the committed manifest under test and report a
    // pass.
    assert!(
        !manifest.smoke.is_empty(),
        "the committed slice is already empty, so emptying it proves nothing",
    );
    assert!(
        !text.contains("\nsmoke = []"),
        "the committed manifest already carries the planted slice",
    );
    let planted = plant_smoke(&text, "smoke = []");
    assert_ne!(planted, text, "the plant did not change the manifest");
    assert!(
        planted.contains("\nsmoke = []"),
        "the plant did not empty the smoke slice",
    );

    let dir = planted_manifest_dir("zero-selection", &planted);
    let error = lane::run_with_manifest_dir(Lane::Smoke, PhaseDeadlines::default(), &dir)
        .err()
        .unwrap_or_else(|| panic!("a zero-selection manifest must fail the lane, not run it"));
    assert!(
        error
            .to_string()
            .contains("the smoke slice selects no case"),
        "the lane refused for the wrong reason: {error}",
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Replace the manifest's whole `smoke = [...]` array.
fn plant_smoke(text: &str, replacement: &str) -> String {
    let start = text
        .find("\nsmoke = [")
        .expect("the manifest lists a smoke slice")
        + 1;
    let end = start + text[start..].find(']').expect("the array is closed") + 1;
    format!("{}{replacement}{}", &text[..start], &text[end..])
}

/// Write one manifest into a fresh directory under the platform's temp
/// directory, named as the loader requires.
fn planted_manifest_dir(label: &str, vue: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("verter-probe-manifest-{unique}-{label}"));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    std::fs::write(dir.join("vue.toml"), vue)
        .unwrap_or_else(|error| panic!("writing {}: {error}", dir.display()));
    dir
}

/// The checkout is AT the commit the manifest pins, read from its own Git
/// state.
///
/// The pin is recorded in the manifest and in the workflow, and every summary
/// republishes it; none of that is evidence about the bytes the lane compiled.
/// A checkout pointed elsewhere whose fixture set still matched the inventory
/// would publish a revision its cases did not come from.
#[test]
fn the_checkout_is_at_the_pinned_revision() {
    let manifest = lane::load_manifest(verter_validation_probe::Framework::Vue)
        .expect("the manifest is valid");
    let checked_out = verter_validation_probe::corpus::vue_benchmarks::checkout_revision()
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        checked_out,
        manifest.external_revision.as_str(),
        "the checkout is at a different commit than the manifest pins",
    );
    lane::check_revision(&manifest).unwrap_or_else(|error| panic!("{error}"));
}
