//! The one place an expectation meets an observation.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::manifest::{ExpectedState, ProbeEntry, ProbeStateManifest};
use crate::outcome::{CaseObservation, InvalidObservation, ProbeOutcomeClass, Terminal};

/// The closed result of evaluating one manifest cell against one case
/// observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Evaluation {
    /// A gate observed exactly its expected class.
    GatePass,
    /// A gate observed anything else, including a not-run or not-applicable
    /// dimension. The only blocking evaluation.
    GateRegression,
    /// A canary observed its expected failure class.
    CanaryExpected,
    /// A `pass` canary observed `pass`. Non-gating: implies no acceptance.
    ObservedPass,
    /// A `pass` canary observed a failure.
    CanaryRegression,
    /// A known-fail observed its expected failure class.
    KnownFailExpected,
    /// A failure canary or known-fail observed `pass`: a promotion candidate,
    /// never an automatic gate.
    Xpass,
    /// A failure canary or known-fail observed a different failure class.
    UnrelatedRegression,
    /// The cell is a skip.
    Skipped,
    /// A non-gate cell whose dimension was never reached.
    NotRun,
    /// A non-gate cell whose dimension cannot be exercised for its framework.
    NotApplicable,
}

impl Evaluation {
    /// Whether this evaluation blocks the lane. Only a gate can block.
    pub const fn blocks(self) -> bool {
        matches!(self, Evaluation::GateRegression)
    }

    /// The serialized name.
    pub const fn as_str(self) -> &'static str {
        match self {
            Evaluation::GatePass => "gate_pass",
            Evaluation::GateRegression => "gate_regression",
            Evaluation::CanaryExpected => "canary_expected",
            Evaluation::ObservedPass => "observed_pass",
            Evaluation::CanaryRegression => "canary_regression",
            Evaluation::KnownFailExpected => "known_fail_expected",
            Evaluation::Xpass => "xpass",
            Evaluation::UnrelatedRegression => "unrelated_regression",
            Evaluation::Skipped => "skipped",
            Evaluation::NotRun => "not_run",
            Evaluation::NotApplicable => "not_applicable",
        }
    }
}

impl fmt::Display for Evaluation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a cell observed, reduced to exactly what the decision reads.
///
/// A [`Terminal`] also carries the evidence behind the outcome and the reason
/// a dimension went unreached or unexercised. None of that decides anything,
/// and a decision that could see it could come to depend on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObservedOutcome {
    /// The dimension produced this class.
    Class(ProbeOutcomeClass),
    /// The dimension was never reached.
    NotRun,
    /// The dimension cannot be exercised for this framework.
    NotApplicable,
}

impl ObservedOutcome {
    /// What a terminal observed.
    pub fn of(terminal: &Terminal) -> Self {
        match terminal {
            Terminal::Class { class, .. } => ObservedOutcome::Class(*class),
            Terminal::NotRun { .. } => ObservedOutcome::NotRun,
            Terminal::NotApplicable { .. } => ObservedOutcome::NotApplicable,
        }
    }
}

/// Decide one cell from its expectation and its observation, and nothing else.
///
/// This is a pure function of the three values a published cell row carries,
/// which is what lets the summary's read-back RE-DERIVE an evaluation instead
/// of trusting the one written beside it. A second implementation over there
/// would be a second authority, and two of those diverge; a document whose
/// gate cell claims `gate_pass` beside observations that say otherwise has to
/// be refused by the same rule that wrote it.
///
/// An expected class is met only by that exact class: a `crash`, `timeout`, or
/// `harness_failure` never satisfies an expected diagnostic, refusal, or
/// unsupported outcome. A cell without an expected class (which validation
/// rejects for every state but `skip`) never matches.
pub fn decide(
    expected_state: ExpectedState,
    expected_class: Option<ProbeOutcomeClass>,
    observed: ObservedOutcome,
) -> Evaluation {
    match expected_state {
        ExpectedState::Skip => Evaluation::Skipped,
        ExpectedState::Gate => match (observed, expected_class) {
            (ObservedOutcome::Class(observed), Some(expected)) if observed == expected => {
                Evaluation::GatePass
            }
            _ => Evaluation::GateRegression,
        },
        ExpectedState::Canary | ExpectedState::KnownFail => {
            let observed = match observed {
                ObservedOutcome::NotRun => return Evaluation::NotRun,
                ObservedOutcome::NotApplicable => return Evaluation::NotApplicable,
                ObservedOutcome::Class(class) => class,
            };
            decide_non_gate(expected_state, expected_class, observed)
        }
    }
}

fn decide_non_gate(
    expected_state: ExpectedState,
    expected_class: Option<ProbeOutcomeClass>,
    observed: ProbeOutcomeClass,
) -> Evaluation {
    let pass = ProbeOutcomeClass::Pass;
    match (expected_state, expected_class) {
        (ExpectedState::Canary, Some(expected)) if expected == pass => {
            if observed == pass {
                Evaluation::ObservedPass
            } else {
                Evaluation::CanaryRegression
            }
        }
        (_, Some(_)) if observed == pass => Evaluation::Xpass,
        (ExpectedState::Canary, Some(expected)) if expected == observed => {
            Evaluation::CanaryExpected
        }
        (ExpectedState::KnownFail, Some(expected)) if expected == observed => {
            Evaluation::KnownFailExpected
        }
        _ => Evaluation::UnrelatedRegression,
    }
}

impl ProbeEntry {
    /// Evaluate this cell against its case's observation, reading only the
    /// terminal of the cell's own dimension.
    ///
    /// # Panics
    ///
    /// When the observation is of another case: pairing a cell with a
    /// foreign case is a runner defect, never an evaluation.
    pub fn evaluate(&self, observation: &CaseObservation) -> Evaluation {
        assert_eq!(
            observation.case_id(),
            self.probe_id,
            "a cell is evaluated only against its own case's observation"
        );
        decide(
            self.expected_state,
            self.expected_class,
            ObservedOutcome::of(observation.terminal(self.dimension)),
        )
    }
}

impl ProbeStateManifest {
    /// Evaluate every cell this manifest holds for `observation`'s case, once
    /// the observation's applicability agrees with what this manifest
    /// declares. Cells keep manifest order; a case this manifest does not
    /// carry evaluates to no cells.
    ///
    /// [`ProbeEntry::evaluate`] reads one terminal and never sees the
    /// manifest header, so a driver reporting [`Terminal::NotApplicable`] for
    /// a dimension this manifest declares applicable is classified as the
    /// non-blocking [`Evaluation::NotApplicable`]. That is the fail-open
    /// [`ProbeStateManifest::check_applicability`] exists to refuse, and
    /// nothing obliged a caller to run it first: a missing producer,
    /// executor, or validator could be read as "cannot be exercised here".
    /// Evaluating through the manifest makes the checked path the reachable
    /// one.
    ///
    /// # Errors
    ///
    /// [`InvalidObservation::ApplicabilityMismatch`] when the observation
    /// reports a dimension inapplicable where this manifest declares it
    /// applicable, or applicable where this manifest declares it not.
    pub fn evaluate_case<'a>(
        &'a self,
        observation: &CaseObservation,
    ) -> Result<Vec<(&'a ProbeEntry, Evaluation)>, InvalidObservation> {
        self.check_applicability(observation)?;
        Ok(self
            .entries
            .iter()
            .filter(|entry| entry.probe_id == observation.case_id())
            .map(|entry| (entry, entry.evaluate(observation)))
            .collect())
    }
}
