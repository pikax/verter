//! Non-gating benchmark observation artifacts.
//!
//! The schema records correctness-labeled timings and the CPER0M peak/live
//! pair. It has no status, verdict, threshold, or baseline field: a
//! performance gate is structurally unrepresentable. Selection throughput is
//! a derived reading over named cold rows, never a stored number.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::authority::Authority;
use crate::disk;
use crate::manifest::{Comparison, Framework, ProbeStateManifest};
use crate::outcome::{Dimension, ProbeOutcomeClass, Terminal};
use crate::request;
use crate::runner::{self, MemoryBytes};
#[cfg(feature = "external-corpus")]
use crate::runner::{
    DriverCommand, ExecutionEvent, LaneStop, Phase, PhaseDeadlines, PlannedCase, ProbeRun,
    RequestedEntry, RunError,
};
#[cfg(feature = "external-corpus")]
use std::process::Child;
#[cfg(feature = "external-corpus")]
use std::sync::mpsc::Receiver;

/// Where the lane writes its one observation artifact.
pub const OBSERVATIONS_RELATIVE_PATH: &str = "target/validation-probe/observations.json";

/// GitHub Actions artifact name prefix. The listing is unfiltered; the
/// fetcher keeps names that start with this.
pub const ARTIFACT_NAME_PREFIX: &str = "validation-probe-observations-";

/// Cold sample cardinality. Immutable.
pub const COLD_SAMPLES: u8 = 1;
/// Warm sample cardinality. Immutable.
pub const WARM_SAMPLES: u8 = 5;

/// At most this many artifacts per fetch.
pub const MAX_FETCH_ARTIFACTS: usize = 500;
/// At most this many compressed bytes per archive.
pub const MAX_COMPRESSED_BYTES: u64 = 64 * 1024 * 1024;
/// At most this many uncompressed bytes per archive member.
pub const MAX_UNCOMPRESSED_BYTES: u64 = 256 * 1024 * 1024;
const PAGE_SIZE: u32 = 100;
const LISTING_RETRIES: u8 = 3;
const MAX_SCAN_MISMATCHES: u8 = 3;
const MANIFEST_VUE: &str = "crates/verter_validation_probe/manifest/vue.toml";
const MANIFEST_SVELTE: &str = "crates/verter_validation_probe/manifest/svelte.toml";
const WORKFLOW_PATH: &str = ".github/workflows/validation-probe.yml";

/// Cold or warm execution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// One fresh worker, one request.
    Cold,
    /// Five in-process repeats after one discarded request.
    Warm,
}

impl Mode {
    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Cold => "cold",
            Mode::Warm => "warm",
        }
    }

    /// The sample cardinality this mode records.
    pub const fn sample_count(self) -> u8 {
        match self {
            Mode::Cold => COLD_SAMPLES,
            Mode::Warm => WARM_SAMPLES,
        }
    }
}

impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// `{ authority, atom }` citation a comparison-eligible row must carry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Citation {
    /// The owning authority.
    pub authority: Authority,
    /// The durable atom.
    pub atom: String,
}

/// Cold/warm sample counts the header advertises.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SamplePlan {
    /// Cold samples per case. Must be [`COLD_SAMPLES`].
    pub cold: u8,
    /// Warm samples per case. Must be [`WARM_SAMPLES`].
    pub warm: u8,
}

/// Canonical request-template digests, one per framework.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateDigests {
    /// Digest of [`request::REQUEST_VUE`].
    pub vue: String,
    /// Digest of [`request::REQUEST_SVELTE`].
    pub svelte: String,
}

/// Why a sample did not complete its protocol.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AbsenceReason {
    /// The worker died before the compile phase.
    LoadFailed,
    /// The worker died inside the compile call; no compile frame arrived.
    CompileTerminated,
    /// The compile frame arrived and the reference work then died.
    ReferenceTerminated,
    /// The driver reported a line-level error before a compile frame.
    DriverError,
    /// This slot never ran because an earlier sample killed the worker.
    WorkerUnavailableAfterSample {
        /// Index of the sample that terminated the worker.
        failed_sample: u8,
    },
}

/// Peak/live-bytes pair copied from a compile frame. No row-level aggregate.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPair {
    /// High-water live bytes.
    pub peak_bytes: u64,
    /// Live bytes at the snapshot.
    pub live_bytes: u64,
}

impl From<MemoryBytes> for MemoryPair {
    fn from(value: MemoryBytes) -> Self {
        MemoryPair {
            peak_bytes: value.peak_bytes,
            live_bytes: value.live_bytes,
        }
    }
}

/// One invocation's timing, present exactly when a compile frame was ingested.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Measurement {
    /// Nanoseconds bracketing the compile call.
    pub elapsed_ns: u64,
    /// Per-invocation memory pair, when the compile frame carried one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory: Option<MemoryPair>,
}

/// One invocation of one case in one mode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sample {
    /// Exactly one terminal per [`Dimension`].
    pub terminals: BTreeMap<Dimension, Terminal>,
    /// Present exactly when the compile frame was ingested.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measurement: Option<Measurement>,
    /// Present exactly when some phase terminated abnormally.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub absence: Option<AbsenceReason>,
}

/// One `(case_id, mode)` observation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationRow {
    /// `<case_id>@<mode>`.
    pub row_id: String,
    /// Manifest case id, verbatim.
    pub case_id: String,
    /// Cold or warm.
    pub mode: Mode,
    /// This row's framework corpus revision.
    pub corpus_revision: String,
    /// Digest of the substituted request this row issued.
    pub request_digest: String,
    /// Per-invocation samples. Never a single aggregate.
    pub samples: Vec<Sample>,
    /// Must equal `samples.len()` and the mode's cardinality.
    pub sample_count: usize,
    /// Highest-precedence terminal per dimension across `samples`.
    pub observed_outcome: BTreeMap<Dimension, Terminal>,
    /// Default false. True only with both bases and passing structural evidence.
    pub comparison_eligible: bool,
    /// Framework product authority, covering Structural.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic_basis: Option<Citation>,
    /// `compiler.equivalent-work-ledger`, covering Performance.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub equivalent_work_basis: Option<Citation>,
}

/// Typed, fully required artifact header. Flattened onto the document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactHeader {
    /// `<verter_commit>/<corpus_digest>/<workflow_run_id>/<run_attempt>`.
    pub artifact_id: String,
    /// The producing Verter commit.
    pub verter_commit: String,
    /// Full `{ vue, svelte }` revision map.
    pub corpus_revisions: BTreeMap<String, String>,
    /// Digest of the canonical corpus-revision encoding.
    pub corpus_digest: String,
    /// GitHub Actions run id, or `"0"` for a local capture.
    pub workflow_run_id: String,
    /// GitHub Actions run attempt.
    pub run_attempt: u32,
    /// `rustc --version` text.
    pub rust_version: String,
    /// `node --version` text.
    pub node_version: String,
    /// `@verter/native` package version.
    pub addon_version: String,
    /// Runner OS.
    pub os: String,
    /// Runner architecture.
    pub arch: String,
    /// `ci` or `local`.
    pub execution_mode: String,
    /// `{ cold: 1, warm: 5 }`.
    pub sample_plan: SamplePlan,
    /// Canonical Vue request template, verbatim.
    pub request_vue: String,
    /// Canonical Svelte request template, verbatim.
    pub request_svelte: String,
    /// Template digests.
    pub template_digests: TemplateDigests,
}

/// One observation document: header fields plus rows.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObservationArtifact {
    /// `<verter_commit>/<corpus_digest>/<workflow_run_id>/<run_attempt>`.
    pub artifact_id: String,
    /// The producing Verter commit.
    pub verter_commit: String,
    /// Full `{ vue, svelte }` revision map.
    pub corpus_revisions: BTreeMap<String, String>,
    /// Digest of the canonical corpus-revision encoding.
    pub corpus_digest: String,
    /// GitHub Actions run id, or `"0"` for a local capture.
    pub workflow_run_id: String,
    /// GitHub Actions run attempt.
    pub run_attempt: u32,
    /// `rustc --version` text.
    pub rust_version: String,
    /// `node --version` text.
    pub node_version: String,
    /// `@verter/native` package version.
    pub addon_version: String,
    /// Runner OS.
    pub os: String,
    /// Runner architecture.
    pub arch: String,
    /// `ci` or `local`.
    pub execution_mode: String,
    /// `{ cold: 1, warm: 5 }`.
    pub sample_plan: SamplePlan,
    /// Canonical Vue request template, verbatim.
    pub request_vue: String,
    /// Canonical Svelte request template, verbatim.
    pub request_svelte: String,
    /// Template digests.
    pub template_digests: TemplateDigests,
    /// One cold and one warm row per selected case.
    pub rows: Vec<ObservationRow>,
}

/// Why an artifact is invalid.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ObserveError {
    /// A human-readable contract violation.
    Invalid(String),
}

impl fmt::Display for ObserveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ObserveError::Invalid(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ObserveError {}

fn invalid(message: impl Into<String>) -> ObserveError {
    ObserveError::Invalid(message.into())
}

/// Canonical corpus-digest encoding: framework keys sorted ascending
/// bytewise, one `key=value` line each, LF-terminated.
pub fn encode_corpus_revisions(revisions: &BTreeMap<String, String>) -> String {
    let mut keys: Vec<&str> = revisions.keys().map(String::as_str).collect();
    keys.sort_unstable();
    let mut out = String::new();
    for key in keys {
        out.push_str(key);
        out.push('=');
        out.push_str(&revisions[key]);
        out.push('\n');
    }
    out
}

/// Lowercase-hex SHA-256 of [`encode_corpus_revisions`].
pub fn corpus_digest(revisions: &BTreeMap<String, String>) -> String {
    request::sha256_hex(encode_corpus_revisions(revisions).as_bytes())
}

/// `<verter_commit>/<corpus_digest>/<workflow_run_id>/<run_attempt>`.
pub fn compose_artifact_id(
    verter_commit: &str,
    digest: &str,
    workflow_run_id: &str,
    run_attempt: u32,
) -> String {
    format!("{verter_commit}/{digest}/{workflow_run_id}/{run_attempt}")
}

/// Filesystem-safe single-component encoding of the identity.
pub fn artifact_key(
    verter_commit: &str,
    digest: &str,
    workflow_run_id: &str,
    run_attempt: u32,
) -> Result<String, ObserveError> {
    let prefix: String = digest.chars().take(16).collect();
    let key = format!("{verter_commit}-{prefix}-{workflow_run_id}-{run_attempt}");
    if key.contains('/') || key.contains('\\') || key.contains("..") {
        return Err(invalid(format!(
            "artifact key `{key}` is not a single path component"
        )));
    }
    Ok(key)
}

/// Highest-precedence terminal across a row's samples, per dimension.
pub fn project_observed_outcome(
    samples: &[Sample],
) -> Result<BTreeMap<Dimension, Terminal>, ObserveError> {
    if samples.is_empty() {
        return Err(invalid("a row has no samples to project"));
    }
    let mut out = BTreeMap::new();
    for dimension in Dimension::ALL {
        let terminals: Vec<&Terminal> = samples
            .iter()
            .map(|sample| {
                sample
                    .terminals
                    .get(&dimension)
                    .ok_or_else(|| invalid(format!("a sample is missing dimension {dimension}")))
            })
            .collect::<Result<_, _>>()?;
        out.insert(dimension, project_dimension(&terminals)?);
    }
    Ok(out)
}

fn evidence_source_name(source: crate::outcome::EvidenceSource) -> &'static str {
    match source {
        crate::outcome::EvidenceSource::AddonDiagnostic => "addon_diagnostic",
        crate::outcome::EvidenceSource::Parser => "parser",
        crate::outcome::EvidenceSource::SemanticBuilder => "semantic_builder",
        crate::outcome::EvidenceSource::Comparator => "comparator",
        crate::outcome::EvidenceSource::Driver => "driver",
        crate::outcome::EvidenceSource::Reference => "reference",
    }
}

fn rank(terminal: &Terminal) -> (u8, u8) {
    match terminal {
        Terminal::Class {
            class: ProbeOutcomeClass::Pass,
            ..
        } => (14, 0),
        Terminal::Class { class, .. } => (class.rank(), 0),
        Terminal::NotRun { blocked_by } => (blocked_by.rank(), 1),
        Terminal::NotApplicable { .. } => (13, 0),
    }
}

fn project_dimension(terminals: &[&Terminal]) -> Result<Terminal, ObserveError> {
    let best = terminals
        .iter()
        .map(|terminal| rank(terminal))
        .min()
        .ok_or_else(|| invalid("no terminal to project"))?;
    let winners: Vec<&Terminal> = terminals
        .iter()
        .copied()
        .filter(|terminal| rank(terminal) == best)
        .collect();
    let concretes: Vec<&Terminal> = winners
        .iter()
        .copied()
        .filter(|terminal| matches!(terminal, Terminal::Class { .. }))
        .collect();
    if concretes.is_empty() {
        return Ok(winners[0].clone());
    }
    let Terminal::Class { class, .. } = concretes[0] else {
        return Err(invalid("projected a non-class concrete terminal"));
    };
    let class = *class;
    let mut secondary: Vec<ProbeOutcomeClass> = Vec::new();
    let mut evidence = Vec::new();
    for terminal in &concretes {
        let Terminal::Class {
            secondary: more,
            evidence: more_evidence,
            ..
        } = terminal
        else {
            continue;
        };
        secondary.extend(more.iter().copied());
        evidence.extend(more_evidence.iter().cloned());
    }
    secondary.sort_by_key(|class| class.rank());
    secondary.dedup();
    evidence.sort_by(|left, right| {
        evidence_source_name(left.source)
            .cmp(evidence_source_name(right.source))
            .then(left.message.cmp(&right.message))
    });
    evidence.dedup();
    Ok(Terminal::Class {
        class,
        secondary,
        evidence,
    })
}

fn pass_class(terminal: &Terminal) -> bool {
    matches!(
        terminal,
        Terminal::Class {
            class: ProbeOutcomeClass::Pass,
            ..
        }
    )
}

impl ObservationArtifact {
    /// Bind this artifact to the selected manifest grid.
    pub fn validate(&self, manifests: &[ProbeStateManifest]) -> Result<(), ObserveError> {
        self.validate_header(manifests)?;
        self.validate_grid(manifests)?;
        for row in &self.rows {
            validate_row(self, manifests, row)?;
        }
        Ok(())
    }

    fn validate_header(&self, manifests: &[ProbeStateManifest]) -> Result<(), ObserveError> {
        if self.verter_commit.is_empty()
            || self.workflow_run_id.is_empty()
            || self.rust_version.is_empty()
            || self.node_version.is_empty()
            || self.addon_version.is_empty()
            || self.os.is_empty()
            || self.arch.is_empty()
            || self.execution_mode.is_empty()
        {
            return Err(invalid("a required header field is empty"));
        }
        if self.run_attempt == 0 {
            return Err(invalid("run_attempt must be at least 1"));
        }
        if self.sample_plan.cold != COLD_SAMPLES || self.sample_plan.warm != WARM_SAMPLES {
            return Err(invalid(format!(
                "sample_plan must be {{ cold: {COLD_SAMPLES}, warm: {WARM_SAMPLES} }}"
            )));
        }
        for framework in Framework::ALL {
            let key = framework.as_str();
            if !self.corpus_revisions.contains_key(key) {
                return Err(invalid(format!("header is missing corpus_revisions.{key}")));
            }
        }
        let recomputed = corpus_digest(&self.corpus_revisions);
        if recomputed != self.corpus_digest {
            return Err(invalid(
                "corpus_digest does not match the canonical encoding of corpus_revisions",
            ));
        }
        let expected_id = compose_artifact_id(
            &self.verter_commit,
            &self.corpus_digest,
            &self.workflow_run_id,
            self.run_attempt,
        );
        if expected_id != self.artifact_id {
            return Err(invalid(
                "artifact_id does not match verter_commit/corpus_digest/workflow_run_id/run_attempt",
            ));
        }
        if self.request_vue != request::REQUEST_VUE {
            return Err(invalid("request_vue is not the canonical Vue template"));
        }
        if self.request_svelte != request::REQUEST_SVELTE {
            return Err(invalid(
                "request_svelte is not the canonical Svelte template",
            ));
        }
        if self.template_digests.vue != request::template_digest(Framework::Vue) {
            return Err(invalid("template_digests.vue does not match request_vue"));
        }
        if self.template_digests.svelte != request::template_digest(Framework::Svelte) {
            return Err(invalid(
                "template_digests.svelte does not match request_svelte",
            ));
        }
        let mut seen = BTreeSet::new();
        for manifest in manifests {
            if !seen.insert(manifest.framework) {
                return Err(invalid(format!(
                    "two manifests for framework {}",
                    manifest.framework
                )));
            }
            let key = manifest.framework.as_str();
            let Some(header_rev) = self.corpus_revisions.get(key) else {
                return Err(invalid(format!("header is missing corpus_revisions.{key}")));
            };
            if header_rev != manifest.external_revision.as_str() {
                return Err(invalid(format!(
                    "header corpus_revisions.{key} differs from the manifest pin"
                )));
            }
        }
        if seen.len() != Framework::ALL.len() {
            return Err(invalid(
                "validation requires a manifest for every first-class framework",
            ));
        }
        Ok(())
    }

    fn validate_grid(&self, manifests: &[ProbeStateManifest]) -> Result<(), ObserveError> {
        let mut ids = BTreeSet::new();
        let mut by_case: BTreeMap<(String, Mode), usize> = BTreeMap::new();
        for (index, row) in self.rows.iter().enumerate() {
            if !ids.insert(&row.row_id) {
                return Err(invalid(format!("duplicated row_id `{}`", row.row_id)));
            }
            let expected = format!("{}@{}", row.case_id, row.mode.as_str());
            if row.row_id != expected {
                return Err(invalid(format!(
                    "row_id `{}` is not `<case_id>@<mode>`",
                    row.row_id
                )));
            }
            if by_case
                .insert((row.case_id.clone(), row.mode), index)
                .is_some()
            {
                return Err(invalid(format!(
                    "duplicated row for {}@{}",
                    row.case_id, row.mode
                )));
            }
        }

        let mut kind: Option<&str> = None;
        for framework in Framework::ALL {
            let manifest = manifests
                .iter()
                .find(|manifest| manifest.framework == framework)
                .ok_or_else(|| invalid(format!("no {framework} manifest")))?;
            let mut cases: BTreeSet<String> = BTreeSet::new();
            for row in &self.rows {
                if runner::framework_of(&row.case_id) == Some(framework) {
                    cases.insert(row.case_id.clone());
                }
            }
            let smoke: BTreeSet<String> = manifest.smoke.iter().cloned().collect();
            let inventory: BTreeSet<String> = manifest
                .inventory
                .iter()
                .map(|case| case.case_id.clone())
                .collect();
            let this_kind = if cases == smoke {
                "smoke"
            } else if cases == inventory {
                "inventory"
            } else {
                return Err(invalid(format!(
                    "{framework}: observation cases are neither the smoke slice nor the inventory"
                )));
            };
            match kind {
                None => kind = Some(this_kind),
                Some(previous) if previous != this_kind => {
                    return Err(invalid(
                        "frameworks selected different grids (smoke vs inventory)",
                    ));
                }
                Some(_) => {}
            }
            for case_id in &cases {
                for mode in [Mode::Cold, Mode::Warm] {
                    if !by_case.contains_key(&(case_id.clone(), mode)) {
                        return Err(invalid(format!("missing row `{case_id}@{mode}`")));
                    }
                }
            }
        }
        Ok(())
    }

    /// Compact JSON. Unknown fields cannot appear: the type denies them.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("an observation artifact is JSON")
    }

    /// Parse without validating against manifests.
    pub fn from_json_str(text: &str) -> Result<Self, ObserveError> {
        serde_json::from_str(text).map_err(|error| invalid(error.to_string()))
    }

    /// The typed header block.
    pub fn header(&self) -> ArtifactHeader {
        ArtifactHeader {
            artifact_id: self.artifact_id.clone(),
            verter_commit: self.verter_commit.clone(),
            corpus_revisions: self.corpus_revisions.clone(),
            corpus_digest: self.corpus_digest.clone(),
            workflow_run_id: self.workflow_run_id.clone(),
            run_attempt: self.run_attempt,
            rust_version: self.rust_version.clone(),
            node_version: self.node_version.clone(),
            addon_version: self.addon_version.clone(),
            os: self.os.clone(),
            arch: self.arch.clone(),
            execution_mode: self.execution_mode.clone(),
            sample_plan: self.sample_plan,
            request_vue: self.request_vue.clone(),
            request_svelte: self.request_svelte.clone(),
            template_digests: self.template_digests.clone(),
        }
    }
}

fn validate_row(
    artifact: &ObservationArtifact,
    manifests: &[ProbeStateManifest],
    row: &ObservationRow,
) -> Result<(), ObserveError> {
    let framework = runner::framework_of(&row.case_id)
        .ok_or_else(|| invalid(format!("case_id `{}` has no framework", row.case_id)))?;
    let manifest = manifests
        .iter()
        .find(|manifest| manifest.framework == framework)
        .ok_or_else(|| invalid(format!("no manifest for {}", framework)))?;
    let expected_rev = artifact
        .corpus_revisions
        .get(framework.as_str())
        .ok_or_else(|| invalid(format!("header has no revision for {framework}")))?;
    if row.corpus_revision != *expected_rev {
        return Err(invalid(format!(
            "{}: row corpus_revision differs from the header",
            row.row_id
        )));
    }
    let relative = row
        .case_id
        .strip_prefix(&format!("{}/", framework.as_str()))
        .ok_or_else(|| {
            invalid(format!(
                "{}: case_id is not prefixed by its framework",
                row.row_id
            ))
        })?;
    let digest = request::request_digest(framework, relative);
    if row.request_digest != digest {
        return Err(invalid(format!(
            "{}: request_digest does not match the substituted template",
            row.row_id
        )));
    }
    let expected_count = row.mode.sample_count() as usize;
    if row.sample_count != row.samples.len() || row.samples.len() != expected_count {
        return Err(invalid(format!(
            "{}: sample_count {} / samples {} do not match the {} plan ({expected_count})",
            row.row_id,
            row.sample_count,
            row.samples.len(),
            row.mode
        )));
    }
    let mut memory_presence: Option<bool> = None;
    for (index, sample) in row.samples.iter().enumerate() {
        validate_sample(row, index, sample)?;
        if let Some(measurement) = &sample.measurement {
            let present = measurement.memory.is_some();
            match memory_presence {
                None => memory_presence = Some(present),
                Some(previous) if previous != present => {
                    return Err(invalid(format!(
                        "{}: samples disagree on memory presence",
                        row.row_id
                    )));
                }
                Some(_) => {}
            }
        }
    }
    let projected = project_observed_outcome(&row.samples)?;
    if projected != row.observed_outcome {
        return Err(invalid(format!(
            "{}: observed_outcome is not the recomputed projection",
            row.row_id
        )));
    }
    validate_eligibility(manifest, row)?;
    Ok(())
}

fn validate_sample(
    row: &ObservationRow,
    index: usize,
    sample: &Sample,
) -> Result<(), ObserveError> {
    let at = format!("{} sample {index}", row.row_id);
    if sample.terminals.len() != Dimension::ALL.len() {
        return Err(invalid(format!(
            "{at}: terminals must cover every dimension"
        )));
    }
    for dimension in Dimension::ALL {
        if !sample.terminals.contains_key(&dimension) {
            return Err(invalid(format!("{at}: missing dimension {dimension}")));
        }
    }
    for terminal in sample.terminals.values() {
        if let Terminal::NotRun {
            blocked_by: ProbeOutcomeClass::Pass,
        } = terminal
        {
            return Err(invalid(format!(
                "{at}: NotRun blocked_by pass is forbidden"
            )));
        }
    }
    match (&sample.measurement, &sample.absence) {
        (None, None) => {
            return Err(invalid(format!(
                "{at}: a sample must carry measurement or absence"
            )));
        }
        (
            Some(_),
            Some(
                AbsenceReason::LoadFailed
                | AbsenceReason::CompileTerminated
                | AbsenceReason::DriverError,
            ),
        ) => {
            return Err(invalid(format!(
                "{at}: load_failed, compile_terminated, and driver_error carry no measurement"
            )));
        }
        (None, Some(AbsenceReason::ReferenceTerminated)) => {
            return Err(invalid(format!(
                "{at}: reference_terminated keeps its compile measurement"
            )));
        }
        (Some(_), None)
        | (
            None,
            Some(
                AbsenceReason::LoadFailed
                | AbsenceReason::CompileTerminated
                | AbsenceReason::DriverError
                | AbsenceReason::WorkerUnavailableAfterSample { .. },
            ),
        )
        | (Some(_), Some(AbsenceReason::ReferenceTerminated)) => {}
        (Some(_), Some(AbsenceReason::WorkerUnavailableAfterSample { .. })) => {
            return Err(invalid(format!(
                "{at}: a slot that never ran carries no measurement"
            )));
        }
    }
    if let Some(AbsenceReason::WorkerUnavailableAfterSample { failed_sample }) = sample.absence {
        if usize::from(failed_sample) >= index {
            return Err(invalid(format!(
                "{at}: worker_unavailable_after_sample.failed_sample must name an earlier sample"
            )));
        }
        for dimension in Dimension::ALL {
            match sample.terminals.get(&dimension) {
                Some(Terminal::NotRun { blocked_by }) if *blocked_by != ProbeOutcomeClass::Pass => {
                }
                _ => {
                    return Err(invalid(format!(
                        "{at}: an unrun slot records NotRun on every dimension"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_eligibility(
    manifest: &ProbeStateManifest,
    row: &ObservationRow,
) -> Result<(), ObserveError> {
    if !row.comparison_eligible {
        return Ok(());
    }
    if row.semantic_basis.is_none() {
        return Err(invalid(format!(
            "{}: comparison_eligible = true requires semantic_basis",
            row.row_id
        )));
    }
    if row.equivalent_work_basis.is_none() {
        return Err(invalid(format!(
            "{}: comparison_eligible = true requires equivalent_work_basis",
            row.row_id
        )));
    }
    if let Some(basis) = &row.equivalent_work_basis {
        if basis.authority != Authority::CompilerEquivalentWorkLedger {
            return Err(invalid(format!(
                "{}: equivalent_work_basis must cite compiler.equivalent-work-ledger",
                row.row_id
            )));
        }
    }
    if manifest.comparison != Comparison::Structural {
        return Err(invalid(format!(
            "{}: comparison_eligible = true requires comparison = structural",
            row.row_id
        )));
    }
    for (index, sample) in row.samples.iter().enumerate() {
        for dimension in [
            Dimension::Route,
            Dimension::Compile,
            Dimension::Structural,
            Dimension::Performance,
        ] {
            let terminal = &sample.terminals[&dimension];
            if !pass_class(terminal) {
                return Err(invalid(format!(
                    "{} sample {index}: comparison_eligible = true requires {dimension} = pass",
                    row.row_id
                )));
            }
        }
        for terminal in sample.terminals.values() {
            if matches!(terminal, Terminal::NotRun { .. }) {
                return Err(invalid(format!(
                    "{} sample {index}: comparison_eligible = true forbids NotRun",
                    row.row_id
                )));
            }
            if let Terminal::Class { class, .. } = terminal {
                if class.is_failure() {
                    return Err(invalid(format!(
                        "{} sample {index}: comparison_eligible = true forbids a failure class",
                        row.row_id
                    )));
                }
            }
        }
    }
    Ok(())
}

/// Derived reading: `cases / Σ cold elapsed_ns` over the named cold rows.
///
/// No value when any named cold row lacks a measurement. Aggregate
/// `comparison_eligible` is true only when every constituent row is.
pub fn selection_throughput(
    artifact: &ObservationArtifact,
    row_ids: &[String],
) -> Option<SelectionThroughput> {
    if row_ids.is_empty() {
        return None;
    }
    let by_id: BTreeMap<&str, &ObservationRow> = artifact
        .rows
        .iter()
        .map(|row| (row.row_id.as_str(), row))
        .collect();
    let mut elapsed_ns = 0u64;
    let mut eligible = true;
    for row_id in row_ids {
        let row = by_id.get(row_id.as_str())?;
        if row.mode != Mode::Cold {
            return None;
        }
        let sample = row.samples.first()?;
        let measurement = sample.measurement.as_ref()?;
        elapsed_ns = elapsed_ns.checked_add(measurement.elapsed_ns)?;
        eligible &= row.comparison_eligible;
    }
    Some(SelectionThroughput {
        row_ids: row_ids.to_vec(),
        cases: row_ids.len(),
        elapsed_ns,
        comparison_eligible: eligible,
    })
}

/// A throughput reading over an identified cold-row set. Not stored on the artifact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionThroughput {
    /// The named cold rows.
    pub row_ids: Vec<String>,
    /// How many named rows.
    pub cases: usize,
    /// Σ cold `elapsed_ns`.
    pub elapsed_ns: u64,
    /// True only when every constituent row is comparison-eligible.
    pub comparison_eligible: bool,
}

/// Where the inventory write lands, relative to the repository root.
pub const INVENTORY_RELATIVE: &str = "target/validation-probe/inventory";

/// The cutoff window a join record carries so a truncated fetch is visible.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RetrievalWindow {
    /// Fetch start, UTC RFC3339.
    pub started_at: String,
    /// Oldest `created_at` among kept artifacts.
    pub oldest_created_at: String,
    /// How many artifacts were kept.
    pub artifact_count: usize,
}

/// One trusted downloaded observation plus its identities.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RetrievedArtifact {
    /// Recomputed, run-bound artifact id.
    pub artifact_id: String,
    /// Filesystem key.
    pub artifact_key: String,
    /// The parsed document.
    pub artifact: ObservationArtifact,
}

/// The exact set of `{ artifact_id, row_id }` identities plus the window.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObservationInventory {
    /// Trusted artifacts, in fetch order.
    pub artifacts: Vec<RetrievedArtifact>,
    /// `{ artifact_id, row_id }` pairs.
    pub identities: BTreeSet<(String, String)>,
    /// Cutoff window.
    pub retrieval_window: RetrievalWindow,
}

/// Why a fetch aborted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FetchError {
    /// A contract or transport failure; a partial inventory is never returned.
    Aborted(String),
}

impl fmt::Display for FetchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FetchError::Aborted(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for FetchError {}

impl FetchError {
    fn transient(message: impl Into<String>) -> Self {
        FetchError::Aborted(message.into())
    }
}

/// One page of the unfiltered Actions artifact listing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtifactListPage {
    /// Artifacts on this page.
    pub artifacts: Vec<ListedArtifact>,
}

/// Listing fields the fetcher may read. Trust decisions use the run GET.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListedArtifact {
    /// Actions artifact id.
    pub id: u64,
    /// Artifact name.
    pub name: String,
    /// Compressed size.
    pub size_in_bytes: u64,
    /// Immutable create timestamp.
    pub created_at: String,
    /// Immutable expiry timestamp.
    pub expires_at: String,
    /// Listing-only run id; the run is then fetched.
    pub workflow_run_id: u64,
}

/// Trusted-run metadata from `GET .../actions/runs/{id}`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkflowRun {
    /// Run id.
    pub id: u64,
    /// Workflow file path.
    pub path: String,
    /// Event that created the run.
    pub event: String,
    /// Conclusion.
    pub conclusion: String,
    /// Head branch.
    pub head_branch: String,
    /// Head SHA.
    pub head_sha: String,
    /// Attempt number.
    pub run_attempt: u32,
}

/// `GET .../repos/{owner}/{repo}` default branch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepoInfo {
    /// Default branch name.
    pub default_branch: String,
}

/// Injected GitHub/git/clock so tests can plant listings and archives.
pub trait ArtifactSource {
    /// One listing page (`per_page=100`).
    fn list_page(&self, page: u32) -> Result<ArtifactListPage, FetchError>;
    /// One workflow run.
    fn workflow_run(&self, id: u64) -> Result<WorkflowRun, FetchError>;
    /// Repository default branch.
    fn repo(&self) -> Result<RepoInfo, FetchError>;
    /// Compressed archive bytes.
    fn download(&self, artifact_id: u64) -> Result<Vec<u8>, FetchError>;
    /// `git show <head_sha>:<path>`.
    fn git_show(&self, head_sha: &str, path: &str) -> Result<String, FetchError>;
    /// Fetch start, UTC RFC3339.
    fn started_at(&self) -> String;
    /// Backoff between transient page retries.
    fn retry_delay(&self, attempt: u8) -> Duration {
        Duration::from_millis(50 * 2u64.pow(u32::from(attempt)))
    }
}

/// Live `gh api` / `git show` transport.
pub struct GhCli {
    /// Owner.
    pub owner: String,
    /// Repo.
    pub repo: String,
}

impl ArtifactSource for GhCli {
    fn list_page(&self, page: u32) -> Result<ArtifactListPage, FetchError> {
        let path = format!(
            "repos/{}/{}/actions/artifacts?per_page={PAGE_SIZE}&page={page}",
            self.owner, self.repo
        );
        parse_list_page(&gh_json(&path)?)
    }

    fn workflow_run(&self, id: u64) -> Result<WorkflowRun, FetchError> {
        let path = format!("repos/{}/{}/actions/runs/{id}", self.owner, self.repo);
        parse_workflow_run(&gh_json(&path)?)
    }

    fn repo(&self) -> Result<RepoInfo, FetchError> {
        let path = format!("repos/{}/{}", self.owner, self.repo);
        let value = gh_json(&path)?;
        let default_branch = value
            .get("default_branch")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| FetchError::Aborted("repository default_branch missing".into()))?
            .to_string();
        Ok(RepoInfo { default_branch })
    }

    fn download(&self, artifact_id: u64) -> Result<Vec<u8>, FetchError> {
        let path = format!(
            "repos/{}/{}/actions/artifacts/{artifact_id}/zip",
            self.owner, self.repo
        );
        gh_bytes(&path)
    }

    fn git_show(&self, head_sha: &str, path: &str) -> Result<String, FetchError> {
        let spec = format!("{head_sha}:{path}");
        let output = Command::new("git")
            .args(["show", &spec])
            .output()
            .map_err(|error| FetchError::Aborted(error.to_string()))?;
        if !output.status.success() {
            return Err(FetchError::Aborted(format!(
                "git show {spec}: {}",
                String::from_utf8_lossy(&output.stderr)
            )));
        }
        String::from_utf8(output.stdout).map_err(|error| FetchError::Aborted(error.to_string()))
    }

    fn started_at(&self) -> String {
        utc_now_rfc3339()
    }
}

fn gh_json(path: &str) -> Result<serde_json::Value, FetchError> {
    let bytes = gh_bytes(path)?;
    serde_json::from_slice(&bytes).map_err(|error| FetchError::Aborted(error.to_string()))
}

fn gh_bytes(path: &str) -> Result<Vec<u8>, FetchError> {
    let output = Command::new("gh")
        .args(["api", path])
        .output()
        .map_err(|error| FetchError::transient(error.to_string()))?;
    if !output.status.success() {
        return Err(FetchError::transient(format!(
            "gh api {path}: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(output.stdout)
}

fn parse_list_page(value: &serde_json::Value) -> Result<ArtifactListPage, FetchError> {
    let artifacts = value
        .get("artifacts")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| FetchError::Aborted("listing has no artifacts array".into()))?;
    let mut out = Vec::new();
    for artifact in artifacts {
        out.push(ListedArtifact {
            id: required_u64(artifact, "id")?,
            name: required_str(artifact, "name")?,
            size_in_bytes: required_u64(artifact, "size_in_bytes")?,
            created_at: required_str(artifact, "created_at")?,
            expires_at: required_str(artifact, "expires_at")?,
            workflow_run_id: artifact
                .get("workflow_run")
                .and_then(|run| run.get("id"))
                .and_then(serde_json::Value::as_u64)
                .ok_or_else(|| {
                    FetchError::Aborted("listing artifact has no workflow_run.id".into())
                })?,
        });
    }
    Ok(ArtifactListPage { artifacts: out })
}

fn parse_workflow_run(value: &serde_json::Value) -> Result<WorkflowRun, FetchError> {
    Ok(WorkflowRun {
        id: required_u64(value, "id")?,
        path: required_str(value, "path")?,
        event: required_str(value, "event")?,
        conclusion: required_str(value, "conclusion")?,
        head_branch: required_str(value, "head_branch")?,
        head_sha: required_str(value, "head_sha")?,
        run_attempt: required_u64(value, "run_attempt")? as u32,
    })
}

fn required_str(value: &serde_json::Value, key: &str) -> Result<String, FetchError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| FetchError::Aborted(format!("missing string field {key}")))
}

fn required_u64(value: &serde_json::Value, key: &str) -> Result<u64, FetchError> {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| FetchError::Aborted(format!("missing integer field {key}")))
}

/// Keep listing names that start with the observation prefix.
pub fn listing_name_kept(name: &str) -> bool {
    name.starts_with(ARTIFACT_NAME_PREFIX)
}

fn run_attempt_from_name(name: &str) -> Option<u32> {
    name.strip_prefix(ARTIFACT_NAME_PREFIX)?
        .rsplit_once('-')
        .and_then(|(_, attempt)| attempt.parse().ok())
}

fn eligible(listed: &ListedArtifact, started_at: &str) -> bool {
    listing_name_kept(&listed.name)
        && listed.created_at.as_str() <= started_at
        && listed.expires_at.as_str() > started_at
}

fn scan_eligible<S: ArtifactSource>(
    source: &S,
    started_at: &str,
) -> Result<Vec<ListedArtifact>, FetchError> {
    let mut page = 1u32;
    let mut all = Vec::new();
    loop {
        let listing = retry(source, || source.list_page(page))?;
        let count = listing.artifacts.len();
        all.extend(listing.artifacts);
        if count < PAGE_SIZE as usize {
            break;
        }
        page += 1;
    }
    let eligible: Vec<ListedArtifact> = all
        .into_iter()
        .filter(|listed| eligible(listed, started_at))
        .collect();
    if eligible.len() > MAX_FETCH_ARTIFACTS {
        return Err(FetchError::Aborted(format!(
            "fetch exceeds the {MAX_FETCH_ARTIFACTS} artifact ceiling"
        )));
    }
    Ok(eligible)
}

fn retry<S: ArtifactSource, T>(
    source: &S,
    mut op: impl FnMut() -> Result<T, FetchError>,
) -> Result<T, FetchError> {
    let mut last = None;
    for attempt in 0..LISTING_RETRIES {
        match op() {
            Ok(value) => return Ok(value),
            Err(error) => {
                last = Some(error);
                if attempt + 1 < LISTING_RETRIES {
                    std::thread::sleep(source.retry_delay(attempt));
                }
            }
        }
    }
    Err(last.unwrap_or_else(|| FetchError::Aborted("retry exhausted".into())))
}

fn stable_eligible<S: ArtifactSource>(
    source: &S,
    started_at: &str,
) -> Result<Vec<ListedArtifact>, FetchError> {
    let mut previous: Option<BTreeSet<u64>> = None;
    let mut mismatches = 0u8;
    loop {
        let scan = scan_eligible(source, started_at)?;
        let ids: BTreeSet<u64> = scan.iter().map(|listed| listed.id).collect();
        if let Some(prev) = &previous {
            if *prev == ids {
                return Ok(scan);
            }
            mismatches += 1;
            if mismatches >= MAX_SCAN_MISMATCHES {
                return Err(FetchError::Aborted(
                    "listing did not stabilize after three non-matching scans".into(),
                ));
            }
        }
        previous = Some(ids);
    }
}

/// Path-safe extraction: one regular member named `observations.json`.
pub fn extract_observations_json(bytes: &[u8]) -> Result<String, FetchError> {
    if bytes.len() as u64 > MAX_COMPRESSED_BYTES {
        return Err(FetchError::Aborted(
            "archive exceeds the 64 MiB compressed ceiling".into(),
        ));
    }
    let cursor = Cursor::new(bytes);
    let mut archive =
        zip::ZipArchive::new(cursor).map_err(|error| FetchError::Aborted(error.to_string()))?;
    if archive.len() != 1 {
        return Err(FetchError::Aborted(
            "archive must contain exactly one member named observations.json".into(),
        ));
    }
    let mut file = archive
        .by_index(0)
        .map_err(|error| FetchError::Aborted(error.to_string()))?;
    if file.is_dir() {
        return Err(FetchError::Aborted("archive member is a directory".into()));
    }
    if file
        .unix_mode()
        .map(|mode| mode & 0o170000 == 0o120000)
        .unwrap_or(false)
    {
        return Err(FetchError::Aborted("archive member is a link".into()));
    }
    let name = file.name().to_string();
    if name != "observations.json" {
        return Err(FetchError::Aborted(format!(
            "archive member `{name}` is not observations.json"
        )));
    }
    if file.enclosed_name().is_none() {
        return Err(FetchError::Aborted(
            "archive member path is not enclosed".into(),
        ));
    }
    if file.size() > MAX_UNCOMPRESSED_BYTES {
        return Err(FetchError::Aborted(
            "archive exceeds the 256 MiB uncompressed ceiling".into(),
        ));
    }
    let mut text = String::new();
    file.read_to_string(&mut text)
        .map_err(|error| FetchError::Aborted(error.to_string()))?;
    Ok(text)
}

impl ObservationInventory {
    /// List, download, extract, historically validate, and bind identities.
    pub fn fetch(owner: &str, repo: &str, dir: &Path) -> Result<Self, FetchError> {
        Self::fetch_with(
            &GhCli {
                owner: owner.to_string(),
                repo: repo.to_string(),
            },
            dir,
        )
    }

    /// Fetch through an injected source.
    pub fn fetch_with<S: ArtifactSource>(source: &S, dir: &Path) -> Result<Self, FetchError> {
        let started_at = source.started_at();
        let eligible = stable_eligible(source, &started_at)?;
        let repo = source.repo()?;
        let mut runs: BTreeMap<u64, WorkflowRun> = BTreeMap::new();
        let mut trusted = Vec::new();
        for listed in &eligible {
            if listed.size_in_bytes > MAX_COMPRESSED_BYTES {
                return Err(FetchError::Aborted(
                    "archive exceeds the 64 MiB compressed ceiling".into(),
                ));
            }
            let run = if let Some(run) = runs.get(&listed.workflow_run_id) {
                run.clone()
            } else {
                let run = source.workflow_run(listed.workflow_run_id)?;
                runs.insert(listed.workflow_run_id, run.clone());
                run
            };
            let Some(name_attempt) = run_attempt_from_name(&listed.name) else {
                continue;
            };
            if run.path != WORKFLOW_PATH
                || run.event != "push"
                || run.conclusion != "success"
                || run.head_branch != repo.default_branch
                || run.run_attempt != name_attempt
            {
                continue;
            }
            trusted.push((listed.clone(), run));
        }

        let mut artifacts = Vec::new();
        let mut identities = BTreeSet::new();
        let mut oldest = started_at.clone();
        for (listed, run) in &trusted {
            let bytes = source.download(listed.id)?;
            if bytes.len() as u64 > MAX_COMPRESSED_BYTES {
                return Err(FetchError::Aborted(
                    "archive exceeds the 64 MiB compressed ceiling".into(),
                ));
            }
            let text = extract_observations_json(&bytes)?;
            let artifact = ObservationArtifact::from_json_str(&text)
                .map_err(|error| FetchError::Aborted(error.to_string()))?;
            if artifact.verter_commit != run.head_sha {
                return Err(FetchError::Aborted(format!(
                    "artifact verter_commit {} differs from run head_sha {}",
                    artifact.verter_commit, run.head_sha
                )));
            }
            if artifact.workflow_run_id != run.id.to_string()
                || artifact.run_attempt != run.run_attempt
            {
                return Err(FetchError::Aborted(
                    "embedded workflow_run_id/run_attempt do not match the trusted run".into(),
                ));
            }
            let vue_text = source.git_show(&run.head_sha, MANIFEST_VUE)?;
            let svelte_text = source.git_show(&run.head_sha, MANIFEST_SVELTE)?;
            let vue = ProbeStateManifest::from_manifest_file("vue.toml", &vue_text)
                .map_err(|error| FetchError::Aborted(error.to_string()))?;
            let svelte = ProbeStateManifest::from_manifest_file("svelte.toml", &svelte_text)
                .map_err(|error| FetchError::Aborted(error.to_string()))?;
            let mut historical_revisions = BTreeMap::new();
            historical_revisions.insert(
                Framework::Vue.as_str().to_string(),
                vue.external_revision.as_str().to_string(),
            );
            historical_revisions.insert(
                Framework::Svelte.as_str().to_string(),
                svelte.external_revision.as_str().to_string(),
            );
            if artifact.corpus_revisions != historical_revisions {
                return Err(FetchError::Aborted(
                    "header corpus_revisions do not equal the historical manifest pins".into(),
                ));
            }
            let digest = corpus_digest(&historical_revisions);
            let expected_id =
                compose_artifact_id(&run.head_sha, &digest, &run.id.to_string(), run.run_attempt);
            if expected_id != artifact.artifact_id {
                return Err(FetchError::Aborted(
                    "recomputed artifact_id does not equal the embedded id".into(),
                ));
            }
            artifact
                .validate(&[vue, svelte])
                .map_err(|error| FetchError::Aborted(error.to_string()))?;
            for row in &artifact.rows {
                if !identities.insert((artifact.artifact_id.clone(), row.row_id.clone())) {
                    return Err(FetchError::Aborted(format!(
                        "duplicate observation identity {{ {}, {} }}",
                        artifact.artifact_id, row.row_id
                    )));
                }
            }
            let key = artifact_key(
                &artifact.verter_commit,
                &digest,
                &artifact.workflow_run_id,
                artifact.run_attempt,
            )
            .map_err(|error| FetchError::Aborted(error.to_string()))?;
            let dest = dir.join(&key).join("observations.json");
            disk::write_text(&dest, &text)
                .map_err(|error| FetchError::Aborted(error.to_string()))?;
            if listed.created_at.as_str() < oldest.as_str() {
                oldest = listed.created_at.clone();
            }
            artifacts.push(RetrievedArtifact {
                artifact_id: artifact.artifact_id.clone(),
                artifact_key: key,
                artifact,
            });
        }
        if artifacts.is_empty() {
            oldest = started_at.clone();
        }
        Ok(ObservationInventory {
            retrieval_window: RetrievalWindow {
                started_at,
                oldest_created_at: oldest,
                artifact_count: artifacts.len(),
            },
            artifacts,
            identities,
        })
    }
}

fn utc_now_rfc3339() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_secs() as i64;
    let (year, month, day, hour, min, sec) = civil_utc(secs);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{min:02}:{sec:02}Z")
}

fn civil_utc(unix: i64) -> (i32, u32, u32, u32, u32, u32) {
    let z = unix.div_euclid(86400);
    let tod = unix.rem_euclid(86400) as u32;
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era as i32 * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, tod / 3600, (tod % 3600) / 60, tod % 60)
}

/// Write the observation artifact and return where it landed.
pub fn write_observations(artifact: &ObservationArtifact) -> Result<PathBuf, ObserveError> {
    let path = observations_path();
    disk::write_text(&path, &artifact.to_json()).map_err(|error| invalid(error.to_string()))?;
    Ok(path)
}

/// Where the observation artifact lands.
pub fn observations_path() -> PathBuf {
    crate::corpus::workspace_root().join(OBSERVATIONS_RELATIVE_PATH)
}

/// Drive the selected slice and emit one observation artifact.
#[cfg(feature = "external-corpus")]
pub fn capture(
    lane: crate::summary::Lane,
    deadlines: PhaseDeadlines,
) -> Result<ObservationArtifact, ObserveError> {
    use crate::lane;

    let driver = DriverCommand::committed(&crate::corpus::workspace_root());
    let mut manifests = Vec::new();
    let mut rows = Vec::new();
    let mut revisions = BTreeMap::new();
    for framework in Framework::ALL {
        let manifest =
            lane::load_manifest(framework).map_err(|error| invalid(error.to_string()))?;
        lane::check_revision(&manifest).map_err(|error| invalid(error.to_string()))?;
        lane::check_inventory(&manifest).map_err(|error| invalid(error.to_string()))?;
        let selected = lane::selection(&manifest, lane);
        let cases = lane::plan(&manifest, &selected).map_err(|error| invalid(error.to_string()))?;
        revisions.insert(
            framework.as_str().to_string(),
            manifest.external_revision.as_str().to_string(),
        );
        for case in &cases {
            rows.push(observe_row(
                &manifest,
                &driver,
                case,
                Mode::Cold,
                deadlines,
            )?);
            rows.push(observe_row(
                &manifest,
                &driver,
                case,
                Mode::Warm,
                deadlines,
            )?);
        }
        manifests.push(manifest);
    }
    let digest = corpus_digest(&revisions);
    let verter_commit = git_head()?;
    let workflow_run_id = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| "0".to_string());
    let run_attempt = std::env::var("GITHUB_RUN_ATTEMPT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    let execution_mode = if std::env::var("GITHUB_ACTIONS").is_ok() {
        "ci"
    } else {
        "local"
    };
    let artifact = ObservationArtifact {
        artifact_id: compose_artifact_id(&verter_commit, &digest, &workflow_run_id, run_attempt),
        verter_commit,
        corpus_revisions: revisions,
        corpus_digest: digest,
        workflow_run_id,
        run_attempt,
        rust_version: command_version("rustc", &["--version"])?,
        node_version: command_version("node", &["--version"])?,
        addon_version: addon_version()?,
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        execution_mode: execution_mode.to_string(),
        sample_plan: SamplePlan {
            cold: COLD_SAMPLES,
            warm: WARM_SAMPLES,
        },
        request_vue: request::REQUEST_VUE.to_string(),
        request_svelte: request::REQUEST_SVELTE.to_string(),
        template_digests: TemplateDigests {
            vue: request::template_digest(Framework::Vue),
            svelte: request::template_digest(Framework::Svelte),
        },
        rows,
    };
    artifact.validate(&manifests)?;
    Ok(artifact)
}

#[cfg(feature = "external-corpus")]
fn git_head() -> Result<String, ObserveError> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(crate::corpus::workspace_root())
        .output()
        .map_err(|error| invalid(error.to_string()))?;
    if !output.status.success() {
        return Err(invalid("git rev-parse HEAD failed"));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(feature = "external-corpus")]
fn command_version(program: &str, args: &[&str]) -> Result<String, ObserveError> {
    let output = Command::new(program)
        .args(args)
        .output()
        .map_err(|error| invalid(error.to_string()))?;
    if !output.status.success() {
        return Err(invalid(format!("{program} --version failed")));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(feature = "external-corpus")]
fn addon_version() -> Result<String, ObserveError> {
    let path = crate::corpus::workspace_root()
        .join("packages")
        .join("native")
        .join("package.json");
    let text = disk::read_text(&path).map_err(|error| invalid(error.to_string()))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|error| invalid(error.to_string()))?;
    value
        .get("version")
        .and_then(serde_json::Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| invalid("packages/native/package.json has no version"))
}

#[cfg(feature = "external-corpus")]
fn observe_row(
    manifest: &ProbeStateManifest,
    driver: &DriverCommand,
    case: &PlannedCase,
    mode: Mode,
    deadlines: PhaseDeadlines,
) -> Result<ObservationRow, ObserveError> {
    let (mut child, lines) =
        runner::spawn_driver(driver).map_err(|error| invalid(error.to_string()))?;
    let samples = match mode {
        Mode::Cold => {
            let (sample, _stop) = observe_one(&mut child, &lines, manifest, case, deadlines)?;
            vec![sample]
        }
        Mode::Warm => observe_warm(&mut child, &lines, manifest, case, deadlines)?,
    };
    let _ = child.stdin.take();
    let _ = child.wait();
    finish_row(manifest, case, mode, samples)
}

#[cfg(feature = "external-corpus")]
fn finish_row(
    manifest: &ProbeStateManifest,
    case: &PlannedCase,
    mode: Mode,
    samples: Vec<Sample>,
) -> Result<ObservationRow, ObserveError> {
    let observed_outcome = project_observed_outcome(&samples)?;
    Ok(ObservationRow {
        row_id: format!("{}@{}", case.case_id, mode.as_str()),
        case_id: case.case_id.clone(),
        mode,
        corpus_revision: manifest.external_revision.as_str().to_string(),
        request_digest: request::request_digest(manifest.framework, &case.relative_path),
        sample_count: samples.len(),
        samples,
        observed_outcome,
        comparison_eligible: false,
        semantic_basis: None,
        equivalent_work_basis: None,
    })
}

#[cfg(feature = "external-corpus")]
fn observe_warm(
    child: &mut Child,
    lines: &Receiver<Result<String, String>>,
    manifest: &ProbeStateManifest,
    case: &PlannedCase,
    deadlines: PhaseDeadlines,
) -> Result<Vec<Sample>, ObserveError> {
    let (warmup, warmup_stop) = observe_one(child, lines, manifest, case, deadlines)?;
    if worker_dead(warmup_stop) {
        return fill_remaining(Vec::new(), warmup, 0, WARM_SAMPLES as usize);
    }
    let mut samples = Vec::with_capacity(WARM_SAMPLES as usize);
    for index in 0..WARM_SAMPLES {
        let (sample, stop) = observe_one(child, lines, manifest, case, deadlines)?;
        let dead = worker_dead(stop);
        if dead {
            return fill_remaining(samples, sample, index, WARM_SAMPLES as usize);
        }
        samples.push(sample);
    }
    Ok(samples)
}

#[cfg(feature = "external-corpus")]
fn worker_dead(stop: Option<LaneStop>) -> bool {
    matches!(stop, Some(LaneStop::Process(_) | LaneStop::Harness))
}

#[cfg(feature = "external-corpus")]
fn fill_remaining(
    mut samples: Vec<Sample>,
    terminating: Sample,
    failed_sample: u8,
    total: usize,
) -> Result<Vec<Sample>, ObserveError> {
    let blocked_by = blocked_by_of(&terminating);
    samples.push(terminating);
    while samples.len() < total {
        samples.push(Sample {
            terminals: Dimension::ALL
                .into_iter()
                .map(|dimension| (dimension, Terminal::NotRun { blocked_by }))
                .collect(),
            measurement: None,
            absence: Some(AbsenceReason::WorkerUnavailableAfterSample { failed_sample }),
        });
    }
    Ok(samples)
}

#[cfg(feature = "external-corpus")]
fn blocked_by_of(sample: &Sample) -> ProbeOutcomeClass {
    match sample.absence {
        Some(AbsenceReason::ReferenceTerminated) => ProbeOutcomeClass::ReferenceFailure,
        _ => match sample.terminals.get(&Dimension::Route) {
            Some(Terminal::Class { class, .. }) if *class != ProbeOutcomeClass::Pass => *class,
            _ => ProbeOutcomeClass::HarnessFailure,
        },
    }
}

#[cfg(feature = "external-corpus")]
fn observe_one(
    child: &mut Child,
    lines: &Receiver<Result<String, String>>,
    manifest: &ProbeStateManifest,
    case: &PlannedCase,
    deadlines: PhaseDeadlines,
) -> Result<(Sample, Option<LaneStop>), ObserveError> {
    let requested = vec![RequestedEntry {
        canonical_id: case.case_id.clone(),
        source: case.source.clone(),
        request_digest: request::request_digest(manifest.framework, &case.relative_path),
    }];
    let mut run = ProbeRun::new(case.case_id.clone(), requested);
    let mut stop = None;
    match runner::write_probe(child, manifest.framework, case, true) {
        Ok(()) => runner::drive_probe(&mut run, lines, deadlines, &mut stop, child),
        Err(message) => {
            run.record_harness_failure(message);
            stop = Some(LaneStop::Harness);
        }
    }
    let sample = sample_from_run(&run, manifest, stop.as_ref().and_then(LaneStop::process))?;
    Ok((sample, stop))
}

#[cfg(feature = "external-corpus")]
fn sample_from_run(
    run: &ProbeRun,
    manifest: &ProbeStateManifest,
    terminated: Option<ExecutionEvent>,
) -> Result<Sample, ObserveError> {
    let observation = run
        .finish(manifest, terminated)
        .into_iter()
        .next()
        .ok_or_else(|| invalid("a probe produced no observation"))?
        .map_err(|error| invalid(error.to_string()))?;
    let terminals = observation.terminals().clone();
    let measurement = run.elapsed_ns().map(|elapsed_ns| Measurement {
        elapsed_ns,
        memory: run.memory().map(MemoryPair::from),
    });
    let absence = absence_of(run, terminated, measurement.is_some());
    Ok(Sample {
        terminals,
        measurement,
        absence,
    })
}

#[cfg(feature = "external-corpus")]
fn absence_of(
    run: &ProbeRun,
    terminated: Option<ExecutionEvent>,
    measured: bool,
) -> Option<AbsenceReason> {
    if terminated.is_none() && measured {
        return None;
    }
    if measured {
        return Some(AbsenceReason::ReferenceTerminated);
    }
    match run.outcome_phase() {
        None | Some(Phase::Load) => {
            if terminated.is_some() {
                Some(AbsenceReason::LoadFailed)
            } else {
                Some(AbsenceReason::DriverError)
            }
        }
        Some(Phase::Compile) => Some(AbsenceReason::CompileTerminated),
        Some(Phase::Reference) => Some(AbsenceReason::ReferenceTerminated),
    }
}

#[cfg(feature = "external-corpus")]
impl From<RunError> for ObserveError {
    fn from(error: RunError) -> Self {
        invalid(error.to_string())
    }
}
