//! Compact query outcomes: a `Ready` value plus a shared evidence handle,
//! or a closed incomplete reason.
//!
//! Evidence records live in a cold intern; a complete direct Empty/One
//! read retains the handle without allocating a per-read vector or a new
//! `Arc`. Reserved ids cover empty diagnostics, no recovery, and
//! genuinely context-free evidence. An empty proof is never claimed for a
//! ready result that recorded dependencies.

use super::{QueryError, QueryResult, ResultCompleteness};

/// Handle of one interned [`OutcomeEvidence`] record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutcomeEvidenceId(u32);

/// Handle of a dependency-proof intern record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DependencyProofId(u32);

/// Handle of a location-independent diagnostic-recipe set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DiagnosticRecipeSetId(u32);

/// Handle of recovery provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RecoveryProvenanceId(u32);

/// Genuinely context-free evidence (no proof, no diagnostics, no recovery).
pub const CONTEXT_FREE_EVIDENCE: OutcomeEvidenceId = OutcomeEvidenceId(0);
/// Empty diagnostic-recipe set.
pub const EMPTY_DIAGNOSTICS: DiagnosticRecipeSetId = DiagnosticRecipeSetId(0);
/// No recovery provenance.
pub const NO_RECOVERY: RecoveryProvenanceId = RecoveryProvenanceId(0);
/// Empty dependency proof. Ready results that recorded dependencies must
/// not use this id.
pub const EMPTY_PROOF: DependencyProofId = DependencyProofId(0);

/// Shared immutable outcome metadata. Not embedded or cloned at each read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutcomeEvidence {
    pub proof: DependencyProofId,
    pub diagnostics: DiagnosticRecipeSetId,
    pub recovery: RecoveryProvenanceId,
}

/// A complete outcome: the value plus a compact evidence handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Ready<T> {
    pub value: T,
    pub evidence: OutcomeEvidenceId,
}

/// Closed incomplete-reason set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IncompleteReason {
    Cancelled,
    Budget,
    UnsettledInput,
    Unsupported,
    UnresolvedObligation,
}

/// Compact query outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryOutcome<T> {
    Ready(Ready<T>),
    Incomplete(IncompleteReason),
}

impl OutcomeEvidenceId {
    /// Context-free reserved id, or a non-zero id for recorded evidence.
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn as_u32(self) -> u32 {
        self.0
    }

    #[must_use]
    pub const fn is_context_free(self) -> bool {
        self.0 == CONTEXT_FREE_EVIDENCE.0
    }
}

impl DependencyProofId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == EMPTY_PROOF.0
    }
}

impl DiagnosticRecipeSetId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == EMPTY_DIAGNOSTICS.0
    }
}

impl RecoveryProvenanceId {
    #[must_use]
    pub const fn from_raw(id: u32) -> Self {
        Self(id)
    }

    #[must_use]
    pub const fn is_none(self) -> bool {
        self.0 == NO_RECOVERY.0
    }
}

impl OutcomeEvidence {
    /// Reserved context-free record: empty proof, empty diagnostics, no
    /// recovery. Used only when the result has no recorded dependencies.
    #[must_use]
    pub const fn context_free() -> Self {
        Self {
            proof: EMPTY_PROOF,
            diagnostics: EMPTY_DIAGNOSTICS,
            recovery: NO_RECOVERY,
        }
    }
}

impl IncompleteReason {
    /// Map a typed query error onto the closed incomplete set.
    #[must_use]
    pub fn from_query_error(error: &QueryError) -> Self {
        match error {
            QueryError::Cancelled => Self::Cancelled,
            QueryError::BudgetExceeded(_) => Self::Budget,
            QueryError::UnsupportedIntrinsic { .. } => Self::Unsupported,
            QueryError::Miss
            | QueryError::UnstableState { .. }
            | QueryError::SignatureOverflow
            | QueryError::ForeignSemanticOperand
            | QueryError::StaleSemanticOperand
            | QueryError::IncompleteSemanticOperand { .. } => Self::UnsettledInput,
            _ => Self::UnresolvedObligation,
        }
    }
}

impl<T> QueryResult<T> {
    /// Map this result onto a compact outcome without allocating a per-read
    /// vector or cloning an `Arc`. Complete values retain `evidence`;
    /// incomplete errors become the closed reason set.
    #[must_use]
    pub fn to_outcome(&self, evidence: OutcomeEvidenceId) -> QueryOutcome<&T> {
        match self {
            QueryResult::Value(value) => QueryOutcome::Ready(Ready { value, evidence }),
            QueryResult::Recursive(_) => {
                QueryOutcome::Incomplete(IncompleteReason::UnresolvedObligation)
            }
            QueryResult::Error(error) => {
                QueryOutcome::Incomplete(IncompleteReason::from_query_error(error))
            }
        }
    }
}

impl ResultCompleteness {
    /// Complete results map to `Ready`; partial results are incomplete.
    #[must_use]
    pub fn is_ready(self) -> bool {
        matches!(self, ResultCompleteness::Complete)
    }
}
