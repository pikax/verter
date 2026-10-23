//! Single-flight for cold resolution queries.
//!
//! Concurrent demands for the same complete resolution query in the same
//! execution domain share ONE producer run: the first demand claims the
//! flight and resolves; every other demand subscribes and receives the
//! completed candidate when the producer finishes. Each subscriber then
//! validates the delivered candidate against ITS OWN captured world before
//! adopting it — the flight coalesces work, never answers — so a
//! subscriber whose view differs (another overlay, a newer world) computes
//! its own answer instead of adopting one that is not valid for it.
//!
//! Lifecycle, with identities that survive suspension:
//!
//! - a flight is named by its key; a subscriber holds the flight itself
//!   (an `Arc`), never a thread or stack identity, so a suspended caller
//!   can poll it later ([`FlightSubscription::poll`]) as well as wait for
//!   it. A poll checks the flight and registers its waker under the
//!   flight's lock; settling takes every registration under that lock and
//!   wakes it after unlocking, so a settlement racing a registration is
//!   never lost. [`FlightSubscription::wait`] is that poll driven by the
//!   external blocking driver;
//! - the producer holds a [`FlightLease`]. Completing it delivers the
//!   candidate; dropping it without completing (refusal, retry exhaustion,
//!   panic) abandons the flight. Either way the flight leaves the registry
//!   and every subscriber is woken exactly once;
//! - a subscriber that stops waiting (cancellation, or a dropped
//!   subscription) detaches and withdraws its registration; the producer,
//!   still owed to its own demand and to the other subscribers, runs on;
//! - a thread that holds a lease never subscribes to another flight: it
//!   resolves directly instead. No thread waits while it owes a flight, and
//!   no wait happens under any Engine lock.

use std::cell::Cell;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Poll, Waker};

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use verter_execution::wake::{WakeRegistration, WakerSet};

thread_local! {
    /// Leases the current thread holds.
    static LEASES_HELD: Cell<usize> = const { Cell::new(0) };
}

/// How often a waiting subscriber with a cancellation check observes it.
const CANCELLATION_SLICE: std::time::Duration = std::time::Duration::from_millis(5);

/// The registry of in-flight cold queries.
pub(crate) struct ResolutionFlights<K, T> {
    flights: Mutex<FxHashMap<K, Arc<Flight<T>>>>,
}

pub(crate) struct Flight<T> {
    slot: Mutex<FlightSlot<T>>,
    subscribers: AtomicUsize,
}

/// A flight's state and the subscriptions polling for it to settle.
struct FlightSlot<T> {
    state: FlightState<T>,
    /// Taken when the flight settles and woken after the lock is released.
    wakers: WakerSet,
}

enum FlightState<T> {
    Running,
    Completed(Arc<T>),
    Abandoned,
}

/// What a subscriber learns about its flight.
#[derive(Debug)]
pub(crate) enum FlightOutcome<T> {
    /// The producer completed: the delivered value, to be validated by the
    /// subscriber before adoption.
    Delivered(Arc<T>),
    /// The producer finished without a value: retry.
    Abandoned,
    /// The subscriber stopped waiting.
    Detached,
}

pub(crate) enum FlightClaim<'a, K: Hash + Eq + Clone, T> {
    Lead(FlightLease<'a, K, T>),
    Join(FlightSubscription<T>),
    /// This thread already owes a flight: resolve without one.
    Direct,
}

/// The producer's obligation to settle one flight.
pub(crate) struct FlightLease<'a, K: Hash + Eq + Clone, T> {
    registry: &'a ResolutionFlights<K, T>,
    key: K,
    flight: Arc<Flight<T>>,
    settled: bool,
}

/// One subscriber's interest in a flight.
pub(crate) struct FlightSubscription<T> {
    flight: Arc<Flight<T>>,
    attached: bool,
    /// This subscription's waker registration while it is pending.
    registration: WakeRegistration,
}

impl<K: Hash + Eq + Clone, T> Default for ResolutionFlights<K, T> {
    fn default() -> Self {
        Self {
            flights: Mutex::new(FxHashMap::default()),
        }
    }
}

impl<K: Hash + Eq + Clone, T> ResolutionFlights<K, T> {
    /// Claim `key`'s flight, or subscribe to the one already running.
    pub(crate) fn claim(&self, key: &K) -> FlightClaim<'_, K, T> {
        if LEASES_HELD.with(Cell::get) > 0 {
            return FlightClaim::Direct;
        }
        let mut flights = self.flights.lock();
        if let Some(flight) = flights.get(key) {
            flight.subscribers.fetch_add(1, Ordering::AcqRel);
            return FlightClaim::Join(FlightSubscription {
                flight: Arc::clone(flight),
                attached: true,
                registration: WakeRegistration::default(),
            });
        }
        let flight = Arc::new(Flight {
            slot: Mutex::new(FlightSlot {
                state: FlightState::Running,
                wakers: WakerSet::default(),
            }),
            subscribers: AtomicUsize::new(0),
        });
        flights.insert(key.clone(), Arc::clone(&flight));
        LEASES_HELD.with(|held| held.set(held.get() + 1));
        FlightClaim::Lead(FlightLease {
            registry: self,
            key: key.clone(),
            flight,
            settled: false,
        })
    }

    /// Flights currently running.
    pub(crate) fn len(&self) -> usize {
        self.flights.lock().len()
    }

    /// Subscribers attached to `key`'s running flight.
    #[cfg(test)]
    pub(crate) fn subscribers(&self, key: &K) -> usize {
        self.flights
            .lock()
            .get(key)
            .map_or(0, |flight| flight.subscribers.load(Ordering::Acquire))
    }
}

impl<K: Hash + Eq + Clone, T> FlightLease<'_, K, T> {
    /// Deliver `value` to every subscriber and retire the flight.
    pub(crate) fn complete(mut self, value: T) {
        self.settle(FlightState::Completed(Arc::new(value)));
    }

    fn settle(&mut self, outcome: FlightState<T>) {
        if self.settled {
            return;
        }
        self.settled = true;
        {
            let mut flights = self.registry.flights.lock();
            if flights
                .get(&self.key)
                .is_some_and(|flight| Arc::ptr_eq(flight, &self.flight))
            {
                flights.remove(&self.key);
            }
        }
        let wakers = {
            let mut slot = self.flight.slot.lock();
            slot.state = outcome;
            slot.wakers.take()
        };
        wakers.wake();
        LEASES_HELD.with(|held| held.set(held.get().saturating_sub(1)));
    }
}

impl<K: Hash + Eq + Clone, T> Drop for FlightLease<'_, K, T> {
    fn drop(&mut self) {
        self.settle(FlightState::Abandoned);
    }
}

impl<T> FlightSubscription<T> {
    /// `Ready` with the flight's outcome once it settled, else `Pending`
    /// with `waker` registered to fire when it does. The check and the
    /// registration share the flight's lock with its settlement, so a
    /// suspended caller polls this instead of parking a worker and still
    /// never misses the wake. A poll after the outcome keeps answering it.
    pub(crate) fn poll(&mut self, waker: &Waker) -> Poll<FlightOutcome<T>> {
        let mut slot = self.flight.slot.lock();
        match &slot.state {
            FlightState::Completed(value) => {
                Poll::Ready(FlightOutcome::Delivered(Arc::clone(value)))
            }
            FlightState::Abandoned => Poll::Ready(FlightOutcome::Abandoned),
            FlightState::Running => {
                slot.wakers.register(&mut self.registration, waker);
                Poll::Pending
            }
        }
    }

    /// Wait for the flight to settle, observing `cancelled` at least every
    /// few milliseconds; a cancelled subscriber detaches and the flight
    /// runs on. This is [`Self::poll`] driven by the external blocking
    /// driver.
    pub(crate) fn wait(mut self, cancelled: Option<&dyn Fn() -> bool>) -> FlightOutcome<T> {
        let slice = cancelled.map(|_| CANCELLATION_SLICE);
        verter_execution::blocking::block_on(slice, |waker| match self.poll(waker) {
            Poll::Pending if cancelled.is_some_and(|cancelled| cancelled()) => {
                self.detach();
                Poll::Ready(FlightOutcome::Detached)
            }
            answer => answer,
        })
    }

    /// Stop waiting: withdraw this subscription's registration and leave
    /// the flight running for its producer and the other subscribers.
    fn detach(&mut self) {
        if self.registration.is_registered() {
            self.flight
                .slot
                .lock()
                .wakers
                .withdraw(&mut self.registration);
        }
        if self.attached {
            self.attached = false;
            self.flight.subscribers.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Wakers registered on this subscription's flight.
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

    /// Subscribe to `key`'s running flight from another thread, since the
    /// thread leading it resolves directly instead.
    fn join(
        flights: &ResolutionFlights<u32, &'static str>,
        key: u32,
    ) -> FlightSubscription<&'static str> {
        std::thread::scope(|scope| {
            scope
                .spawn(|| match flights.claim(&key) {
                    FlightClaim::Join(subscription) => subscription,
                    _ => panic!("a second demand subscribes"),
                })
                .join()
                .unwrap()
        })
    }

    #[test]
    fn a_flight_delivers_once_to_every_subscriber_then_leaves_the_registry() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let FlightClaim::Lead(lease) = flights.claim(&1) else {
            panic!("the first demand leads");
        };
        assert!(
            matches!(flights.claim(&1), FlightClaim::Direct),
            "a thread that owes a flight never waits on one"
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
            Poll::Ready(FlightOutcome::Delivered(value)) if *value == "answer"
        ));
        assert_eq!(flights.len(), 0);
    }

    #[test]
    fn a_dropped_lease_abandons_its_flight() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let lease = flights.claim(&1);
        let subscriber = join(&flights, 1);
        drop(lease);
        assert!(matches!(subscriber.wait(None), FlightOutcome::Abandoned));
        assert!(
            matches!(flights.claim(&1), FlightClaim::Lead(_)),
            "the next demand leads a fresh flight"
        );
    }

    #[test]
    fn a_dropped_subscription_withdraws_only_its_own_registration() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let FlightClaim::Lead(lease) = flights.claim(&1) else {
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
        let FlightClaim::Lead(lease) = flights.claim(&1) else {
            panic!("the first demand leads");
        };
        let cancelled = join(&flights, 1);
        let mut sibling = join(&flights, 1);
        let stop = || true;
        assert!(matches!(
            cancelled.wait(Some(&stop)),
            FlightOutcome::Detached
        ));
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
            Poll::Ready(FlightOutcome::Delivered(value)) if *value == "answer"
        ));
    }

    /// A settlement racing a subscription's registration is never lost:
    /// either the registering poll sees the outcome, or its waker fires.
    #[test]
    fn settlement_racing_registration_never_loses_the_wake() {
        for round in 0..256u32 {
            let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
            let start = std::sync::Barrier::new(2);
            let (counter, waker) = counting_waker();
            let mut subscriber = None;
            std::thread::scope(|scope| {
                let (led, leading) = std::sync::mpsc::channel();
                let (flights, start) = (&flights, &start);
                // The producer leads and settles on its own thread, so the
                // lease it owes never belongs to the subscribing thread.
                let producer = scope.spawn(move || {
                    let FlightClaim::Lead(lease) = flights.claim(&round) else {
                        panic!("the first demand leads");
                    };
                    led.send(()).expect("the subscriber is listening");
                    start.wait();
                    lease.complete("answer");
                });
                leading.recv().expect("the producer leads");
                let FlightClaim::Join(mut joined) = flights.claim(&round) else {
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
