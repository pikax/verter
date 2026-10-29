//! Owned admission handoffs over the scheduler's bounded submission inbox.

use super::*;
use std::task::{Poll, Waker};
use verter_execution::wake::{WakeRegistration, WakerSet};

/// One owned request, spanning inbox backpressure and the admitted producer's
/// completion. Only external drivers execute stages; polling cannot access a
/// blocking handle. Cancellation withdraws this subscriber without stopping
/// shared stage work or another request's subscriber.
pub struct OwnedSchedulerRequest {
    admission: Option<PendingSubmission>,
    completion: crate::job::OwnedCompletionSubscription<RequestResult>,
    scheduler: Option<Arc<Scheduler>>,
    stop: Option<verter_execution::cancellation::CancellationSubscription>,
    finished: bool,
}

impl OwnedSchedulerRequest {
    pub(super) fn new(
        scheduler: &Arc<Scheduler>,
        request: Request,
        waiter: verter_execution::tasks::ExecutionTask,
        cancellation: CancellationToken,
    ) -> Self {
        let (submission, handle) = scheduler.prepare_request_submission(request);
        Self {
            admission: Some(PendingSubmission::new(&scheduler.inbox, Some(submission))),
            completion: handle
                .subscribe()
                .owned_producer(waiter, cancellation.clone()),
            scheduler: Some(Arc::clone(scheduler)),
            stop: Some(cancellation.subscribe()),
            finished: false,
        }
    }

    /// Retain one queue offer on backpressure and subscribe to completion under
    /// its producer lock. Terminal and stop paths release the owned wait edge.
    pub fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<CompletionState<RequestResult>, verter_execution::dependency::DependencyStop>>
    {
        assert!(!self.finished, "request already completed");
        if self
            .stop
            .as_mut()
            .is_some_and(|stop| stop.poll(waker).is_ready())
        {
            self.admission = None;
            self.finished = true;
            self.scheduler = None;
            self.stop = None;
            return self.completion.poll(waker);
        }
        if let Some(admission) = &mut self.admission {
            let scheduler = self.scheduler.as_ref().expect("live request");
            match admission.poll(scheduler, waker) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Ok(())) => scheduler.record_inbox_submission(),
                Poll::Ready(Err(submission)) => scheduler.shutdown_drained_submission(*submission),
            }
            self.admission = None;
            self.stop = None;
        }
        let result = self.completion.poll(waker);
        self.finished = result.is_ready();
        if self.finished {
            self.scheduler = None;
        }
        result
    }
}

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
