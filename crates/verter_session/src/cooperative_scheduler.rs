//! Cooperative drive adapter for nonthreaded scheduler execution.
//!
//! The browser closure runs the scheduler inline: no driver thread, no
//! worker pools — the calling thread pumps ready stages. A nonthreaded
//! worker therefore needs cooperative points where (a) a cancelled
//! request stops admitting further stage work, and (b) control can be
//! offered back to the embedding runtime between driven stages.
//!
//! [`CooperativeSchedulerAdapter`] is that seam, and ONLY that seam. It
//! owns no runtime, no queue, and no cache: it wraps the existing
//! execution contracts (`Scheduler::submit_request` +
//! `Scheduler::wait_or_drive`) with a [`CancellationToken`] and a
//! pluggable yield hook, and preserves their outcomes exactly whenever
//! it is not cancelled — an uncancellable adapter with the inline yield
//! hook is behaviourally identical to calling the scheduler directly.
//!
//! Cancellation never exposes partial results: the cancelled outcome
//! carries no value at the type level, so a half-driven request cannot
//! be mistaken for a complete-empty answer, and any snapshot a
//! cancelled drive did publish went through the existing
//! [`crate::input_basis::SnapshotFence`] admission unchanged.

use std::sync::Arc;

use verter_scheduler::cancellation::CancellationToken;
use verter_scheduler::job::{CompletionHandle, CompletionState, RequestResult};
use verter_scheduler::scheduler::{Request, Scheduler};

/// Decision returned by one cooperative yield point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum YieldDecision {
    /// The drive may continue.
    Continue,
    /// The embedding runtime withdrew the worker at the yield point.
    /// No further stages are driven by this adapter.
    Cancelled,
}

/// One cooperative yield point for a nonthreaded worker.
///
/// The embedding runtime installs the hook that returns control to it
/// (a browser worker schedules its continuation; a native host has
/// nothing to yield to). The hook is consulted at every cooperative
/// point of a drive — before the first driven stage and between
/// stages. The default [`InlineYield`] continues immediately.
pub trait CooperativeYield: Send + Sync + std::fmt::Debug {
    /// Offer one yield point. Pure: no scheduler state is touched.
    fn yield_to_runtime(&self) -> YieldDecision;
}

/// The no-op yield hook: every yield point continues. Native callers
/// and uncancellable paths use this.
#[derive(Debug, Default)]
pub struct InlineYield;

impl CooperativeYield for InlineYield {
    fn yield_to_runtime(&self) -> YieldDecision {
        YieldDecision::Continue
    }
}

/// Named cooperative point at which a drive stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CooperativePoint {
    /// Stopped before the request was submitted: the scheduler was
    /// never consulted, so nothing was admitted and no handle exists.
    BeforeSubmit,
    /// Stopped after submission, before the drive: the handle stays
    /// pending with the scheduler and is released by dropping it.
    BeforeDrive,
    /// Stopped at a yield point (before the first stage or between
    /// driven stages): the scheduler was not consulted for the next
    /// stage of this drive call. The handle stays pending; a later
    /// drive call resumes it.
    AtYield,
    /// Stopped on cancellation observed after at least one stage of
    /// this drive was pumped: no further stage is driven by this
    /// adapter, and the pending handle is released by dropping it.
    BetweenStages,
}

/// Typed refusal to continue a cooperative drive. Carries no result
/// value: partial work is never presented as an outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CooperativeStop {
    /// Where the drive stopped.
    pub point: CooperativePoint,
}

/// Submit outcome through the adapter. The submitted handle is not
/// `Debug` (the scheduler's handle type is opaque); match on the
/// variants rather than printing the value.
pub enum CooperativeSubmit {
    /// Submitted; drive the returned handle through
    /// [`CooperativeSchedulerAdapter::drive`].
    Submitted(CompletionHandle<RequestResult>),
    /// Refused before the scheduler was consulted.
    Refused(CooperativeStop),
}

/// Drive outcome through the adapter.
///
/// Not `PartialEq`: the scheduler's own `CompletionState` carries
/// result payloads that are compared by their own tests; discriminant
/// checks use [`Self::driven_ready`] / `matches!`.
#[derive(Debug, Clone)]
pub enum CooperativeDrive<T: Clone> {
    /// Not cancelled: exactly the state the existing
    /// `Scheduler::wait_or_drive` produced, unchanged.
    Driven(CompletionState<T>),
    /// Cancelled at a cooperative point. No result value is carried.
    Cancelled(CooperativeStop),
}

impl<T: Clone> CooperativeDrive<T> {
    /// Whether this outcome is a driven `Ready` state.
    #[must_use]
    pub fn driven_ready(&self) -> bool {
        matches!(self, Self::Driven(CompletionState::Ready(_)))
    }
}

/// The cooperative adapter over the existing execution contracts.
///
/// Default construction is uncancellable with the inline yield hook —
/// behaviourally identical to driving the scheduler directly.
pub struct CooperativeSchedulerAdapter {
    cancellation: CancellationToken,
    yield_hook: Arc<dyn CooperativeYield>,
}

impl std::fmt::Debug for CooperativeSchedulerAdapter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CooperativeSchedulerAdapter")
            .field("cancelled", &self.cancellation.is_cancelled())
            .finish_non_exhaustive()
    }
}

impl Default for CooperativeSchedulerAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl CooperativeSchedulerAdapter {
    /// Uncancellable adapter with the inline yield hook.
    #[must_use]
    pub fn new() -> Self {
        Self {
            cancellation: CancellationToken::new(),
            yield_hook: Arc::new(InlineYield),
        }
    }

    /// Adapter with an embedding-runtime yield hook. The hook is
    /// consulted at every cooperative point of a drive: before the
    /// first driven stage and between stages.
    #[must_use]
    pub fn with_yield_hook(hook: Arc<dyn CooperativeYield>) -> Self {
        Self {
            cancellation: CancellationToken::new(),
            yield_hook: hook,
        }
    }

    /// Adapter with an embedding-runtime yield hook AND a token the
    /// runtime owns separately from the adapter. An embedding runtime
    /// that hands its worker to `drive` keeps a clone of `cancellation`
    /// so it can withdraw the worker from outside (the browser
    /// closure's one cancellation authority); a hook that observes the
    /// same token reports stops between the stages it already drove.
    #[must_use]
    pub fn with_yield_hook_and_cancellation(
        hook: Arc<dyn CooperativeYield>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            cancellation,
            yield_hook: hook,
        }
    }

    /// The shared cancellation token. Cancelling it stops every later
    /// submit and drive through this adapter at its next cooperative
    /// point; already-driven work keeps its own completion semantics.
    #[must_use]
    pub fn cancellation(&self) -> &CancellationToken {
        &self.cancellation
    }

    /// Cancel the adapter. Idempotent.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    /// Submit through the existing `Scheduler::submit_request` unless
    /// already cancelled. A refused submit admits no work and creates
    /// no handle.
    pub fn submit(&self, scheduler: &Arc<Scheduler>, request: Request) -> CooperativeSubmit {
        if self.cancellation.is_cancelled() {
            return CooperativeSubmit::Refused(CooperativeStop {
                point: CooperativePoint::BeforeSubmit,
            });
        }
        CooperativeSubmit::Submitted(scheduler.submit_request(request))
    }

    /// Drive `handle` to its terminal state through the existing
    /// scheduler contracts, offering cooperative points before the
    /// first stage and between every driven stage.
    ///
    /// When the calling thread is the pump (no driver thread: WASM,
    /// sync schedulers), the adapter drives ONE ready stage per
    /// iteration through [`Scheduler::drive_one`] and consults
    /// cancellation plus the yield hook between stages, so a
    /// nonthreaded worker can withdraw mid-request; a later drive
    /// call resumes the same handle, so withdrawal never rewrites
    /// the request's eventual outcome. When a driver thread owns the
    /// pump (native threaded scheduler), the calling thread is NOT
    /// the pump: `drive_one` would dequeue and inline-execute
    /// scheduler stage work on the caller's thread, breaking the
    /// dual-pool isolation between host-coordinator threads and the
    /// scheduler's stage pools — so the adapter offers the pre-park
    /// cooperative point once and defers to
    /// [`Scheduler::wait_or_drive`] (parking on the driver)
    /// unchanged, never pumping stages itself. An uncancellable
    /// adapter with the inline yield hook is therefore behaviourally
    /// identical to driving the scheduler directly: query outcomes
    /// are never rewritten by this adapter.
    pub fn drive<T: Clone>(
        &self,
        scheduler: &Arc<Scheduler>,
        handle: &CompletionHandle<T>,
    ) -> CooperativeDrive<T> {
        if self.cancellation.is_cancelled() {
            return CooperativeDrive::Cancelled(CooperativeStop {
                point: CooperativePoint::BeforeDrive,
            });
        }
        if scheduler.has_driver_thread() {
            // The driver owns the pump: this thread must not dequeue
            // and inline-execute scheduler stages. Offer the one
            // pre-park cooperative point, then hand the wait to the
            // scheduler's own contract.
            if self.yield_hook.yield_to_runtime() == YieldDecision::Cancelled {
                return CooperativeDrive::Cancelled(CooperativeStop {
                    point: CooperativePoint::AtYield,
                });
            }
            if let Some(state) = handle.try_get() {
                return CooperativeDrive::Driven(state);
            }
            if self.cancellation.is_cancelled() {
                return CooperativeDrive::Cancelled(CooperativeStop {
                    point: CooperativePoint::BetweenStages,
                });
            }
            return CooperativeDrive::Driven(scheduler.wait_or_drive(handle));
        }
        loop {
            if self.yield_hook.yield_to_runtime() == YieldDecision::Cancelled {
                return CooperativeDrive::Cancelled(CooperativeStop {
                    point: CooperativePoint::AtYield,
                });
            }
            if let Some(state) = handle.try_get() {
                return CooperativeDrive::Driven(state);
            }
            if self.cancellation.is_cancelled() {
                return CooperativeDrive::Cancelled(CooperativeStop {
                    point: CooperativePoint::BetweenStages,
                });
            }
            if !scheduler.drive_one() {
                // Nothing ready on this thread right now. The terminal
                // decision — park on a native driver, the controlled
                // dry-failure of the inline loop, or a state resolved
                // since the last check — belongs to the scheduler's
                // own wait contract, unchanged.
                return CooperativeDrive::Driven(scheduler.wait_or_drive(handle));
            }
        }
    }
}

#[cfg(test)]
#[path = "cooperative_scheduler_tests.rs"]
mod cooperative_scheduler_tests;
