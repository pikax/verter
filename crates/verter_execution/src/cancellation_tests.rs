use super::*;

fn nested_cancellation_probe(subscription: bool) {
    let leaf = CancellationToken::new();
    let inner = CancellationToken::aggregate();
    let outer = CancellationToken::aggregate();
    let _inner_owner = inner.register_owner(Some(leaf.clone())).unwrap();
    let _outer_owner = outer.register_owner(Some(inner.clone())).unwrap();
    let (count, waker) = counting();
    let mut waiting = outer.subscribe();
    assert!(waiting.poll(&waker).is_pending());
    // The leaf flag can become visible before its callbacks are delivered.
    // Discovering it through either outer entry must not re-enter locks.
    leaf.state.cancelled.store(true, Ordering::Release);
    if subscription {
        assert!(waiting.poll(&waker).is_ready());
        assert_eq!(
            wakes(&count),
            0,
            "the ready subscriber withdrew before delivery"
        );
    } else {
        assert!(outer.is_cancelled());
        assert_eq!(wakes(&count), 1);
    }
    assert!(inner.is_cancelled());
    assert!(waiting.poll(&waker).is_ready());
    assert_eq!(registered(&inner) + registered(&outer), 0);
}

#[test]
fn nested_aggregate_owner_probing_delivers_after_unlock() {
    nested_cancellation_probe(false);
}

#[test]
fn nested_aggregate_subscription_probing_delivers_after_unlock() {
    nested_cancellation_probe(true);
}

fn probes_opened() -> u64 {
    PROBES_OPENED.with(std::cell::Cell::get)
}

/// Every semantic checkpoint asks a request token: it answers from its flag
/// alone, before and after it latches. Only an unlatched aggregate opens a
/// probe of its owners; a latched one answers from its flag too.
#[test]
fn only_an_unlatched_aggregate_opens_a_probe() {
    let request = CancellationToken::new();
    let before = probes_opened();
    assert!(!request.is_cancelled());
    request.cancel();
    assert!(request.is_cancelled());
    assert_eq!(
        probes_opened() - before,
        0,
        "a request token opens no probe"
    );

    let owner_request = CancellationToken::new();
    let aggregate = CancellationToken::aggregate();
    let owner = aggregate
        .register_owner(Some(owner_request.clone()))
        .unwrap();
    let before = probes_opened();
    assert!(!aggregate.is_cancelled());
    assert_eq!(
        probes_opened() - before,
        1,
        "a live aggregate probes its owners"
    );
    owner_request.cancel();
    assert!(aggregate.is_cancelled());
    let before = probes_opened();
    assert!(aggregate.is_cancelled());
    assert_eq!(
        probes_opened() - before,
        0,
        "a latched aggregate answers from its flag"
    );
    drop(owner);
}

#[test]
fn new_token_is_not_cancelled() {
    assert!(!CancellationToken::new().is_cancelled());
}

#[test]
fn cancel_then_observe() {
    let t = CancellationToken::new();
    t.cancel();
    assert!(t.is_cancelled());
}

#[test]
fn cancel_is_idempotent() {
    let t = CancellationToken::new();
    t.cancel();
    t.cancel();
    assert!(t.is_cancelled());
}

#[test]
fn clones_share_state() {
    let t = CancellationToken::new();
    let c = t.clone();
    t.cancel();
    assert!(c.is_cancelled(), "clone observes a cancel on the sibling");
}

#[test]
fn default_matches_new() {
    assert!(!CancellationToken::default().is_cancelled());
}

#[test]
fn aggregate_stays_live_while_any_request_owner_is_live() {
    let aggregate = CancellationToken::aggregate();
    let first_request = CancellationToken::new();
    let second_request = CancellationToken::new();
    let _first = aggregate
        .register_owner(Some(first_request.clone()))
        .expect("first owner registers");
    let _second = aggregate
        .register_owner(Some(second_request.clone()))
        .expect("second owner registers");

    first_request.cancel();
    assert!(!aggregate.is_cancelled());
    second_request.cancel();
    assert!(aggregate.is_cancelled());
}

#[test]
fn late_owner_cannot_revive_an_unpolled_cancelled_aggregate() {
    let aggregate = CancellationToken::aggregate();
    let first_request = CancellationToken::new();
    let _first = aggregate
        .register_owner(Some(first_request.clone()))
        .expect("first owner registers");

    first_request.cancel();
    let late_request = CancellationToken::new();
    assert!(aggregate.register_owner(Some(late_request)).is_none());
    assert!(aggregate.is_cancelled());
}

#[test]
fn owner_batch_keeps_live_requests_in_either_order() {
    for cancelled_first in [false, true] {
        let aggregate = CancellationToken::aggregate();
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        let live = CancellationToken::new();
        let requests = if cancelled_first {
            [Some(cancelled), Some(live.clone())]
        } else {
            [Some(live.clone()), Some(cancelled)]
        };
        let (count, waker) = counting();
        let mut waiting = aggregate.subscribe();
        assert!(waiting.poll(&waker).is_pending());
        let registrations = aggregate.register_owners(requests).unwrap();
        assert_eq!(registrations.len(), 2);
        assert!(registrations[usize::from(cancelled_first)]
            .state
            .request
            .as_ref()
            .unwrap()
            .same_source(&live));
        assert!(!aggregate.is_cancelled());
        assert_eq!(wakes(&count), 0);
        live.cancel();
        assert!(waiting.poll(&waker).is_ready());
        assert_eq!(wakes(&count), 1);
        assert!(aggregate.register_owners([None]).is_none());
        drop(registrations);
        assert_eq!(registered(&live), 0);
    }
}

#[test]
fn owner_batch_preserves_ownerless_and_terminal_states() {
    let aggregate = CancellationToken::aggregate();
    assert!(aggregate.register_owners([]).unwrap().is_empty());
    assert!(!aggregate.has_registered_owners());
    assert!(!aggregate.is_cancelled());
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    let mut registrations = aggregate
        .register_owners([Some(cancelled.clone()), None])
        .unwrap();
    assert!(!aggregate.is_cancelled(), "an uncancellable owner is live");
    drop(registrations.pop());
    // No probe between the last live owner's detach and registration: the
    // registration itself must discover the terminal gap, just as a single
    // late owner does.
    assert!(aggregate.register_owners([None]).is_none());
    assert!(aggregate.is_cancelled());
    drop(registrations);

    let all_cancelled = CancellationToken::aggregate();
    let (count, waker) = counting();
    let mut waiting = all_cancelled.subscribe();
    assert!(waiting.poll(&waker).is_pending());
    let registrations = all_cancelled
        .register_owners([Some(cancelled.clone()), Some(cancelled)])
        .unwrap();
    assert!(waiting.poll(&waker).is_ready());
    assert_eq!(wakes(&count), 1);
    assert!(all_cancelled.register_owners([None]).is_none());
    drop(registrations);

    let explicitly_cancelled = CancellationToken::aggregate();
    explicitly_cancelled.cancel();
    assert!(explicitly_cancelled.register_owners([None]).is_none());
}

fn examined(aggregate: &CancellationToken) -> u64 {
    aggregate
        .state
        .owners
        .as_ref()
        .expect("aggregate token")
        .entries
        .lock()
        .examined
}

/// Cancelling sibling owners one by one, polling after each, examines
/// every registration a bounded number of times: the work grows
/// linearly with the sibling count, never with the owners still
/// registered.
#[test]
fn sibling_owner_cancellations_examine_each_owner_a_bounded_number_of_times() {
    let mut per_sibling = Vec::new();
    for siblings in [128_u64, 256, 512, 1024] {
        let aggregate = CancellationToken::aggregate();
        let requests: Vec<CancellationToken> =
            (0..siblings).map(|_| CancellationToken::new()).collect();
        let registrations: Vec<CancellationOwner> = requests
            .iter()
            .map(|request| {
                aggregate
                    .register_owner(Some(request.clone()))
                    .expect("live aggregate accepts owners")
            })
            .collect();
        let before = examined(&aggregate);
        // Cancel from the oldest registration forward, then from the
        // newest back: both ends of the owner list are exercised.
        let half = requests.len() / 2;
        for request in requests[..half].iter().chain(requests[half..].iter().rev()) {
            assert!(!aggregate.is_cancelled() || request.is_cancelled());
            request.cancel();
        }
        assert!(aggregate.is_cancelled(), "no live owner remains");
        let visited = examined(&aggregate) - before;
        assert!(
            visited <= 3 * siblings,
            "{siblings} sibling cancellations examined {visited} owner entries",
        );
        per_sibling.push(visited / siblings);
        drop(registrations);
    }
    assert!(
        per_sibling.windows(2).all(|pair| pair[0] == pair[1]),
        "per-sibling probe work is independent of the sibling count: {per_sibling:?}",
    );
}

/// Dropping every registration lets the probe discard each owner, and
/// later live owners are still observed.
#[test]
fn dropped_owners_are_discarded_and_a_live_owner_keeps_the_job() {
    let aggregate = CancellationToken::aggregate();
    let survivor_request = CancellationToken::new();
    let survivor = aggregate
        .register_owner(Some(survivor_request.clone()))
        .expect("first owner registers");
    let transient: Vec<CancellationOwner> = (0..64)
        .map(|_| aggregate.register_owner(None).expect("owner registers"))
        .collect();
    drop(transient);
    assert!(!aggregate.is_cancelled(), "the first owner is still live");
    let owners = aggregate.state.owners.as_ref().expect("aggregate token");
    assert_eq!(
        owners.entries.lock().owners.len(),
        1,
        "only the live owner stays registered",
    );
    survivor_request.cancel();
    assert!(aggregate.is_cancelled());
    assert!(owners.entries.lock().owners.is_empty());
    drop(survivor);
}

/// A job that is never left ownerless keeps a bounded owner list: each
/// successor registers before its predecessor departs, so every
/// departed owner sits below a live one, with or without a long-lived
/// builder at the head. The retired-registration storage stays bounded
/// by the live owners, and the work per handoff stays constant.
#[test]
fn rolling_owner_handoffs_retain_only_a_bounded_owner_list() {
    for with_builder in [false, true] {
        let mut per_handoff = Vec::new();
        for handoffs in [128_u64, 256, 512, 1024] {
            let aggregate = CancellationToken::aggregate();
            let builder = with_builder.then(|| {
                aggregate
                    .register_owner(None)
                    .expect("the builder registers")
            });
            let mut request = CancellationToken::new();
            let mut current = aggregate
                .register_owner(Some(request.clone()))
                .expect("the first joiner registers");
            let before = examined(&aggregate);
            for _ in 0..handoffs {
                let next_request = CancellationToken::new();
                let next = aggregate
                    .register_owner(Some(next_request.clone()))
                    .expect("a successor joins a live job");
                request.cancel();
                drop(current);
                assert!(!aggregate.is_cancelled(), "the successor keeps the job");
                (request, current) = (next_request, next);
            }
            let owners = aggregate.state.owners.as_ref().expect("aggregate token");
            let entries = owners.entries.lock();
            assert!(
                entries.owners.len() <= MIN_OWNER_COMPACTION
                    && entries.owners.capacity() <= 2 * MIN_OWNER_COMPACTION,
                "{handoffs} handoffs with builder={with_builder} retain {} owner entries \
                 (capacity {})",
                entries.owners.len(),
                entries.owners.capacity(),
            );
            let visited = entries.examined - before;
            drop(entries);
            assert!(
                visited <= 4 * handoffs,
                "{handoffs} handoffs examined {visited} owner entries",
            );
            per_handoff.push(visited / handoffs);
            drop(current);
            drop(builder);
            assert!(aggregate.is_cancelled(), "no owner remains");
        }
        assert!(
            per_handoff.windows(2).all(|pair| pair[0] == pair[1]),
            "per-handoff work is independent of the handoff count: {per_handoff:?}",
        );
    }
}

/// Counts its wakes.
#[derive(Default)]
struct Counting(std::sync::atomic::AtomicUsize);

impl Wake for Counting {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn counting() -> (Arc<Counting>, Waker) {
    let counter = Arc::new(Counting::default());
    (Arc::clone(&counter), Waker::from(counter))
}

fn wakes(counter: &Counting) -> usize {
    counter.0.load(Ordering::SeqCst)
}

fn registered(token: &CancellationToken) -> usize {
    token.state.wakers.lock().len()
}

/// A cancellation that lands before the first poll answers it without
/// a registration; one that lands while it is pending wakes it once,
/// and the next poll answers.
#[test]
fn a_subscription_answers_or_is_woken_by_the_cancellation() {
    let early = CancellationToken::new();
    early.cancel();
    let (count, waker) = counting();
    assert!(early.subscribe().poll(&waker).is_ready());
    assert_eq!((registered(&early), wakes(&count)), (0, 0));

    let token = CancellationToken::new();
    let mut subscription = token.subscribe();
    assert!(subscription.poll(&waker).is_pending());
    assert!(subscription.poll(&waker).is_pending());
    assert_eq!(registered(&token), 1, "a re-poll keeps one registration");
    token.cancel();
    token.cancel();
    assert_eq!(wakes(&count), 1, "the cancellation wakes the poll once");
    assert!(subscription.poll(&waker).is_ready());
    assert!(subscription.poll(&waker).is_ready());
}

/// Re-polling with another waker replaces the registration; dropping a
/// subscription withdraws only its own, so a sibling registered with the
/// same waker is still woken, and nothing is cancelled.
#[test]
fn a_dropped_subscription_withdraws_only_itself() {
    let token = CancellationToken::new();
    let (first_count, first) = counting();
    let (second_count, second) = counting();
    let mut replaced = token.subscribe();
    assert!(replaced.poll(&first).is_pending());
    assert!(replaced.poll(&second).is_pending());
    let mut sibling = token.subscribe();
    assert!(sibling.poll(&second).is_pending());
    let mut dropped = token.subscribe();
    assert!(dropped.poll(&second).is_pending());
    drop(dropped);
    assert_eq!(registered(&token), 2);
    assert!(!token.is_cancelled(), "a dropped wait cancels nothing");
    token.cancel();
    assert_eq!(wakes(&first_count), 0, "the replaced waker is not woken");
    assert_eq!(wakes(&second_count), 2, "each live subscription is woken");
    assert!(sibling.poll(&second).is_ready());
    drop(replaced);
    assert_eq!(registered(&token), 0);
}

/// An aggregate wakes its subscribers when its last live owner's request
/// cancels, not before, and when its last live owner detaches.
#[test]
fn an_aggregate_wakes_when_its_last_live_owner_goes() {
    for detach_last in [false, true] {
        let aggregate = CancellationToken::aggregate();
        let first = CancellationToken::new();
        let second = CancellationToken::new();
        let first_owner = aggregate.register_owner(Some(first.clone())).unwrap();
        let mut second_owner = Some(aggregate.register_owner(Some(second.clone())).unwrap());
        let (count, waker) = counting();
        let mut subscription = aggregate.subscribe();
        assert!(subscription.poll(&waker).is_pending());
        first.cancel();
        assert_eq!(wakes(&count), 0, "a live sibling owner keeps the job");
        assert!(subscription.poll(&waker).is_pending());
        if detach_last {
            drop(second_owner.take());
        } else {
            second.cancel();
        }
        assert_eq!(wakes(&count), 1, "detach_last={detach_last}");
        assert!(subscription.poll(&waker).is_ready());
        drop(first_owner);
        drop(second_owner);
        assert_eq!(registered(&first) + registered(&second), 0);
    }
}

#[test]
fn an_aggregate_latches_before_its_owner_delivers_callbacks() {
    #[derive(Default)]
    struct Deferred(Mutex<Vec<TakenWakers>>);
    impl std::fmt::Debug for Deferred {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("Deferred").finish_non_exhaustive()
        }
    }
    impl CancellationWakeDelivery for Deferred {
        fn deliver(&self, wakers: TakenWakers) {
            self.0.lock().push(wakers);
        }
    }
    for terminal in ["cancel", "detach", "request", "probe"] {
        let aggregate = CancellationToken::aggregate();
        let delivery = Arc::new(Deferred::default());
        assert!(aggregate.set_wake_delivery(delivery.clone()));
        let request = CancellationToken::new();
        let mut owner = Some(aggregate.register_owner(Some(request.clone())).unwrap());
        let (count, waker) = counting();
        let mut subscription = aggregate.subscribe();
        assert!(subscription.poll(&waker).is_pending());
        match terminal {
            "cancel" => aggregate.cancel(),
            "detach" => drop(owner.take()),
            "request" => request.cancel(),
            "probe" => {
                // Model a request flag changing before its callback runs:
                // the subscription's readiness probe discovers it first.
                request.state.cancelled.store(true, Ordering::Release);
                assert!(aggregate.subscribe().poll(Waker::noop()).is_ready());
            }
            _ => unreachable!(),
        }
        assert!(aggregate.is_cancelled(), "{terminal}");
        assert_eq!(wakes(&count), 0, "callbacks wait for owner unlock");
        let pending = std::mem::take(&mut *delivery.0.lock());
        assert_eq!(pending.len(), 1, "{terminal}");
        for wakers in pending {
            wakers.wake();
        }
        assert_eq!(wakes(&count), 1, "{terminal}");
        assert!(subscription.poll(&waker).is_ready());
    }
}

/// An owner whose request was already cancelled when it registered still
/// wakes the aggregate's subscribers once no live owner remains.
#[test]
fn an_owner_registered_cancelled_delivers_its_aggregate() {
    let aggregate = CancellationToken::aggregate();
    let builder = aggregate.register_owner(None).unwrap();
    let (count, waker) = counting();
    let mut subscription = aggregate.subscribe();
    assert!(subscription.poll(&waker).is_pending());
    let request = CancellationToken::new();
    request.cancel();
    let late = aggregate.register_owner(Some(request)).unwrap();
    assert_eq!(wakes(&count), 0, "the builder keeps the job live");
    drop(builder);
    assert_eq!(wakes(&count), 1);
    drop(late);
    assert!(subscription.poll(&waker).is_ready());
}

/// A cancellation racing the poll's registration is never lost: every
/// poll that returned `Pending` is woken.
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_cancellation_racing_registration_always_wakes() {
    for _ in 0..200 {
        for aggregate in [false, true] {
            let (token, request, owner) = if aggregate {
                let token = CancellationToken::aggregate();
                let request = CancellationToken::new();
                let owner = token.register_owner(Some(request.clone())).unwrap();
                (token, request, Some(owner))
            } else {
                let token = CancellationToken::new();
                (token.clone(), token, None)
            };
            let (count, waker) = counting();
            let mut subscription = token.subscribe();
            let canceller = std::thread::spawn(move || request.cancel());
            let pending = subscription.poll(&waker).is_pending();
            canceller.join().unwrap();
            if pending {
                assert_eq!(wakes(&count), 1, "aggregate={aggregate}");
            }
            assert!(subscription.poll(&waker).is_ready());
            drop(owner);
        }
    }
}

#[test]
fn detached_final_owner_cancels_before_a_late_registration() {
    let aggregate = CancellationToken::aggregate();
    let owner = aggregate
        .register_owner(None)
        .expect("uncancellable owner registers");
    owner.detach();

    assert!(aggregate.register_owner(None).is_none());
    assert!(aggregate.is_cancelled());
}
