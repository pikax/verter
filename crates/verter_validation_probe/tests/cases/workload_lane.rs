//! The workload lane against the pinned external corpora.
//!
//! Gated behind `external-corpus` in its entirety: the canonical hermetic run
//! neither reads nor requires either checkout. Only the dedicated probe
//! workflow provisions them and enables this feature.
//!
//! The lane is ONE table-driven test per lane, never one per fixture and never
//! one per framework: which cases run, what they are expected to do, and who
//! owns that expectation are all manifest data, and `framework` is a case
//! attribute the one runner carries.
#![cfg(feature = "external-corpus")]

use verter_validation_probe::corpus::Corpus;
use verter_validation_probe::lane;
use verter_validation_probe::outcome::{Dimension, ProbeOutcomeClass};
use verter_validation_probe::runner::{self, DriverCommand, PhaseDeadlines, PlannedCase};
use verter_validation_probe::summary::{FrameworkSummary, Lane};
use verter_validation_probe::Framework;

/// Which lane this run drives. The workflow's smoke job leaves it unset; the
/// main job sets it to `main`.
fn lane_from_env() -> Lane {
    match std::env::var("VALIDATION_PROBE_LANE").as_deref() {
        Ok("main") => Lane::Main,
        _ => Lane::Smoke,
    }
}

fn block(summary: &verter_validation_probe::Summary, framework: Framework) -> &FrameworkSummary {
    summary
        .frameworks
        .iter()
        .find(|block| block.framework == framework)
        .unwrap_or_else(|| panic!("the lane drove the {framework} framework"))
}

/// Drive the selected lane, publish the ONE summary artifact, and fail on a
/// gate regression.
///
/// The test asserts the lane's OWN disposition, which is the same one
/// `validation-probe-summary --dispose` computes from the published artifact.
/// Both must agree, and both are computed from gate cells alone: a canary or
/// known-fail never blocks here either.
#[test]
fn workload_lane_runs_every_pinned_slice_and_publishes_one_summary() {
    let lane = lane_from_env();
    let summary = match lane::run(lane, PhaseDeadlines::default()) {
        Ok(summary) => summary,
        Err(error) => panic!("the validation probe lane could not run: {error}"),
    };
    let path = lane::write_summary(&summary).expect("the summary artifact is writable");

    // ONE document, with a block for EVERY framework. A lane that quietly
    // stopped driving one corpus would otherwise publish a green summary of
    // the other.
    summary
        .require_every_framework()
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));

    let mut combined_smoke = 0usize;
    for framework in Framework::ALL {
        let block = block(&summary, framework);

        // Non-zero work, proven from the document rather than from this test's
        // own idea of what it selected.
        assert!(
            block.counters.selected > 0,
            "the lane selected no {framework} case; {} would otherwise be a green summary \
             of nothing",
            path.display(),
        );
        assert_eq!(
            block.counters.attempted, block.counters.selected,
            "the lane attempted {} of {} selected {framework} cases",
            block.counters.attempted, block.counters.selected,
        );
        if lane == Lane::Smoke {
            assert!(
                block.counters.selected <= verter_validation_probe::MAX_SMOKE_CASES,
                "the {framework} smoke slice drove {} cases",
                block.counters.selected,
            );
            combined_smoke += block.counters.selected;
        }

        // Determinism: the slice is re-DERIVED from an independently loaded
        // manifest and must be the same case set in the same order, and the
        // same one the summary actually recorded. A slice that shuffled
        // between runs would make every comparison of two summaries
        // meaningless. (Comparing one `selection` call against another would
        // prove nothing: it clones a field.)
        let manifest = lane::load_manifest(framework).expect("the manifest is valid");
        let reloaded = lane::load_manifest(framework).expect("the manifest is valid");
        assert_eq!(
            manifest.derived_smoke_slice(),
            reloaded.derived_smoke_slice(),
            "the {framework} smoke derivation is not deterministic",
        );
        let replanned: Vec<String> = lane::plan(&reloaded, &lane::selection(&reloaded, lane))
            .expect("the selection is loadable")
            .into_iter()
            .map(|case| case.case_id)
            .collect();
        assert_eq!(
            replanned, block.selected_cases,
            "re-planning the {framework} lane selected a different case set than the summary \
             recorded",
        );
    }

    if lane == Lane::Smoke {
        assert!(
            combined_smoke <= Framework::ALL.len() * verter_validation_probe::MAX_SMOKE_CASES,
            "the combined smoke inventory drove {combined_smoke} cases in one job",
        );
    }

    // The per-framework counters and the totals are the SAME numbers. The
    // document's own validation already recomputes this; asserting it here is
    // what makes the lane's published artifact, not just its type, carry the
    // property.
    let summed: usize = summary
        .frameworks
        .iter()
        .map(|block| block.counters.attempted)
        .sum();
    assert_eq!(
        summed, summary.totals.attempted,
        "the per-framework counters do not sum to the totals",
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

/// A CONSTRUCTED, supported component of each framework reaches a
/// `runtimeClient` product through the real addon.
///
/// The corpus cases already exercise the route, but their outcomes depend on
/// what the upstream project happens to contain. These do not: each source is
/// written here and is unambiguously supported, so the only ways they can fail
/// are the ones worth knowing about — the driver never reaching the compiler at
/// all (a `binding` or `host` failure, a spawn failure, an addon that will not
/// load). Without them, a lane whose driver silently stopped calling the addon
/// could still look like a lane of known failures.
#[test]
fn a_constructed_supported_component_reaches_a_runtime_client_product() {
    let sources = [
        (
            Framework::Vue,
            concat!(
                "<template>\n  <p :class=\"tone\">{{ label }}</p>\n</template>\n\n",
                "<script setup>\nconst label = 'probe'\nconst tone = 'plain'\n</script>\n",
            ),
        ),
        (
            Framework::Svelte,
            concat!(
                "<script>\n  let message = $state('probe')\n</script>\n\n",
                "<div class=\"probe\">{message}</div>\n",
            ),
        ),
    ];

    for (framework, source) in sources {
        let manifest = lane::load_manifest(framework).expect("the manifest is valid");
        // Any inventoried case id will do as the carrier identity; what is
        // compiled is the source written here, which the driver never re-reads
        // from the corpus.
        let case_id = manifest
            .smoke
            .first()
            .expect("the smoke slice is non-empty")
            .clone();
        let relative_path = Corpus::for_framework(framework)
            .relative_path_of(&case_id)
            .expect("an inventoried case id belongs to its own framework")
            .to_string();

        let planned = PlannedCase {
            case_id: case_id.clone(),
            relative_path,
            source: source.to_string(),
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
            "the public route did not answer a supported {framework} component: {:?}",
            observation.terminal(Dimension::Route),
        );
        assert_eq!(
            observation.terminal(Dimension::Compile).class(),
            Some(ProbeOutcomeClass::Pass),
            "a supported {framework} component did not reach a valid runtimeClient product: {:?}",
            observation.terminal(Dimension::Compile),
        );
    }
}

/// A ZERO-SELECTION manifest fails the lane through the workflow's own entry
/// point rather than publishing a green summary of nothing — for EVERY
/// framework, so emptying either corpus's slice is caught.
///
/// The refusal is driven over a PLANTED manifest directory, not the committed
/// one: a lane whose re-scoping refusal could only be exercised by editing the
/// files it ships is a refusal nobody can test without breaking the lane.
#[test]
fn a_planted_zero_selection_manifest_fails_the_lane() {
    for emptied in Framework::ALL {
        let mut texts = Vec::new();
        for framework in Framework::ALL {
            let committed = lane::manifest_dir().join(format!("{framework}.toml"));
            let text = std::fs::read_to_string(&committed)
                .unwrap_or_else(|error| panic!("reading {}: {error}", committed.display()));
            let manifest = lane::load_manifest(framework).expect("the committed manifest is valid");
            if framework != emptied {
                texts.push((framework, text));
                continue;
            }

            // Empty the slice, and prove the plant applied: a mutation that
            // silently did not match would leave the committed manifest under
            // test and report a pass.
            assert!(
                !manifest.smoke.is_empty(),
                "the committed {framework} slice is already empty, so emptying it proves nothing",
            );
            assert!(
                !text.contains("\nsmoke = []"),
                "the committed {framework} manifest already carries the planted slice",
            );
            let planted = plant_smoke(&text, "smoke = []");
            assert_ne!(planted, text, "the plant did not change the manifest");
            assert!(
                planted.contains("\nsmoke = []"),
                "the plant did not empty the smoke slice",
            );
            texts.push((framework, planted));
        }

        let dir = planted_manifest_dir(&format!("zero-selection-{emptied}"), &texts);
        let error = lane::run_with_manifest_dir(Lane::Smoke, PhaseDeadlines::default(), &dir)
            .err()
            .unwrap_or_else(|| {
                panic!("a zero-selection {emptied} manifest must fail the lane, not run it")
            });
        assert!(
            error
                .to_string()
                .contains("the smoke slice selects no case"),
            "the lane refused for the wrong reason: {error}",
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
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

/// Write every framework's manifest into a fresh directory under the
/// platform's temp directory, named as the loader requires.
fn planted_manifest_dir(label: &str, texts: &[(Framework, String)]) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("the clock is after the epoch")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("verter-probe-manifest-{unique}-{label}"));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|error| panic!("creating {}: {error}", dir.display()));
    for (framework, text) in texts {
        let path = dir.join(format!("{framework}.toml"));
        std::fs::write(&path, text)
            .unwrap_or_else(|error| panic!("writing {}: {error}", path.display()));
    }
    dir
}

/// Each checkout is AT the commit its manifest pins, read from its own Git
/// state.
///
/// The pin is recorded in the manifest and in the workflow, and every summary
/// republishes it; none of that is evidence about the bytes the lane compiled.
/// A checkout pointed elsewhere whose case set still matched the inventory
/// would publish a revision its cases did not come from.
#[test]
fn every_checkout_is_at_the_pinned_revision() {
    for framework in Framework::ALL {
        let manifest = lane::load_manifest(framework).expect("the manifest is valid");
        let checked_out = Corpus::for_framework(framework)
            .checkout_revision()
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!(
            checked_out,
            manifest.external_revision.as_str(),
            "the {framework} checkout is at a different commit than the manifest pins",
        );
        lane::check_revision(&manifest).unwrap_or_else(|error| panic!("{error}"));
    }
}

/// A planted case whose bytes are not the ones the manifest digests fails the
/// lane rather than being classified as though it were the ratified case.
///
/// This is the whole safety of a GENERATED corpus: its components are not
/// committed anywhere, so the revision pin says nothing about what the
/// generator produced. The digest is the only record of the bytes the
/// inventory was reviewed against.
#[test]
fn a_case_whose_bytes_drift_from_its_digest_fails_the_lane() {
    let framework = Framework::Svelte;
    let manifest = lane::load_manifest(framework).expect("the manifest is valid");
    let case = manifest
        .inventory
        .iter()
        .find(|case| case.digest.is_some())
        .expect("the generated corpus digests its cases");

    // The digest the lane recomputes is over the LOADED bytes, so a mutated
    // manifest digest is the plant that proves the check runs at all.
    let mut drifted = manifest.clone();
    let slot = drifted
        .inventory
        .iter_mut()
        .find(|entry| entry.case_id == case.case_id)
        .expect("the case is inventoried");
    let recorded = slot.digest.clone().expect("the case carries a digest");
    let mutated: String = {
        let hex = recorded.as_str();
        let first = if hex.starts_with('0') { '1' } else { '0' };
        format!("{first}{}", &hex[1..])
    };
    assert_ne!(
        mutated,
        recorded.as_str(),
        "the plant did not change the digest"
    );
    slot.digest = Some(
        verter_validation_probe::Sha256::try_from(mutated).expect("the mutation is still a digest"),
    );

    let error = lane::plan(&drifted, std::slice::from_ref(&case.case_id))
        .expect_err("a case whose bytes do not match its digest must fail the lane");
    assert!(
        error
            .to_string()
            .contains("the corpus generator no longer produces"),
        "the lane refused for the wrong reason: {error}",
    );
}

/// A generated-corpus case the manifest carries NO digest for fails the lane
/// before it is loaded, rather than being classified from unverified bytes.
///
/// Manifest validation accepts an undigested row, because a committed corpus
/// needs none. For a generated corpus that row would otherwise skip the
/// comparison entirely, so the digest requirement has to hold where the case
/// is planned, not only in a test over the committed file.
#[test]
fn a_generated_case_without_a_digest_fails_the_lane() {
    let framework = Framework::Svelte;
    let manifest = lane::load_manifest(framework).expect("the manifest is valid");
    let case_id = manifest
        .smoke
        .first()
        .expect("the smoke slice is non-empty")
        .clone();

    let mut undigested = manifest.clone();
    let slot = undigested
        .inventory
        .iter_mut()
        .find(|entry| entry.case_id == case_id)
        .expect("the case is inventoried");
    assert!(
        slot.digest.take().is_some(),
        "the case already carries no digest, so removing it proves nothing",
    );

    let error = lane::plan(&undigested, std::slice::from_ref(&case_id))
        .expect_err("an undigested generated case must fail the lane");
    assert!(
        error.to_string().contains("carries no digest"),
        "the lane refused for the wrong reason: {error}",
    );
}
