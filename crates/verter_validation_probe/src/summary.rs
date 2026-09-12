//! The lane's one compact machine-readable summary.
//!
//! The summary is evidence, not a verdict dressed as a number. Every counter
//! it carries is required — a summary missing one is refused rather than read
//! as a zero — and the lane's disposition is a real process exit computed from
//! the gate cells, so a required job cannot go green while a gate regressed.
//!
//! The shape is framework-neutral: `framework` is a row attribute and a
//! per-framework counter block, never a summary variant, so a second corpus
//! reports through this same document.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use serde::{Deserialize, Serialize};

use crate::evaluate::Evaluation;
use crate::manifest::{ExpectedState, Framework, ProbeStateManifest};
use crate::outcome::{CaseObservation, Dimension, ProbeOutcomeClass, Terminal};
use crate::request;

/// The summary document's schema version. A reader that does not recognise it
/// refuses the document rather than guessing at its fields.
pub const SCHEMA_VERSION: u32 = 1;

/// Which lane produced a summary.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// The bounded pull-request slice.
    Smoke,
    /// The complete ratified inventory.
    Main,
}

impl Lane {
    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Lane::Smoke => "smoke",
            Lane::Main => "main",
        }
    }
}

/// Every counter the summary must carry. All of them are required: a document
/// missing one is refused by [`Summary::validate`].
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counters {
    /// Cases the lane selected.
    pub selected: usize,
    /// Cases the lane actually drove.
    pub attempted: usize,
    /// Cases that actually succeeded: every exercised cell observed `pass`,
    /// and no cell was left unreached.
    ///
    /// Deliberately NOT "every cell met its expectation": a case whose
    /// `Compile` cell is a recorded known-fail is behaving exactly as declared,
    /// and counting it as passed would report a lane of known failures as a
    /// lane of successes. Whether expectations were met is what the gate,
    /// canary, known-fail and XPASS counters carry.
    pub passed: usize,
    /// Gate cells that evaluated to anything but a gate pass.
    pub gated_regressions: usize,
    /// Canary cells that met their expected failure class, by class.
    pub canary_failures: BTreeMap<String, usize>,
    /// Known-fail cells that met their expected failure class, by class.
    pub known_failures: BTreeMap<String, usize>,
    /// Cells the manifest declares skipped.
    pub skips: usize,
    /// Failure canary or known-fail cells that observed a pass.
    pub xpass_candidates: usize,
    /// Cells whose terminal is `crash`.
    pub crashes: usize,
    /// Cells whose terminal is `timeout`.
    pub timeouts: usize,
    /// Cells whose terminal is `harness_failure`.
    pub harness_failures: usize,
}

impl Counters {
    /// The counter names every summary must carry, in document order.
    pub const REQUIRED: [&'static str; 11] = [
        "selected",
        "attempted",
        "passed",
        "gated_regressions",
        "canary_failures",
        "known_failures",
        "skips",
        "xpass_candidates",
        "crashes",
        "timeouts",
        "harness_failures",
    ];
}

/// One evaluated cell.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellRow {
    /// The case.
    pub probe_id: String,
    /// The dimension.
    pub dimension: Dimension,
    /// What the manifest declared.
    pub expected_state: ExpectedState,
    /// The exact class the manifest expected, when it declared one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_class: Option<ProbeOutcomeClass>,
    /// The class observed, when the dimension produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_class: Option<ProbeOutcomeClass>,
    /// The evaluation.
    pub evaluation: Evaluation,
}

/// One case's row.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseRow {
    /// The stable case id.
    pub case_id: String,
    /// The case's framework.
    pub framework: Framework,
    /// The SHA-256 of this case's substituted canonical request.
    pub request_digest: String,
    /// Nanoseconds the bracketed compile call took, when it was observed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub elapsed_ns: Option<u64>,
    /// Every cell of this case, in dimension order.
    pub cells: Vec<CellRow>,
}

/// One framework's block of the summary.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrameworkSummary {
    /// The framework.
    pub framework: Framework,
    /// The pinned corpus revision this framework's cases came from.
    pub external_revision: String,
    /// The canonical request template, verbatim.
    pub request_template: String,
    /// The template's SHA-256.
    pub template_digest: String,
    /// This framework's counters.
    pub counters: Counters,
    /// This framework's case rows, in case-id order.
    pub cases: Vec<CaseRow>,
}

/// The lane's summary document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    /// The document schema.
    pub schema_version: u32,
    /// Which lane produced it.
    pub lane: Lane,
    /// One block per framework, in framework order.
    pub frameworks: Vec<FrameworkSummary>,
    /// The totals across every framework.
    pub totals: Counters,
}

/// Why a summary is not usable.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SummaryError {
    /// The document declares a schema this reader does not know.
    UnknownSchema {
        /// The declared version.
        schema_version: u32,
    },
    /// A required counter is missing from a framework block.
    MissingCounter {
        /// The framework.
        framework: String,
        /// The counter.
        counter: String,
    },
    /// A framework attempted a different case set than it selected.
    InventoryMismatch {
        /// The framework.
        framework: String,
        /// What was selected.
        selected: usize,
        /// What was attempted.
        attempted: usize,
    },
    /// A framework selected no case at all.
    EmptySelection {
        /// The framework.
        framework: String,
    },
    /// A framework's rows do not equal the case set its counters claim.
    RowCountMismatch {
        /// The framework.
        framework: String,
        /// The rows present.
        rows: usize,
        /// The cases attempted.
        attempted: usize,
    },
    /// The totals are not the sum of the per-framework counters.
    TotalsMismatch,
    /// The document could not be parsed.
    Parse(String),
}

impl std::fmt::Display for SummaryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SummaryError::UnknownSchema { schema_version } => {
                write!(f, "unknown summary schema version {schema_version}")
            }
            SummaryError::MissingCounter { framework, counter } => {
                write!(f, "{framework}: required counter `{counter}` is missing")
            }
            SummaryError::InventoryMismatch {
                framework,
                selected,
                attempted,
            } => write!(
                f,
                "{framework}: attempted {attempted} of {selected} selected cases"
            ),
            SummaryError::EmptySelection { framework } => {
                write!(f, "{framework}: the lane selected no case")
            }
            SummaryError::RowCountMismatch {
                framework,
                rows,
                attempted,
            } => write!(
                f,
                "{framework}: {rows} case rows for {attempted} attempted cases"
            ),
            SummaryError::TotalsMismatch => {
                f.write_str("the totals are not the sum of the per-framework counters")
            }
            SummaryError::Parse(message) => write!(f, "summary malformed: {message}"),
        }
    }
}

impl std::error::Error for SummaryError {}

/// The lane's disposition: a real exit, computed from the gate cells alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// Every gate cell evaluated to a gate pass.
    Clean,
    /// At least one gate cell did not.
    GateRegressed {
        /// How many.
        count: usize,
    },
}

impl Disposition {
    /// The process exit code this disposition demands.
    pub const fn exit_code(self) -> i32 {
        match self {
            Disposition::Clean => 0,
            Disposition::GateRegressed { .. } => 1,
        }
    }
}

impl Summary {
    /// Parse a summary document, checking every required counter is present
    /// before any value of it is read.
    ///
    /// Presence is checked against the RAW document, not the deserialized
    /// struct: a missing counter would otherwise deserialize to its type's
    /// default and read as an honest zero.
    pub fn from_json_str(text: &str) -> Result<Self, SummaryError> {
        let raw: serde_json::Value =
            serde_json::from_str(text).map_err(|error| SummaryError::Parse(error.to_string()))?;
        let frameworks = raw
            .get("frameworks")
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| SummaryError::Parse("frameworks must be an array".to_string()))?;
        for block in frameworks {
            let name = block
                .get("framework")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?")
                .to_string();
            let counters = block.get("counters").and_then(serde_json::Value::as_object);
            for counter in Counters::REQUIRED {
                let present = counters.is_some_and(|counters| counters.contains_key(counter));
                if !present {
                    return Err(SummaryError::MissingCounter {
                        framework: name,
                        counter: counter.to_string(),
                    });
                }
            }
        }
        let totals = raw.get("totals").and_then(serde_json::Value::as_object);
        for counter in Counters::REQUIRED {
            if !totals.is_some_and(|totals| totals.contains_key(counter)) {
                return Err(SummaryError::MissingCounter {
                    framework: "totals".to_string(),
                    counter: counter.to_string(),
                });
            }
        }
        let summary: Summary =
            serde_json::from_value(raw).map_err(|error| SummaryError::Parse(error.to_string()))?;
        summary.validate()?;
        Ok(summary)
    }

    /// Check the document's own internal consistency.
    ///
    /// The inventory check runs in BOTH directions: a framework that attempted
    /// fewer cases than it selected has silently skipped work, and one that
    /// attempted more has run work no manifest ratified.
    pub fn validate(&self) -> Result<(), SummaryError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SummaryError::UnknownSchema {
                schema_version: self.schema_version,
            });
        }
        let mut totals = Counters::default();
        for block in &self.frameworks {
            let name = block.framework.as_str().to_string();
            if block.counters.selected == 0 {
                return Err(SummaryError::EmptySelection { framework: name });
            }
            if block.counters.attempted != block.counters.selected {
                return Err(SummaryError::InventoryMismatch {
                    framework: name,
                    selected: block.counters.selected,
                    attempted: block.counters.attempted,
                });
            }
            if block.cases.len() != block.counters.attempted {
                return Err(SummaryError::RowCountMismatch {
                    framework: name,
                    rows: block.cases.len(),
                    attempted: block.counters.attempted,
                });
            }
            add(&mut totals, &block.counters);
        }
        if totals != self.totals {
            return Err(SummaryError::TotalsMismatch);
        }
        Ok(())
    }

    /// The lane's disposition.
    pub fn disposition(&self) -> Disposition {
        match self.totals.gated_regressions {
            0 => Disposition::Clean,
            count => Disposition::GateRegressed { count },
        }
    }

    /// The document, as canonical JSON.
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("the summary is serializable")
    }

    /// The compact human summary: one table, every counter, per framework and
    /// total.
    pub fn to_markdown(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "### Validation probe — {} lane\n", self.lane.as_str());
        let _ = writeln!(
            out,
            "| framework | attempted | passed | gated regressions | canary failures | \
             known failures | skips | XPASS candidates | crashes | timeouts | harness failures |"
        );
        let _ = writeln!(
            out,
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"
        );
        for block in &self.frameworks {
            row(&mut out, block.framework.as_str(), &block.counters);
        }
        row(&mut out, "**total**", &self.totals);
        let _ = writeln!(out);
        for block in &self.frameworks {
            let _ = writeln!(
                out,
                "`{}` pinned at `{}`, request `{}`.",
                block.framework.as_str(),
                block.external_revision,
                block.template_digest
            );
        }
        if !self.totals.canary_failures.is_empty() || !self.totals.known_failures.is_empty() {
            let _ = writeln!(out, "\nBy outcome class:\n");
            let _ = writeln!(out, "| class | canary | known-fail |");
            let _ = writeln!(out, "| --- | ---: | ---: |");
            let mut classes: Vec<&String> = self
                .totals
                .canary_failures
                .keys()
                .chain(self.totals.known_failures.keys())
                .collect();
            classes.sort();
            classes.dedup();
            for class in classes {
                let _ = writeln!(
                    out,
                    "| {class} | {} | {} |",
                    self.totals.canary_failures.get(class).copied().unwrap_or(0),
                    self.totals.known_failures.get(class).copied().unwrap_or(0),
                );
            }
        }
        out
    }
}

fn row(out: &mut String, name: &str, counters: &Counters) {
    let _ = writeln!(
        out,
        "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        counters.attempted,
        counters.passed,
        counters.gated_regressions,
        counters.canary_failures.values().sum::<usize>(),
        counters.known_failures.values().sum::<usize>(),
        counters.skips,
        counters.xpass_candidates,
        counters.crashes,
        counters.timeouts,
        counters.harness_failures,
    );
}

fn add(totals: &mut Counters, counters: &Counters) {
    totals.selected += counters.selected;
    totals.attempted += counters.attempted;
    totals.passed += counters.passed;
    totals.gated_regressions += counters.gated_regressions;
    totals.skips += counters.skips;
    totals.xpass_candidates += counters.xpass_candidates;
    totals.crashes += counters.crashes;
    totals.timeouts += counters.timeouts;
    totals.harness_failures += counters.harness_failures;
    for (class, count) in &counters.canary_failures {
        *totals.canary_failures.entry(class.clone()).or_insert(0) += count;
    }
    for (class, count) in &counters.known_failures {
        *totals.known_failures.entry(class.clone()).or_insert(0) += count;
    }
}

/// One framework's evaluated run, ready to be folded into a summary.
pub struct FrameworkRun<'a> {
    /// The framework's validated manifest.
    pub manifest: &'a ProbeStateManifest,
    /// The case ids the lane selected, in order.
    pub selected: Vec<String>,
    /// Each attempted case's observation, request digest, and elapsed time.
    pub observed: Vec<ObservedCase>,
}

/// One attempted case.
pub struct ObservedCase {
    /// The case id.
    pub case_id: String,
    /// The SHA-256 of this case's substituted canonical request.
    pub request_digest: String,
    /// Nanoseconds the bracketed compile call took, when it was observed.
    pub elapsed_ns: Option<u64>,
    /// The folded observation.
    pub observation: CaseObservation,
}

/// Build the summary from every framework's evaluated run.
///
/// Evaluation goes through [`ProbeStateManifest::evaluate_case`], never
/// [`crate::manifest::ProbeEntry::evaluate`] directly, so a driver reporting a
/// dimension inapplicable that this manifest declares applicable is refused
/// rather than silently counted as a non-blocking skip.
pub fn build(lane: Lane, runs: &[FrameworkRun<'_>]) -> Result<Summary, SummaryError> {
    let mut frameworks = Vec::with_capacity(runs.len());
    let mut totals = Counters::default();
    for run in runs {
        let mut counters = Counters {
            selected: run.selected.len(),
            attempted: run.observed.len(),
            ..Counters::default()
        };
        let mut cases = Vec::with_capacity(run.observed.len());
        for observed in &run.observed {
            let evaluated = run
                .manifest
                .evaluate_case(&observed.observation)
                .map_err(|error| SummaryError::Parse(error.to_string()))?;
            let mut cells = Vec::with_capacity(evaluated.len());
            let mut case_clean = true;
            for (entry, evaluation) in evaluated {
                let terminal = observed.observation.terminal(entry.dimension);
                let observed_class = terminal.class();
                count_cell(&mut counters, entry.expected_state, evaluation, terminal);
                let cell_succeeded = match terminal {
                    Terminal::Class { class, .. } => !class.is_failure(),
                    Terminal::NotApplicable { .. } => true,
                    Terminal::NotRun { .. } => false,
                };
                if !cell_succeeded {
                    case_clean = false;
                }
                cells.push(CellRow {
                    probe_id: entry.probe_id.clone(),
                    dimension: entry.dimension,
                    expected_state: entry.expected_state,
                    expected_class: entry.expected_class,
                    observed_class,
                    evaluation,
                });
            }
            if case_clean {
                counters.passed += 1;
            }
            cases.push(CaseRow {
                case_id: observed.case_id.clone(),
                framework: run.manifest.framework,
                request_digest: observed.request_digest.clone(),
                elapsed_ns: observed.elapsed_ns,
                cells,
            });
        }
        cases.sort_by(|left, right| left.case_id.cmp(&right.case_id));
        add(&mut totals, &counters);
        frameworks.push(FrameworkSummary {
            framework: run.manifest.framework,
            external_revision: run.manifest.external_revision.as_str().to_string(),
            request_template: request::REQUEST_VUE.to_string(),
            template_digest: request::template_digest(),
            counters,
            cases,
        });
    }
    let summary = Summary {
        schema_version: SCHEMA_VERSION,
        lane,
        frameworks,
        totals,
    };
    summary.validate()?;
    Ok(summary)
}

fn count_cell(
    counters: &mut Counters,
    expected_state: ExpectedState,
    evaluation: Evaluation,
    terminal: &Terminal,
) {
    match evaluation {
        Evaluation::GateRegression => counters.gated_regressions += 1,
        Evaluation::Skipped => counters.skips += 1,
        Evaluation::Xpass => counters.xpass_candidates += 1,
        Evaluation::CanaryExpected => {
            if let Some(class) = terminal.class() {
                *counters
                    .canary_failures
                    .entry(class.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
        Evaluation::KnownFailExpected => {
            if let Some(class) = terminal.class() {
                *counters
                    .known_failures
                    .entry(class.as_str().to_string())
                    .or_insert(0) += 1;
            }
        }
        Evaluation::CanaryRegression | Evaluation::UnrelatedRegression => {
            // A regression against a non-gate expectation is reported through
            // its own observed class below; it never becomes a gate here.
            let bucket = match expected_state {
                ExpectedState::KnownFail => &mut counters.known_failures,
                _ => &mut counters.canary_failures,
            };
            if let Some(class) = terminal.class() {
                *bucket.entry(class.as_str().to_string()).or_insert(0) += 1;
            }
        }
        Evaluation::GatePass
        | Evaluation::ObservedPass
        | Evaluation::NotRun
        | Evaluation::NotApplicable => {}
    }
    match terminal.class() {
        Some(ProbeOutcomeClass::Crash) => counters.crashes += 1,
        Some(ProbeOutcomeClass::Timeout) => counters.timeouts += 1,
        Some(ProbeOutcomeClass::HarnessFailure) => counters.harness_failures += 1,
        _ => {}
    }
}
