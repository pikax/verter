use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::{Poll, Wake, Waker};
use verter_execution::cancellation::CancellationToken;
use verter_execution::tasks::{ExecutionScope, TaskRegistry};

const TOKEN: StoreViewCompatToken = StoreViewCompatToken {
    epoch: 1,
    session: None,
    validity_fingerprint: 0,
};

#[derive(Default)]
struct Wakes(AtomicUsize);
impl Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn owned_resolver_flight_transfers_work_and_cancels_only_one_follower() {
    fn assert_send<T: Send>() {}
    assert_send::<OwnedResolverFlight<String, usize, ()>>();
    assert_send::<OwnedResolverProducer<String, usize, ()>>();
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    let owner = registry.register_task();
    let owner_id = owner.id();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let owner_stop = CancellationToken::new();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        owner.clone(),
        owner_stop.clone(),
        move || {
            assert!(
                !verter_execution::cancellation::current_job_cancellation_token()
                    .is_some_and(|token| token.is_cancelled())
            );
            assert_ne!(ExecutionScope::current_task().unwrap().id(), owner_id);
            observed.fetch_add(1, Ordering::SeqCst);
            Ok(7)
        },
        |_| true,
    );
    let waker = Waker::noop();
    let producer = match leader.poll(waker) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("the first poll transfers the producer"),
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0, "poll must not compute");
    let stop = CancellationToken::new();
    let mut cancelled = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        stop.clone(),
        || Ok(99),
        |_| true,
    );
    let mut sibling = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    let wakes = Arc::new(Wakes::default());
    let wake = Waker::from(wakes.clone());
    assert!(cancelled.poll(&wake).is_pending());
    assert!(sibling.poll(waker).is_pending());
    stop.cancel();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        cancelled.poll(&wake),
        Poll::Ready(Err(ResolverFlightError::Cancelled))
    ));
    owner_stop.cancel();
    assert!(matches!(
        leader.poll(waker),
        Poll::Ready(Err(ResolverFlightError::Cancelled))
    ));
    let producer_thread = std::thread::spawn(move || {
        let _scope = verter_execution::cancellation::JobCancellationGuard::install(owner_stop);
        producer.run();
        assert!(
            verter_execution::cancellation::current_job_cancellation_token()
                .unwrap()
                .is_cancelled()
        );
    });
    producer_thread.join().unwrap();
    assert!(
        matches!(sibling.poll(waker), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    drop((leader, cancelled, sibling, group, owner));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_resolver_flight_reports_dropped_panicked_and_failed_production() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    for mode in 0..4 {
        let registry = TaskRegistry::default();
        let group = SingleflightGroup::<String, usize, &'static str>::default();
        let mut leader = group.begin_owned_retaining(
            "key".into(),
            TOKEN,
            registry.register_task(),
            CancellationToken::new(),
            move || match mode {
                1 => panic!("producer panic"),
                2 => Err("failed"),
                _ => Ok(7),
            },
            move |_| {
                assert_ne!(mode, 3, "retention panic");
                true
            },
        );
        let producer = match leader.poll(Waker::noop()) {
            Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
            _ => panic!("cold producer"),
        };
        let mut sibling = group.begin_owned_retaining(
            "key".into(),
            TOKEN,
            registry.register_task(),
            CancellationToken::new(),
            || Ok(99),
            |_| true,
        );
        assert!(sibling.poll(Waker::noop()).is_pending());
        if mode == 0 {
            drop(producer);
        } else {
            assert!(
                catch_unwind(AssertUnwindSafe(|| producer.run())).is_ok(),
                "owned producer panics are typed outcomes, never driver unwinds"
            );
        }
        for flight in [&mut leader, &mut sibling] {
            let error = match flight.poll(Waker::noop()) {
                Poll::Ready(Err(error)) => error,
                _ => panic!("producer failure is terminal"),
            };
            assert_eq!(
                error,
                match mode {
                    0 => ResolverFlightError::ProducerDropped,
                    2 => ResolverFlightError::Failed("failed"),
                    _ => ResolverFlightError::ProducerPanicked,
                }
            );
        }
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(group.test_flight_strong_count(&"key".into(), TOKEN), 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn owned_resolver_flight_refuses_same_path_and_mixed_dependency_cycles() {
    use verter_execution::dependency::{DependencyPoll, DependencyStop, OwnedDependency};
    struct Held;
    impl DependencyPoll for Held {
        type Output = ();
        fn poll_dependency(&mut self, _: &Waker) -> Poll<()> {
            Poll::Pending
        }
    }
    let registry = TaskRegistry::default();
    let group = Arc::new(SingleflightGroup::<String, usize, ()>::default());
    let owner = registry.register_task();
    let parent = owner.clone();
    let nested_group = group.clone();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        owner,
        CancellationToken::new(),
        move || {
            let producer = ExecutionScope::current_task().unwrap();
            let mut recursive = nested_group.begin_owned_retaining(
                "key".into(),
                TOKEN,
                producer.clone(),
                CancellationToken::new(),
                || Ok(99),
                |_| true,
            );
            assert!(matches!(
                recursive.poll(Waker::noop()),
                Poll::Ready(Err(ResolverFlightError::Cycle))
            ));
            let mut other_family =
                OwnedDependency::new(Held, producer, parent, CancellationToken::new());
            assert_eq!(
                other_family.poll(Waker::noop()),
                Poll::Ready(Err(DependencyStop::Cycle))
            );
            Ok(7)
        },
        |_| true,
    );
    match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer.run(),
        _ => panic!("cold producer"),
    }
    assert!(
        matches!(leader.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn resolver_reset_wakes_pending_owner_and_follower_without_running_old_work() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || panic!("retired producer ran"),
        |_| true,
    );
    let producer = match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("cold producer"),
    };
    let mut sibling = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(leader.poll(&waker).is_pending());
    assert!(sibling.poll(&waker).is_pending());
    group.clear();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 2);
    for flight in [&mut leader, &mut sibling] {
        assert!(matches!(
            flight.poll(&waker),
            Poll::Ready(Err(ResolverFlightError::Reset))
        ));
    }
    let mut successor = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(11),
        |_| true,
    );
    let successor_producer = match successor.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("successor producer"),
    };
    producer.run();
    assert_eq!(group.test_flight_strong_count(&"key".into(), TOKEN), 2);
    successor_producer.run();
    assert!(
        matches!(successor.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 11)
    );
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn completed_resolver_rendezvous_needs_no_task_edge_and_withdraws_subscriptions() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    // Pin the burst so a subscriber arriving after publication can adopt it.
    let participation = group.participate_owned("key".into(), TOKEN);
    let owner = registry.register_task();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        owner.clone(),
        CancellationToken::new(),
        || Ok(7),
        |_| true,
    );
    match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer.run(),
        _ => panic!("cold producer"),
    }
    let mut completed = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        owner,
        CancellationToken::new(),
        || panic!("complete answer must not recompute"),
        |_| true,
    );
    assert!(
        matches!(completed.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert!(
        matches!(leader.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert_eq!(registry.active_task_count_for_tests(), 0);
    drop(participation);
    assert_eq!(group.test_flight_strong_count(&"key".into(), TOKEN), 0);
}

#[test]
fn resolver_stop_before_poll_never_claims_a_flight() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    let stop = CancellationToken::new();
    stop.cancel();
    let mut flight = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        stop,
        || panic!("cancelled work ran"),
        |_| true,
    );
    assert!(matches!(
        flight.poll(Waker::noop()),
        Poll::Ready(Err(ResolverFlightError::Cancelled))
    ));
    assert_eq!(group.test_flight_strong_count(&"key".into(), TOKEN), 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn dropping_owned_resolver_follower_withdraws_only_its_edge_and_wake() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(7),
        |_| true,
    );
    let producer = match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("cold producer"),
    };
    let mut dropped = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    let mut kept = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(dropped.poll(&waker).is_pending());
    assert!(kept.poll(&waker).is_pending());
    assert_eq!(registry.wait_graph_occupancy().edges, 3);
    drop(dropped);
    assert_eq!(registry.wait_graph_occupancy().edges, 2);
    producer.run();
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert!(
        matches!(kept.poll(&waker), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert!(
        matches!(leader.poll(&waker), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn composed_resolver_compatibility_uses_its_shared_task_authority() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    group.bind_task_registry(registry.clone());
    let producer_task = group
        .run("key".into(), TOKEN, || {
            let task = ExecutionScope::current_task().expect("actual production task installed");
            let other = registry.register_task();
            let edge = other.wait_for(&task).expect("host authority must match");
            drop(edge);
            Ok(7)
        })
        .unwrap();
    assert_eq!(*producer_task.value, 7);
    assert_eq!(registry.active_task_count_for_tests(), 0);
    let mut foreign = group.begin_owned_retaining(
        "other".into(),
        TOKEN,
        TaskRegistry::default().register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    assert!(matches!(
        foreign.poll(Waker::noop()),
        Poll::Ready(Err(ResolverFlightError::Cycle))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn resolver_reset_keeps_synchronous_producers_complete_answer() {
    let group = Arc::new(SingleflightGroup::<String, usize, ()>::default());
    let (entered, waiting) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let producing = group.clone();
    let producer = std::thread::spawn(move || {
        producing.run("key".into(), TOKEN, || {
            entered.send(()).unwrap();
            released
                .recv_timeout(std::time::Duration::from_secs(30))
                .unwrap();
            Ok(7)
        })
    });
    let observed = waiting.recv_timeout(std::time::Duration::from_secs(30));
    group.clear();
    let _ = release.send(());
    let completed = producer.join();
    observed.expect("producer started");
    assert_eq!(
        *completed
            .expect("reset must not panic the compatibility producer")
            .unwrap()
            .value,
        7
    );
    assert_eq!(group.test_flight_strong_count(&"key".into(), TOKEN), 0);
}

#[test]
fn owned_resolver_poll_yields_before_the_held_producer_is_released() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(7),
        |_| true,
    );
    let producer = match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("cold producer"),
    };
    let mut follower = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        registry.register_task(),
        CancellationToken::new(),
        || Ok(99),
        |_| true,
    );
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    let (entered, ready) = std::sync::mpsc::channel();
    let (polled, pending) = std::sync::mpsc::channel();
    let polling = std::thread::spawn(move || {
        entered.send(()).unwrap();
        polled.send(follower.poll(&waker).is_pending()).unwrap();
        follower
    });
    // Cleanup precedes every assertion, including a watchdog failure. A
    // planted blocking poll is released by production before its join.
    let started = ready.recv_timeout(std::time::Duration::from_secs(30));
    let yielded = pending.recv_timeout(std::time::Duration::from_secs(5));
    producer.run();
    let joined = polling.join();
    started.expect("polling thread started");
    let mut follower = joined.expect("polling thread returned");
    assert_eq!(
        yielded.ok(),
        Some(true),
        "poll must yield while its producer is held"
    );
    assert_eq!(
        wakes.0.load(Ordering::SeqCst),
        1,
        "production must wake the parked subscriber"
    );
    assert!(
        matches!(follower.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    assert!(
        matches!(leader.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
}

#[test]
fn synchronous_resolver_cycle_uses_external_nested_compute_escape() {
    let registry = TaskRegistry::default();
    let group = SingleflightGroup::<String, usize, ()>::default();
    group.bind_task_registry(registry.clone());
    let root = registry.register_task();
    let mut leader = group.begin_owned_retaining(
        "key".into(),
        TOKEN,
        root.clone(),
        CancellationToken::new(),
        || Ok(7),
        |_| true,
    );
    let producer = match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(ResolverFlightStep::Produce(producer))) => producer,
        _ => panic!("cold producer"),
    };
    // The root already waits on the held producer. A compatibility demand
    // must not turn graph refusal into an invented generic caller error.
    let scope = ExecutionScope::install(&registry, &root);
    assert_eq!(*group.run("key".into(), TOKEN, || Ok(9)).unwrap().value, 9);
    drop(scope);
    producer.run();
    assert!(
        matches!(leader.poll(Waker::noop()), Poll::Ready(Ok(ResolverFlightStep::Value(value))) if *value.value == 7)
    );
    drop(root);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
