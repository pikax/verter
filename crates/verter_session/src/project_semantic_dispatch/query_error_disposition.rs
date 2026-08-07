//! The single [`QueryError`] disposition authority.
//!
//! Every [`QueryError`] variant has exactly ONE disposition, and
//! [`classify_query_error`] is the ONE exhaustive match that assigns it.
//! Consumers never re-derive a classification by arm-matching `QueryError`
//! themselves: a second classifier diverges, and divergence is precisely the
//! erasure class this module exists to remove — a typed failure silently
//! answered as absence, or a control carrier published as a missing
//! dependency.
//!
//! The dispositions, and what each obliges a boundary to do:
//!
//! | Disposition | Variants | Obligation |
//! |---|---|---|
//! | [`OptionalAbsence`] | `Miss`, `RaiseMiss` | May become absence, but ONLY at a boundary that explicitly owns optional absence. Anywhere else it is a typed missing dependency. |
//! | [`RecursionCarrier`] | `RecursiveRef` | Raises AS a recursive reference — never an error, never absence. |
//! | [`ExpandableDecl`] | `DeclPlaceholder` | Raises as the expandable `Instantiate` carrier — never "not found". |
//! | [`ControlCarrier`] | `AliasCycle`, `RaiseAliasCycle`, `TypeParamCycle` | Stays a typed control sentinel; its payload (the `AliasCycle` participant chain) is preserved. |
//! | [`UnsupportedSurface`] | `UnrepresentableSurface`, `UnrepresentableSurfaceMember` | Typed unsupported-surface / unsupported-member result, retaining its existing sentinel output semantics. |
//! | [`Partial`] | `BudgetExceeded`, `Cancelled`, `UnstableState` | Typed partial — `ReturnOnly`, never shared, never warmed. |
//! | [`Failure`] | `Other`, `UnsupportedIntrinsic`, `ValueDomainMismatch` | Genuine typed failure (the §22 error type). |
//!
//! The classification also carries the PRECISE typed unresolved reason a
//! consumer publishes, because the disposition classes are deliberately
//! coarser than the published vocabulary: `BudgetExceeded`, `Cancelled` and
//! `UnstableState` share the `Partial` disposition but publish three different
//! reasons. Both come out of the same match, so the coarse class and the
//! precise reason cannot drift apart.
//!
//! [`OptionalAbsence`]: QueryErrorDisposition::OptionalAbsence
//! [`RecursionCarrier`]: QueryErrorDisposition::RecursionCarrier
//! [`ExpandableDecl`]: QueryErrorDisposition::ExpandableDecl
//! [`ControlCarrier`]: QueryErrorDisposition::ControlCarrier
//! [`UnsupportedSurface`]: QueryErrorDisposition::UnsupportedSurface
//! [`Partial`]: QueryErrorDisposition::Partial
//! [`Failure`]: QueryErrorDisposition::Failure

use verter_type_expr::{ClosedLiteralDomainUnresolvedReason, ReactiveWrapperUnresolvedReason};

use crate::semantic_query::QueryError;

/// The closed disposition vocabulary a [`QueryError`] carries across every
/// boundary. See the module docs for the per-variant obligations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum QueryErrorDisposition {
    /// "No result under this view." Legitimately renders as absence at a
    /// boundary that explicitly owns optional absence, and ONLY there.
    OptionalAbsence,
    /// A recursion back-edge. Raises as a recursive reference so the walk
    /// stops at the back-edge instead of recursing indefinitely.
    RecursionCarrier,
    /// A resolved-but-unmaterialized declaration. Raises as the expandable
    /// `Instantiate` carrier so the consumer can demand it on its own terms.
    ExpandableDecl,
    /// A walker / raise-boundary control sentinel whose typed payload
    /// (the participant chain) must survive the boundary.
    ControlCarrier,
    /// A surface (or surface member) the projection boundary cannot
    /// represent. Keeps its existing sentinel output semantics.
    UnsupportedSurface,
    /// A resource / completion-fence partial. `ReturnOnly`: answered to the
    /// caller, never published to a shared cache.
    Partial,
    /// A genuine typed failure — the §22 error type.
    Failure,
}

impl QueryErrorDisposition {
    /// Whether this disposition is the §22 ERROR TYPE — a genuine "this type
    /// IS an error" result, as opposed to a control / recursion / partial
    /// signal. Exactly [`Failure`](Self::Failure).
    #[must_use]
    pub(crate) const fn is_error_type(self) -> bool {
        matches!(self, Self::Failure)
    }

    /// Whether an interned `Opaque` carrier with this disposition would
    /// materialize as a failure `Unknown { raw }` shell. The two legitimately
    /// publishable carriers — a recursive reference and a declaration
    /// placeholder — are NOT failures: they raise to real published shapes
    /// (`TypeExpr::RecursiveRef` and the named `Ref` shell respectively).
    #[must_use]
    pub(crate) const fn is_unknown_materializing(self) -> bool {
        !matches!(self, Self::RecursionCarrier | Self::ExpandableDecl)
    }
}

/// The full classification of one [`QueryError`]: its Part-B disposition plus
/// the precise typed unresolved reason a consumer publishes for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct QueryErrorClass {
    /// The coarse obligation class. See [`QueryErrorDisposition`].
    pub(crate) disposition: QueryErrorDisposition,
    /// The precise closed-literal-domain unresolved reason.
    pub(crate) domain_reason: ClosedLiteralDomainUnresolvedReason,
}

impl QueryErrorClass {
    /// The reactive-wrapper unresolved reason mirroring
    /// [`Self::domain_reason`] over the sibling vocabulary. A total
    /// enum-to-enum mapping — it never re-inspects the `QueryError`.
    #[must_use]
    pub(crate) const fn wrapper_reason(self) -> ReactiveWrapperUnresolvedReason {
        match self.domain_reason {
            ClosedLiteralDomainUnresolvedReason::AnalysisUnavailable => {
                ReactiveWrapperUnresolvedReason::AnalysisUnavailable
            }
            ClosedLiteralDomainUnresolvedReason::RevisionMismatch => {
                ReactiveWrapperUnresolvedReason::RevisionMismatch
            }
            ClosedLiteralDomainUnresolvedReason::MissingDependency => {
                ReactiveWrapperUnresolvedReason::MissingDependency
            }
            ClosedLiteralDomainUnresolvedReason::Cycle => ReactiveWrapperUnresolvedReason::Cycle,
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded => {
                ReactiveWrapperUnresolvedReason::BudgetExceeded
            }
            ClosedLiteralDomainUnresolvedReason::WorkLimitExceeded => {
                ReactiveWrapperUnresolvedReason::WorkLimitExceeded
            }
            ClosedLiteralDomainUnresolvedReason::Cancelled => {
                ReactiveWrapperUnresolvedReason::Cancelled
            }
            ClosedLiteralDomainUnresolvedReason::Unsupported => {
                ReactiveWrapperUnresolvedReason::Unsupported
            }
            ClosedLiteralDomainUnresolvedReason::Fault => ReactiveWrapperUnresolvedReason::Fault,
        }
    }
}

/// THE exhaustive [`QueryError`] classification. A new `QueryError` variant
/// fails compilation here until its disposition and published reason are
/// stated; no other site in the crate arm-matches `QueryError` to classify it.
#[must_use]
pub(crate) const fn classify_query_error(err: &QueryError) -> QueryErrorClass {
    let (disposition, domain_reason) = match err {
        // "No result yet." Absence-eligible, but only where the boundary
        // explicitly owns optional absence; elsewhere it is a typed missing
        // dependency.
        QueryError::Miss | QueryError::RaiseMiss => (
            QueryErrorDisposition::OptionalAbsence,
            ClosedLiteralDomainUnresolvedReason::MissingDependency,
        ),
        // A recursion back-edge raises AS recursion. Reaching a failure sink
        // with one means a boundary declined to raise it; the honest published
        // reason for the sub-result is still the cycle it stopped at.
        QueryError::RecursiveRef { .. } => (
            QueryErrorDisposition::RecursionCarrier,
            ClosedLiteralDomainUnresolvedReason::Cycle,
        ),
        // A resolved-but-unmaterialized declaration raises as the expandable
        // `Instantiate` carrier. It is emphatically NOT a missing dependency:
        // the declaration WAS found, so publishing "missing" is the "not
        // found" answer the disposition table forbids. If it reaches a failure
        // sink at all, the boundary failed to expand it — a genuine fault.
        QueryError::DeclPlaceholder { .. } => (
            QueryErrorDisposition::ExpandableDecl,
            ClosedLiteralDomainUnresolvedReason::Fault,
        ),
        // Cycle sentinels: the walker-side chain carrier and the two
        // raise-boundary sentinels.
        QueryError::AliasCycle { .. }
        | QueryError::RaiseAliasCycle
        | QueryError::TypeParamCycle => (
            QueryErrorDisposition::ControlCarrier,
            ClosedLiteralDomainUnresolvedReason::Cycle,
        ),
        // Projection-boundary unrepresentable surface / member.
        QueryError::UnrepresentableSurface | QueryError::UnrepresentableSurfaceMember => (
            QueryErrorDisposition::UnsupportedSurface,
            ClosedLiteralDomainUnresolvedReason::Unsupported,
        ),
        // A projected surface whose member set is not closed-world: the
        // domain cannot be enumerated exactly, so the published reason is the
        // same unsupported-surface class, never a fault.
        QueryError::OpenSurface => (
            QueryErrorDisposition::UnsupportedSurface,
            ClosedLiteralDomainUnresolvedReason::Unsupported,
        ),
        // A position the flow substrate has no model for is a well-formed
        // "no resolved node", exactly like `Miss` — the flow rail already
        // folded its own partial rails at the consumer boundary.
        QueryError::UnmodeledPosition => (
            QueryErrorDisposition::OptionalAbsence,
            ClosedLiteralDomainUnresolvedReason::MissingDependency,
        ),
        // Resource / completion-fence control — typed partials, `ReturnOnly`.
        // The three publish DIFFERENT reasons: a budget trip, a cancellation
        // and a fence that gave up because the world moved under the read are
        // three distinct diagnostics.
        QueryError::BudgetExceeded(_) => (
            QueryErrorDisposition::Partial,
            ClosedLiteralDomainUnresolvedReason::BudgetExceeded,
        ),
        QueryError::Cancelled => (
            QueryErrorDisposition::Partial,
            ClosedLiteralDomainUnresolvedReason::Cancelled,
        ),
        // The completion fence exhausted its retry budget because the observed
        // inputs kept moving — a revision mismatch, not a fault in the query.
        QueryError::UnstableState { .. } => (
            QueryErrorDisposition::Partial,
            ClosedLiteralDomainUnresolvedReason::RevisionMismatch,
        ),
        // Genuine typed failures — the §22 error type.
        QueryError::UnsupportedIntrinsic { .. } => (
            QueryErrorDisposition::Failure,
            ClosedLiteralDomainUnresolvedReason::Unsupported,
        ),
        QueryError::Other(_) | QueryError::ValueDomainMismatch { .. } => (
            QueryErrorDisposition::Failure,
            ClosedLiteralDomainUnresolvedReason::Fault,
        ),
    };
    QueryErrorClass {
        disposition,
        domain_reason,
    }
}

/// The Part-B disposition of `err`. Thin projection of
/// [`classify_query_error`] for the boundaries that need only the class.
#[must_use]
pub(crate) const fn query_error_disposition(err: &QueryError) -> QueryErrorDisposition {
    classify_query_error(err).disposition
}

#[cfg(test)]
#[path = "query_error_disposition_tests.rs"]
mod query_error_disposition_tests;
