//! Poll-only admission, producer transfer and result subscription for cache nodes.

use super::*;
use std::marker::PhantomData;
use std::task::{Poll, Waker};
use verter_execution::tasks::WaitEdge;
use verter_execution::wake::WakeRegistration;

/// One advancement of a scoped cache-node computation.
///
/// `Produce` transfers the sole producer; it must run outside the polling
/// computation. Poll the same handoff again to receive `Value`, which is terminal.
/// Followers receive only `Value`. Readiness wakes request a later poll; they
/// never execute the producer or recursively drive this handoff.
pub enum ScopedCacheNodeStep<T: Send + Sync + 'static> {
    /// The DAG admitted this producer. Run or drop it to settle the flight.
    Produce(ScopedCacheNodeProducer<T>),
    /// The shared producer's result, also delivered to every live follower.
    Value(Arc<T>),
}

/// An owned, exclusive subscription to a scoped cache-node computation.
///
/// Polling offers one retained inbox submission and yields on backpressure,
/// dispatch or another caller's producer. It never pumps the scheduler, enters
/// a pool, waits on a channel, or runs a producer. The scheduler and producer
/// must be driven separately. A handoff is `Send` and may resume on another
/// thread; `poll` requires exclusive access. It retains the scheduler while live.
///
/// Drop withdraws this subscriber and detaches only its request owner. A
/// transferred producer keeps working for any surviving sibling. Cancellation
/// is checked on every poll; an executor must also re-poll or drop a handoff
/// when its request cancels. Reset/shutdown and producer settlement wake all
/// subscribers. Polling after a terminal value or error is a contract violation.
///
/// Construction captures the execution task installed for this scheduler's
/// registry, or creates one if this is an external root. That task follows the
/// handoff across threads. A follower holds a shared-registry wait edge to the
/// transferred producer until it completes or drops; self-dependency and a
/// cycle return `Reentrant` without cancelling other owners. Producer election
/// wakes subscribers so waits made before election can register their edges.
pub struct ScopedCacheNodeHandoff<T: Send + Sync + 'static> {
    scheduler: Arc<Scheduler>,
    identity: WorkNodeIdentity,
    flight: Arc<ScopedCacheFlight>,
    owner_id: u64,
    task: ExecutionTask,
    wait: Option<WaitEdge>,
    context: Option<verter_execution::request_context::OpaqueRequestContext>,
    request: Option<CancellationToken>,
    admission: Option<PendingSubmission>,
    registration: WakeRegistration,
    finished: bool,
    value: PhantomData<T>,
}

impl<T: Send + Sync + 'static> ScopedCacheNodeHandoff<T> {
    pub(super) fn new(
        scheduler: Arc<Scheduler>,
        identity: WorkNodeIdentity,
        flight: Arc<ScopedCacheFlight>,
        owner_id: u64,
        context: Option<verter_execution::request_context::OpaqueRequestContext>,
        request: Option<CancellationToken>,
        submission: Submission,
    ) -> Self {
        let admission = Some(PendingSubmission::new(&scheduler.inbox, Some(submission)));
        let task = ExecutionScope::current(&scheduler.task_registry)
            .unwrap_or_else(|| scheduler.task_registry.register_task());
        Self {
            scheduler,
            identity,
            flight,
            owner_id,
            task,
            wait: None,
            context,
            request,
            admission,
            registration: WakeRegistration::default(),
            finished: false,
            value: PhantomData,
        }
    }

    /// Advance admission or read readiness atomically with wake registration.
    /// `Produce` is an intermediate transfer; `Value` and errors end this handoff.
    pub fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<ScopedCacheNodeStep<T>, ScopedCacheNodeError>> {
        assert!(!self.finished, "scoped cache handoff already completed");
        let delivery = Arc::clone(&self.scheduler.dag);
        let _wakes = delivery.defer_wakes();
        if self
            .request
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            return self.finish(Err(ScopedCacheNodeError::Cancelled));
        }
        if caller_kind::active_path_contains_work(&self.identity) {
            return self.finish(Err(ScopedCacheNodeError::Reentrant));
        }
        // A sibling may already have admitted or completed this flight.
        // Its answer must not wait for capacity in a later inbox incarnation.
        let terminal = self.flight.terminal();
        if let Some(terminal) = terminal {
            return self.finish_terminal(terminal);
        }
        if let Some(admission) = &mut self.admission {
            match admission.poll(&self.scheduler, waker) {
                Poll::Pending => {
                    // Subscribe to settlement as well as capacity: reset or a
                    // sibling's completion can answer without admitting us.
                    let flight = Arc::clone(&self.flight);
                    let mut state = flight.state.lock();
                    if let Some(terminal) = state.terminal.clone() {
                        drop(state);
                        return self.finish_terminal(terminal);
                    }
                    if let Err(error) = self.subscribe(&mut state, waker) {
                        drop(state);
                        return self.finish(Err(error));
                    }
                    return Poll::Pending;
                }
                Poll::Ready(Ok(())) => self.scheduler.record_inbox_submission(),
                Poll::Ready(Err(submission)) => {
                    self.scheduler.shutdown_drained_submission(*submission)
                }
            }
            self.admission = None;
        }
        // Neither admission nor this poll dispatches work. Claiming and
        // registering share the same flight hold as dispatch and settlement.
        let flight = Arc::clone(&self.flight);
        let mut state = flight.state.lock();
        if let Some(terminal) = state.terminal.clone() {
            drop(state);
            return self.finish_terminal(terminal);
        }
        if state.dispatched && state.producer.is_none() {
            let aggregate = state
                .aggregate
                .clone()
                .expect("dispatch attaches the job token");
            state.producer = Some((self.task.clone(), self.owner_id));
            state.wakers.withdraw(&mut self.registration);
            // Followers that yielded before election must register their wait
            // edges now that the producer is known, even if it never runs.
            self.scheduler.dag.deliver(vec![state.wakers.take()]);
            return Poll::Ready(Ok(ScopedCacheNodeStep::Produce(ScopedCacheNodeProducer {
                scheduler: Arc::clone(&self.scheduler),
                identity: self.identity.clone(),
                flight: Arc::clone(&self.flight),
                context: self.context.clone(),
                aggregate,
                task: self.task.clone(),
                settled: false,
                value: PhantomData,
            })));
        }
        if let Err(error) = self.subscribe(&mut state, waker) {
            drop(state);
            return self.finish(Err(error));
        }
        Poll::Pending
    }

    /// Check task recursion and register the follower's wait before subscribing
    /// under the flight hold. The transferring handoff itself may await its
    /// externally driven producer; it does not depend on a different task.
    fn subscribe(
        &mut self,
        state: &mut ScopedCacheFlightState,
        waker: &Waker,
    ) -> Result<(), ScopedCacheNodeError> {
        if let Some((producer, owner_id)) = &state.producer {
            if *owner_id != self.owner_id && self.wait.is_none() {
                self.wait = Some(
                    self.scheduler
                        .task_registry
                        .register_wait(self.task.id(), producer.id())
                        .map_err(|_| ScopedCacheNodeError::Reentrant)?,
                );
            }
        }
        state.wakers.register(&mut self.registration, waker);
        Ok(())
    }

    fn finish_terminal(
        &mut self,
        terminal: ScopedCacheTerminal,
    ) -> Poll<Result<ScopedCacheNodeStep<T>, ScopedCacheNodeError>> {
        let answer = match terminal {
            ScopedCacheTerminal::Value(value) => Arc::downcast::<T>(value)
                .map(ScopedCacheNodeStep::Value)
                .map_err(|_| ScopedCacheNodeError::TypeMismatch),
            ScopedCacheTerminal::Cancelled => Err(ScopedCacheNodeError::Cancelled),
            ScopedCacheTerminal::Shutdown => Err(ScopedCacheNodeError::Shutdown),
            ScopedCacheTerminal::Panicked => Err(ScopedCacheNodeError::Panicked),
        };
        self.finish(answer)
    }

    fn finish(
        &mut self,
        answer: Result<ScopedCacheNodeStep<T>, ScopedCacheNodeError>,
    ) -> Poll<Result<ScopedCacheNodeStep<T>, ScopedCacheNodeError>> {
        self.finished = true;
        self.wait = None;
        self.admission = None;
        self.flight
            .state
            .lock()
            .wakers
            .withdraw(&mut self.registration);
        self.scheduler
            .detach_scoped_cache_owner(&self.identity, &self.flight, self.owner_id);
        if self
            .request
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
        {
            Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
        } else {
            Poll::Ready(answer)
        }
    }
}

impl<T: Send + Sync + 'static> Drop for ScopedCacheNodeHandoff<T> {
    fn drop(&mut self) {
        self.wait = None;
        self.flight
            .state
            .lock()
            .wakers
            .withdraw(&mut self.registration);
        if !self.finished {
            self.scheduler
                .detach_scoped_cache_owner(&self.identity, &self.flight, self.owner_id);
        }
    }
}

/// The sole dispatched producer of one scoped cache-node flight.
///
/// This is executor-neutral owned work authority, with no pool or blocking
/// adapter. `run` executes on its caller's thread, installs the submitting
/// request, execution-task and aggregate cancellation scopes, contains producer panic, and
/// publishes once. A cancelled aggregate refuses publication. Dropping the
/// producer without running it settles the flight as cancelled, waking followers.
/// The closure may borrow its caller's data; parser arenas need not cross threads.
pub struct ScopedCacheNodeProducer<T: Send + Sync + 'static> {
    scheduler: Arc<Scheduler>,
    identity: WorkNodeIdentity,
    flight: Arc<ScopedCacheFlight>,
    context: Option<verter_execution::request_context::OpaqueRequestContext>,
    aggregate: CancellationToken,
    task: ExecutionTask,
    settled: bool,
    value: PhantomData<T>,
}

impl<T: Send + Sync + 'static> ScopedCacheNodeProducer<T> {
    /// Run the admitted producer outside its waiting root's poll.
    pub fn run(mut self, build: impl FnOnce(&CancellationToken) -> T) {
        let _task = ExecutionScope::install(&self.scheduler.task_registry, &self.task);
        let _request = self
            .context
            .as_ref()
            .map(|context| Arc::clone(&context.0).install_tls());
        let _job =
            verter_execution::cancellation::JobCancellationGuard::install(self.aggregate.clone());
        let terminal = if self.aggregate.is_cancelled() {
            ScopedCacheTerminal::Cancelled
        } else {
            match caller_kind::with_active_path(self.identity.clone(), || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&self.aggregate)))
            }) {
                Ok(value) if !self.aggregate.is_cancelled() => {
                    ScopedCacheTerminal::Value(Arc::new(value))
                }
                Ok(_) => ScopedCacheTerminal::Cancelled,
                Err(_) => ScopedCacheTerminal::Panicked,
            }
        };
        let success = matches!(terminal, ScopedCacheTerminal::Value(_));
        self.scheduler.terminalize_scoped_cache_flight(
            &self.identity,
            &self.flight,
            terminal,
            success,
        );
        self.settled = true;
    }
}

impl<T: Send + Sync + 'static> Drop for ScopedCacheNodeProducer<T> {
    fn drop(&mut self) {
        if !self.settled {
            self.scheduler.terminalize_scoped_cache_flight(
                &self.identity,
                &self.flight,
                ScopedCacheTerminal::Cancelled,
                false,
            );
        }
    }
}
