//! The typed reuse rail: how far a completed cold compute may travel.
//!
//! A cold compute produces a value together with a [`ReuseClass`]:
//!
//! * [`ReuseClass::Shared`] — complete, nothing refused. Safe for shared
//!   publication and for request-scoped reuse.
//! * [`ReuseClass::RequestOnly`] — complete and deterministic under the
//!   request's immutable view, but a DETERMINISTIC non-cacheable read was
//!   consumed. Reusable within the request; never publishable. Every
//!   cold return, request-memo hit and singleflight follower of such a
//!   value must replay the stored refusal ([`ReuseClass::refusal_replay`])
//!   into the enclosing tracer before returning.
//! * [`ReuseClass::NoReuse`] — a TRANSIENT refusal, an unattributed
//!   refusal, an incomplete result, or a non-reproducible miss. Not safe
//!   even for request-scoped reuse.
//!
//! ## Why the reason has to be observed, not inferred
//!
//! The fact tracer records a BOOLEAN plus a
//! [`NonCacheablePropagation`]: enough to refuse a shared-cache
//! admission, not enough to decide whether the value stays usable for
//! the rest of the request. A fenced serve and a broken decl-body lease
//! both set that boolean, and they classify differently — the fenced
//! serve produced a definite answer from a superseded artifact (stable
//! for this request's view), the lease miss produced a degraded answer
//! that a later demand under a live lease would improve.
//!
//! So the engine's marking chokepoint also records its TYPED reason into
//! every active refusal-observation scope on the thread — the same fan-out
//! shape the tracer stack uses, so an inner producer's refusal is observed
//! by every enclosing scope that consumes its value. This module holds only
//! the pure vocabulary and operations; applying a refusal to the running
//! thread's tracers and scopes belongs to the engine that owns them.
//!
//! ## Dominance
//!
//! A scope that observes several refusals keeps ONE ([`dominant_refusal`]).
//! A transient refusal DOMINATES a deterministic one (the conservative
//! direction: a value whose basis includes a transient miss must not be
//! frozen for the request), and within a class the FIRST observation wins
//! so the recorded cause is the earliest, not the last.

use crate::facts::fact_read_set::NonCacheablePropagation;

/// Whether a complete result whose compute consumed a given
/// non-cacheable read stays DETERMINISTIC under the request's immutable
/// view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RequestReuse {
    /// The read produced a definite answer for this view. Re-running it
    /// inside the same request world reproduces the same value, so the
    /// result may be reused within the request as long as its refusal
    /// travels with it.
    Deterministic,
    /// The read produced a DEGRADED answer that a later demand may
    /// improve (a broken lease, a budget stop, a structural preparation
    /// failure). Freezing it for the request would make a recoverable
    /// miss permanent, so such a result is never request-memoised.
    Transient,
}

/// A typed, movable non-cacheable refusal: the exact reason a compute's
/// basis refuses shared admission, plus the propagation that reason
/// selects.
///
/// The pair is what a REUSED value returns to its caller. Tracer
/// finalisation exposes only the boolean; this exposes why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonCacheableRefusal {
    reason: NonCacheableReadReason,
}

impl NonCacheableRefusal {
    #[inline]
    pub fn new(reason: NonCacheableReadReason) -> Self {
        Self { reason }
    }

    /// The exact marking-site discriminant. Fixture-facing: the reuse
    /// DECISION is made by [`classify_reuse`] and its replay is described
    /// by [`ReuseClass::refusal_replay`], so only a discriminating test
    /// needs to read the reason back out.
    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn reason(&self) -> NonCacheableReadReason {
        self.reason
    }

    /// The propagation the reason selects. DERIVED, never stored
    /// alongside the reason: a stored copy could drift from the reason's
    /// own policy the moment a reason's propagation changes.
    /// Fixture-facing, for the same reason as [`Self::reason`].
    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn propagation(&self) -> NonCacheablePropagation {
        self.reason.propagation()
    }
}

/// Why a completed compute is not safe even for request-scoped reuse.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoReuseCause {
    /// A TRANSIENT non-cacheable read (see [`RequestReuse::Transient`]).
    TransientRefusal(NonCacheableReadReason),
    /// The compute was refused with NO typed reason: a fact-signature
    /// overflow, a mutation-instability verdict, or a propagation an
    /// executor captured straight off its tracer. The cause is
    /// unattributed, so the conservative class is the only sound one —
    /// but the PROPAGATION is still carried, because a reuse of such a
    /// value (a singleflight follower joining a refused rendezvous) must
    /// still taint its own reader.
    UnattributedRefusal(NonCacheablePropagation),
    /// The result is not complete (cancelled, partial, budget-truncated).
    Incomplete,
    /// A negative result (an absence) whose basis makes the absence
    /// itself non-reproducible — a miss concluded from a superseded
    /// surface. Reusing it would answer "nothing here" for a subject
    /// that has live content.
    NonReproducibleMiss,
}

/// How far a completed compute may be reused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReuseClass {
    Shared,
    RequestOnly(NonCacheableRefusal),
    NoReuse(NoReuseCause),
}

impl ReuseClass {
    /// `true` only for [`Self::Shared`] — the sole class a shared /
    /// persistent cache may admit.
    #[inline]
    pub fn is_shared(&self) -> bool {
        matches!(self, Self::Shared)
    }

    /// `true` for [`Self::Shared`] and [`Self::RequestOnly`] — the
    /// classes a request-scoped memo may hold.
    #[inline]
    pub fn is_request_reusable(&self) -> bool {
        matches!(self, Self::Shared | Self::RequestOnly(_))
    }

    /// The stored refusal, for a [`Self::RequestOnly`] value.
    /// Fixture-facing: production replays through
    /// [`Self::refusal_replay`] rather than unwrapping the class.
    #[cfg(any(test, feature = "test-support"))]
    #[inline]
    pub fn request_only_refusal(&self) -> Option<&NonCacheableRefusal> {
        match self {
            Self::RequestOnly(refusal) => Some(refusal),
            Self::Shared | Self::NoReuse(_) => None,
        }
    }

    /// The refusal EVERY return of a reused value — cold, memo hit,
    /// singleflight follower — must re-apply to its reader, or `None` when
    /// the class replays nothing. Pure: the engine owning the running
    /// thread's tracers applies it.
    ///
    /// Replaying is idempotent: the tracer records a monotone
    /// strongest-propagation and a refusal scope keeps its dominant reason,
    /// so replaying on the cold return (where the original fan-out already
    /// fired on this thread) changes nothing, while replaying on a memo hit
    /// or a singleflight follower — where it did NOT fire on this thread —
    /// is the only thing that keeps the taint attached to the value.
    #[inline]
    #[must_use]
    pub fn refusal_replay(&self) -> Option<RefusalReplay> {
        match self {
            Self::RequestOnly(refusal) => Some(RefusalReplay::Reason(refusal.reason)),
            Self::NoReuse(NoReuseCause::TransientRefusal(reason)) => {
                Some(RefusalReplay::Reason(*reason))
            }
            Self::NoReuse(NoReuseCause::UnattributedRefusal(propagation)) => {
                Some(RefusalReplay::Propagation(*propagation))
            }
            Self::Shared
            | Self::NoReuse(NoReuseCause::Incomplete)
            | Self::NoReuse(NoReuseCause::NonReproducibleMiss) => None,
        }
    }

    /// Lift a propagation an executor captured straight off its tracer
    /// into the reuse rail. The reason is unavailable at that seam — the
    /// tracer records only the propagation — so the result is the
    /// conservative unattributed class that still replays.
    #[inline]
    pub fn from_captured_propagation(propagation: Option<NonCacheablePropagation>) -> Self {
        match propagation {
            None => Self::Shared,
            Some(propagation) => Self::NoReuse(NoReuseCause::UnattributedRefusal(propagation)),
        }
    }
}

/// The refusal a reused value re-applies to its reader
/// ([`ReuseClass::refusal_replay`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefusalReplay {
    /// A typed reason: recorded into every refusal-observation scope and
    /// fanned out with the propagation the reason selects.
    Reason(NonCacheableReadReason),
    /// An unattributed refusal: only its propagation reaches the tracers.
    Propagation(NonCacheablePropagation),
}

/// What a producer's refusal-observation scope saw, folded with any
/// cacheability-scope verdict the producer also holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservedRefusal {
    /// Nothing refused.
    None,
    /// A typed reason reached the scope through the marking chokepoint.
    Typed(NonCacheableReadReason),
    /// A cacheability scope reported non-cacheable while no typed reason
    /// was recorded — a fact-signature overflow or a mutation-instability
    /// verdict.
    Unattributed,
}

/// Classify a completed compute.
///
/// `complete` is the caller's own completeness verdict; an incomplete
/// result is [`NoReuseCause::Incomplete`] regardless of what was
/// observed, because a partial value must never be frozen for the
/// request.
pub fn classify_reuse(observed: ObservedRefusal, complete: bool) -> ReuseClass {
    if !complete {
        return ReuseClass::NoReuse(NoReuseCause::Incomplete);
    }
    match observed {
        ObservedRefusal::None => ReuseClass::Shared,
        ObservedRefusal::Unattributed => ReuseClass::NoReuse(NoReuseCause::UnattributedRefusal(
            NonCacheablePropagation::Transitive,
        )),
        ObservedRefusal::Typed(reason) => match reason.request_reuse() {
            RequestReuse::Deterministic => {
                ReuseClass::RequestOnly(NonCacheableRefusal::new(reason))
            }
            RequestReuse::Transient => ReuseClass::NoReuse(NoReuseCause::TransientRefusal(reason)),
        },
    }
}

/// Keep the more restrictive of two observed reasons; on a tie keep the
/// FIRST observed so the recorded cause is the earliest one.
#[inline]
#[must_use]
pub fn dominant_refusal(
    existing: NonCacheableReadReason,
    incoming: NonCacheableReadReason,
) -> NonCacheableReadReason {
    match (existing.request_reuse(), incoming.request_reuse()) {
        (RequestReuse::Transient, _) => existing,
        (RequestReuse::Deterministic, RequestReuse::Transient) => incoming,
        (RequestReuse::Deterministic, RequestReuse::Deterministic) => existing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deterministic_reason_classifies_request_only_and_a_transient_one_does_not() {
        assert_eq!(
            classify_reuse(
                ObservedRefusal::Typed(NonCacheableReadReason::FencedServe),
                true
            ),
            ReuseClass::RequestOnly(NonCacheableRefusal::new(
                NonCacheableReadReason::FencedServe
            )),
            "a fenced serve produced a definite answer for this view — reusable within \
             the request, never publishable"
        );
        assert_eq!(
            classify_reuse(
                ObservedRefusal::Typed(NonCacheableReadReason::LeaseMiss),
                true
            ),
            ReuseClass::NoReuse(NoReuseCause::TransientRefusal(
                NonCacheableReadReason::LeaseMiss
            )),
            "a broken decl-body lease is recoverable on a later demand — freezing it for \
             the request would make a transient miss permanent"
        );
    }

    #[test]
    fn an_unattributed_refusal_and_an_incomplete_result_are_never_reusable() {
        assert_eq!(
            classify_reuse(ObservedRefusal::Unattributed, true),
            ReuseClass::NoReuse(NoReuseCause::UnattributedRefusal(
                NonCacheablePropagation::Transitive
            )),
        );
        assert_eq!(
            classify_reuse(ObservedRefusal::None, false),
            ReuseClass::NoReuse(NoReuseCause::Incomplete),
            "completeness dominates: a partial value is never frozen for the request even \
             when nothing was refused"
        );
        assert_eq!(
            classify_reuse(ObservedRefusal::None, true),
            ReuseClass::Shared
        );
    }

    #[test]
    fn a_transient_refusal_dominates_a_deterministic_one_in_either_order() {
        assert_eq!(
            dominant_refusal(
                NonCacheableReadReason::FencedServe,
                NonCacheableReadReason::LeaseMiss
            ),
            NonCacheableReadReason::LeaseMiss,
            "a transient refusal must UPGRADE a deterministic one"
        );
        assert_eq!(
            dominant_refusal(
                NonCacheableReadReason::LeaseMiss,
                NonCacheableReadReason::FencedServe
            ),
            NonCacheableReadReason::LeaseMiss,
            "a deterministic refusal must never DOWNGRADE an already-transient one"
        );
        assert_eq!(
            dominant_refusal(
                NonCacheableReadReason::LeaseMiss,
                NonCacheableReadReason::PreparationFailure
            ),
            NonCacheableReadReason::LeaseMiss,
            "within a class the FIRST observation is kept"
        );
    }

    #[test]
    fn every_class_describes_its_replay() {
        let fenced = NonCacheableRefusal::new(NonCacheableReadReason::FencedServe);
        assert_eq!(
            ReuseClass::RequestOnly(fenced).refusal_replay(),
            Some(RefusalReplay::Reason(NonCacheableReadReason::FencedServe))
        );
        assert_eq!(
            ReuseClass::NoReuse(NoReuseCause::TransientRefusal(
                NonCacheableReadReason::LeaseMiss
            ))
            .refusal_replay(),
            Some(RefusalReplay::Reason(NonCacheableReadReason::LeaseMiss))
        );
        assert_eq!(
            ReuseClass::NoReuse(NoReuseCause::UnattributedRefusal(
                NonCacheablePropagation::LocalOnly
            ))
            .refusal_replay(),
            Some(RefusalReplay::Propagation(
                NonCacheablePropagation::LocalOnly
            ))
        );
        for silent in [
            ReuseClass::Shared,
            ReuseClass::NoReuse(NoReuseCause::Incomplete),
            ReuseClass::NoReuse(NoReuseCause::NonReproducibleMiss),
        ] {
            assert_eq!(silent.refusal_replay(), None, "{silent:?} replays nothing");
        }
    }
}

/// A typed reason a read was NON-CACHEABLE — the discriminant a marking
/// site passes to the engine's non-cacheable read fan-out.
///
/// The tracer records only a boolean (any non-cacheable read refuses
/// shared-cache admission for the enclosing compute); the reason is a
/// structural, self-documenting signal at the marking site so the
/// non-cacheability class is closed by TYPED dispatch rather than an
/// untyped "mark suppress" call. Orthogonal to completeness: marking a
/// read non-cacheable never makes the result `Partial`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NonCacheableReadReason {
    /// A FENCED (ReturnOnly, `store_published == false`) `IndexedReady`
    /// serve consumed inside the tracer scope: the payload was computed
    /// from a served-without-publication (superseded) artifact.
    FencedServe,
    /// A broken decl-body lease pin (`DemandOutcome::LeaseMiss` /
    /// `PreparedDeclOutcome::LeaseMiss` / `LocatorBodyDerefError::LeaseMiss`):
    /// the demanded body did not lower and produced nothing — a TRANSIENT
    /// no-warm signal, recoverable on a later demand under a live lease.
    LeaseMiss,
    /// An unrootable / unadmitted import route: the served value's basis
    /// cannot be soundly fact-rooted for warm admission.
    UnrootableRoute,
    /// An unobservable contributor source-env identity: the exact artifact
    /// key the read served from is unavailable, so the result cannot be
    /// fact-rooted.
    UnobservableSource,
    /// A local semantic-inference safety budget stopped before producing an
    /// authoritative declaration result.
    InferenceBudgetExceeded,
    /// Declaration preparation failed with a typed structural failure. The
    /// failed slot remains vacant and any Option-shaped caller may serve the
    /// failure only as ReturnOnly; it must never publish it as real absence.
    PreparationFailure,
    /// A terminal output-materialization LOST typed degradation: a
    /// present-but-unraisable fold (`None`) or a non-empty degradation
    /// sidecar discarded at the capability-gated terminal unwrap. The
    /// compat tree may still be published for display, but the compute must
    /// never be warm-admitted as a complete result. Orthogonal to
    /// completeness: the result is NOT partial, it is non-cacheable.
    OutputMaterializationLoss,
}

impl NonCacheableReadReason {
    /// A non-cacheable READ taints the derivation basis, rather than merely
    /// declining retention in one cache family, so every current read class
    /// propagates through all enclosing cold-compute scopes.
    #[inline]
    #[must_use]
    pub fn propagation(self) -> crate::facts::fact_read_set::NonCacheablePropagation {
        match self {
            Self::FencedServe
            | Self::LeaseMiss
            | Self::UnrootableRoute
            | Self::UnobservableSource
            | Self::InferenceBudgetExceeded
            | Self::PreparationFailure
            | Self::OutputMaterializationLoss => {
                crate::facts::fact_read_set::NonCacheablePropagation::Transitive
            }
        }
    }

    /// Whether a COMPLETE result whose compute consumed this read stays
    /// deterministic under the request's immutable view — the axis the
    /// [`ReuseClass`](super::reuse::ReuseClass) splits on.
    ///
    /// Exhaustive by design: a new reason cannot be added without
    /// deciding whether a value built on it may be reused inside the
    /// request. Defaulting a new arm either way is exactly the silent
    /// mistake this match exists to prevent — the permissive default
    /// freezes a recoverable miss, the conservative one re-runs a
    /// deterministic compute on every touch.
    #[inline]
    pub(crate) fn request_reuse(self) -> super::reuse::RequestReuse {
        match self {
            // The payload is a definite answer for this view: a
            // superseded artifact's content, a route whose basis cannot
            // be fact-rooted, a contributor whose source-env identity is
            // unobservable. Re-running inside the same request world
            // reproduces it, so only PUBLICATION is refused.
            Self::FencedServe | Self::UnrootableRoute | Self::UnobservableSource => {
                super::reuse::RequestReuse::Deterministic
            }
            // The payload is DEGRADED and a later demand may improve it:
            // a broken decl-body lease recovers under a live lease, a
            // safety-budget stop and a structural preparation failure
            // both leave a slot vacant rather than answering it.
            Self::LeaseMiss | Self::InferenceBudgetExceeded | Self::PreparationFailure => {
                super::reuse::RequestReuse::Transient
            }
            // A materialization loss is a definite answer under the
            // request's immutable view: the same fold loses the same
            // degradation sidecar on every re-run, so only warm
            // PUBLICATION is refused, never intra-request reuse.
            Self::OutputMaterializationLoss => super::reuse::RequestReuse::Deterministic,
        }
    }
}
