//! Observation artifact contract: schema, projection, eligibility, retrieval.
//!
//! Each case names a regression the validator or fetcher must refuse, or a
//! termination state it must accept as a total row. Nothing here thresholds
//! a recorded number.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::io::{Cursor, Write};
use std::path::PathBuf;
use std::time::Duration;

use verter_validation_probe::authority::Authority;
use verter_validation_probe::manifest::{Framework, ProbeStateManifest};
use verter_validation_probe::observe::{
    self, AbsenceReason, ArtifactSource, Citation, FetchError, ListedArtifact, Measurement,
    MemoryPair, Mode, ObservationArtifact, ObservationInventory, ObservationRow, RepoInfo, Sample,
    SamplePlan, TemplateDigests, WorkflowRun, ARTIFACT_NAME_PREFIX, MAX_COMPRESSED_BYTES,
    MAX_UNCOMPRESSED_BYTES, WARM_SAMPLES,
};
use verter_validation_probe::outcome::{
    Dimension, Evidence, EvidenceSource, NotApplicableReason, ProbeOutcomeClass, Terminal,
};
use verter_validation_probe::request;
use verter_validation_probe::runner::{self, DriverLine, MemoryBytes};

const VUE_CASE: &str = "vue/fixtures/App.vue";
const SVELTE_CASE: &str = "svelte/fixtures/App.svelte";
const VUE_REV: &str = "0123456789abcdef0123456789abcdef01234567";
const SVELTE_REV: &str = "89abcdef0123456789abcdef0123456789abcdef";
const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const STARTED: &str = "2026-09-13T12:00:00Z";

fn manifest(framework: &str, case: &str, comparison: &str, revision: &str) -> ProbeStateManifest {
    let probe_id = format!("{framework}/{case}");
    let product = format!("{framework}.runtime-client-product");
    let (structural, comparator) = if comparison == "structural" {
        (
            format!(
                r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-identity"
"#
            ),
            r#"
[comparator]
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"
"#,
        )
    } else {
        (
            format!(
                r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "skip"
authority = "{product}"
atom = "product-identity"
reason = "comparator_absent"
"#
            ),
            "",
        )
    };
    let toml = format!(
        r#"
framework = "{framework}"
comparison = "{comparison}"
external_revision = "{revision}"
smoke = ["{probe_id}"]
{comparator}
[applicability]
runtime = "inapplicable"
map = "inapplicable"

[[strata]]
id = "app"
pattern = "App*"
min_cases = 1

[[inventory]]
case_id = "{probe_id}"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Route"
expected_state = "gate"
expected_class = "pass"
authority = "compiler.public-request-route"
atom = "route-callable"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Compile"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-shape"
{structural}
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Runtime"
expected_state = "skip"
authority = "{product}"
atom = "runtime-behavior"
reason = "runtime_executor_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Map"
expected_state = "skip"
authority = "{product}"
atom = "source-map"
reason = "map_validator_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Performance"
expected_state = "canary"
expected_class = "pass"
authority = "compiler.equivalent-work-ledger"
atom = "equivalent-work-ledger"
"#
    );
    ProbeStateManifest::from_toml_str(&toml).expect("fixture manifest is valid")
}

fn vue() -> ProbeStateManifest {
    manifest("vue", "fixtures/App.vue", "structural", VUE_REV)
}

fn svelte() -> ProbeStateManifest {
    manifest("svelte", "fixtures/App.svelte", "none", SVELTE_REV)
}

fn manifests() -> Vec<ProbeStateManifest> {
    vec![vue(), svelte()]
}

fn pass() -> Terminal {
    Terminal::Class {
        class: ProbeOutcomeClass::Pass,
        secondary: Vec::new(),
        evidence: Vec::new(),
    }
}

fn class_term(class: ProbeOutcomeClass) -> Terminal {
    Terminal::Class {
        class,
        secondary: Vec::new(),
        evidence: Vec::new(),
    }
}

fn na(reason: NotApplicableReason) -> Terminal {
    Terminal::NotApplicable { reason }
}

fn not_run(blocked_by: ProbeOutcomeClass) -> Terminal {
    Terminal::NotRun { blocked_by }
}

fn terminals_for(framework: Framework, structural: Terminal) -> BTreeMap<Dimension, Terminal> {
    let mut terminals = BTreeMap::new();
    terminals.insert(Dimension::Route, pass());
    terminals.insert(Dimension::Compile, pass());
    terminals.insert(Dimension::Structural, structural);
    terminals.insert(
        Dimension::Runtime,
        na(NotApplicableReason::RuntimeExecutorAbsent),
    );
    terminals.insert(Dimension::Map, na(NotApplicableReason::MapValidatorAbsent));
    terminals.insert(Dimension::Performance, pass());
    let _ = framework;
    terminals
}

fn vue_pass() -> BTreeMap<Dimension, Terminal> {
    terminals_for(Framework::Vue, pass())
}

fn svelte_pass() -> BTreeMap<Dimension, Terminal> {
    terminals_for(Framework::Svelte, na(NotApplicableReason::ComparatorAbsent))
}

fn measured(terminals: BTreeMap<Dimension, Terminal>) -> Sample {
    Sample {
        terminals,
        measurement: Some(Measurement {
            elapsed_ns: 1_000,
            memory: Some(MemoryPair {
                peak_bytes: 8,
                live_bytes: 4,
            }),
        }),
        absence: None,
    }
}

fn revisions() -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    map.insert("svelte".to_string(), SVELTE_REV.to_string());
    map.insert("vue".to_string(), VUE_REV.to_string());
    map
}

fn header_fields(rows: Vec<ObservationRow>) -> ObservationArtifact {
    let corpus_revisions = revisions();
    let corpus_digest = observe::corpus_digest(&corpus_revisions);
    ObservationArtifact {
        artifact_id: observe::compose_artifact_id(COMMIT, &corpus_digest, "1", 1),
        verter_commit: COMMIT.to_string(),
        corpus_revisions,
        corpus_digest,
        workflow_run_id: "1".to_string(),
        run_attempt: 1,
        rust_version: "rustc 1.97.1".to_string(),
        node_version: "v24.0.0".to_string(),
        addon_version: "0.0.1-beta.3".to_string(),
        os: "linux".to_string(),
        arch: "x86_64".to_string(),
        execution_mode: "ci".to_string(),
        sample_plan: SamplePlan { cold: 1, warm: 5 },
        request_vue: request::REQUEST_VUE.to_string(),
        request_svelte: request::REQUEST_SVELTE.to_string(),
        template_digests: TemplateDigests {
            vue: request::template_digest(Framework::Vue),
            svelte: request::template_digest(Framework::Svelte),
        },
        rows,
    }
}

fn row(
    case_id: &str,
    mode: Mode,
    _terminals: BTreeMap<Dimension, Terminal>,
    samples: Vec<Sample>,
) -> ObservationRow {
    let framework = runner::framework_of(case_id).expect("case id names a framework");
    let relative = case_id
        .strip_prefix(&format!("{}/", framework.as_str()))
        .expect("prefix");
    let observed_outcome = observe::project_observed_outcome(&samples).expect("projectable");
    let corpus_revision = match framework {
        Framework::Vue => VUE_REV,
        Framework::Svelte => SVELTE_REV,
    };
    ObservationRow {
        row_id: format!("{}@{}", case_id, mode.as_str()),
        case_id: case_id.to_string(),
        mode,
        corpus_revision: corpus_revision.to_string(),
        request_digest: request::request_digest(framework, relative),
        sample_count: samples.len(),
        samples,
        observed_outcome,
        comparison_eligible: false,
        semantic_basis: None,
        equivalent_work_basis: None,
    }
}

fn warm_samples(terminals: BTreeMap<Dimension, Terminal>) -> Vec<Sample> {
    (0..WARM_SAMPLES)
        .map(|_| measured(terminals.clone()))
        .collect()
}

fn valid_artifact() -> ObservationArtifact {
    header_fields(vec![
        row(VUE_CASE, Mode::Cold, vue_pass(), vec![measured(vue_pass())]),
        row(VUE_CASE, Mode::Warm, vue_pass(), warm_samples(vue_pass())),
        row(
            SVELTE_CASE,
            Mode::Cold,
            svelte_pass(),
            vec![measured(svelte_pass())],
        ),
        row(
            SVELTE_CASE,
            Mode::Warm,
            svelte_pass(),
            warm_samples(svelte_pass()),
        ),
    ])
}

fn reject(artifact: &ObservationArtifact, pattern: &str) {
    let error = artifact
        .validate(&manifests())
        .expect_err("the planted artifact must be refused");
    let message = error.to_string();
    assert!(
        message.contains(pattern),
        "expected `{pattern}` in `{message}`"
    );
}

fn bases() -> (Citation, Citation) {
    (
        Citation {
            authority: Authority::VueRuntimeClientProduct,
            atom: "product-identity".to_string(),
        },
        Citation {
            authority: Authority::CompilerEquivalentWorkLedger,
            atom: "equivalent-work-ledger".to_string(),
        },
    )
}

/// A complete metadata block on a two-framework smoke grid validates, and
/// the adapter's default is comparison_eligible = false.
#[test]
fn a_full_metadata_artifact_validates_and_defaults_ineligible() {
    let artifact = valid_artifact();
    artifact
        .validate(&manifests())
        .unwrap_or_else(|error| panic!("{error}"));
    for row in &artifact.rows {
        assert!(!row.comparison_eligible);
        assert_eq!(
            row.observed_outcome,
            observe::project_observed_outcome(&row.samples).expect("projectable"),
        );
    }
    let encoded = observe::encode_corpus_revisions(&artifact.corpus_revisions);
    assert_eq!(encoded, format!("svelte={SVELTE_REV}\nvue={VUE_REV}\n"));
}

/// Digest encoding is bytewise on keys: svelte before vue. Swapping the
/// revisions in the map without changing the digest is refused.
#[test]
fn swapped_framework_revisions_are_rejected() {
    let mut artifact = valid_artifact();
    let vue = artifact.corpus_revisions["vue"].clone();
    let svelte = artifact.corpus_revisions["svelte"].clone();
    artifact.corpus_revisions.insert("vue".to_string(), svelte);
    artifact.corpus_revisions.insert("svelte".to_string(), vue);
    reject(&artifact, "corpus_digest does not match");
}

/// Missing one warm row is a smaller artifact, never a smaller inventory.
#[test]
fn an_artifact_missing_one_warm_row_is_rejected() {
    let mut artifact = valid_artifact();
    artifact
        .rows
        .retain(|row| row.row_id != format!("{VUE_CASE}@warm"));
    reject(&artifact, "missing row");
}

#[test]
fn a_duplicated_row_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows.push(artifact.rows[0].clone());
    reject(&artifact, "duplicated row");
}

#[test]
fn a_warm_row_with_four_samples_is_rejected() {
    let mut artifact = valid_artifact();
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm row");
    row.samples.pop();
    row.sample_count = 4;
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    reject(&artifact, "do not match the warm plan");
}

#[test]
fn comparison_eligible_without_semantic_basis_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, equivalent) = bases();
    let row = &mut artifact.rows[0];
    row.comparison_eligible = true;
    row.equivalent_work_basis = Some(equivalent);
    let _ = semantic;
    reject(&artifact, "requires semantic_basis");
}

#[test]
fn comparison_eligible_without_equivalent_work_basis_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, _) = bases();
    let row = &mut artifact.rows[0];
    row.comparison_eligible = true;
    row.semantic_basis = Some(semantic);
    reject(&artifact, "requires equivalent_work_basis");
}

#[test]
fn a_svelte_row_claiming_comparison_eligible_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, equivalent) = bases();
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.case_id == SVELTE_CASE && row.mode == Mode::Cold)
        .expect("svelte cold");
    row.comparison_eligible = true;
    row.semantic_basis = Some(semantic);
    row.equivalent_work_basis = Some(equivalent);
    reject(&artifact, "requires comparison = structural");
}

#[test]
fn eligible_with_a_cold_structural_mismatch_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, equivalent) = bases();
    let row = &mut artifact.rows[0];
    row.samples[0].terminals.insert(
        Dimension::Structural,
        class_term(ProbeOutcomeClass::SemanticMismatch),
    );
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    row.comparison_eligible = true;
    row.semantic_basis = Some(semantic);
    row.equivalent_work_basis = Some(equivalent);
    reject(&artifact, "requires Structural = pass");
}

#[test]
fn eligible_with_mixed_warm_structural_samples_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, equivalent) = bases();
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples[4].terminals.insert(
        Dimension::Structural,
        class_term(ProbeOutcomeClass::SemanticMismatch),
    );
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    row.comparison_eligible = true;
    row.semantic_basis = Some(semantic);
    row.equivalent_work_basis = Some(equivalent);
    reject(&artifact, "requires Structural = pass");
}

#[test]
fn eligible_with_a_warm_performance_timeout_is_rejected() {
    let mut artifact = valid_artifact();
    let (semantic, equivalent) = bases();
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples[0].terminals.insert(
        Dimension::Performance,
        class_term(ProbeOutcomeClass::Timeout),
    );
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    row.comparison_eligible = true;
    row.semantic_basis = Some(semantic);
    row.equivalent_work_basis = Some(equivalent);
    reject(&artifact, "requires Performance = pass");
}

#[test]
fn a_sample_with_neither_measurement_nor_absence_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows[0].samples[0].measurement = None;
    artifact.rows[0].samples[0].absence = None;
    reject(&artifact, "measurement or absence");
}

#[test]
fn compile_terminated_beside_a_measurement_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows[0].samples[0].absence = Some(AbsenceReason::CompileTerminated);
    reject(&artifact, "carry no measurement");
}

fn crash_terminals() -> BTreeMap<Dimension, Terminal> {
    let mut terminals = BTreeMap::new();
    terminals.insert(Dimension::Route, class_term(ProbeOutcomeClass::Crash));
    terminals.insert(Dimension::Compile, class_term(ProbeOutcomeClass::Crash));
    terminals.insert(Dimension::Structural, class_term(ProbeOutcomeClass::Crash));
    terminals.insert(
        Dimension::Runtime,
        na(NotApplicableReason::RuntimeExecutorAbsent),
    );
    terminals.insert(Dimension::Map, na(NotApplicableReason::MapValidatorAbsent));
    terminals.insert(Dimension::Performance, class_term(ProbeOutcomeClass::Crash));
    terminals
}

fn unavailable_after(failed_sample: u8, blocked_by: ProbeOutcomeClass) -> Sample {
    Sample {
        terminals: Dimension::ALL
            .into_iter()
            .map(|dimension| (dimension, not_run(blocked_by)))
            .collect(),
        measurement: None,
        absence: Some(AbsenceReason::WorkerUnavailableAfterSample { failed_sample }),
    }
}

/// Load failure on the first warm sample: typed absence, no measurement,
/// remaining slots worker_unavailable.
#[test]
fn a_load_failure_mid_warm_validates_as_a_total_row() {
    let mut artifact = valid_artifact();
    let mut failed = crash_terminals();
    failed.insert(
        Dimension::Route,
        class_term(ProbeOutcomeClass::HarnessFailure),
    );
    failed.insert(
        Dimension::Compile,
        not_run(ProbeOutcomeClass::HarnessFailure),
    );
    failed.insert(
        Dimension::Structural,
        not_run(ProbeOutcomeClass::HarnessFailure),
    );
    failed.insert(
        Dimension::Performance,
        class_term(ProbeOutcomeClass::HarnessFailure),
    );
    let mut samples = vec![Sample {
        terminals: failed,
        measurement: None,
        absence: Some(AbsenceReason::LoadFailed),
    }];
    while samples.len() < WARM_SAMPLES as usize {
        samples.push(unavailable_after(0, ProbeOutcomeClass::HarnessFailure));
    }
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples = samples;
    row.sample_count = WARM_SAMPLES as usize;
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    artifact
        .validate(&manifests())
        .unwrap_or_else(|error| panic!("{error}"));
}

/// Compile crash on warm sample 1: no measurement on the terminating sample,
/// remaining slots NotRun blocked by Crash, not a copied termination.
#[test]
fn a_compile_crash_mid_warm_validates_as_a_total_row() {
    let mut artifact = valid_artifact();
    let mut samples = vec![measured(vue_pass())];
    samples.push(Sample {
        terminals: crash_terminals(),
        measurement: None,
        absence: Some(AbsenceReason::CompileTerminated),
    });
    while samples.len() < WARM_SAMPLES as usize {
        samples.push(unavailable_after(1, ProbeOutcomeClass::Crash));
    }
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples = samples;
    row.sample_count = WARM_SAMPLES as usize;
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    artifact
        .validate(&manifests())
        .unwrap_or_else(|error| panic!("{error}"));
    let rest = &artifact
        .rows
        .iter()
        .find(|row| row.row_id.ends_with("@warm") && row.case_id == VUE_CASE)
        .unwrap()
        .samples[2];
    assert!(matches!(
        rest.absence,
        Some(AbsenceReason::WorkerUnavailableAfterSample { failed_sample: 1 })
    ));
    assert_eq!(
        rest.terminals[&Dimension::Route],
        not_run(ProbeOutcomeClass::Crash)
    );
}

/// Reference death keeps the compile measurement and uses reference_failure
/// as the remaining slots' blocked_by, not a copied Route pass.
#[test]
fn a_reference_death_keeps_its_compile_measurement() {
    let mut artifact = valid_artifact();
    let mut terminals = vue_pass();
    terminals.insert(
        Dimension::Structural,
        class_term(ProbeOutcomeClass::ReferenceFailure),
    );
    let mut samples = vec![Sample {
        terminals,
        measurement: Some(Measurement {
            elapsed_ns: 2_000,
            memory: Some(MemoryPair {
                peak_bytes: 8,
                live_bytes: 4,
            }),
        }),
        absence: Some(AbsenceReason::ReferenceTerminated),
    }];
    while samples.len() < WARM_SAMPLES as usize {
        samples.push(unavailable_after(0, ProbeOutcomeClass::ReferenceFailure));
    }
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples = samples;
    row.sample_count = WARM_SAMPLES as usize;
    row.observed_outcome = observe::project_observed_outcome(&row.samples).expect("projectable");
    artifact
        .validate(&manifests())
        .unwrap_or_else(|error| panic!("{error}"));
    let rest = &artifact
        .rows
        .iter()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .unwrap()
        .samples[1];
    assert_eq!(
        rest.terminals[&Dimension::Route],
        not_run(ProbeOutcomeClass::ReferenceFailure)
    );
    assert!(rest.measurement.is_none());
}

#[test]
fn a_stored_projection_that_differs_from_the_recompute_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows[0]
        .observed_outcome
        .insert(Dimension::Compile, class_term(ProbeOutcomeClass::Crash));
    reject(&artifact, "not the recomputed projection");
}

/// Mixed NotApplicable / NotRun: NotRun outranks inapplicability.
#[test]
fn not_run_outranks_not_applicable_in_the_projection() {
    let mut samples = warm_samples(vue_pass());
    samples[0].terminals.insert(
        Dimension::Structural,
        na(NotApplicableReason::ComparatorAbsent),
    );
    samples[1]
        .terminals
        .insert(Dimension::Structural, not_run(ProbeOutcomeClass::Crash));
    let projected = observe::project_observed_outcome(&samples).expect("projectable");
    assert_eq!(
        projected[&Dimension::Structural],
        not_run(ProbeOutcomeClass::Crash)
    );
}

/// Mixed NotApplicable / failure: the failure outranks inapplicability.
#[test]
fn a_failure_outranks_not_applicable_in_the_projection() {
    let mut samples = warm_samples(vue_pass());
    samples[0].terminals.insert(
        Dimension::Structural,
        na(NotApplicableReason::ComparatorAbsent),
    );
    samples[1].terminals.insert(
        Dimension::Structural,
        class_term(ProbeOutcomeClass::SemanticMismatch),
    );
    let projected = observe::project_observed_outcome(&samples).expect("projectable");
    assert_eq!(
        projected[&Dimension::Structural],
        class_term(ProbeOutcomeClass::SemanticMismatch)
    );
}

/// Tied-class terminals union secondary and evidence in canonical order.
#[test]
fn tied_class_evidence_is_unioned_and_sorted() {
    let e1 = Evidence {
        source: EvidenceSource::Comparator,
        message: "b".to_string(),
    };
    let e2 = Evidence {
        source: EvidenceSource::Comparator,
        message: "a".to_string(),
    };
    let e3 = Evidence {
        source: EvidenceSource::Parser,
        message: "z".to_string(),
    };
    let mut left = vue_pass();
    left.insert(
        Dimension::Compile,
        Terminal::Class {
            class: ProbeOutcomeClass::ProductMalformed,
            secondary: vec![ProbeOutcomeClass::VerterDiagnostic],
            evidence: vec![e1.clone(), e3.clone()],
        },
    );
    let mut right = vue_pass();
    right.insert(
        Dimension::Compile,
        Terminal::Class {
            class: ProbeOutcomeClass::ProductMalformed,
            secondary: vec![ProbeOutcomeClass::Unsupported],
            evidence: vec![e2.clone(), e1.clone()],
        },
    );
    let samples = vec![measured(left), measured(right)];
    let projected = observe::project_observed_outcome(&samples).expect("projectable");
    match &projected[&Dimension::Compile] {
        Terminal::Class {
            class,
            secondary,
            evidence,
        } => {
            assert_eq!(*class, ProbeOutcomeClass::ProductMalformed);
            assert_eq!(
                secondary,
                &vec![
                    ProbeOutcomeClass::Unsupported,
                    ProbeOutcomeClass::VerterDiagnostic,
                ]
            );
            assert_eq!(evidence, &vec![e2, e1, e3]);
        }
        other => panic!("{other:?}"),
    }
}

/// Concrete terminal wins a class-rank tie with NotRun.
#[test]
fn a_concrete_terminal_wins_a_tie_with_not_run() {
    let mut samples = warm_samples(vue_pass());
    samples[0]
        .terminals
        .insert(Dimension::Compile, class_term(ProbeOutcomeClass::Crash));
    samples[1]
        .terminals
        .insert(Dimension::Compile, not_run(ProbeOutcomeClass::Crash));
    let projected = observe::project_observed_outcome(&samples).expect("projectable");
    assert_eq!(
        projected[&Dimension::Compile],
        class_term(ProbeOutcomeClass::Crash)
    );
}

#[test]
fn samples_that_disagree_on_memory_presence_are_rejected() {
    let mut artifact = valid_artifact();
    let row = artifact
        .rows
        .iter_mut()
        .find(|row| row.row_id == format!("{VUE_CASE}@warm"))
        .expect("warm");
    row.samples[0].measurement.as_mut().unwrap().memory = None;
    reject(&artifact, "disagree on memory presence");
}

#[test]
fn a_sample_missing_a_dimension_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows[0].samples[0]
        .terminals
        .remove(&Dimension::Map);
    reject(&artifact, "cover every dimension");
}

#[test]
fn not_run_blocked_by_pass_is_rejected() {
    let mut artifact = valid_artifact();
    artifact.rows[0].samples[0]
        .terminals
        .insert(Dimension::Compile, not_run(ProbeOutcomeClass::Pass));
    artifact.rows[0].observed_outcome =
        observe::project_observed_outcome(&artifact.rows[0].samples).expect("projectable");
    reject(&artifact, "blocked_by pass");
}

#[test]
fn a_threshold_field_is_structurally_unrepresentable() {
    let mut value: serde_json::Value =
        serde_json::from_str(&valid_artifact().to_json()).expect("json");
    value["threshold"] = serde_json::json!(5);
    assert!(ObservationArtifact::from_json_str(&value.to_string()).is_err());
    value = serde_json::from_str(&valid_artifact().to_json()).expect("json");
    value["verdict"] = serde_json::json!("fail");
    assert!(ObservationArtifact::from_json_str(&value.to_string()).is_err());
    value = serde_json::from_str(&valid_artifact().to_json()).expect("json");
    value["status"] = serde_json::json!("pass");
    assert!(ObservationArtifact::from_json_str(&value.to_string()).is_err());
    value = serde_json::from_str(&valid_artifact().to_json()).expect("json");
    value["baseline"] = serde_json::json!(1);
    assert!(ObservationArtifact::from_json_str(&value.to_string()).is_err());
}

#[test]
fn a_missing_header_field_is_rejected() {
    let mut value: serde_json::Value =
        serde_json::from_str(&valid_artifact().to_json()).expect("json");
    value.as_object_mut().unwrap().remove("rust_version");
    assert!(ObservationArtifact::from_json_str(&value.to_string()).is_err());
}

#[test]
fn selection_throughput_is_derived_and_absent_without_a_cold_measurement() {
    let artifact = valid_artifact();
    let reading = observe::selection_throughput(
        &artifact,
        &[format!("{VUE_CASE}@cold"), format!("{SVELTE_CASE}@cold")],
    )
    .expect("both cold rows measured");
    assert_eq!(reading.cases, 2);
    assert_eq!(reading.elapsed_ns, 2_000);
    assert!(!reading.comparison_eligible);

    let mut missing = artifact.clone();
    missing.rows[0].samples[0].measurement = None;
    missing.rows[0].samples[0].absence = Some(AbsenceReason::CompileTerminated);
    missing.rows[0].observed_outcome =
        observe::project_observed_outcome(&missing.rows[0].samples).expect("projectable");
    assert!(observe::selection_throughput(&missing, &[format!("{VUE_CASE}@cold")]).is_none());
}

/// The listing filter is a prefix on the name; GitHub's `name` query is not used.
#[test]
fn listing_keeps_the_observation_prefix_and_drops_other_suffixes() {
    assert!(observe::listing_name_kept(&format!(
        "{ARTIFACT_NAME_PREFIX}99-1"
    )));
    assert!(!observe::listing_name_kept("validation-probe-summary-99-1"));
    assert!(!observe::listing_name_kept("validation-probe-observations"));
    assert!(!observe::listing_name_kept("other-99-1"));
}

fn zip_named(name: &str, body: &[u8]) -> Vec<u8> {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file(name, options).expect("start");
    writer.write_all(body).expect("write");
    writer.finish().expect("finish").into_inner()
}

fn zip_observations(json: &str) -> Vec<u8> {
    zip_named("observations.json", json.as_bytes())
}

#[test]
fn a_traversal_archive_member_is_rejected() {
    let bytes = zip_named("../observations.json", b"{}");
    let error = observe::extract_observations_json(&bytes).expect_err("traversal");
    assert!(error.to_string().contains("observations.json"), "{error}");
}

#[test]
fn an_archive_with_an_extra_member_is_rejected() {
    let cursor = Cursor::new(Vec::new());
    let mut writer = zip::ZipWriter::new(cursor);
    let options =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
    writer.start_file("observations.json", options).expect("a");
    writer.write_all(b"{}").expect("a");
    writer.start_file("notes.txt", options).expect("b");
    writer.write_all(b"x").expect("b");
    let bytes = writer.finish().expect("finish").into_inner();
    let error = observe::extract_observations_json(&bytes).expect_err("extra");
    assert!(error.to_string().contains("exactly one member"), "{error}");
}

#[test]
fn an_oversized_uncompressed_member_is_rejected() {
    let mut bytes = zip_observations("{}");
    // Local-file uncompressed-size field sits at offset 22 (u32 LE).
    let size = u32::try_from(MAX_UNCOMPRESSED_BYTES + 1)
        .expect("the uncompressed ceiling fits in a ZIP u32")
        .to_le_bytes();
    bytes[22..26].copy_from_slice(&size);
    // Central-directory uncompressed size: find the second PK header.
    if let Some(at) = bytes.windows(4).position(|w| w == b"PK\x01\x02") {
        bytes[at + 24..at + 28].copy_from_slice(&size);
    }
    let error = observe::extract_observations_json(&bytes).expect_err("oversize");
    assert!(error.to_string().contains("256 MiB"), "{error}");
}

struct MockSource {
    started_at: String,
    listed: std::cell::RefCell<Vec<ListedArtifact>>,
    shift: Option<ListedArtifact>,
    scans: Cell<u32>,
    runs: BTreeMap<u64, WorkflowRun>,
    repo: RepoInfo,
    zips: BTreeMap<u64, Vec<u8>>,
    git: BTreeMap<(String, String), String>,
    page_failures: Cell<u8>,
}

impl ArtifactSource for MockSource {
    fn list_page(&self, page: u32) -> Result<observe::ArtifactListPage, FetchError> {
        if self.page_failures.get() > 0 {
            self.page_failures.set(self.page_failures.get() - 1);
            return Err(FetchError::Aborted("transient page failure".into()));
        }
        if page == 1 {
            let scan = self.scans.get() + 1;
            self.scans.set(scan);
            if scan == 2 {
                if let Some(extra) = &self.shift {
                    self.listed.borrow_mut().insert(0, extra.clone());
                }
            }
        }
        let listed = self.listed.borrow();
        let start = ((page - 1) * 100) as usize;
        let artifacts = listed.iter().skip(start).take(100).cloned().collect();
        Ok(observe::ArtifactListPage { artifacts })
    }

    fn workflow_run(&self, id: u64) -> Result<WorkflowRun, FetchError> {
        self.runs
            .get(&id)
            .cloned()
            .ok_or_else(|| FetchError::Aborted(format!("no run {id}")))
    }

    fn repo(&self) -> Result<RepoInfo, FetchError> {
        Ok(self.repo.clone())
    }

    fn download(&self, artifact_id: u64) -> Result<Vec<u8>, FetchError> {
        self.zips
            .get(&artifact_id)
            .cloned()
            .ok_or_else(|| FetchError::Aborted(format!("no zip {artifact_id}")))
    }

    fn git_show(&self, head_sha: &str, path: &str) -> Result<String, FetchError> {
        self.git
            .get(&(head_sha.to_string(), path.to_string()))
            .cloned()
            .ok_or_else(|| FetchError::Aborted(format!("no git {head_sha}:{path}")))
    }

    fn started_at(&self) -> String {
        self.started_at.clone()
    }

    fn retry_delay(&self, _attempt: u8) -> Duration {
        Duration::ZERO
    }
}

fn trusted_run() -> WorkflowRun {
    WorkflowRun {
        id: 1,
        path: ".github/workflows/validation-probe.yml".to_string(),
        event: "push".to_string(),
        conclusion: "success".to_string(),
        head_branch: "main".to_string(),
        head_sha: COMMIT.to_string(),
        run_attempt: 1,
    }
}

fn listed(id: u64, name: &str, created: &str, run: u64, size: u64) -> ListedArtifact {
    ListedArtifact {
        id,
        name: name.to_string(),
        size_in_bytes: size,
        created_at: created.to_string(),
        expires_at: "2026-10-13T12:00:00Z".to_string(),
        workflow_run_id: run,
    }
}

fn git_map() -> BTreeMap<(String, String), String> {
    let mut git = BTreeMap::new();
    git.insert(
        (
            COMMIT.to_string(),
            "crates/verter_validation_probe/manifest/vue.toml".to_string(),
        ),
        fixture_toml("vue", "fixtures/App.vue", "structural", VUE_REV),
    );
    git.insert(
        (
            COMMIT.to_string(),
            "crates/verter_validation_probe/manifest/svelte.toml".to_string(),
        ),
        fixture_toml("svelte", "fixtures/App.svelte", "none", SVELTE_REV),
    );
    git
}

fn fixture_toml(framework: &str, case: &str, comparison: &str, revision: &str) -> String {
    let probe_id = format!("{framework}/{case}");
    let product = format!("{framework}.runtime-client-product");
    let (structural, comparator) = if comparison == "structural" {
        (
            format!(
                r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-identity"
"#
            ),
            r#"
[comparator]
crate = "verter_vue_conformance"
path = "src/compare.rs"
function = "compare_modules"
atom = "product-identity"
"#,
        )
    } else {
        (
            format!(
                r#"
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Structural"
expected_state = "skip"
authority = "{product}"
atom = "product-identity"
reason = "comparator_absent"
"#
            ),
            "",
        )
    };
    format!(
        r#"
framework = "{framework}"
comparison = "{comparison}"
external_revision = "{revision}"
smoke = ["{probe_id}"]
{comparator}
[applicability]
runtime = "inapplicable"
map = "inapplicable"

[[strata]]
id = "app"
pattern = "App*"
min_cases = 1

[[inventory]]
case_id = "{probe_id}"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Route"
expected_state = "gate"
expected_class = "pass"
authority = "compiler.public-request-route"
atom = "route-callable"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Compile"
expected_state = "canary"
expected_class = "pass"
authority = "{product}"
atom = "product-shape"
{structural}
[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Runtime"
expected_state = "skip"
authority = "{product}"
atom = "runtime-behavior"
reason = "runtime_executor_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Map"
expected_state = "skip"
authority = "{product}"
atom = "source-map"
reason = "map_validator_absent"

[[entries]]
probe_id = "{probe_id}"
framework = "{framework}"
case = "{case}"
dimension = "Performance"
expected_state = "canary"
expected_class = "pass"
authority = "compiler.equivalent-work-ledger"
atom = "equivalent-work-ledger"
"#
    )
}

fn fetch_dir() -> tempfile::TempDir {
    tempfile::tempdir().expect("tempdir")
}

fn base_mock() -> MockSource {
    let artifact = valid_artifact();
    let zip = zip_observations(&artifact.to_json());
    let mut runs = BTreeMap::new();
    runs.insert(1, trusted_run());
    let mut zips = BTreeMap::new();
    zips.insert(10, zip);
    MockSource {
        started_at: STARTED.to_string(),
        listed: std::cell::RefCell::new(vec![listed(
            10,
            &format!("{ARTIFACT_NAME_PREFIX}1-1"),
            "2026-09-13T11:00:00Z",
            1,
            100,
        )]),
        shift: None,
        scans: Cell::new(0),
        runs,
        repo: RepoInfo {
            default_branch: "main".to_string(),
        },
        zips,
        git: git_map(),
        page_failures: Cell::new(0),
    }
}

#[test]
fn fetch_keeps_a_trusted_artifact_and_excludes_a_concurrent_upload() {
    let source = base_mock();
    source.listed.borrow_mut().push(listed(
        11,
        &format!("{ARTIFACT_NAME_PREFIX}1-1-late"),
        "2026-09-13T12:00:01Z",
        1,
        100,
    ));
    let dir = fetch_dir();
    let inventory =
        ObservationInventory::fetch_with(&source, dir.path()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(inventory.retrieval_window.artifact_count, 1);
    assert_eq!(inventory.artifacts.len(), 1);
}

#[test]
fn fetch_rejects_a_verter_commit_that_differs_from_head_sha() {
    let mut source = base_mock();
    source.runs.get_mut(&1).unwrap().head_sha = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    let dir = fetch_dir();
    let error = ObservationInventory::fetch_with(&source, dir.path()).expect_err("mismatch");
    assert!(
        error.to_string().contains("differs from run head_sha"),
        "{error}"
    );
}

#[test]
fn fetch_excludes_a_different_workflow() {
    let mut source = base_mock();
    source.runs.get_mut(&1).unwrap().path = ".github/workflows/ci.yml".into();
    let dir = fetch_dir();
    let inventory =
        ObservationInventory::fetch_with(&source, dir.path()).unwrap_or_else(|e| panic!("{e}"));
    assert!(inventory.artifacts.is_empty());
}

#[test]
fn fetch_rejects_a_stale_file_reuploaded_under_a_newer_identity() {
    let mut source = base_mock();
    let artifact = valid_artifact();
    let zip = zip_observations(&artifact.to_json());
    source.listed.borrow_mut().push(listed(
        12,
        &format!("{ARTIFACT_NAME_PREFIX}2-1"),
        "2026-09-13T11:30:00Z",
        2,
        100,
    ));
    let mut run2 = trusted_run();
    run2.id = 2;
    source.runs.insert(2, run2);
    source.zips.insert(12, zip);
    let dir = fetch_dir();
    let error = ObservationInventory::fetch_with(&source, dir.path()).expect_err("duplicate");
    assert!(
        error.to_string().contains("duplicate observation identity")
            || error.to_string().contains("workflow_run_id"),
        "{error}"
    );
}

#[test]
fn fetch_aborts_an_oversized_compressed_listing() {
    let source = base_mock();
    source.listed.borrow_mut()[0].size_in_bytes = MAX_COMPRESSED_BYTES + 1;
    let dir = fetch_dir();
    let error = ObservationInventory::fetch_with(&source, dir.path()).expect_err("ceiling");
    assert!(error.to_string().contains("64 MiB"), "{error}");
}

#[test]
fn a_post_cutoff_insert_that_shifts_a_page_does_not_change_membership() {
    let mut source = base_mock();
    let mut names = Vec::new();
    for i in 0..100u64 {
        names.push(listed(
            100 + i,
            &format!("{ARTIFACT_NAME_PREFIX}untrusted-{i}-1"),
            "2026-09-13T11:00:00Z",
            9,
            10,
        ));
    }
    names.push(source.listed.borrow()[0].clone());
    source.listed = std::cell::RefCell::new(names);
    source.shift = Some(listed(
        999,
        &format!("{ARTIFACT_NAME_PREFIX}late-1"),
        "2026-09-13T12:00:01Z",
        9,
        10,
    ));
    let mut untrusted = trusted_run();
    untrusted.id = 9;
    untrusted.path = ".github/workflows/ci.yml".into();
    source.runs.insert(9, untrusted);
    let dir = fetch_dir();
    let inventory =
        ObservationInventory::fetch_with(&source, dir.path()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(inventory.artifacts.len(), 1);
    assert!(source.scans.get() >= 2);
}

/// Historical grid, digest, and pin stay valid after a re-pin of the live manifests.
#[test]
fn a_historical_artifact_validates_against_its_snapshots_and_fails_the_current_grid() {
    let historical = valid_artifact();
    historical
        .validate(&manifests())
        .unwrap_or_else(|error| panic!("historical: {error}"));
    let current = vec![
        manifest(
            "vue",
            "fixtures/AppOther.vue",
            "structural",
            "ffffffffffffffffffffffffffffffffffffffff",
        ),
        manifest(
            "svelte",
            "fixtures/AppOther.svelte",
            "none",
            "ffffffffffffffffffffffffffffffffffffffff",
        ),
    ];
    let error = historical
        .validate(&current)
        .expect_err("current grid must refuse the historical cases");
    assert!(
        error
            .to_string()
            .contains("neither the smoke slice nor the inventory")
            || error.to_string().contains("corpus_revisions"),
        "{error}"
    );
}

#[test]
fn the_compile_frame_carries_memory_only_when_present() {
    let parsed = runner::parse_line(
        r#"{"probe_id":"a","frame":"compile","elapsed_ns":1,"entries":[],"memory":{"peak_bytes":8,"live_bytes":4}}"#,
    )
    .expect("parse");
    match parsed {
        DriverLine::Compile { memory, .. } => {
            assert_eq!(
                memory,
                Some(MemoryBytes {
                    peak_bytes: 8,
                    live_bytes: 4
                })
            );
        }
        other => panic!("{other:?}"),
    }
    let parsed =
        runner::parse_line(r#"{"probe_id":"a","frame":"compile","elapsed_ns":1,"entries":[]}"#)
            .expect("parse");
    match parsed {
        DriverLine::Compile { memory, .. } => assert_eq!(memory, None),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_driver_arms_memory_audit_in_enable_reset_call_snapshot_order_and_never_samples_sites() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("driver")
        .join("probe-driver.mjs");
    let text = std::fs::read_to_string(&path).expect("driver source");
    let enable = text.find("memoryAuditEnable()").expect("enable");
    let reset = text.find("memoryAuditResetHighWater()").expect("reset");
    let call = text.find("compileRequests(inputs)").expect("call");
    let snapshot = text.find("memoryAuditSnapshot()").expect("snapshot");
    assert!(
        enable < reset && reset < call && call < snapshot,
        "{enable} {reset} {call} {snapshot}"
    );
    assert!(
        !text.contains("memoryAuditSites"),
        "the observation command must not call memoryAuditSites"
    );
}

#[test]
fn the_observation_job_uploads_the_prefixed_artifact_and_never_disposes_on_a_number() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|crates| crates.parent())
        .expect("repo")
        .join(".github/workflows/validation-probe.yml");
    let text = std::fs::read_to_string(&path).expect("workflow");
    let observe = text
        .find("  observe:")
        .expect("the workflow declares an observe job");
    let job = &text[observe..];
    assert!(job
        .contains("validation-probe-observations-${{ github.run_id }}-${{ github.run_attempt }}"));
    assert!(
        !job.contains("--dispose"),
        "the observation job must not inherit the probe disposition"
    );
    assert!(
        !job.contains("threshold") && !job.contains("rebaseline"),
        "the observation job must not threshold or rebaseline"
    );
}

#[test]
fn the_observation_source_carries_no_threshold_or_rebaseline_policy() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("observe.rs");
    let text = std::fs::read_to_string(&path).expect("observe.rs");
    assert!(
        !text.contains("rebaseline") && !text.contains("must be within"),
        "observe.rs must not carry rebaseline or percent-gate policy"
    );
    assert!(
        text.contains("structurally unrepresentable"),
        "the module must document that a performance verdict is unrepresentable"
    );
}

#[cfg(feature = "external-corpus")]
#[test]
fn observation_lane_emits_one_artifact() {
    use verter_validation_probe::runner::PhaseDeadlines;
    use verter_validation_probe::summary::Lane;

    let lane = match std::env::var("VALIDATION_PROBE_LANE").as_deref() {
        Ok("main") => Lane::Main,
        _ => Lane::Smoke,
    };
    let artifact = observe::capture(lane, PhaseDeadlines::default())
        .unwrap_or_else(|error| panic!("observation capture failed: {error}"));
    let path = observe::write_observations(&artifact)
        .unwrap_or_else(|error| panic!("writing observations: {error}"));
    assert!(path.is_file(), "{}", path.display());
    assert!(!artifact.artifact_id.is_empty());
    assert!(!artifact.rows.is_empty());
    for row in &artifact.rows {
        assert!(
            !row.comparison_eligible,
            "{}: the adapter never sets comparison_eligible",
            row.row_id
        );
        assert_eq!(
            row.observed_outcome,
            observe::project_observed_outcome(&row.samples).expect("projectable"),
        );
        assert_eq!(row.samples.len(), row.mode.sample_count() as usize);
    }
}
