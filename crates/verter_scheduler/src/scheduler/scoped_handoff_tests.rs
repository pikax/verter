use super::*;
use crate::source_loader::MemorySourceLoader;
use std::sync::atomic::AtomicUsize;
use std::task::{Poll, Wake, Waker};

#[derive(Default)]
struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn scheduler() -> Arc<Scheduler> {
    Scheduler::test_new_sync(
        SchedulerConfig::default(),
        Arc::new(MemorySourceLoader::new()),
    )
}

fn request() -> ScopedCacheNodeRequest {
    ScopedCacheNodeRequest {
        cache_id: crate::cache_id::SchedulerCacheId(0x51FC),
        key_hash: [0xA5; 16],
        view_epoch: 7,
        snapshot_pin_id: PinId(11),
        priority: Priority::Interactive,
        request_context: None,
    }
}

#[test]
fn transferred_producer_clears_an_unrelated_executor_request() {
    use verter_execution::request_context::{
        current_request_id, OpaqueContextGuard, OpaqueRequestContext, RequestContextLike,
        TlsUninstall,
    };
    struct Request(u64);
    impl RequestContextLike for Request {
        fn request_id(&self) -> u64 {
            self.0
        }
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            struct Installed {
                _guard: OpaqueContextGuard,
            }
            impl TlsUninstall for Installed {
                fn uninstall(self: Box<Self>) {}
            }
            Box::new(Installed {
                _guard: OpaqueContextGuard::install(OpaqueRequestContext(self)),
            })
        }
    }
    for captured in [None, Some(11)] {
        for driver_request in [None, Some(999)] {
            for panics in [false, true] {
                let scheduler = scheduler();
                let mut submitted = request();
                submitted.request_context =
                    captured.map(|id| OpaqueRequestContext(Arc::new(Request(id))));
                let mut root = scheduler
                    .begin_scoped_cache_node::<Option<u64>>(submitted)
                    .unwrap();
                assert!(root.poll(Waker::noop()).is_pending());
                scheduler.drive_all();
                let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) =
                    root.poll(Waker::noop())
                else {
                    panic!("dispatch transfers the producer");
                };
                let _driver = driver_request.map(|id| {
                    OpaqueContextGuard::install(OpaqueRequestContext(Arc::new(Request(id))))
                });
                producer.run(|_| {
                    let observed = current_request_id();
                    if panics {
                        panic!("producer failure");
                    }
                    observed
                });
                assert_eq!(current_request_id(), driver_request, "restore driver TLS");
                match root.poll(Waker::noop()) {
                    Poll::Ready(Ok(ScopedCacheNodeStep::Value(observed))) if !panics => {
                        assert_eq!(*observed, captured, "work observes its captured request");
                    }
                    Poll::Ready(Err(ScopedCacheNodeError::Panicked)) if panics => {}
                    _ => panic!("producer settles with the expected terminal"),
                }
            }
        }
    }
}

#[test]
fn aggregate_cancellation_wakes_after_scoped_teardown_unlocks() {
    struct Probe {
        scheduler: std::sync::Weak<Scheduler>,
        flight: std::sync::Weak<ScopedCacheFlight>,
        calls: AtomicUsize,
        locked: AtomicBool,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().unwrap();
            let flight = self.flight.upgrade().unwrap();
            // Avoid hanging the test if a regression invokes this callback
            // under a hold. The unlocked branch exercises actual re-entry.
            if scheduler.scoped_cache_gate.try_lock().is_none()
                || scheduler.dag.try_lock().is_none()
                || flight.state.try_lock().is_none()
            {
                self.locked.store(true, Ordering::SeqCst);
            } else {
                drop(scheduler.begin_scoped_cache_node::<u64>(request()).unwrap());
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
    for terminal in [
        "success",
        "panic",
        "producer_drop",
        "follower_drop",
        "reset",
    ] {
        let scheduler = scheduler();
        let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
        assert!(root.poll(Waker::noop()).is_pending());
        scheduler.drive_all();
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop())
        else {
            panic!("dispatch transfers the producer");
        };
        let flight = Arc::clone(
            scheduler
                .scoped_cache_flights
                .get(&request().identity())
                .unwrap()
                .value(),
        );
        let aggregate = flight.state.lock().aggregate.clone().unwrap();
        let probe = Arc::new(Probe {
            scheduler: Arc::downgrade(&scheduler),
            flight: Arc::downgrade(&flight),
            calls: AtomicUsize::new(0),
            locked: AtomicBool::new(false),
        });
        let mut stop = aggregate.subscribe();
        assert!(stop.poll(&Waker::from(Arc::clone(&probe))).is_pending());
        match terminal {
            "success" => producer.run(|_| 41),
            "panic" => producer.run(|_| panic!("producer failure")),
            "producer_drop" => drop(producer),
            "follower_drop" => {
                drop(root);
                drop(producer);
            }
            "reset" => {
                scheduler.reset();
                drop(producer);
            }
            _ => unreachable!(),
        }
        assert_eq!(probe.calls.load(Ordering::SeqCst), 1, "{terminal}");
        assert!(!probe.locked.load(Ordering::SeqCst), "{terminal}");
    }
}

#[test]
fn scoped_handoff_suspends_without_running_its_producer_and_resumes_elsewhere() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    assert!(root.poll(&waker).is_pending());
    assert!(follower.poll(&waker).is_pending());
    assert!(scheduler.drive_one());
    assert!(counter.0.load(Ordering::SeqCst) >= 2);
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(&waker) else {
        panic!("dispatch transfers one producer");
    };
    assert!(follower.poll(&waker).is_pending());
    // No root stack is active while the admitted dependency runs elsewhere.
    std::thread::spawn(move || producer.run(|_| 42))
        .join()
        .unwrap();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) = root.poll(&waker) else {
        panic!("the completed producer answers the root");
    };
    let Poll::Ready(Ok(ScopedCacheNodeStep::Value(shared))) = follower.poll(&waker) else {
        panic!("the same producer answers its follower");
    };
    assert_eq!(*value, 42);
    assert!(Arc::ptr_eq(&value, &shared));
    assert!(scheduler.scoped_cache_flights.is_empty());
}

#[test]
fn dropping_a_scoped_producer_resolves_followers_without_a_second_producer() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    assert!(follower.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("the root claims the producer");
    };
    drop(producer);
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
    ));
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
    ));
    assert!(scheduler.scoped_cache_flights.is_empty());
}

#[test]
fn scoped_handoff_yields_on_inbox_backpressure_before_any_dependency_is_released() {
    let scheduler = scheduler();
    for _ in 0..crate::driver::SUBMISSION_INBOX_CAPACITY {
        scheduler.inbox.sender.try_send(Submission::Wake).unwrap();
    }
    let scheduler_for_poll = Arc::clone(&scheduler);
    let (send, receive) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let mut handoff = scheduler_for_poll
            .begin_scoped_cache_node::<u64>(request())
            .unwrap();
        let pending = handoff.poll(Waker::noop()).is_pending();
        send.send((handoff, pending))
            .unwrap_or_else(|_| panic!("receiver stays live"));
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(1));
    // Release before assertions and join, even if a planted blocking send
    // prevents the initial poll from returning.
    scheduler.drain_inbox();
    poller.join().unwrap();
    let (mut handoff, pending) =
        result.expect("a full inbox must yield before capacity is released");
    assert!(pending);
    assert!(handoff.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = handoff.poll(Waker::noop())
    else {
        panic!("released inbox capacity admits the retained producer");
    };
    producer.run(|_| 17);
    assert!(
        matches!(handoff.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 17)
    );
}

#[test]
fn scoped_result_poll_returns_before_the_held_producer_finishes() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    assert!(follower.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    let (send, receive) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let pending = follower.poll(Waker::noop()).is_pending();
        send.send((follower, pending))
            .unwrap_or_else(|_| panic!("receiver stays live"));
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(1));
    producer.run(|_| 23);
    poller.join().unwrap();
    let (mut follower, pending) =
        result.expect("a result wait must yield before the producer finishes");
    assert!(pending);
    assert!(
        matches!(follower.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 23)
    );
}

#[test]
fn scoped_handoff_cancellation_detaches_only_its_follower() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let (context, token) = super::tests::scoped_test_context(19);
    let mut follower_request = request();
    follower_request.request_context = Some(context);
    let mut follower = scheduler
        .begin_scoped_cache_node::<u64>(follower_request)
        .unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    assert!(follower.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    // With the producer held and no timer, the cancellation alone wakes the
    // pending follower, and its next poll ends it.
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    assert!(follower.poll(&waker).is_pending());
    let before = counter.0.load(Ordering::SeqCst);
    token.cancel();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        before + 1,
        "cancelling the request wakes its pending follower once"
    );
    assert!(matches!(
        follower.poll(&waker),
        Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
    ));
    producer.run(|token| {
        assert!(!token.is_cancelled());
        31
    });
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 31)
    );
}

#[test]
fn scoped_handoff_contains_panics_and_refuses_producer_reentry() {
    for panic in [false, true] {
        let scheduler = scheduler();
        let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
        assert!(root.poll(Waker::noop()).is_pending());
        scheduler.drive_all();
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop())
        else {
            panic!("dispatch transfers the producer");
        };
        producer.run(|_| {
            assert!(matches!(
                scheduler.begin_scoped_cache_node::<u64>(request()),
                Err(ScopedCacheNodeError::Reentrant)
            ));
            if panic {
                panic!("producer failure");
            }
            37
        });
        if panic {
            assert!(matches!(
                root.poll(Waker::noop()),
                Poll::Ready(Err(ScopedCacheNodeError::Panicked))
            ));
        } else {
            assert!(
                matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 37)
            );
        }
    }
}

#[test]
fn reset_resolves_a_scoped_handoff_even_if_the_new_inbox_is_full() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    scheduler.reset();
    for _ in 0..crate::driver::SUBMISSION_INBOX_CAPACITY {
        scheduler.inbox.sender.try_send(Submission::Wake).unwrap();
    }
    let result = root.poll(Waker::noop());
    scheduler.drain_inbox();
    assert!(matches!(
        result,
        Poll::Ready(Err(ScopedCacheNodeError::Shutdown))
    ));
}

#[test]
fn scoped_wakes_observe_released_flight_and_scheduler_locks() {
    struct Probe {
        scheduler: std::sync::Weak<Scheduler>,
        flight: std::sync::Weak<ScopedCacheFlight>,
        calls: AtomicUsize,
        locked: AtomicBool,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().unwrap();
            let flight = self.flight.upgrade().unwrap();
            if scheduler.dag.try_lock().is_none()
                || scheduler.scoped_cache_gate.try_lock().is_none()
                || flight.state.try_lock().is_none()
            {
                self.locked.store(true, Ordering::SeqCst);
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let flight = Arc::clone(
        scheduler
            .scoped_cache_flights
            .get(&request().identity())
            .unwrap()
            .value(),
    );
    let probe = Arc::new(Probe {
        scheduler: Arc::downgrade(&scheduler),
        flight: Arc::downgrade(&flight),
        calls: AtomicUsize::new(0),
        locked: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&probe));
    assert!(root.poll(&waker).is_pending());
    assert!(follower.poll(&waker).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(&waker) else {
        panic!("dispatch transfers the producer");
    };
    assert!(follower.poll(&waker).is_pending());
    producer.run(|_| 41);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 3);
    assert!(!probe.locked.load(Ordering::SeqCst));
}

#[test]
fn scoped_subscribers_replace_and_withdraw_their_own_wakes() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let old = Arc::new(CountWake::default());
    let kept = Arc::new(CountWake::default());
    let old_waker = Waker::from(Arc::clone(&old));
    let kept_waker = Waker::from(Arc::clone(&kept));
    assert!(root.poll(&old_waker).is_pending());
    assert!(root.poll(&old_waker).is_pending());
    assert!(root.poll(&kept_waker).is_pending());
    assert!(follower.poll(&kept_waker).is_pending());
    drop(follower);
    scheduler.drive_all();
    assert_eq!(old.0.load(Ordering::SeqCst), 0);
    assert_eq!(kept.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("the remaining subscriber produces the value");
    };
    producer.run(|_| 43);
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 43)
    );
}

#[test]
fn a_higher_priority_scoped_follower_upgrades_the_admitted_job() {
    let scheduler = scheduler();
    let mut background = request();
    background.priority = Priority::Background;
    let mut root = scheduler
        .begin_scoped_cache_node::<u64>(background)
        .unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drain_inbox();
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(follower.poll(Waker::noop()).is_pending());
    scheduler.drain_inbox();
    let job = scheduler
        .dag
        .lock()
        .next_ready()
        .expect("the shared job is ready");
    assert_eq!(job.priority, Priority::Interactive);
    scheduler.execute_stage_inline(job);
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("the shared job dispatches once");
    };
    producer.run(|_| 47);
    assert!(
        matches!(follower.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 47)
    );
}

#[test]
fn producer_election_wakes_followers_to_register_task_waits() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    // A late follower can remain behind capacity even though the shared
    // flight was dispatched. Election must wake that subscriber as well.
    for _ in 0..crate::driver::SUBMISSION_INBOX_CAPACITY {
        scheduler.inbox.sender.try_send(Submission::Wake).unwrap();
    }
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    assert!(follower.poll(&waker).is_pending());
    assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
    assert_eq!(counter.0.load(Ordering::SeqCst), 0);
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("one handoff receives the producer");
    };
    assert_eq!(counter.0.load(Ordering::SeqCst), 1, "election wake");
    assert!(root.poll(Waker::noop()).is_pending());
    assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
    for _ in 0..3 {
        assert!(follower.poll(&waker).is_pending());
        assert_eq!(scheduler.task_registry.wait_count_for_tests(), 1);
    }
    producer.run(|_| 47);
    assert_eq!(counter.0.load(Ordering::SeqCst), 2, "settlement wake");
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 47
    ));
    assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 47
    ));
    drop((root, follower));
    scheduler.quiesce();
    assert_eq!(scheduler.task_registry.active_task_count_for_tests(), 0);
}

#[test]
fn suspended_same_task_reentry_is_refused_on_another_thread() {
    let scheduler = scheduler();
    let task = scheduler.task_registry.register_task();
    let (mut root, mut nested) = {
        let _scope = ExecutionScope::install(&scheduler.task_registry, &task);
        (
            scheduler.begin_scoped_cache_node::<u64>(request()).unwrap(),
            scheduler.begin_scoped_cache_node::<u64>(request()).unwrap(),
        )
    };
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("the task owns the transferred producer");
    };
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                let unrelated = ExecutionScope::enter(&scheduler.task_registry);
                assert_ne!(unrelated.task().id(), task.id());
                assert!(matches!(
                    nested.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Reentrant))
                ));
                assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
            })
            .join()
            .unwrap();
    });
    producer.run(|_| 53);
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 53
    ));
    drop((root, nested, task));
    assert_eq!(scheduler.task_registry.active_task_count_for_tests(), 0);
}

#[test]
fn scoped_task_waits_release_on_follower_drop_cancellation_reset_and_producer_failure() {
    #[derive(Clone, Copy)]
    enum Terminal {
        FollowerDrop,
        FollowerCancel,
        Reset,
        ProducerPanic,
        ProducerDrop,
    }
    for terminal in [
        Terminal::FollowerDrop,
        Terminal::FollowerCancel,
        Terminal::Reset,
        Terminal::ProducerPanic,
        Terminal::ProducerDrop,
    ] {
        let scheduler = scheduler();
        let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
        let (context, token) = super::tests::scoped_test_context(23);
        let mut joined = request();
        joined.request_context = Some(context);
        let mut follower = scheduler.begin_scoped_cache_node::<u64>(joined).unwrap();
        assert!(root.poll(Waker::noop()).is_pending());
        scheduler.drive_all();
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop())
        else {
            panic!("the root receives the producer");
        };
        assert!(follower.poll(Waker::noop()).is_pending());
        assert_eq!(scheduler.task_registry.wait_count_for_tests(), 1);
        match terminal {
            Terminal::FollowerDrop => {
                drop(follower);
                assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
                producer.run(|_| 59);
            }
            Terminal::FollowerCancel => {
                token.cancel();
                assert!(matches!(
                    follower.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
                ));
                assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
                producer.run(|_| 59);
            }
            Terminal::Reset => {
                scheduler.reset();
                assert!(matches!(
                    follower.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Shutdown))
                ));
                assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
                drop(producer);
            }
            Terminal::ProducerPanic => {
                producer.run(|_| panic!("producer failure"));
                assert!(matches!(
                    follower.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Panicked))
                ));
            }
            Terminal::ProducerDrop => {
                drop(producer);
                assert!(matches!(
                    follower.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Cancelled))
                ));
            }
        }
        assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
        if matches!(terminal, Terminal::FollowerDrop | Terminal::FollowerCancel) {
            assert!(matches!(
                root.poll(Waker::noop()),
                Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 59
            ));
        }
    }
}
