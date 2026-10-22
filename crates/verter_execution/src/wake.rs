//! Waker registrations for a producer that pollable computations wait on.
//!
//! A producer (a flight, a completion slot, a bounded queue) keeps one
//! [`WakerSet`] beside the state it publishes, under the same lock. A poll
//! checks that state and, while it is not ready, registers its waker in
//! that critical section, so a publication racing the registration still
//! wakes it. The producer takes every registration as it publishes and
//! wakes them after releasing its lock ([`TakenWakers::wake`]), so no wake
//! runs under a producer lock.
//!
//! Each subscriber owns one [`WakeRegistration`]. Re-polling with a waker
//! that wakes the same task keeps the registration; any other waker
//! replaces it. Withdrawing removes exactly that subscriber's registration,
//! even when a sibling registered the same executor waker.

use std::task::Waker;

/// The wakers of the pollers waiting on one producer, each under the id
/// its subscriber withdraws by.
#[derive(Default)]
pub struct WakerSet {
    wakers: Vec<(u64, Waker)>,
    next_registration: u64,
}

/// One subscriber's registration on a [`WakerSet`].
///
/// It names the set's entry the subscriber registered last. Once the set's
/// producer took that entry, it names nothing, and the next
/// [`WakerSet::register`] registers anew.
#[derive(Default, Debug)]
pub struct WakeRegistration(Option<u64>);

impl WakeRegistration {
    /// Whether this subscriber registered since it last withdrew, so a
    /// drop knows whether it has an entry to withdraw. The producer may
    /// already have taken that entry.
    pub fn is_registered(&self) -> bool {
        self.0.is_some()
    }
}

/// Wakers taken from a [`WakerSet`] under its producer's lock, to be woken
/// after the lock is released.
#[must_use = "taken wakers must be woken once the producer lock is released"]
pub struct TakenWakers(Vec<(u64, Waker)>);

impl WakerSet {
    /// Keep `registration` for `waker`: register it unless it is still on
    /// the set with a waker that wakes the same task, replacing any older
    /// registration it held.
    pub fn register(&mut self, registration: &mut WakeRegistration, waker: &Waker) {
        let held = registration.0.is_some_and(|id| {
            self.wakers
                .iter()
                .any(|(registered, existing)| *registered == id && existing.will_wake(waker))
        });
        if held {
            return;
        }
        self.withdraw(registration);
        let id = self.next_registration;
        self.next_registration += 1;
        self.wakers.push((id, waker.clone()));
        registration.0 = Some(id);
    }

    /// Remove exactly `registration`'s entry, if the set still holds it.
    pub fn withdraw(&mut self, registration: &mut WakeRegistration) {
        if let Some(id) = registration.0.take() {
            self.wakers.retain(|(registered, _)| *registered != id);
        }
    }

    /// Take every registration, leaving the set empty.
    pub fn take(&mut self) -> TakenWakers {
        TakenWakers(std::mem::take(&mut self.wakers))
    }

    /// Registrations currently on the set.
    pub fn len(&self) -> usize {
        self.wakers.len()
    }

    /// Whether no poller is registered.
    pub fn is_empty(&self) -> bool {
        self.wakers.is_empty()
    }
}

impl TakenWakers {
    /// Wake every taken registration once.
    pub fn wake(self) {
        for (_, waker) in self.0 {
            waker.wake();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::task::Wake;

    struct Counting(AtomicUsize);

    impl Wake for Counting {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    fn counting() -> (Arc<Counting>, Waker) {
        let counter = Arc::new(Counting(AtomicUsize::new(0)));
        (Arc::clone(&counter), Waker::from(counter))
    }

    #[test]
    fn a_repoll_keeps_one_registration_and_another_waker_replaces_it() {
        let mut set = WakerSet::default();
        let mut registration = WakeRegistration::default();
        let (first_count, first) = counting();
        let (second_count, second) = counting();
        set.register(&mut registration, &first);
        set.register(&mut registration, &first);
        assert_eq!(set.len(), 1);
        set.register(&mut registration, &second);
        assert_eq!(set.len(), 1);
        set.take().wake();
        assert_eq!(first_count.0.load(Ordering::SeqCst), 0);
        assert_eq!(second_count.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn withdrawing_leaves_a_sibling_with_the_same_waker() {
        let mut set = WakerSet::default();
        let (count, waker) = counting();
        let mut dropped = WakeRegistration::default();
        let mut kept = WakeRegistration::default();
        set.register(&mut dropped, &waker);
        set.register(&mut kept, &waker);
        set.withdraw(&mut dropped);
        assert_eq!(set.len(), 1);
        set.take().wake();
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_registration_taken_by_its_producer_registers_anew() {
        let mut set = WakerSet::default();
        let (count, waker) = counting();
        let mut registration = WakeRegistration::default();
        set.register(&mut registration, &waker);
        set.take().wake();
        assert!(set.is_empty());
        set.register(&mut registration, &waker);
        assert_eq!(set.len(), 1, "a taken registration is not still held");
        set.withdraw(&mut registration);
        assert!(set.is_empty());
        assert_eq!(count.0.load(Ordering::SeqCst), 1);
    }
}
