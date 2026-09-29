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
//!   (an `Arc`), never a thread or stack identity, so a suspended caller can poll it later
//!   ([`FlightSubscription::poll`]) as well as wait for it;
//! - the producer holds a [`FlightLease`]. Completing it delivers the
//!   candidate; dropping it without completing (refusal, retry exhaustion,
//!   panic) abandons the flight. Either way the flight leaves the registry
//!   and every subscriber is woken exactly once;
//! - a subscriber that stops waiting (cancellation) detaches; the producer,
//!   still owed to its own demand and to the other subscribers, runs on;
//! - a thread that holds a lease never subscribes to another flight: it
//!   resolves directly instead. No thread waits while it owes a flight, and
//!   no wait happens under any Engine lock.

use std::cell::Cell;
use std::hash::Hash;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};
use rustc_hash::FxHashMap;

thread_local! {
    /// Leases the current thread holds.
    static LEASES_HELD: Cell<usize> = const { Cell::new(0) };
}

/// The registry of in-flight cold queries.
pub(crate) struct ResolutionFlights<K, T> {
    flights: Mutex<FxHashMap<K, Arc<Flight<T>>>>,
}

pub(crate) struct Flight<T> {
    state: Mutex<FlightState<T>>,
    settled: Condvar,
    subscribers: AtomicUsize,
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
            });
        }
        let flight = Arc::new(Flight {
            state: Mutex::new(FlightState::Running),
            settled: Condvar::new(),
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
        *self.flight.state.lock() = outcome;
        self.flight.settled.notify_all();
        LEASES_HELD.with(|held| held.set(held.get().saturating_sub(1)));
    }
}

impl<K: Hash + Eq + Clone, T> Drop for FlightLease<'_, K, T> {
    fn drop(&mut self) {
        self.settle(FlightState::Abandoned);
    }
}

impl<T> FlightSubscription<T> {
    /// The flight's outcome if it has settled, without waiting: the
    /// non-blocking half of a subscription, which a suspended caller polls
    /// instead of parking a worker.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn poll(&self) -> Option<FlightOutcome<T>> {
        match &*self.flight.state.lock() {
            FlightState::Running => None,
            FlightState::Completed(value) => Some(FlightOutcome::Delivered(Arc::clone(value))),
            FlightState::Abandoned => Some(FlightOutcome::Abandoned),
        }
    }

    /// Wait for the flight to settle, checking `cancelled` between bounded
    /// waits; a cancelled subscriber detaches and the flight runs on.
    pub(crate) fn wait(mut self, cancelled: Option<&dyn Fn() -> bool>) -> FlightOutcome<T> {
        let mut state = self.flight.state.lock();
        loop {
            match &*state {
                FlightState::Completed(value) => {
                    return FlightOutcome::Delivered(Arc::clone(value))
                }
                FlightState::Abandoned => return FlightOutcome::Abandoned,
                FlightState::Running => {}
            }
            if cancelled.is_some_and(|cancelled| cancelled()) {
                drop(state);
                self.detach();
                return FlightOutcome::Detached;
            }
            let _ = self
                .flight
                .settled
                .wait_for(&mut state, std::time::Duration::from_millis(5));
        }
    }

    fn detach(&mut self) {
        if self.attached {
            self.attached = false;
            self.flight.subscribers.fetch_sub(1, Ordering::AcqRel);
        }
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
        let subscriber = std::thread::scope(|scope| {
            scope
                .spawn(|| match flights.claim(&1) {
                    FlightClaim::Join(subscription) => subscription,
                    _ => panic!("a second demand subscribes"),
                })
                .join()
                .unwrap()
        });
        assert!(subscriber.poll().is_none(), "the flight is still running");
        assert_eq!(flights.subscribers(&1), 1);
        lease.complete("answer");
        assert!(matches!(
            subscriber.poll(),
            Some(FlightOutcome::Delivered(value)) if *value == "answer"
        ));
        assert_eq!(flights.len(), 0);
    }

    #[test]
    fn a_dropped_lease_abandons_its_flight() {
        let flights: ResolutionFlights<u32, &str> = ResolutionFlights::default();
        let lease = flights.claim(&1);
        let subscriber = std::thread::scope(|scope| {
            scope
                .spawn(|| match flights.claim(&1) {
                    FlightClaim::Join(subscription) => subscription,
                    _ => panic!("a second demand subscribes"),
                })
                .join()
                .unwrap()
        });
        drop(lease);
        assert!(matches!(subscriber.wait(None), FlightOutcome::Abandoned));
        assert!(
            matches!(flights.claim(&1), FlightClaim::Lead(_)),
            "the next demand leads a fresh flight"
        );
    }
}
