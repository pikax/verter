//! Solver result types: exactness model, execution status, and relation outcomes.
//!
//! These types replace the old `Exact | LowerBound | OpaqueFallback` completion
//! model with a semantically richer three-way exactness distinction plus
//! separate operational status tracking.

use std::fmt;

// ---------------------------------------------------------------------------
// Exactness
// ---------------------------------------------------------------------------

/// Semantic exactness of a solver result.
///
/// - `ExactConcrete`: fully materialized finite result.
/// - `ExactSymbolic`: exact but not finitely materialized (e.g. `Record<string, T>`,
///   open mapped types, recursive type identities).
/// - `Incomplete`: missing source, unsupported syntax, cancelled request, or
///   hard recursion-policy stop.
///
/// Important: "infinite keyspace" is `ExactSymbolic`, not `Incomplete`.
/// "Operator stayed symbolic" is `ExactSymbolic` if the symbolic form is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SolverExactness {
    ExactConcrete,
    ExactSymbolic,
    Incomplete,
}

impl SolverExactness {
    /// Returns `true` if the result is exact (concrete or symbolic).
    pub fn is_exact(self) -> bool {
        matches!(self, Self::ExactConcrete | Self::ExactSymbolic)
    }

    /// Merge two exactness values — the result is the "least exact" of the two.
    pub fn merge(self, other: Self) -> Self {
        match (self, other) {
            (Self::Incomplete, _) | (_, Self::Incomplete) => Self::Incomplete,
            (Self::ExactSymbolic, _) | (_, Self::ExactSymbolic) => Self::ExactSymbolic,
            (Self::ExactConcrete, Self::ExactConcrete) => Self::ExactConcrete,
        }
    }
}

impl fmt::Display for SolverExactness {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ExactConcrete => write!(f, "ExactConcrete"),
            Self::ExactSymbolic => write!(f, "ExactSymbolic"),
            Self::Incomplete => write!(f, "Incomplete"),
        }
    }
}

// ---------------------------------------------------------------------------
// Execution status
// ---------------------------------------------------------------------------

/// Operational status of a solver query, tracked separately from semantic
/// exactness.
///
/// This prevents operational interruption from being modeled as a semantic
/// approximation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ExecutionStatus {
    /// Query completed normally within all operational guards.
    Completed,
    /// Query was cancelled by the caller (e.g. request timeout).
    Cancelled,
    /// Query was interrupted by an operational guard (e.g. instantiation depth).
    Interrupted,
    /// Query hit a deterministic hard stop (e.g. template literal explosion).
    HardStop,
}

impl ExecutionStatus {
    pub fn is_completed(self) -> bool {
        self == Self::Completed
    }
}

impl fmt::Display for ExecutionStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => write!(f, "Completed"),
            Self::Cancelled => write!(f, "Cancelled"),
            Self::Interrupted => write!(f, "Interrupted"),
            Self::HardStop => write!(f, "HardStop"),
        }
    }
}

// ---------------------------------------------------------------------------
// Incomplete reason
// ---------------------------------------------------------------------------

/// Why a solver result is `Incomplete`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncompleteReason {
    /// Source file not available for the required declaration.
    MissingSource {
        canonical_id: String,
        symbol_name: String,
    },
    /// Syntax or operator not yet implemented in the solver.
    UnsupportedSyntax { description: String },
    /// Request was externally cancelled.
    Cancelled,
    /// Recursion policy hard stop — the recursive group requires a solver
    /// feature not yet implemented or convergence cannot be represented.
    RecursionPolicy { description: String },
}

impl fmt::Display for IncompleteReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSource {
                canonical_id,
                symbol_name,
            } => write!(f, "missing source: {}::{}", canonical_id, symbol_name),
            Self::UnsupportedSyntax { description } => {
                write!(f, "unsupported: {}", description)
            }
            Self::Cancelled => write!(f, "cancelled"),
            Self::RecursionPolicy { description } => {
                write!(f, "recursion policy: {}", description)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Solver diagnostic (non-semantic observability)
// ---------------------------------------------------------------------------

/// Non-semantic diagnostic emitted by the solver.
///
/// These diagnostics are for observability and do NOT affect `exactness` or
/// `execution_status`. They propagate through the expansion pipeline as
/// `ExpansionDiagnostic` entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolverDiagnostic {
    /// The conditional context was truncated because more frames were available
    /// than the capture limit allows.
    ConditionalContextTruncated {
        /// Total frames available before truncation.
        available: usize,
        /// Frames actually captured (≤ MAX_CONDITIONAL_CONTEXT_FRAMES).
        captured: usize,
    },
}

impl fmt::Display for SolverDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConditionalContextTruncated {
                available,
                captured,
            } => write!(
                f,
                "conditional context truncated: {} available, {} captured",
                available, captured
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// Solver result wrapper
// ---------------------------------------------------------------------------

/// Complete solver result: value + exactness + execution status + optional
/// incomplete reasons + non-semantic diagnostics.
#[derive(Debug, Clone)]
pub struct SolverResult<T> {
    pub value: T,
    pub exactness: SolverExactness,
    pub execution_status: ExecutionStatus,
    pub incomplete_reasons: Vec<IncompleteReason>,
    pub diagnostics: Vec<SolverDiagnostic>,
    /// Total resolve steps consumed by this solve. Available in production
    /// for audit/observability without requiring `RecordingAudit`.
    pub steps: u64,
}

impl<T> SolverResult<T> {
    /// Create a fully exact concrete result.
    pub fn exact_concrete(value: T) -> Self {
        Self {
            value,
            exactness: SolverExactness::ExactConcrete,
            execution_status: ExecutionStatus::Completed,
            incomplete_reasons: Vec::new(),
            diagnostics: Vec::new(),
            steps: 0,
        }
    }

    /// Create an exact symbolic result.
    pub fn exact_symbolic(value: T) -> Self {
        Self {
            value,
            exactness: SolverExactness::ExactSymbolic,
            execution_status: ExecutionStatus::Completed,
            incomplete_reasons: Vec::new(),
            diagnostics: Vec::new(),
            steps: 0,
        }
    }

    /// Create an incomplete result.
    pub fn incomplete(value: T, reason: IncompleteReason) -> Self {
        Self {
            value,
            exactness: SolverExactness::Incomplete,
            execution_status: ExecutionStatus::Completed,
            incomplete_reasons: vec![reason],
            diagnostics: Vec::new(),
            steps: 0,
        }
    }

    /// Create an incomplete result due to hard stop.
    pub fn hard_stop(value: T, reason: IncompleteReason) -> Self {
        Self {
            value,
            exactness: SolverExactness::Incomplete,
            execution_status: ExecutionStatus::HardStop,
            incomplete_reasons: vec![reason],
            diagnostics: Vec::new(),
            steps: 0,
        }
    }

    /// Map the value while preserving metadata.
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> SolverResult<U> {
        SolverResult {
            value: f(self.value),
            exactness: self.exactness,
            execution_status: self.execution_status,
            incomplete_reasons: self.incomplete_reasons,
            diagnostics: self.diagnostics,
            steps: self.steps,
        }
    }

    /// Merge metadata from another result (keeps the "least exact" status).
    pub fn merge_status<U>(&mut self, other: &SolverResult<U>) {
        self.exactness = self.exactness.merge(other.exactness);
        if other.execution_status != ExecutionStatus::Completed {
            self.execution_status = other.execution_status;
        }
        self.incomplete_reasons
            .extend(other.incomplete_reasons.iter().cloned());
        self.diagnostics.extend(other.diagnostics.iter().cloned());
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exactness_merge_picks_least_exact() {
        assert_eq!(
            SolverExactness::ExactConcrete.merge(SolverExactness::ExactConcrete),
            SolverExactness::ExactConcrete
        );
        assert_eq!(
            SolverExactness::ExactConcrete.merge(SolverExactness::ExactSymbolic),
            SolverExactness::ExactSymbolic
        );
        assert_eq!(
            SolverExactness::ExactSymbolic.merge(SolverExactness::Incomplete),
            SolverExactness::Incomplete
        );
        assert_eq!(
            SolverExactness::ExactConcrete.merge(SolverExactness::Incomplete),
            SolverExactness::Incomplete
        );
    }

    #[test]
    fn exactness_is_exact() {
        assert!(SolverExactness::ExactConcrete.is_exact());
        assert!(SolverExactness::ExactSymbolic.is_exact());
        assert!(!SolverExactness::Incomplete.is_exact());
    }

    #[test]
    fn solver_result_map_preserves_metadata() {
        let result = SolverResult::exact_symbolic(42);
        let mapped = result.map(|x| x.to_string());
        assert_eq!(mapped.value, "42");
        assert_eq!(mapped.exactness, SolverExactness::ExactSymbolic);
        assert_eq!(mapped.execution_status, ExecutionStatus::Completed);
    }

    #[test]
    fn solver_result_merge_status_picks_worst() {
        let mut a = SolverResult::exact_concrete(1);
        let b = SolverResult::incomplete(
            2,
            IncompleteReason::MissingSource {
                canonical_id: "foo".into(),
                symbol_name: "Bar".into(),
            },
        );
        a.merge_status(&b);
        assert_eq!(a.exactness, SolverExactness::Incomplete);
        assert_eq!(a.incomplete_reasons.len(), 1);
    }

    #[test]
    fn recording_context_truncation_does_not_change_exactness() {
        let mut result = SolverResult::exact_concrete(42);
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
        assert!(result.diagnostics.is_empty());

        // Record the diagnostic
        result
            .diagnostics
            .push(SolverDiagnostic::ConditionalContextTruncated {
                available: 12,
                captured: 8,
            });

        // Exactness and execution status must be unchanged
        assert_eq!(result.exactness, SolverExactness::ExactConcrete);
        assert_eq!(result.execution_status, ExecutionStatus::Completed);
        assert_eq!(result.diagnostics.len(), 1);
        assert_eq!(
            result.diagnostics[0],
            SolverDiagnostic::ConditionalContextTruncated {
                available: 12,
                captured: 8,
            }
        );
        // Negative: exactness is NOT downgraded to incomplete
        assert_ne!(result.exactness, SolverExactness::Incomplete);
    }

    #[test]
    fn merge_status_propagates_diagnostics() {
        let mut a = SolverResult::exact_concrete(1);
        let mut b = SolverResult::exact_symbolic(2);
        b.diagnostics
            .push(SolverDiagnostic::ConditionalContextTruncated {
                available: 10,
                captured: 8,
            });
        a.merge_status(&b);
        assert_eq!(a.diagnostics.len(), 1);
        assert_eq!(a.exactness, SolverExactness::ExactSymbolic);
    }
}
