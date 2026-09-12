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

use crate::evaluate::{decide, Evaluation, ObservedOutcome};
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
    ///
    /// EXPECTED failures only. A canary that changed class counts in
    /// `canary_regressions` instead, so "the canaries are behaving as
    /// recorded" and "a canary's class changed" are never the same number.
    pub canary_failures: BTreeMap<String, usize>,
    /// Known-fail cells that met their expected failure class, by class.
    ///
    /// EXPECTED failures only, on the same reasoning as `canary_failures`; a
    /// known-fail that changed class counts in `unrelated_regressions`.
    pub known_failures: BTreeMap<String, usize>,
    /// `pass` canaries that observed a failure, by the class they observed.
    pub canary_regressions: BTreeMap<String, usize>,
    /// Failure canaries and known-fails that observed a DIFFERENT failure
    /// class than the one recorded, by the class they observed.
    pub unrelated_regressions: BTreeMap<String, usize>,
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
    pub const REQUIRED: [&'static str; 13] = [
        "selected",
        "attempted",
        "passed",
        "gated_regressions",
        "canary_failures",
        "known_failures",
        "canary_regressions",
        "unrelated_regressions",
        "skips",
        "xpass_candidates",
        "crashes",
        "timeouts",
        "harness_failures",
    ];
}

/// Which terminal a cell's dimension reached.
///
/// Recorded because "no class" is two different facts: a dimension that could
/// not be exercised at all, and one that was never reached because a lower one
/// failed. A reader recomputing the counters from the cell rows — which is
/// what makes the counters auditable rather than self-declared — cannot tell
/// them apart from `observed_class: null`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservedTerminal {
    /// An outcome class, carried in `observed_class`.
    Class,
    /// The dimension was never reached.
    NotRun,
    /// The dimension cannot be exercised for this framework.
    NotApplicable,
}

impl ObservedTerminal {
    fn of(terminal: &Terminal) -> Self {
        match terminal {
            Terminal::Class { .. } => ObservedTerminal::Class,
            Terminal::NotRun { .. } => ObservedTerminal::NotRun,
            Terminal::NotApplicable { .. } => ObservedTerminal::NotApplicable,
        }
    }
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
    /// Which terminal the dimension reached.
    pub observed_terminal: ObservedTerminal,
    /// The class observed, when the dimension produced one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_class: Option<ProbeOutcomeClass>,
    /// The evaluation.
    pub evaluation: Evaluation,
}

impl CellRow {
    /// Whether this cell actually succeeded — not whether it met its
    /// expectation. A recorded known failure is behaving as declared and is
    /// still not a success.
    fn succeeded(&self) -> bool {
        match self.observed_terminal {
            ObservedTerminal::Class => self.observed_class.is_some_and(|class| !class.is_failure()),
            ObservedTerminal::NotApplicable => true,
            ObservedTerminal::NotRun => false,
        }
    }

    /// What this row observed, or `None` when its two observation fields
    /// contradict each other. A `class` terminal carries its class and the
    /// other two carry none; a row that says otherwise is malformed, and
    /// anything recomputed from it would look honest without being so.
    fn observed(&self) -> Option<ObservedOutcome> {
        match (self.observed_terminal, self.observed_class) {
            (ObservedTerminal::Class, Some(class)) => Some(ObservedOutcome::Class(class)),
            (ObservedTerminal::NotRun, None) => Some(ObservedOutcome::NotRun),
            (ObservedTerminal::NotApplicable, None) => Some(ObservedOutcome::NotApplicable),
            _ => None,
        }
    }
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
    /// The canonical request template this framework's cases issued, verbatim.
    pub request_template: String,
    /// The template's SHA-256.
    pub template_digest: String,
    /// The case ids the lane selected, in order.
    ///
    /// Recorded so the attempted-versus-selected claim can be AUDITED from the
    /// document instead of trusted: a run that observed one case twice and
    /// skipped another has the right count and the wrong work.
    pub selected_cases: Vec<String>,
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
    /// The document carries no framework block at all, so its all-zero totals
    /// would dispose clean having reported nothing.
    NoFrameworks,
    /// A framework's attempted case set is not its selection, whatever the
    /// counts say.
    SelectionMismatch {
        /// The framework.
        framework: String,
        /// What the disagreement is.
        detail: String,
    },
    /// A counter does not equal what the document's own cell rows add up to.
    CounterMismatch {
        /// The framework.
        framework: String,
        /// The counter.
        counter: String,
    },
    /// A cell row's terminal and class contradict each other.
    MalformedCell {
        /// The case.
        probe_id: String,
        /// The dimension.
        dimension: Dimension,
    },
    /// A cell row's evaluation is not the one its own expectation and
    /// observation decide.
    EvaluationMismatch {
        /// The case.
        probe_id: String,
        /// The dimension.
        dimension: Dimension,
        /// What the row claims.
        declared: Evaluation,
        /// What its fields decide.
        decided: Evaluation,
    },
    /// An observation could not be evaluated against the manifest that
    /// produced it — a runner defect, not a malformed document.
    Observation(String),
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
            SummaryError::NoFrameworks => {
                f.write_str("the summary carries no framework block at all")
            }
            SummaryError::SelectionMismatch { framework, detail } => {
                write!(f, "{framework}: {detail}")
            }
            SummaryError::CounterMismatch { framework, counter } => write!(
                f,
                "{framework}: counter `{counter}` is not what the cell rows add up to"
            ),
            SummaryError::MalformedCell {
                probe_id,
                dimension,
            } => write!(
                f,
                "{probe_id} [{dimension}]: the cell's terminal and observed class disagree"
            ),
            SummaryError::EvaluationMismatch {
                probe_id,
                dimension,
                declared,
                decided,
            } => write!(
                f,
                "{probe_id} [{dimension}]: the cell claims `{declared}`, but its own \
                 expectation and observation decide `{decided}`"
            ),
            SummaryError::Observation(message) => {
                write!(f, "an observation could not be evaluated: {message}")
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
    /// Every counter is RECOMPUTED from the cell rows the document carries and
    /// compared with what it declared. Without that, the disposition reads a
    /// self-declared number: a document whose rows hold gate regressions and
    /// whose counter claims zero would be accepted and disposed clean, which is
    /// precisely the inconsistency this read-back exists to catch.
    ///
    /// The inventory check runs in BOTH directions and over the case ids, not
    /// only their count: a framework that attempted fewer cases than it
    /// selected has silently skipped work, one that attempted more has run work
    /// no manifest ratified, and one that observed a case twice has the right
    /// count and the wrong work.
    pub fn validate(&self) -> Result<(), SummaryError> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(SummaryError::UnknownSchema {
                schema_version: self.schema_version,
            });
        }
        if self.frameworks.is_empty() {
            // All-zero totals are internally consistent with no blocks at all,
            // so the same fail-open `EmptySelection` closes per framework has
            // to be closed here too.
            return Err(SummaryError::NoFrameworks);
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
            check_selection(block)?;
            let recounted = recount(block)?;
            compare_counters(&name, &block.counters, &recounted)?;
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
            "| framework | selected | attempted | passed | gated regressions | \
             canary failures | known failures | canary regressions | unrelated regressions | \
             skips | XPASS candidates | crashes | timeouts | harness failures |"
        );
        let _ = writeln!(
            out,
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | \
             ---: | ---: | ---: |"
        );
        for block in &self.frameworks {
            row(&mut out, block.framework.as_str(), &block.counters);
        }
        row(&mut out, "**total**", &self.totals);
        let _ = writeln!(out);
        for block in &self.frameworks {
            let _ = writeln!(
                out,
                "`{}` pinned at `{}`, request template digest `{}`.",
                block.framework.as_str(),
                block.external_revision,
                block.template_digest
            );
        }
        let by_class = [
            &self.totals.canary_failures,
            &self.totals.known_failures,
            &self.totals.canary_regressions,
            &self.totals.unrelated_regressions,
        ];
        if by_class.iter().any(|map| !map.is_empty()) {
            let _ = writeln!(out, "\nBy outcome class:\n");
            let _ = writeln!(
                out,
                "| class | canary | known-fail | canary regression | unrelated regression |"
            );
            let _ = writeln!(out, "| --- | ---: | ---: | ---: | ---: |");
            let mut classes: Vec<&String> = by_class.iter().flat_map(|map| map.keys()).collect();
            classes.sort();
            classes.dedup();
            for class in classes {
                let _ = writeln!(
                    out,
                    "| {class} | {} | {} | {} | {} |",
                    by_class[0].get(class).copied().unwrap_or(0),
                    by_class[1].get(class).copied().unwrap_or(0),
                    by_class[2].get(class).copied().unwrap_or(0),
                    by_class[3].get(class).copied().unwrap_or(0),
                );
            }
        }
        out
    }
}

fn row(out: &mut String, name: &str, counters: &Counters) {
    let _ = writeln!(
        out,
        "| {name} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} | {} |",
        counters.selected,
        counters.attempted,
        counters.passed,
        counters.gated_regressions,
        counters.canary_failures.values().sum::<usize>(),
        counters.known_failures.values().sum::<usize>(),
        counters.canary_regressions.values().sum::<usize>(),
        counters.unrelated_regressions.values().sum::<usize>(),
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
    for (map, source) in [
        (&mut totals.canary_failures, &counters.canary_failures),
        (&mut totals.known_failures, &counters.known_failures),
        (&mut totals.canary_regressions, &counters.canary_regressions),
        (
            &mut totals.unrelated_regressions,
            &counters.unrelated_regressions,
        ),
    ] {
        for (class, count) in source {
            *map.entry(class.clone()).or_insert(0) += count;
        }
    }
}

/// Check a block's attempted case set IS its selection — the same ids, each
/// exactly once.
fn check_selection(block: &FrameworkSummary) -> Result<(), SummaryError> {
    let framework = block.framework.as_str().to_string();
    if block.selected_cases.len() != block.counters.selected {
        return Err(SummaryError::SelectionMismatch {
            framework,
            detail: format!(
                "{} selected case ids recorded for {} selected",
                block.selected_cases.len(),
                block.counters.selected
            ),
        });
    }
    let selected: BTreeMap<&str, usize> = tally(block.selected_cases.iter().map(String::as_str));
    let attempted: BTreeMap<&str, usize> = tally(block.cases.iter().map(|case| &*case.case_id));
    if let Some((case_id, _)) = attempted.iter().find(|(_, count)| **count > 1) {
        return Err(SummaryError::SelectionMismatch {
            framework,
            detail: format!("`{case_id}` was attempted more than once"),
        });
    }
    if selected != attempted {
        let missing: Vec<&str> = selected
            .keys()
            .filter(|case_id| !attempted.contains_key(*case_id))
            .copied()
            .collect();
        let extra: Vec<&str> = attempted
            .keys()
            .filter(|case_id| !selected.contains_key(*case_id))
            .copied()
            .collect();
        return Err(SummaryError::SelectionMismatch {
            framework,
            detail: format!(
                "the attempted case set is not the selection; selected but not attempted: \
                 [{}]; attempted but not selected: [{}]",
                missing.join(", "),
                extra.join(", ")
            ),
        });
    }
    Ok(())
}

fn tally<'a>(ids: impl Iterator<Item = &'a str>) -> BTreeMap<&'a str, usize> {
    let mut counts = BTreeMap::new();
    for id in ids {
        *counts.entry(id).or_insert(0) += 1;
    }
    counts
}

/// Recompute a block's counters from the cell rows it carries.
///
/// `selected` and `attempted` are copied rather than derived: they are the
/// lane's selection and its work, which `check_selection` and the row-count
/// check already pin against the recorded case ids.
fn recount(block: &FrameworkSummary) -> Result<Counters, SummaryError> {
    let mut counters = Counters {
        selected: block.counters.selected,
        attempted: block.counters.attempted,
        ..Counters::default()
    };
    for case in &block.cases {
        let mut case_clean = true;
        for cell in &case.cells {
            let Some(observed) = cell.observed() else {
                return Err(SummaryError::MalformedCell {
                    probe_id: cell.probe_id.clone(),
                    dimension: cell.dimension,
                });
            };
            // Re-DECIDED, not re-counted from what the row claims. Counting a
            // declared evaluation answers "do these counters add up", which a
            // document that miswrote one cell's verdict and the counters to
            // match still passes — and a gate cell claiming `gate_pass` beside
            // observations that say otherwise is exactly what the lane's one
            // real exit must never read as clean. The evaluation is a pure
            // function of the three fields beside it, so the rule that wrote
            // it is the rule that checks it.
            let decided = decide(cell.expected_state, cell.expected_class, observed);
            if decided != cell.evaluation {
                return Err(SummaryError::EvaluationMismatch {
                    probe_id: cell.probe_id.clone(),
                    dimension: cell.dimension,
                    declared: cell.evaluation,
                    decided,
                });
            }
            count_cell(&mut counters, decided, cell.observed_class);
            if !cell.succeeded() {
                case_clean = false;
            }
        }
        if case_clean {
            counters.passed += 1;
        }
    }
    Ok(counters)
}

fn compare_counters(
    framework: &str,
    declared: &Counters,
    recounted: &Counters,
) -> Result<(), SummaryError> {
    if declared == recounted {
        return Ok(());
    }
    let differing = [
        ("passed", declared.passed != recounted.passed),
        (
            "gated_regressions",
            declared.gated_regressions != recounted.gated_regressions,
        ),
        (
            "canary_failures",
            declared.canary_failures != recounted.canary_failures,
        ),
        (
            "known_failures",
            declared.known_failures != recounted.known_failures,
        ),
        (
            "canary_regressions",
            declared.canary_regressions != recounted.canary_regressions,
        ),
        (
            "unrelated_regressions",
            declared.unrelated_regressions != recounted.unrelated_regressions,
        ),
        ("skips", declared.skips != recounted.skips),
        (
            "xpass_candidates",
            declared.xpass_candidates != recounted.xpass_candidates,
        ),
        ("crashes", declared.crashes != recounted.crashes),
        ("timeouts", declared.timeouts != recounted.timeouts),
        (
            "harness_failures",
            declared.harness_failures != recounted.harness_failures,
        ),
    ];
    let counter = differing
        .iter()
        .find(|(_, differs)| *differs)
        .map(|(name, _)| *name)
        .unwrap_or("selected");
    Err(SummaryError::CounterMismatch {
        framework: framework.to_string(),
        counter: counter.to_string(),
    })
}

/// One framework's evaluated run, ready to be folded into a summary.
pub struct FrameworkRun<'a> {
    /// The framework's validated manifest.
    pub manifest: &'a ProbeStateManifest,
    /// The canonical request template this framework's cases issued, verbatim.
    ///
    /// Carried by the RUN, not read from a crate constant inside the builder:
    /// a summary that stamped one framework's template onto another's block
    /// would publish a request its cases never sent, with a digest that agreed
    /// with itself.
    pub request_template: &'a str,
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
                .map_err(|error| SummaryError::Observation(error.to_string()))?;
            let mut cells = Vec::with_capacity(evaluated.len());
            for (entry, evaluation) in evaluated {
                let terminal = observed.observation.terminal(entry.dimension);
                cells.push(CellRow {
                    probe_id: entry.probe_id.clone(),
                    dimension: entry.dimension,
                    expected_state: entry.expected_state,
                    expected_class: entry.expected_class,
                    observed_terminal: ObservedTerminal::of(terminal),
                    observed_class: terminal.class(),
                    evaluation,
                });
            }
            // Counted from the ROWS, so the published counters and the
            // published rows cannot disagree — the same derivation
            // `Summary::validate` re-runs on read.
            let mut case_clean = true;
            for cell in &cells {
                count_cell(&mut counters, cell.evaluation, cell.observed_class);
                if !cell.succeeded() {
                    case_clean = false;
                }
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
            request_template: run.request_template.to_string(),
            template_digest: request::sha256_hex(run.request_template.as_bytes()),
            selected_cases: run.selected.clone(),
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

/// Count one cell.
///
/// A regression NEVER lands in the expected-failure buckets: they answer "are
/// the recorded failures still the recorded failures", and a class change is
/// exactly the thing that question must not absorb. It still never becomes a
/// gate — only a gate cell does that.
fn count_cell(
    counters: &mut Counters,
    evaluation: Evaluation,
    observed_class: Option<ProbeOutcomeClass>,
) {
    let bucket = match evaluation {
        Evaluation::GateRegression => {
            counters.gated_regressions += 1;
            None
        }
        Evaluation::Skipped => {
            counters.skips += 1;
            None
        }
        Evaluation::Xpass => {
            counters.xpass_candidates += 1;
            None
        }
        Evaluation::CanaryExpected => Some(&mut counters.canary_failures),
        Evaluation::KnownFailExpected => Some(&mut counters.known_failures),
        Evaluation::CanaryRegression => Some(&mut counters.canary_regressions),
        Evaluation::UnrelatedRegression => Some(&mut counters.unrelated_regressions),
        Evaluation::GatePass
        | Evaluation::ObservedPass
        | Evaluation::NotRun
        | Evaluation::NotApplicable => None,
    };
    if let (Some(bucket), Some(class)) = (bucket, observed_class) {
        *bucket.entry(class.as_str().to_string()).or_insert(0) += 1;
    }
    match observed_class {
        Some(ProbeOutcomeClass::Crash) => counters.crashes += 1,
        Some(ProbeOutcomeClass::Timeout) => counters.timeouts += 1,
        Some(ProbeOutcomeClass::HarnessFailure) => counters.harness_failures += 1,
        _ => {}
    }
}
