//! The one place an expectation meets an observation.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::manifest::{ExpectedState, ProbeEntry};
use crate::outcome::{CaseObservation, ProbeOutcomeClass, Terminal};

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

impl ProbeEntry {
    /// Evaluate this cell against its case's observation, reading only the
    /// terminal of the cell's own dimension.
    ///
    /// An expected class is met only by that exact class: a `crash`,
    /// `timeout`, or `harness_failure` never satisfies an expected
    /// diagnostic, refusal, or unsupported outcome. A cell without an
    /// expected class (which validation rejects for every state but `skip`)
    /// never matches.
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
        let terminal = observation.terminal(self.dimension);
        match self.expected_state {
            ExpectedState::Skip => Evaluation::Skipped,
            ExpectedState::Gate => match (terminal.class(), self.expected_class) {
                (Some(observed), Some(expected)) if observed == expected => Evaluation::GatePass,
                _ => Evaluation::GateRegression,
            },
            ExpectedState::Canary | ExpectedState::KnownFail => {
                let observed = match terminal {
                    Terminal::NotRun { .. } => return Evaluation::NotRun,
                    Terminal::NotApplicable { .. } => return Evaluation::NotApplicable,
                    Terminal::Class { class, .. } => *class,
                };
                self.evaluate_non_gate(observed)
            }
        }
    }

    fn evaluate_non_gate(&self, observed: ProbeOutcomeClass) -> Evaluation {
        let pass = ProbeOutcomeClass::Pass;
        match (self.expected_state, self.expected_class) {
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
}
