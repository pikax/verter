//! Task-owned admission protocol shared by artifact and query-slot adapters.

use super::singleflight::{
    retire_slot_if_current, InflightFailureKind, InflightSlot, InflightTable, SlotJoin,
};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use std::task::{Poll, Waker};
use verter_execution::cancellation::{CancellationSubscription, CancellationToken};
use verter_execution::dependency::{DependencyStop, OwnedDependency};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, WaitEdge};
use verter_execution::wake::{WakeRegistration, WakerSet};

type FlightTable<K> = Arc<Mutex<HashMap<K, Arc<InflightSlot>>>>;

/// A terminal refusal carries no partial cache value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OwnedAdmissionError {
    /// This subscriber stopped; siblings and the transferred producer survive.
    Cancelled,
    /// The dependency would close a task cycle or uses another registry.
    Cycle,
    /// A legacy producer is running without execution-task authority.
    ProducerUnowned,
    /// The sole transferred producer was abandoned before it ran.
    ProducerDropped,
    /// The producer unwound before delivering its answer.
    ProducerPanicked,
}

pub(super) trait AdmissionPolicy<V> {
    fn lookup(&mut self) -> Option<V>;
    fn produce(&mut self, slot: &Arc<InflightSlot>) -> Option<V>;
    fn retry_failure(&self, failure: InflightFailureKind) -> bool;
}

impl<V, P: AdmissionPolicy<V> + ?Sized> AdmissionPolicy<V> for Box<P> {
    fn lookup(&mut self) -> Option<V> {
        (**self).lookup()
    }
    fn produce(&mut self, slot: &Arc<InflightSlot>) -> Option<V> {
        (**self).produce(slot)
    }
    fn retry_failure(&self, failure: InflightFailureKind) -> bool {
        (**self).retry_failure(failure)
    }
}

struct ProducerResult<V> {
    answer: Option<Result<Option<V>, OwnedAdmissionError>>,
    wakers: WakerSet,
}

pub(super) enum AdmissionStep<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> {
    Produce(AdmissionProducer<K, V, P>),
    Value(Option<V>),
}

pub(super) struct AdmissionProducer<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> {
    policy: Option<P>,
    slot: Arc<InflightSlot>,
    table: FlightTable<K>,
    key: K,
    task: Option<ExecutionTask>,
    result: Arc<Mutex<ProducerResult<V>>>,
    started: bool,
    settled: bool,
}

impl<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> AdmissionProducer<K, V, P> {
    fn run(mut self) {
        self.started = true;
        let _scope = self
            .task
            .as_ref()
            .map(|task| ExecutionScope::install(task.registry(), task));
        let answer = self
            .policy
            .as_mut()
            .expect("live producer")
            .produce(&self.slot);
        retire_slot_if_current(&self.table, &self.key, &self.slot);
        let wakes = {
            let mut result = self.result.lock();
            result.answer = Some(Ok(answer));
            result.wakers.take()
        };
        self.settled = true;
        wakes.wake();
    }
}

impl<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> Drop for AdmissionProducer<K, V, P> {
    fn drop(&mut self) {
        if self.settled {
            return;
        }
        let slot_wakes = self
            .slot
            .complete(|state| state.failure = Some(InflightFailureKind::WinnerPanicked));
        retire_slot_if_current(&self.table, &self.key, &self.slot);
        let wakes = {
            let mut result = self.result.lock();
            result.answer = Some(Err(if self.started {
                OwnedAdmissionError::ProducerPanicked
            } else {
                OwnedAdmissionError::ProducerDropped
            }));
            result.wakers.take()
        };
        slot_wakes.wake();
        wakes.wake();
    }
}

enum Join {
    Owned(OwnedDependency<SlotJoin>),
    Standalone(SlotJoin),
}
impl Join {
    fn poll(&mut self, waker: &Waker) -> Poll<Result<(), OwnedAdmissionError>> {
        match self {
            Self::Standalone(join) => join.poll(waker).map(Ok),
            Self::Owned(join) => join.poll(waker).map(|answer| {
                answer.map_err(|stop| match stop {
                    DependencyStop::Cancelled => OwnedAdmissionError::Cancelled,
                    DependencyStop::Cycle => OwnedAdmissionError::Cycle,
                })
            }),
        }
    }
}

/// Private generic core also accepts borrowed policies for synchronous callers.
pub(super) struct AdmissionCore<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> {
    table: Option<FlightTable<K>>,
    key: Option<K>,
    policy: Option<P>,
    waiter: Option<ExecutionTask>,
    cancellation: Option<CancellationToken>,
    stop: Option<CancellationSubscription>,
    slot: Option<Arc<InflightSlot>>,
    join: Option<Join>,
    producer_edge: Option<WaitEdge>,
    result: Option<Arc<Mutex<ProducerResult<V>>>>,
    registration: WakeRegistration,
    finished: bool,
    #[cfg(any(test, feature = "test-support"))]
    joiner_park_tx: Arc<Mutex<Option<std::sync::mpsc::Sender<()>>>>,
}

impl<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> AdmissionCore<K, V, P> {
    pub(super) fn new(
        inflight: &InflightTable<K>,
        key: K,
        policy: P,
        waiter: Option<ExecutionTask>,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            table: Some(inflight.table.clone()),
            key: Some(key),
            policy: Some(policy),
            waiter,
            stop: Some(cancellation.subscribe()),
            cancellation: Some(cancellation),
            slot: None,
            join: None,
            producer_edge: None,
            result: None,
            registration: WakeRegistration::default(),
            finished: false,
            #[cfg(any(test, feature = "test-support"))]
            joiner_park_tx: inflight.joiner_park_tx.clone(),
        }
    }

    pub(super) fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<AdmissionStep<K, V, P>, OwnedAdmissionError>> {
        assert!(!self.finished, "admission already completed");
        if self
            .stop
            .as_mut()
            .is_some_and(|stop| stop.poll(waker).is_ready())
        {
            return self.finish(Err(OwnedAdmissionError::Cancelled));
        }
        if let Some(result) = &self.result {
            let answer = {
                let mut result = result.lock();
                if result.answer.is_none() {
                    result.wakers.register(&mut self.registration, waker);
                }
                result.answer.take()
            };
            return match answer {
                Some(answer) => self.finish(answer.map(AdmissionStep::Value)),
                None => Poll::Pending,
            };
        }
        if let Some(join) = &mut self.join {
            match join.poll(waker) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return self.finish(Err(error)),
                Poll::Ready(Ok(())) => {}
            }
            self.join = None;
            if self.resume_stop(waker) {
                return self.finish(Err(OwnedAdmissionError::Cancelled));
            }
        }
        if let Some(slot) = self.slot.take() {
            let (failure, non_cacheable) = {
                let state = slot.state.lock();
                (state.failure, state.non_cacheable_winner)
            };
            if let Some(failure) = failure {
                if !self
                    .policy
                    .as_ref()
                    .expect("follower policy")
                    .retry_failure(failure)
                {
                    return self.finish(Ok(AdmissionStep::Value(None)));
                }
            } else if !non_cacheable {
                if let Some(value) = self.policy.as_mut().expect("follower policy").lookup() {
                    return self.finish(Ok(AdmissionStep::Value(Some(value))));
                }
            }
            retire_slot_if_current(
                self.table.as_ref().expect("live table"),
                self.key.as_ref().expect("live key"),
                &slot,
            );
        }
        if let Some(value) = self.policy.as_mut().expect("admission policy").lookup() {
            return self.finish(Ok(AdmissionStep::Value(Some(value))));
        }
        let slot = self
            .table
            .as_ref()
            .expect("live table")
            .lock()
            .entry(self.key.as_ref().expect("live key").clone())
            .or_insert_with(|| Arc::new(InflightSlot::new()))
            .clone();
        let (winner, producer, completed) = {
            let mut state = slot.state.lock();
            let winner = !state.claimed;
            if winner {
                state.claimed = true;
                state.producer = self
                    .waiter
                    .as_ref()
                    .map(|task| task.registry().register_task());
            }
            (winner, state.producer.clone(), state.completed)
        };
        self.slot = Some(slot.clone());
        if winner {
            // Register the parent's dependency BEFORE transferring its producer.
            if let (Some(waiter), Some(producer)) = (&self.waiter, &producer) {
                match waiter.wait_for(producer) {
                    Ok(edge) => self.producer_edge = Some(edge),
                    Err(_) => {
                        slot.complete(|state| {
                            state.failure = Some(InflightFailureKind::WinnerPanicked)
                        })
                        .wake();
                        retire_slot_if_current(
                            self.table.as_ref().expect("live table"),
                            self.key.as_ref().expect("live key"),
                            &slot,
                        );
                        return self.finish(Err(OwnedAdmissionError::Cycle));
                    }
                }
            }
            let mut wakers = WakerSet::default();
            wakers.register(&mut self.registration, waker);
            let result = Arc::new(Mutex::new(ProducerResult {
                answer: None,
                wakers,
            }));
            self.result = Some(result.clone());
            return Poll::Ready(Ok(AdmissionStep::Produce(AdmissionProducer {
                policy: self.policy.take(),
                slot,
                table: self.table.as_ref().expect("live table").clone(),
                key: self.key.as_ref().expect("live key").clone(),
                task: producer,
                result,
                started: false,
                settled: false,
            })));
        }
        self.join = Some(match (&self.waiter, &producer) {
            (Some(waiter), Some(producer)) => {
                // Transfer stop observation to the join; one subscriber must
                // never register twice on the same cancellation token.
                self.stop = None;
                Join::Owned(OwnedDependency::new(
                    SlotJoin::new(&slot),
                    waiter.clone(),
                    producer.clone(),
                    self.cancellation.as_ref().expect("live stop").clone(),
                ))
            }
            (None, _) => Join::Standalone(SlotJoin::new(&slot)),
            (Some(_), None) if completed => Join::Standalone(SlotJoin::new(&slot)),
            (Some(_), None) => return self.finish(Err(OwnedAdmissionError::ProducerUnowned)),
        });
        #[cfg(any(test, feature = "test-support"))]
        if let Some(tx) = self.joiner_park_tx.lock().clone() {
            let _ = tx.send(());
        }
        match self.join.as_mut().expect("new join").poll(waker) {
            Poll::Ready(Err(error)) => self.finish(Err(error)),
            // Completion may race the claim. The registration is already atomic;
            // request another poll rather than recursively entering admission.
            Poll::Ready(Ok(())) => {
                // OwnedDependency is terminal now. Retain the slot outcome,
                // consume the join, and process that outcome on the next poll.
                self.join = None;
                if self.resume_stop(waker) {
                    return self.finish(Err(OwnedAdmissionError::Cancelled));
                }
                waker.wake_by_ref();
                Poll::Pending
            }
            Poll::Pending => Poll::Pending,
        }
    }

    fn resume_stop(&mut self, waker: &Waker) -> bool {
        if self.stop.is_none() {
            self.stop = Some(self.cancellation.as_ref().expect("live stop").subscribe());
        }
        self.stop
            .as_mut()
            .expect("live stop")
            .poll(waker)
            .is_ready()
    }

    fn finish(
        &mut self,
        answer: Result<AdmissionStep<K, V, P>, OwnedAdmissionError>,
    ) -> Poll<Result<AdmissionStep<K, V, P>, OwnedAdmissionError>> {
        self.finished = true;
        self.join = None;
        self.producer_edge = None;
        self.slot = None;
        self.waiter = None;
        self.stop = None;
        self.cancellation = None;
        self.policy = None;
        self.withdraw_result();
        self.table = None;
        self.key = None;
        Poll::Ready(answer)
    }

    fn withdraw_result(&mut self) {
        if let Some(result) = self.result.take() {
            result.lock().wakers.withdraw(&mut self.registration);
        }
    }
}

impl<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>> Drop for AdmissionCore<K, V, P> {
    fn drop(&mut self) {
        self.join = None;
        self.producer_edge = None;
        self.stop = None;
        self.withdraw_result();
    }
}

pub(super) fn drive<K: Eq + Hash + Clone, V, P: AdmissionPolicy<V>>(
    mut core: AdmissionCore<K, V, P>,
) -> Option<V> {
    loop {
        match verter_execution::blocking::block_on(None, |waker| core.poll(waker)) {
            Ok(AdmissionStep::Produce(producer)) => producer.run(),
            Ok(AdmissionStep::Value(value)) => return value,
            Err(_) => return None,
        }
    }
}

type OwnedPolicy<V> = Box<dyn AdmissionPolicy<V> + Send>;

/// Exclusive owned cache admission; poll transfers cold work instead of running it.
pub struct OwnedAdmission<K: Eq + Hash + Clone, V> {
    core: AdmissionCore<K, V, OwnedPolicy<V>>,
}

/// A single admission advancement. Run or drop `Produce`, then poll again.
pub enum OwnedAdmissionStep<K: Eq + Hash + Clone, V> {
    /// Sole producer, movable to another execution thread.
    Produce(OwnedAdmissionProducer<K, V>),
    /// Terminal value; `None` retains the adapter's deterministic failure policy.
    Value(Option<V>),
}

/// The sole cold producer. Dropping it settles the flight and wakes subscribers.
pub struct OwnedAdmissionProducer<K: Eq + Hash + Clone, V> {
    inner: AdmissionProducer<K, V, OwnedPolicy<V>>,
}
impl<K: Eq + Hash + Clone, V> OwnedAdmissionProducer<K, V> {
    /// Run outside polling, under this producer's shared-registry task scope.
    pub fn run(self) {
        // Shared work must not inherit the executing waiter's stop authority.
        // A live aggregate owner also takes precedence over request-context
        // cancellation without discarding request accounting or observation.
        let cancellation = CancellationToken::aggregate();
        let _owner = cancellation
            .register_owner(None)
            .expect("fresh producer owner");
        let _cancellation =
            verter_execution::cancellation::JobCancellationGuard::install(cancellation);
        // The owned producer reports unwind through its result cell. The
        // synchronous driver uses the generic producer directly and retains
        // its historical panic propagation.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.inner.run()));
    }
}
impl<K: Eq + Hash + Clone, V> OwnedAdmission<K, V> {
    pub(super) fn new<P: AdmissionPolicy<V> + Send + 'static>(
        inflight: &InflightTable<K>,
        key: K,
        policy: P,
        waiter: ExecutionTask,
        cancellation: CancellationToken,
    ) -> Self {
        Self {
            core: AdmissionCore::new(inflight, key, Box::new(policy), Some(waiter), cancellation),
        }
    }
    /// Poll readiness or transfer the admitted producer; never run cold work.
    pub fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<OwnedAdmissionStep<K, V>, OwnedAdmissionError>> {
        self.core.poll(waker).map(|answer| {
            answer.map(|step| match step {
                AdmissionStep::Produce(inner) => {
                    OwnedAdmissionStep::Produce(OwnedAdmissionProducer { inner })
                }
                AdmissionStep::Value(value) => OwnedAdmissionStep::Value(value),
            })
        })
    }
}
