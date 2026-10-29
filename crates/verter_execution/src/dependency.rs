//! Cancellable, task-owned subscriptions over poll-only dependency APIs.

use std::task::{Poll, Waker};

use crate::cancellation::{CancellationSubscription, CancellationToken};
use crate::tasks::{ExecutionTask, WaitEdge};

/// A dependency polling capability. Implementations register readiness under
/// the producer lock and withdraw that registration on drop. They do no work
/// that waits, pumps an executor, or submits to a bounded blocking queue.
pub trait DependencyPoll {
    /// The owned terminal answer.
    type Output;
    /// Observe readiness or register this subscriber's wake.
    fn poll_dependency(&mut self, waker: &Waker) -> Poll<Self::Output>;
}

/// Refusal carries no partial dependency value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DependencyStop {
    /// Only this subscriber stopped; the producer and siblings remain live.
    Cancelled,
    /// Waiting would cycle, the task already waits, or authorities differ.
    Cycle,
}

/// Exclusive, movable ownership of one dependency wait. The polling capability
/// never exposes its underlying handoff or any synchronous driver. Terminal
/// answers and drop release the readiness registration, stop registration,
/// task pins and exactly this subscriber's wait edge.
pub struct OwnedDependency<H: DependencyPoll> {
    handoff: Option<H>,
    waiter: Option<ExecutionTask>,
    producer: Option<ExecutionTask>,
    cancellation: Option<CancellationSubscription>,
    edge: Option<WaitEdge>,
    finished: bool,
}

impl<H: DependencyPoll> OwnedDependency<H> {
    /// Bind an existing pollable handoff to its actual producer and waiter.
    /// The producer identity must be the task that will execute that work.
    pub fn new(
        handoff: H,
        waiter: ExecutionTask,
        producer: ExecutionTask,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            handoff: Some(handoff),
            waiter: Some(waiter),
            producer: Some(producer),
            cancellation: Some(cancellation.subscribe()),
            edge: None,
            finished: false,
        }
    }

    /// Poll without running the producer. Cancellation wins when already
    /// observed; an immediately ready answer needs no wait edge, including
    /// a completed dependency produced by the caller's own task.
    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<H::Output, DependencyStop>> {
        assert!(!self.finished, "dependency already completed");
        if self
            .cancellation
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
        {
            return self.finish(Err(DependencyStop::Cancelled));
        }
        if let Poll::Ready(answer) = self
            .handoff
            .as_mut()
            .expect("live handoff")
            .poll_dependency(waker)
        {
            return self.finish(Ok(answer));
        }
        if self.edge.is_none() {
            match self
                .waiter
                .as_ref()
                .expect("live waiter")
                .wait_for(self.producer.as_ref().expect("live producer"))
            {
                Ok(edge) => self.edge = Some(edge),
                Err(_) => return self.finish(Err(DependencyStop::Cycle)),
            }
        }
        Poll::Pending
    }

    fn finish(
        &mut self,
        answer: Result<H::Output, DependencyStop>,
    ) -> Poll<Result<H::Output, DependencyStop>> {
        self.finished = true;
        self.release();
        Poll::Ready(answer)
    }

    fn release(&mut self) {
        // Handoff destruction may notify a producer. That callback must not
        // observe the wait this subscriber has already abandoned.
        self.edge = None;
        self.cancellation = None;
        self.waiter = None;
        self.producer = None;
        self.handoff = None;
    }
}

impl<H: DependencyPoll> Drop for OwnedDependency<H> {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests;
