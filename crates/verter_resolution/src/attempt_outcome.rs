//! Completed kernel attempts carry their operation-owned output.

use verter_session_query::resolution::AttemptOutcome;

/// A successfully completed kernel attempt's answer, paired with the
/// [`crate::AttemptOutput`] it accumulated along the way.
///
/// The only envelope that publishes an [`crate::AttemptOutput`] with a completed
/// kernel answer.
/// `AttemptOutcome::Complete(T)` itself stays UNCHANGED — this wrapper
/// exists at the TOP-LEVEL kernel entry point ([`KernelAttempt`]), not on
/// [`crate::ResolverObservation`]'s 13 inbound query methods, which have no
/// outbound effects of their own to accumulate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedAttempt<T> {
    pub value: T,
    pub output: crate::AttemptOutput,
}

impl<T> CompletedAttempt<T> {
    #[must_use]
    pub const fn new(value: T, output: crate::AttemptOutput) -> Self {
        Self { value, output }
    }
}

/// The top-level kernel attempt envelope: [`AttemptOutcome`] specialized
/// so a successful attempt carries its accumulated
/// [`crate::AttemptOutput`] alongside the answer.
/// `NeedInputs`/`Terminal` carry no output — an attempt that does not
/// reach `Complete` discards everything it accumulated (contract §4: no
/// torn/partial output is ever promoted).
pub type KernelAttempt<T> = AttemptOutcome<CompletedAttempt<T>>;

#[cfg(test)]
#[path = "attempt_outcome_tests.rs"]
mod attempt_outcome_tests;
