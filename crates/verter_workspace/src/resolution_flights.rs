//! One task-owned rendezvous for cold workspace resolutions. A flight only
//! shares candidates; each subscriber validates them in its own captured world.

use crate::resolution_handoff::ResolutionError;
use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Poll, Waker};
use verter_execution::tasks::{ExecutionTask, WaitEdge};
use verter_execution::wake::{WakeRegistration, WakerSet};

type Table<K, T> = Arc<Mutex<FxHashMap<K, Arc<Flight<T>>>>>;
pub(crate) struct ResolutionFlights<K, T> {
    flights: Table<K, T>,
}
struct Flight<T> {
    slot: Mutex<FlightSlot<T>>,
    subscribers: AtomicUsize,
    producer: ExecutionTask,
}
struct FlightSlot<T> {
    state: FlightState<T>,
    wakers: WakerSet,
}
enum FlightState<T> {
    Running,
    Completed(Arc<T>),
    Abandoned,
}
pub(crate) enum FlightOutcome<T> {
    Delivered(Arc<T>),
    Abandoned,
}
pub(crate) enum FlightClaim<K: Hash + Eq + Clone, T> {
    Lead(FlightLease<K, T>),
    Join(FlightSubscription<T>),
}
/// The obligation is owned and movable, never tied to a thread or stack.
pub(crate) struct FlightLease<K: Hash + Eq + Clone, T> {
    registry: Table<K, T>,
    key: K,
    flight: Arc<Flight<T>>,
    settled: bool,
    /// The claiming thread's lease count holds this lease.
    _thread: std::marker::PhantomData<*const ()>,
}
pub(crate) struct FlightSubscription<T> {
    flight: Arc<Flight<T>>,
    attached: bool,
    registration: WakeRegistration,
    waiter: ExecutionTask,
    edge: Option<WaitEdge>,
}
impl<K: Hash + Eq + Clone, T> Default for ResolutionFlights<K, T> {
    fn default() -> Self {
        Self {
            flights: Arc::new(Mutex::new(FxHashMap::default())),
        }
    }
}
impl<K: Hash + Eq + Clone, T> ResolutionFlights<K, T> {
    pub(crate) fn claim(&self, key: &K, task: &ExecutionTask) -> FlightClaim<K, T> {
        let mut flights = self.flights.lock();
        if let Some(flight) = flights.get(key) {
            flight.subscribers.fetch_add(1, Ordering::AcqRel);
            return FlightClaim::Join(FlightSubscription {
                flight: flight.clone(),
                attached: true,
                registration: WakeRegistration::default(),
                waiter: task.clone(),
                edge: None,
            });
        }
        let flight = Arc::new(Flight {
            slot: Mutex::new(FlightSlot {
                state: FlightState::Running,
                wakers: WakerSet::default(),
            }),
            subscribers: AtomicUsize::new(0),
            producer: task.clone(),
        });
        flights.insert(key.clone(), flight.clone());
        FlightClaim::Lead(FlightLease {
            registry: self.flights.clone(),
            key: key.clone(),
            flight,
            settled: false,
            _thread: std::marker::PhantomData,
        })
    }
    pub(crate) fn len(&self) -> usize {
        self.flights.lock().len()
    }
    #[cfg(test)]
    pub(crate) fn subscribers(&self, key: &K) -> usize {
        self.flights
            .lock()
            .get(key)
            .map_or(0, |f| f.subscribers.load(Ordering::Acquire))
    }
}
impl<K: Hash + Eq + Clone, T> FlightLease<K, T> {
    pub(crate) fn complete(mut self, value: T) {
        self.settle(FlightState::Completed(Arc::new(value)));
    }
    fn settle(&mut self, outcome: FlightState<T>) {
        if self.settled {
            return;
        }
        self.settled = true;
        {
            let mut table = self.registry.lock();
            if table
                .get(&self.key)
                .is_some_and(|f| Arc::ptr_eq(f, &self.flight))
            {
                table.remove(&self.key);
            }
        }
        let wakes = {
            let mut slot = self.flight.slot.lock();
            slot.state = outcome;
            slot.wakers.take()
        };
        wakes.wake();
    }
}
impl<K: Hash + Eq + Clone, T> Drop for FlightLease<K, T> {
    fn drop(&mut self) {
        self.settle(FlightState::Abandoned);
    }
}
impl<T> FlightSubscription<T> {
    pub(crate) fn poll(
        &mut self,
        waker: &Waker,
    ) -> Poll<Result<FlightOutcome<T>, ResolutionError>> {
        let answer = {
            let mut slot = self.flight.slot.lock();
            match &slot.state {
                FlightState::Completed(value) => Some(Ok(FlightOutcome::Delivered(value.clone()))),
                FlightState::Abandoned => Some(Ok(FlightOutcome::Abandoned)),
                FlightState::Running => {
                    if self.edge.is_none() {
                        match self.waiter.wait_for(&self.flight.producer) {
                            Ok(edge) => self.edge = Some(edge),
                            Err(_) => {
                                drop(slot);
                                self.detach();
                                return Poll::Ready(Err(ResolutionError::Cycle));
                            }
                        }
                    }
                    slot.wakers.register(&mut self.registration, waker);
                    None
                }
            }
        };
        match answer {
            Some(answer) => {
                self.detach();
                Poll::Ready(answer)
            }
            None => Poll::Pending,
        }
    }
    fn detach(&mut self) {
        self.edge = None;
        self.flight
            .slot
            .lock()
            .wakers
            .withdraw(&mut self.registration);
        if self.attached {
            self.attached = false;
            self.flight.subscribers.fetch_sub(1, Ordering::AcqRel);
        }
    }
    #[cfg(test)]
    fn registered_wakers(&self) -> usize {
        self.flight.slot.lock().wakers.len()
    }
}
impl<T> Drop for FlightSubscription<T> {
    fn drop(&mut self) {
        self.detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::task::Wake;

    struct CountingWake(AtomicUsize);

    impl Wake for CountingWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counting_waker() -> (Arc<CountingWake>, Waker) {
        let counter = Arc::new(CountingWake(AtomicUsize::new(0)));
        (Arc::clone(&counter), Waker::from(counter))
    }

    fn wakes(counter: &CountingWake) -> usize {
        counter.0.load(Ordering::SeqCst)
    }

    fn join(
        flights: &ResolutionFlights<u32, &'static str>,
        key: u32,
    ) -> FlightSubscription<&'static str> {
        let task = flights
            .flights
            .lock()
            .get(&key)
            .unwrap()
            .producer
            .registry()
            .register_task();
        match flights.claim(&key, &task) {
            FlightClaim::Join(subscription) => subscription,
            _ => panic!("a second demand subscribes on the same thread"),
        }
    }

    #[test]
    fn a_flight_delivers_once_to_every_subscriber_then_leaves_the_registry() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let registry = verter_execution::tasks::TaskRegistry::default();
        let task = registry.register_task();
        let FlightClaim::Lead(lease) = flights.claim(&1, &task) else {
            panic!("the first demand leads");
        };
        assert!(
            matches!(
                flights.claim(&1, &registry.register_task()),
                FlightClaim::Join(_)
            ),
            "another task on the same thread must be able to subscribe"
        );
        let mut subscriber = join(&flights, 1);
        let (counter, waker) = counting_waker();
        assert!(
            subscriber.poll(&waker).is_pending(),
            "the flight is still running"
        );
        assert!(subscriber.poll(&waker).is_pending());
        assert_eq!(
            subscriber.registered_wakers(),
            1,
            "a re-poll keeps one registration"
        );
        assert_eq!(flights.subscribers(&1), 1);
        lease.complete("answer");
        assert_eq!(wakes(&counter), 1, "settling wakes the pending poller once");
        assert_eq!(subscriber.registered_wakers(), 0);
        assert!(matches!(
            subscriber.poll(&waker),
            Poll::Ready(Ok(FlightOutcome::Delivered(value))) if *value == "answer"
        ));
        assert_eq!(flights.len(), 0);
    }

    #[test]
    fn a_dropped_lease_abandons_its_flight() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let registry = verter_execution::tasks::TaskRegistry::default();
        let task = registry.register_task();
        let lease = flights.claim(&1, &task);
        let mut subscriber = join(&flights, 1);
        drop(lease);
        assert!(matches!(
            subscriber.poll(&counting_waker().1),
            Poll::Ready(Ok(FlightOutcome::Abandoned))
        ));
        assert!(
            matches!(flights.claim(&1, &task), FlightClaim::Lead(_)),
            "the next demand leads a fresh flight"
        );
    }

    #[test]
    fn a_dropped_subscription_withdraws_only_its_own_registration() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let registry = verter_execution::tasks::TaskRegistry::default();
        let task = registry.register_task();
        let FlightClaim::Lead(lease) = flights.claim(&1, &task) else {
            panic!("the first demand leads");
        };
        let (counter, waker) = counting_waker();
        let mut dropped = join(&flights, 1);
        let mut kept = join(&flights, 1);
        assert!(dropped.poll(&waker).is_pending());
        assert!(kept.poll(&waker).is_pending());
        assert_eq!(kept.registered_wakers(), 2);
        drop(dropped);
        assert_eq!(
            kept.registered_wakers(),
            1,
            "drop withdraws its registration"
        );
        assert_eq!(flights.subscribers(&1), 1);
        lease.complete("answer");
        assert_eq!(
            wakes(&counter),
            1,
            "only the remaining subscription is woken"
        );
        assert!(kept.poll(&waker).is_ready());
    }

    #[test]
    fn a_cancelled_wait_detaches_and_the_flight_runs_on() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let registry = verter_execution::tasks::TaskRegistry::default();
        let task = registry.register_task();
        let FlightClaim::Lead(lease) = flights.claim(&1, &task) else {
            panic!("the first demand leads");
        };
        let mut cancelled = join(&flights, 1);
        let mut sibling = join(&flights, 1);
        let (_, waker) = counting_waker();
        assert!(cancelled.poll(&waker).is_pending());
        cancelled.detach();
        assert_eq!(flights.subscribers(&1), 1, "the sibling stays attached");
        assert_eq!(
            sibling.registered_wakers(),
            0,
            "the cancelled wait withdrew"
        );
        lease.complete("answer");
        let (_, waker) = counting_waker();
        assert!(matches!(
            sibling.poll(&waker),
            Poll::Ready(Ok(FlightOutcome::Delivered(value))) if *value == "answer"
        ));
    }

    /// A settlement racing a subscription's registration is never lost:
    /// either the registering poll sees the outcome, or its waker fires.
    #[test]
    fn settlement_racing_registration_never_loses_the_wake() {
        for round in 0..256u32 {
            let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
            let registry = verter_execution::tasks::TaskRegistry::default();
            let task = registry.register_task();
            let start = std::sync::Barrier::new(2);
            let (counter, waker) = counting_waker();
            let mut subscriber = None;
            std::thread::scope(|scope| {
                let (led, leading) = std::sync::mpsc::channel();
                let (flights, start, task) = (&flights, &start, &task);
                // The producer leads and settles on its own thread, so the
                // lease it owes never belongs to the subscribing thread.
                let producer = scope.spawn(move || {
                    let FlightClaim::Lead(lease) = flights.claim(&round, task) else {
                        panic!("the first demand leads");
                    };
                    led.send(()).expect("the subscriber is listening");
                    start.wait();
                    lease.complete("answer");
                });
                leading.recv().expect("the producer leads");
                let FlightClaim::Join(mut joined) =
                    flights.claim(&round, &registry.register_task())
                else {
                    panic!("a second demand subscribes");
                };
                start.wait();
                let answered_at_poll = joined.poll(&waker).is_ready();
                producer.join().expect("the producer settles");
                assert!(
                    answered_at_poll || wakes(&counter) == 1,
                    "round {round}: a pending poll must be woken by the settlement"
                );
                subscriber = Some(joined);
            });
            let mut subscriber = subscriber.expect("the subscription survives the race");
            assert!(subscriber.poll(&waker).is_ready());
            assert_eq!(subscriber.registered_wakers(), 0);
        }
    }
}

#[cfg(test)]
mod task_cycles {
    use super::*;
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::dependency::DependencyStop;
    use verter_execution::tasks::TaskRegistry;

    #[test]
    fn same_path_and_mixed_scheduler_flight_cycles_are_typed_and_drain() {
        let registry = TaskRegistry::default();
        let a = registry.register_task();
        let b = registry.register_task();
        let flights: ResolutionFlights<u32, u32> = ResolutionFlights::default();
        let FlightClaim::Lead(lease) = flights.claim(&1, &a) else {
            panic!("leader")
        };
        let waker = Waker::noop();
        let FlightClaim::Join(mut recursive) = flights.claim(&1, &a) else {
            panic!("recursive subscription")
        };
        assert!(matches!(
            recursive.poll(waker),
            Poll::Ready(Err(ResolutionError::Cycle))
        ));
        drop(recursive);
        for completion_first in [false, true] {
            let (handle, _sender) = verter_scheduler::job::completion_pair::<u32>();
            let mut completion =
                handle
                    .subscribe()
                    .owned(a.clone(), b.clone(), CancellationToken::default());
            let FlightClaim::Join(mut follower) = flights.claim(&1, &b) else {
                panic!("follower")
            };
            if completion_first {
                assert!(completion.poll(waker).is_pending());
                assert!(matches!(
                    follower.poll(waker),
                    Poll::Ready(Err(ResolutionError::Cycle))
                ));
            } else {
                assert!(follower.poll(waker).is_pending());
                assert!(matches!(
                    completion.poll(waker),
                    Poll::Ready(Err(DependencyStop::Cycle))
                ));
            }
            drop((completion, follower));
            assert_eq!(registry.wait_graph_occupancy().edges, 0);
        }
        let weak = Arc::downgrade(&lease.flight);
        drop(lease);
        assert!(
            weak.upgrade().is_none(),
            "flight retirement releases its producer pin"
        );
    }
}
