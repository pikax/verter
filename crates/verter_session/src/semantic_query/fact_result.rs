//! The result contract of one demanded fact.
//!
//! A fact is the answer to one specific semantic question under the
//! request's fixed view: a member DOMAIN (which members exist), a member's
//! PRESENCE, a member's VALUE type, a runtime classification, an authored
//! splice. Each is answered on its own, so a complete member domain can hold
//! a member whose value type is unavailable, and an authored splice can be
//! complete while the semantic evaluation of its members failed.
//!
//! [`FactResult`] is the one authority for how good an answer is:
//!
//! - [`FactResult::Complete`]: the exact demanded fact, established under
//!   the pinned checker semantics. A checker-deferred conditional, a proven
//!   absence (`Complete(None)`), and a legitimately empty member domain are
//!   all complete answers.
//! - [`FactResult::Approximate`]: the producer has a meaningful
//!   representation of the fact with explicitly bounded limitations, named
//!   by its causes. What an approximation permits is the DOMAIN's contract
//!   (an approximate member domain is a lower-bound enumeration: every
//!   member is real, an omission proves nothing), never an exact answer.
//! - [`FactResult::Unavailable`]: the producer cannot supply such a
//!   representation. Verter's inability to evaluate something is never a
//!   TypeScript `unknown`, a negative relation, or a proven absence.
//!
//! Availability is the ARM, never a policy over the causes: causes explain
//! (diagnostics, the legacy summary projection) and a consumer never
//! decides from them whether a value exists. Work that was not demanded is
//! not `Unavailable`; it has no result at all.
//!
//! Cancellation and a superseded view are not fact results: they abort the
//! computation ([`ExecutionAbort`]) and publish nothing.
//!
//! A result is stored and reused together with the dependency evidence
//! that validates it and how far it may be reused, so a request-table hit,
//! a warm hit and a singleflight follower hand the reader the same result
//! as the cold computation.
//!
//! Composition is explicit. There is no "worst status of everything that
//! ran" rule: [`FactResult::and_then`] and [`FactResult::zip`] are STRICT
//! compositions for a derived fact that needs its inputs, and a producer
//! whose rule establishes independence from an input simply does not
//! compose with it.

use super::{PartialReasonSet, ResultCompleteness};
use crate::typeinfo::surface_resolution::NonEmptyReasons;

/// The result of one demanded fact.
#[must_use]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactResult<T> {
    /// The exact demanded fact.
    Complete(T),
    /// A meaningful representation with bounded limitations.
    Approximate {
        /// The representation, valid only under its domain's approximation
        /// contract.
        value: T,
        /// Why the representation is not exact. Non-empty by type.
        causes: NonEmptyReasons,
    },
    /// No representation of the fact.
    Unavailable {
        /// Why the fact is unavailable. Non-empty by type.
        causes: NonEmptyReasons,
    },
}

/// The status of a [`FactResult`], without its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactStatus {
    /// The exact demanded fact.
    Complete,
    /// A bounded approximation, and why.
    Approximate(NonEmptyReasons),
    /// No representation, and why.
    Unavailable(NonEmptyReasons),
}

impl<T> FactResult<T> {
    /// A complete answer.
    pub const fn complete(value: T) -> Self {
        Self::Complete(value)
    }

    /// A bounded approximation of the fact.
    pub const fn approximate(value: T, causes: NonEmptyReasons) -> Self {
        Self::Approximate { value, causes }
    }

    /// No representation of the fact.
    pub const fn unavailable(causes: NonEmptyReasons) -> Self {
        Self::Unavailable { causes }
    }

    /// The status of this result.
    #[must_use]
    pub fn status(&self) -> FactStatus {
        match self {
            Self::Complete(_) => FactStatus::Complete,
            Self::Approximate { causes, .. } => FactStatus::Approximate(*causes),
            Self::Unavailable { causes } => FactStatus::Unavailable(*causes),
        }
    }

    /// The exact answer: `Some` only for [`Self::Complete`]. An
    /// approximation is never an exact value.
    #[must_use]
    pub fn exact(&self) -> Option<&T> {
        match self {
            Self::Complete(value) => Some(value),
            Self::Approximate { .. } | Self::Unavailable { .. } => None,
        }
    }

    /// The exact answer by value: `Some` only for [`Self::Complete`].
    #[must_use]
    pub fn into_exact(self) -> Option<T> {
        match self {
            Self::Complete(value) => Some(value),
            Self::Approximate { .. } | Self::Unavailable { .. } => None,
        }
    }

    /// Why the answer is not exact; `None` for a complete answer.
    #[must_use]
    pub fn causes(&self) -> Option<NonEmptyReasons> {
        match self {
            Self::Complete(_) => None,
            Self::Approximate { causes, .. } | Self::Unavailable { causes } => Some(*causes),
        }
    }

    /// Borrow the value, keeping the status.
    pub fn as_ref(&self) -> FactResult<&T> {
        match self {
            Self::Complete(value) => FactResult::Complete(value),
            Self::Approximate { value, causes } => FactResult::Approximate {
                value,
                causes: *causes,
            },
            Self::Unavailable { causes } => FactResult::Unavailable { causes: *causes },
        }
    }

    /// Transform the value, keeping the status: `f` must preserve the
    /// domain's meaning (an approximate input stays approximate).
    pub fn map<U>(self, f: impl FnOnce(T) -> U) -> FactResult<U> {
        match self {
            Self::Complete(value) => FactResult::Complete(f(value)),
            Self::Approximate { value, causes } => FactResult::Approximate {
                value: f(value),
                causes,
            },
            Self::Unavailable { causes } => FactResult::Unavailable { causes },
        }
    }

    /// STRICT dependent composition: the fact `f` derives from this one.
    /// An unavailable input leaves the derived fact unavailable; an
    /// approximate input makes a complete derivation approximate. Causes
    /// accumulate.
    pub fn and_then<U>(self, f: impl FnOnce(T) -> FactResult<U>) -> FactResult<U> {
        match self {
            Self::Complete(value) => f(value),
            Self::Approximate { value, causes } => f(value).degraded_by(causes),
            Self::Unavailable { causes } => FactResult::Unavailable { causes },
        }
    }

    /// STRICT conjunction: a fact that needs both inputs. Complete only when
    /// both are; unavailable when either is; otherwise approximate. Causes
    /// accumulate from every non-complete input.
    pub fn zip<U>(self, other: FactResult<U>) -> FactResult<(T, U)> {
        match (self, other) {
            (FactResult::Complete(left), FactResult::Complete(right)) => {
                FactResult::Complete((left, right))
            }
            (left, right) => {
                let causes = union_causes(left.causes(), right.causes())
                    .expect("a non-complete pair has at least one cause");
                match (left, right) {
                    (
                        FactResult::Complete(left) | FactResult::Approximate { value: left, .. },
                        FactResult::Complete(right) | FactResult::Approximate { value: right, .. },
                    ) => FactResult::Approximate {
                        value: (left, right),
                        causes,
                    },
                    _ => FactResult::Unavailable { causes },
                }
            }
        }
    }

    /// Degrade this result by an input's limitation: complete becomes
    /// approximate, and the causes join.
    fn degraded_by(self, causes: NonEmptyReasons) -> Self {
        match self {
            Self::Complete(value) => Self::Approximate { value, causes },
            Self::Approximate { value, causes: own } => Self::Approximate {
                value,
                causes: own.union(causes),
            },
            Self::Unavailable { causes: own } => Self::Unavailable {
                causes: own.union(causes),
            },
        }
    }

    /// The legacy summary of this one result: complete, or partial with its
    /// causes. A DERIVED view for the boundaries that still speak
    /// [`ResultCompleteness`]; never an authority of its own.
    #[must_use]
    pub fn completeness(&self) -> ResultCompleteness {
        self.status().completeness()
    }
}

impl FactStatus {
    /// Whether the fact is exact.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Complete)
    }

    /// The legacy summary of this status.
    #[must_use]
    pub fn completeness(self) -> ResultCompleteness {
        match self {
            Self::Complete => ResultCompleteness::Complete,
            Self::Approximate(causes) | Self::Unavailable(causes) => {
                ResultCompleteness::Partial(causes.get())
            }
        }
    }
}

fn union_causes(
    left: Option<NonEmptyReasons>,
    right: Option<NonEmptyReasons>,
) -> Option<NonEmptyReasons> {
    match (left, right) {
        (Some(left), Some(right)) => Some(left.union(right)),
        (one, None) | (None, one) => one,
    }
}

/// The legacy summary of the facts a published output is made of: complete
/// only when every one of them is. Derived at the boundary that publishes
/// those facts, from exactly the facts it publishes.
#[must_use]
pub fn summarize<'a>(statuses: impl IntoIterator<Item = &'a FactStatus>) -> ResultCompleteness {
    let mut reasons = PartialReasonSet::empty();
    let mut partial = false;
    for status in statuses {
        if let FactStatus::Approximate(causes) | FactStatus::Unavailable(causes) = status {
            partial = true;
            reasons = reasons.union(causes.get());
        }
    }
    if partial {
        ResultCompleteness::Partial(reasons)
    } else {
        ResultCompleteness::Complete
    }
}

/// Which members a surface has.
///
/// The domain's OPENNESS is part of a complete answer, not a degradation:
/// an open domain (an open spread, an unbound generic's constraint surface)
/// is described exactly by its present members, and an omission from it
/// proves nothing. An APPROXIMATE member domain is always presence-only:
/// a lower-bound enumeration of members that are real.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberDomain<T> {
    /// Every member: an omission is absence.
    Closed(T),
    /// The members present in an open domain: an omission proves nothing.
    Open(T),
    /// There is no such surface (a primitive, a union, a function where a
    /// one-level object surface was demanded): the complete negative answer.
    NoSurface,
}

/// Why a computation publishes no answer at all.
///
/// Not a fact status: an aborted computation's partial work is discarded,
/// and whoever demanded it retries or gives up. It never reaches a cache or
/// an output as an approximation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionAbort {
    /// The request was cancelled.
    Cancelled,
    /// The view the computation read was superseded, or was torn, before
    /// it could publish.
    Superseded,
    /// The host shut down before the computation could publish.
    Shutdown,
}

impl ExecutionAbort {
    /// Whether the view the computation read was superseded (a newer view
    /// answers the same request), as opposed to the request being cancelled
    /// or the host shutting down.
    #[must_use]
    pub const fn is_superseded(self) -> bool {
        matches!(self, Self::Superseded)
    }

    /// The abort a finished computation observed in its accumulated
    /// completeness: a cancelled read, or a read of a superseded or torn
    /// view. Such a computation publishes nothing.
    ///
    /// A bridge: cancellation and supersession still travel through the
    /// accumulator as reason classes until they get their own channel, and
    /// this is the one place that reads them back as an abort.
    #[must_use]
    pub(crate) fn observed_in(completeness: ResultCompleteness) -> Option<Self> {
        let reasons = completeness.reasons();
        if reasons.contains(PartialReasonSet::CANCELLED) {
            Some(Self::Cancelled)
        } else if reasons.contains(PartialReasonSet::SUPERSEDED_GENERATION)
            || reasons.contains(PartialReasonSet::UNSTABLE_STATE)
        {
            Some(Self::Superseded)
        } else {
            None
        }
    }
}
