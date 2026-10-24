//! One resolver rendezvous protocol for owned polling and synchronous entries.

use super::*;
use std::task::{Poll, Waker};
use verter_execution::cancellation::{CancellationSubscription, CancellationToken};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry, WaitEdge};
use verter_execution::wake::{TakenWakers, WakeRegistration, WakerSet};

/// A terminal refusal contains no partial resolver value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolverFlightError<E> {
    /// Only this subscriber stopped; the producer and siblings survive.
    Cancelled,
    /// Waiting would cycle, or waiter and producer authorities differ.
    Cycle,
    /// A standalone compatibility producer has no execution-task authority.
    ProducerUnowned,
    /// The transferred producer was dropped without running.
    ProducerDropped,
    /// Compute or its retention predicate unwound.
    ProducerPanicked,
    /// The owning resolver invalidated the flight.
    Reset,
    /// The producer's original error.
    Failed(E),
}

#[derive(Clone, Copy, Debug)]
pub(super) enum FlightAbort {
    Dropped,
    Panicked,
    Reset,
}
impl FlightAbort {
    fn error<E>(self) -> ResolverFlightError<E> {
        match self {
            Self::Dropped => ResolverFlightError::ProducerDropped,
            Self::Panicked => ResolverFlightError::ProducerPanicked,
            Self::Reset => ResolverFlightError::Reset,
        }
    }
}

type Answer<V, E> = Result<Arc<V>, ResolverFlightError<E>>;
#[derive(Debug)]
pub(super) struct LeaderAnswer<V, E> {
    value: Option<Answer<V, E>>,
    wakers: WakerSet,
    compatibility: bool,
}
impl<V, E> Default for LeaderAnswer<V, E> {
    fn default() -> Self {
        Self {
            value: None,
            wakers: WakerSet::default(),
            compatibility: false,
        }
    }
}

impl<V, E> FlightState<V, E> {
    /// Caller holds the table lock; notification remains deferred to it.
    pub(super) fn abort(&self, reason: FlightAbort) -> TakenWakers {
        let mut inner = self.inner.lock();
        if matches!(*inner, FlightInner::Done(_) | FlightInner::Aborted(_)) {
            return WakerSet::default().take();
        }
        let mut wakes = self.wakers.lock().take();
        if let FlightInner::Running { answer, .. } = &*inner {
            let mut answer = answer.lock();
            if answer.value.is_none() {
                // Compatibility producers keep their complete answer across
                // reset; only their publication authority is invalidated.
                if !matches!(reason, FlightAbort::Reset) || !answer.compatibility {
                    answer.value = Some(Err(reason.error()));
                }
                wakes.append(answer.wakers.take());
            }
        }
        *inner = FlightInner::Aborted(reason);
        wakes
    }
}

type Compute<V, E> = Box<dyn FnOnce() -> Result<V, E> + Send>;
type Retain<V> = Box<dyn FnOnce(&V) -> bool + Send>;

/// An owned burst pin acquired before a caller's warm-cache probe. It keeps
/// the matching flight joinable across suspension without borrowing the group.
pub struct OwnedFlightParticipation<K: Eq + Hash + Clone, V, E> {
    group: SingleflightGroup<K, V, E>,
    key: (K, StoreViewCompatToken),
    state: Arc<FlightState<V, E>>,
}
impl<K: Eq + Hash + Clone, V, E> Drop for OwnedFlightParticipation<K, V, E> {
    fn drop(&mut self) {
        self.group.unpin(&self.key, &self.state);
    }
}
impl<K: Eq + Hash + Clone, V, E> SingleflightGroup<K, V, E> {
    pub fn participate_owned(
        &self,
        key: K,
        token: StoreViewCompatToken,
    ) -> OwnedFlightParticipation<K, V, E> {
        let (key, state) = self.pin_lane(key, token);
        OwnedFlightParticipation {
            group: self.shared(),
            key,
            state,
        }
    }
}

/// A poll-only resolver capability, owning its subscriber and all suspended
/// inputs. Terminal answers and drop release its registrations and task pins.
pub struct OwnedResolverFlight<K: Eq + Hash + Clone, V, E: Clone> {
    core: FlightCore<K, V, E, Compute<V, E>, Retain<V>>,
}
/// A poll transfers cold work or returns a complete resolver rendezvous.
pub enum ResolverFlightStep<K: Eq + Hash + Clone, V, E: Clone> {
    Produce(OwnedResolverProducer<K, V, E>),
    Value(SingleflightRunResult<V>),
}
/// The sole execution owner. `run` installs its task on the current thread;
/// dropping it publishes a typed failure and wakes existing followers.
pub struct OwnedResolverProducer<K: Eq + Hash + Clone, V, E: Clone> {
    core: CoreProducer<K, V, E, Compute<V, E>, Retain<V>>,
}
impl<K: Eq + Hash + Clone, V, E: Clone> OwnedResolverProducer<K, V, E> {
    pub fn run(self) {
        // Producer ownership takes precedence over either ambient job or
        // request cancellation, preserving the request's other accounting.
        let cancellation = CancellationToken::aggregate();
        let _owner = cancellation
            .register_owner(None)
            .expect("fresh producer owner");
        let _cancellation =
            verter_execution::cancellation::JobCancellationGuard::install(cancellation);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.core.run()));
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone> OwnedResolverFlight<K, V, E> {
    pub fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<ResolverFlightStep<K, V, E>, ResolverFlightError<E>>> {
        self.core.poll(waker).map(|answer| {
            answer.map(|step| match step {
                CoreStep::Produce(core) => {
                    ResolverFlightStep::Produce(OwnedResolverProducer { core })
                }
                CoreStep::Value(value) => ResolverFlightStep::Value(value),
            })
        })
    }
}

impl<K: Eq + Hash, V, E> SingleflightGroup<K, V, E> {
    /// Internal shared handle, deliberately absent from the polling capability.
    pub(super) fn shared(&self) -> Self {
        Self {
            flights: self.flights.clone(),
            registry: self.registry.clone(),
            #[cfg(test)]
            non_retained_seam_hook: self.non_retained_seam_hook.clone(),
            #[cfg(test)]
            joiner_park_tx: self.joiner_park_tx.clone(),
        }
    }
    /// Inject the host's existing cycle authority at composition time.
    pub(crate) fn bind_task_registry(&self, registry: TaskRegistry) {
        self.registry
            .set(registry)
            .expect("resolver authority is composed once");
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone> SingleflightGroup<K, V, E> {
    /// Begin without computing, waiting, or entering a worker pool. The waiter
    /// supplies the host's shared authority; no private registry is allocated.
    pub fn begin_owned_retaining<F, R>(
        &self,
        key: K,
        token: StoreViewCompatToken,
        waiter: ExecutionTask,
        stop: CancellationToken,
        compute: F,
        retain: R,
    ) -> OwnedResolverFlight<K, V, E>
    where
        F: FnOnce() -> Result<V, E> + Send + 'static,
        R: FnOnce(&V) -> bool + Send + 'static,
    {
        OwnedResolverFlight {
            core: FlightCore::new(
                self.shared(),
                key,
                token,
                Some(waiter),
                stop,
                Box::new(compute),
                Box::new(retain),
                false,
            ),
        }
    }
}

pub(super) enum CoreStep<K: Eq + Hash + Clone, V, E: Clone, F, R> {
    Produce(CoreProducer<K, V, E, F, R>),
    Value(SingleflightRunResult<V>),
}

pub(super) type CorePoll<K, V, E, F, R> =
    Poll<Result<CoreStep<K, V, E, F, R>, ResolverFlightError<E>>>;

/// A generic private core permits borrowed synchronous callbacks without a
/// second lifecycle or a blocking method on the public polling capability.
pub(super) struct FlightCore<K: Eq + Hash + Clone, V, E: Clone, F, R> {
    group: Option<SingleflightGroup<K, V, E>>,
    key: Option<(K, StoreViewCompatToken)>,
    waiter: Option<ExecutionTask>,
    stop: Option<CancellationSubscription>,
    compute: Option<F>,
    retain: Option<R>,
    join: Option<FlightJoin<V, E>>,
    answer: Option<Arc<Mutex<LeaderAnswer<V, E>>>>,
    registration: WakeRegistration,
    edge: Option<WaitEdge>,
    role: SingleflightRole,
    forked_lane: bool,
    compatibility: bool,
    finished: bool,
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> FlightCore<K, V, E, F, R> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        group: SingleflightGroup<K, V, E>,
        key: K,
        token: StoreViewCompatToken,
        waiter: Option<ExecutionTask>,
        stop: CancellationToken,
        compute: F,
        retain: R,
        compatibility: bool,
    ) -> Self {
        Self {
            group: Some(group),
            key: Some((key, token)),
            waiter,
            stop: Some(stop.subscribe()),
            compute: Some(compute),
            retain: Some(retain),
            join: None,
            answer: None,
            registration: WakeRegistration::default(),
            edge: None,
            role: SingleflightRole::Follower,
            forked_lane: false,
            compatibility,
            finished: false,
        }
    }
    fn release_join(&mut self) {
        // A joined value's destructor may re-enter the task graph.
        self.edge = None;
        if let Some(join) = self.join.take() {
            self.group
                .as_ref()
                .expect("live group")
                .unpin(self.key.as_ref().expect("live key"), &join.state);
            drop(join);
        }
    }
    fn transfer_inline(&mut self) -> CorePoll<K, V, E, F, R> {
        // The synchronous recursion/cycle escape transfers nested work to
        // its external driver; poll never evaluates the caller's callback.
        self.release_join();
        self.role = SingleflightRole::Leader;
        let answer = Arc::new(Mutex::new(LeaderAnswer {
            compatibility: true,
            ..LeaderAnswer::default()
        }));
        self.answer = Some(answer.clone());
        Poll::Ready(Ok(CoreStep::Produce(CoreProducer {
            flight: None,
            task: ExecutionScope::current_task(),
            answer,
            compute: self.compute.take(),
            retain: None,
            started: false,
            settled: false,
        })))
    }
    fn cleanup(&mut self) {
        self.release_join();
        if let Some(answer) = self.answer.take() {
            answer.lock().wakers.withdraw(&mut self.registration);
        }
        self.stop = None;
        self.waiter = None;
        self.compute = None;
        self.retain = None;
        self.group = None;
        self.key = None;
    }
    fn finish(
        &mut self,
        answer: Result<SingleflightRunResult<V>, ResolverFlightError<E>>,
    ) -> CorePoll<K, V, E, F, R> {
        self.finished = true;
        self.cleanup();
        Poll::Ready(answer.map(CoreStep::Value))
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> Drop for FlightCore<K, V, E, F, R> {
    fn drop(&mut self) {
        self.cleanup();
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> FlightCore<K, V, E, F, R>
where
    F: FnOnce() -> Result<V, E>,
    R: FnOnce(&V) -> bool,
{
    pub(super) fn poll(&mut self, waker: &Waker) -> CorePoll<K, V, E, F, R> {
        assert!(!self.finished, "resolver flight already completed");
        if self
            .stop
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
        {
            return self.finish(Err(ResolverFlightError::Cancelled));
        }
        if let Some(answer) = &self.answer {
            let value = {
                let mut answer = answer.lock();
                if answer.value.is_none() {
                    answer.wakers.register(&mut self.registration, waker);
                }
                answer.value.clone()
            };
            return match value {
                Some(value) => self.finish(value.map(|value| SingleflightRunResult {
                    value,
                    role: self.role,
                    forked_lane: self.forked_lane,
                })),
                None => Poll::Pending,
            };
        }
        loop {
            if self.join.is_none() {
                let group = self.group.as_ref().expect("live group");
                let lane_key = self.key.as_ref().expect("live key");
                let (state, task, answer, leader, forked) = {
                    let mut flights = group.flights.lock();
                    let forked = flights.has_other_live_token(&lane_key.0, lane_key.1);
                    let state = if let Some(state) = flights.lanes.get(lane_key) {
                        state
                            .pins
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        state.clone()
                    } else {
                        let state = Arc::new(FlightState {
                            inner: Mutex::new(FlightInner::Pending),
                            wakers: Mutex::default(),
                            pins: std::sync::atomic::AtomicUsize::new(1),
                            live_indexed: std::sync::atomic::AtomicBool::new(false),
                        });
                        flights.lanes.insert(lane_key.clone(), state.clone());
                        flights.mark_live(&lane_key.0, lane_key.1, &state);
                        state
                    };
                    let mut inner = state.inner.lock();
                    let leader = matches!(*inner, FlightInner::Pending);
                    let (task, answer) = if leader {
                        let task = group
                            .registry
                            .get()
                            .or_else(|| self.waiter.as_ref().map(ExecutionTask::registry))
                            .map(TaskRegistry::register_task);
                        let answer = Arc::new(Mutex::new(LeaderAnswer {
                            compatibility: self.compatibility,
                            ..LeaderAnswer::default()
                        }));
                        *inner = FlightInner::Running {
                            owner: None,
                            task: task.clone(),
                            answer: answer.clone(),
                        };
                        (task, Some(answer))
                    } else {
                        (None, None)
                    };
                    drop(inner);
                    (state, task, answer, leader, forked)
                };
                self.forked_lane = forked;
                if leader {
                    self.role = SingleflightRole::Leader;
                    let producer = CoreProducer {
                        flight: Some((group.shared(), lane_key.clone(), state)),
                        task: task.clone(),
                        answer: answer.expect("leader answer"),
                        compute: self.compute.take(),
                        retain: self.retain.take(),
                        started: false,
                        settled: false,
                    };
                    if let (Some(waiter), Some(task)) = (&self.waiter, task) {
                        match waiter.wait_for(&task) {
                            Ok(edge) => self.edge = Some(edge),
                            Err(_) if self.compatibility => {}
                            Err(_) => {
                                drop(producer);
                                return self.finish(Err(ResolverFlightError::Cycle));
                            }
                        }
                    }
                    self.answer = Some(producer.answer.clone());
                    return Poll::Ready(Ok(CoreStep::Produce(producer)));
                }
                self.join = Some(FlightJoin::new(state));
            }
            let join = self.join.as_mut().expect("follower join");
            match join.poll(waker) {
                Poll::Ready(FlightTerminal::Done(answer)) => {
                    return self.finish(
                        answer
                            .map(|value| SingleflightRunResult {
                                value,
                                role: SingleflightRole::Follower,
                                forked_lane: self.forked_lane,
                            })
                            .map_err(ResolverFlightError::Failed),
                    )
                }
                Poll::Ready(FlightTerminal::Aborted(reason)) => {
                    if !self.compatibility {
                        return self.finish(Err(reason.error()));
                    }
                    self.release_join();
                    continue;
                }
                Poll::Pending => {}
            }
            let (task, same_thread, running) = {
                let inner = join.state.inner.lock();
                match &*inner {
                    FlightInner::Running { task, owner, .. } => (
                        task.clone(),
                        *owner == Some(std::thread::current().id()),
                        true,
                    ),
                    _ => (None, false, false),
                }
            };
            if !running {
                continue;
            }
            if self.compatibility && same_thread {
                return self.transfer_inline();
            }
            if self.edge.is_none() {
                if let Some(waiter) = &self.waiter {
                    if let Some(task) = task {
                        match waiter.wait_for(&task) {
                            Ok(edge) => self.edge = Some(edge),
                            Err(_) if self.compatibility => return self.transfer_inline(),
                            Err(_) => return self.finish(Err(ResolverFlightError::Cycle)),
                        }
                    } else if !self.compatibility {
                        return self.finish(Err(ResolverFlightError::ProducerUnowned));
                    }
                }
            }
            #[cfg(test)]
            if let Some(tx) = self
                .group
                .as_ref()
                .expect("live group")
                .joiner_park_tx
                .lock()
                .clone()
            {
                let _ = tx.send(());
            }
            return Poll::Pending;
        }
    }
}

type PinnedFlight<K, V, E> = (
    SingleflightGroup<K, V, E>,
    (K, StoreViewCompatToken),
    Arc<FlightState<V, E>>,
);
pub(super) struct CoreProducer<K: Eq + Hash + Clone, V, E: Clone, F, R> {
    flight: Option<PinnedFlight<K, V, E>>,
    task: Option<ExecutionTask>,
    answer: Arc<Mutex<LeaderAnswer<V, E>>>,
    compute: Option<F>,
    retain: Option<R>,
    started: bool,
    settled: bool,
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> CoreProducer<K, V, E, F, R> {
    fn settle_answer(&self, value: Answer<V, E>) -> TakenWakers {
        let mut answer = self.answer.lock();
        if answer.value.is_none() {
            answer.value = Some(value);
        }
        answer.wakers.take()
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> CoreProducer<K, V, E, F, R>
where
    F: FnOnce() -> Result<V, E>,
    R: FnOnce(&V) -> bool,
{
    pub(super) fn run(mut self) {
        self.started = true;
        let _scope = self
            .task
            .as_ref()
            .map(|task| ExecutionScope::install(task.registry(), task));
        if let Some((_, _, state)) = &self.flight {
            let mut inner = state.inner.lock();
            if let FlightInner::Running { owner, .. } = &mut *inner {
                *owner = Some(std::thread::current().id());
            } else if !self.answer.lock().compatibility {
                // Reset settled the owned answer and invalidated its work.
                self.settled = true;
                drop(inner);
                if let Some((group, key, state)) = self.flight.take() {
                    group.unpin(&key, &state);
                }
                return;
            }
        }
        let result = self.compute.take().expect("one producer computation")().map(Arc::new);
        let keep = if self.flight.is_some() {
            matches!(&result, Ok(value) if self.retain.take().expect("retention policy")(value))
        } else {
            false
        };
        let mut wakes = WakerSet::default().take();
        if let Some((group, key, state)) = &self.flight {
            let mut flights = group.flights.lock();
            if matches!(*state.inner.lock(), FlightInner::Running { .. }) {
                wakes.append(state.terminate(FlightInner::Done(result.clone())));
                flights.clear_live(&key.0, key.1, state);
                if !keep {
                    #[cfg(test)]
                    if let Some(hook) = group.non_retained_seam_hook.0.lock().as_ref() {
                        hook();
                    }
                    if flights
                        .lanes
                        .get(key)
                        .is_some_and(|current| Arc::ptr_eq(current, state))
                    {
                        flights.lanes.remove(key);
                    }
                }
            }
        }
        wakes.append(self.settle_answer(result.map_err(ResolverFlightError::Failed)));
        self.settled = true;
        if let Some((group, key, state)) = self.flight.take() {
            group.unpin(&key, &state);
        }
        wakes.wake();
    }
}
impl<K: Eq + Hash + Clone, V, E: Clone, F, R> Drop for CoreProducer<K, V, E, F, R> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let reason = if self.started {
            FlightAbort::Panicked
        } else {
            FlightAbort::Dropped
        };
        let mut wakes = WakerSet::default().take();
        if let Some((group, key, state)) = self.flight.take() {
            {
                let mut flights = group.flights.lock();
                if matches!(*state.inner.lock(), FlightInner::Running { .. }) {
                    wakes.append(state.abort(reason));
                }
                flights.clear_live(&key.0, key.1, &state);
                if flights
                    .lanes
                    .get(&key)
                    .is_some_and(|current| Arc::ptr_eq(current, &state))
                {
                    flights.lanes.remove(&key);
                }
            }
            group.unpin(&key, &state);
        }
        wakes.append(self.settle_answer(Err(reason.error())));
        wakes.wake();
    }
}
