//! Owned admission handoffs over the scheduler's bounded submission inbox.

use super::*;
use std::task::{Poll, Waker};
use verter_execution::wake::{WakeRegistration, WakerSet};

/// One request or atomic batch waiting to enter the scheduler inbox.
///
/// `poll` never drives a stage or waits for a dependency. A full inbox retains
/// the entire submission and registers for capacity under the offer lock.
/// Dequeue wakes schedule a later poll, after that lock is released. Re-polls
/// replace the same registration; dropping a handoff withdraws only its own
/// interest and drops unsubmitted work. Submitted work follows the scheduler's
/// existing completion/cancellation contract and is unaffected by this drop.
///
/// The handoff retains the scheduler until admission completes or it is
/// dropped. It is `Send` when its output is, and takes an exclusive mutable
/// borrow to poll, so it may migrate between threads without two pollers.
pub struct SchedulerSubmission<T> {
    scheduler: Arc<Scheduler>,
    pending: PendingSubmission,
    output: Option<T>,
}

impl<T> SchedulerSubmission<T> {
    pub(super) fn new(
        scheduler: &Arc<Scheduler>,
        submission: Option<Submission>,
        output: T,
    ) -> Self {
        Self {
            scheduler: Arc::clone(scheduler),
            pending: PendingSubmission::new(&scheduler.inbox, submission),
            output: Some(output),
        }
    }

    /// Offer the retained submission once. `Ready` transfers the completion
    /// subscription(s) in input order; shutdown resolves them as `Shutdown` instead
    /// of leaving an unsubmitted request pending. Polling after `Ready` is a
    /// contract violation. This capability exposes no blocking driver.
    pub fn poll(&mut self, waker: &Waker) -> Poll<T> {
        assert!(self.output.is_some(), "submission already completed");
        let nonempty = self.pending.submission.is_some();
        match self.pending.poll(&self.scheduler, waker) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Err(submission)) => self.scheduler.shutdown_drained_submission(*submission),
            Poll::Ready(Ok(())) if nonempty => self.scheduler.record_inbox_submission(),
            Poll::Ready(Ok(())) => {}
        }
        Poll::Ready(self.output.take().expect("output transfers once"))
    }
}

/// The admission state shared with synchronous external-boundary drivers.
pub(super) struct PendingSubmission {
    submission: Option<Submission>,
    space: Arc<Mutex<WakerSet>>,
    registration: WakeRegistration,
}

impl PendingSubmission {
    pub(super) fn new(inbox: &SubmissionInbox, submission: Option<Submission>) -> Self {
        Self {
            submission,
            space: Arc::clone(&inbox.space),
            registration: WakeRegistration::default(),
        }
    }

    pub(super) fn poll(
        &mut self,
        scheduler: &Scheduler,
        waker: &Waker,
    ) -> Poll<Result<(), Box<Submission>>> {
        let Some(submission) = self.submission.take() else {
            return Poll::Ready(Ok(()));
        };
        let mut space = self.space.lock();
        if scheduler.shutdown.load(Ordering::Acquire) {
            space.withdraw(&mut self.registration);
            return Poll::Ready(Err(Box::new(submission)));
        }
        match scheduler.inbox.sender.try_send(submission) {
            Ok(()) => {
                space.withdraw(&mut self.registration);
                Poll::Ready(Ok(()))
            }
            Err(crossbeam_channel::TrySendError::Full(submission)) => {
                self.submission = Some(submission);
                space.register(&mut self.registration, waker);
                Poll::Pending
            }
            Err(crossbeam_channel::TrySendError::Disconnected(submission)) => {
                space.withdraw(&mut self.registration);
                Poll::Ready(Err(Box::new(submission)))
            }
        }
    }
}

impl Drop for PendingSubmission {
    fn drop(&mut self) {
        if self.registration.is_registered() {
            self.space.lock().withdraw(&mut self.registration);
        }
    }
}
