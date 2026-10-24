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
fn attaching_pre_admission_owners_preserves_a_live_sibling_after_cancelled_first_owner() {
    for cancel_all in [false, true] {
        let scheduler = scheduler();
        let flight = ScopedCacheFlight::new(
            scheduler.dag.clone(),
            scheduler.task_registry.register_task(),
        );
        assert!(flight.try_add_owner(1, Some(CancellationToken::new())));
        assert!(flight.try_add_owner(2, Some(CancellationToken::new())));
        // Use the actual map order: the cancelled owner must be attached first.
        let (cancelled_id, sibling_id) = {
            let state = flight.state.lock();
            let mut owners = state.owners.iter();
            let (cancelled_id, first) = owners.next().unwrap();
            let (sibling_id, sibling) = owners.next().unwrap();
            first.request.as_ref().unwrap().cancel();
            if cancel_all {
                sibling.request.as_ref().unwrap().cancel();
            }
            (*cancelled_id, *sibling_id)
        };
        let aggregate = CancellationToken::aggregate();
        assert_eq!(flight.attach_aggregate(aggregate.clone()), !cancel_all);
        assert_eq!(aggregate.is_cancelled(), cancel_all);
        if cancel_all {
            assert!(matches!(
                flight.terminal(),
                Some(ScopedCacheTerminal::Cancelled)
            ));
            assert_eq!(flight.owner_count(), 0);
        } else {
            assert!(flight.terminal().is_none());
            assert_eq!(flight.owner_count(), 2);
            assert!(!flight.detach_owner(cancelled_id));
            assert!(flight.detach_owner(sibling_id));
            assert!(aggregate.is_cancelled());
        }
    }
}

#[test]
fn dropping_the_last_scoped_owner_releases_its_edge_before_notifying_the_producer() {
    use verter_execution::tasks::{ExecutionScope, ExecutionTask};
    struct Probe {
        waiter: ExecutionTask,
        producer: ExecutionTask,
        observed: Arc<AtomicUsize>,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.observed.store(
                if self.producer.wait_for(&self.waiter).is_ok() {
                    1
                } else {
                    2
                },
                Ordering::SeqCst,
            );
        }
    }
    let scheduler = scheduler();
    let waiter = scheduler.task_registry.register_task();
    let mut root = {
        let _scope = ExecutionScope::install(&scheduler.task_registry, &waiter);
        scheduler.begin_scoped_cache_node::<u64>(request()).unwrap()
    };
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    let flight = scheduler
        .scoped_cache_flights
        .get(&request().identity())
        .unwrap()
        .clone();
    let aggregate = flight.state.lock().aggregate.clone().unwrap();
    let observed = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(Probe {
        waiter,
        producer: flight.task.clone(),
        observed: observed.clone(),
    }));
    let mut stop = aggregate.subscribe();
    assert!(stop.poll(&waker).is_pending());
    drop(root);
    assert_eq!(
        observed.load(Ordering::SeqCst),
        1,
        "last-owner detach must notify after removing its wait edge"
    );
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
    drop(producer);
}

#[test]
fn scoped_handoff_suspends_without_running_its_producer_and_resumes_elsewhere() {
    let scheduler = scheduler();
    let scheduler_lifetime = Arc::downgrade(&scheduler);
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
    assert_eq!(
        scheduler.task_registry.active_task_count_for_tests(),
        0,
        "completed handoffs release their task pins before they are dropped"
    );
    drop(scheduler);
    assert!(
        scheduler_lifetime.upgrade().is_none(),
        "completed handoffs must release their scheduler owner"
    );
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
    let cancelled_wake = Arc::new(CountWake::default());
    let cancelled_waker = Waker::from(Arc::clone(&cancelled_wake));
    assert!(root.poll(Waker::noop()).is_pending());
    assert!(follower.poll(&cancelled_waker).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    assert!(follower.poll(&cancelled_waker).is_pending());
    let before = cancelled_wake.0.load(Ordering::SeqCst);
    token.cancel();
    assert_eq!(cancelled_wake.0.load(Ordering::SeqCst), before + 1);
    assert!(matches!(
        follower.poll(Waker::noop()),
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
fn scoped_and_completion_waits_share_one_cycle_authority() {
    use verter_execution::tasks::ExecutionScope;
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the sole producer");
    };
    let task = scheduler
        .scoped_cache_flights
        .get(&request().identity())
        .unwrap()
        .task
        .clone();
    let other = scheduler.task_registry.register_task();
    let (handle, sender) = completion_pair::<u64>();
    let mut completion = handle
        .subscribe()
        .owned(task, other.clone(), CancellationToken::new());
    assert!(completion.poll(Waker::noop()).is_pending());
    let mut follower = {
        let _scope = ExecutionScope::install(&scheduler.task_registry, &other);
        scheduler.begin_scoped_cache_node::<u64>(request()).unwrap()
    };
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Err(ScopedCacheNodeError::Reentrant))
    ));
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 2);
    sender.send(CompletionState::Ready(7));
    assert!(matches!(
        completion.poll(Waker::noop()),
        Poll::Ready(Ok(CompletionState::Ready(7)))
    ));
    producer.run(|_| 49);
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 49)
    );
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn a_transferred_producer_cannot_wait_for_its_waiting_parent_task() {
    use verter_execution::dependency::DependencyStop;
    use verter_execution::tasks::ExecutionScope;
    let scheduler = scheduler();
    let parent = scheduler.task_registry.register_task();
    let mut root = {
        let _scope = ExecutionScope::install(&scheduler.task_registry, &parent);
        scheduler
            .begin_scoped_cache_node::<bool>(request())
            .unwrap()
    };
    // A sibling dispatches before the parent's first poll. Claiming must
    // register the parent wait even without a previous Pending result.
    let mut dispatcher = scheduler
        .begin_scoped_cache_node::<bool>(request())
        .unwrap();
    assert!(dispatcher.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    let (handle, _sender) = completion_pair::<u64>();
    producer.run(|_| {
        let task = ExecutionScope::current(&scheduler.task_registry).unwrap();
        let mut child = handle
            .subscribe()
            .owned(task, parent.clone(), CancellationToken::new());
        matches!(
            child.poll(Waker::noop()),
            Poll::Ready(Err(DependencyStop::Cycle))
        )
    });
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(refused))) if *refused)
    );
    assert!(
        matches!(dispatcher.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(refused))) if *refused)
    );
    scheduler.drive_all();
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn owned_request_closes_a_scoped_cache_source_cycle_across_threads() {
    use verter_execution::tasks::ExecutionScope;
    struct CycleLoader {
        scheduler: Mutex<std::sync::Weak<Scheduler>>,
    }
    impl SourceLoader for CycleLoader {
        fn load(&self, _: &str) -> Option<Arc<str>> {
            let scheduler = self.scheduler.lock().upgrade().expect("live scheduler");
            let task =
                ExecutionScope::current(&scheduler.task_registry).expect("actual source task");
            // A fresh thread has no active-path fallback. Only the shared
            // registry can see cache -> Source -> cache here.
            std::thread::spawn(move || {
                let _scope = ExecutionScope::install(&scheduler.task_registry, &task);
                let mut back = scheduler
                    .begin_scoped_cache_node::<bool>(request())
                    .unwrap();
                assert!(matches!(
                    back.poll(Waker::noop()),
                    Poll::Ready(Err(ScopedCacheNodeError::Reentrant))
                ));
            })
            .join()
            .unwrap();
            Some(Arc::from("export const n = 1;"))
        }
        fn exists(&self, _: &str) -> bool {
            true
        }
        fn classify(&self, _: &str) -> verter_language::FileLanguage {
            verter_language::FileLanguage::script_ts()
        }
        fn realpath(&self, _: &str) -> Option<String> {
            None
        }
    }
    let loader = Arc::new(CycleLoader {
        scheduler: Mutex::new(std::sync::Weak::new()),
    });
    let scheduler = Scheduler::test_new_sync(SchedulerConfig::default(), loader.clone());
    *loader.scheduler.lock() = Arc::downgrade(&scheduler);
    let mut root = scheduler
        .begin_scoped_cache_node::<bool>(request())
        .unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.drive_all();
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatched cache producer");
    };
    producer.run(|_| {
        let task = ExecutionScope::current(&scheduler.task_registry).expect("cache task");
        let mut source = scheduler.begin_owned_request(
            Request {
                file_id: "/cycle.ts".into(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: None,
                file_language: None,
                request_context: None,
            },
            task,
            CancellationToken::new(),
        );
        assert!(source.poll(Waker::noop()).is_pending());
        scheduler.drain_inbox();
        assert!(source.poll(Waker::noop()).is_pending());
        assert!(scheduler.drive_one());
        scheduler.drain_inbox();
        matches!(
            source.poll(Waker::noop()),
            Poll::Ready(Ok(CompletionState::Ready(_)))
        )
    });
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(answer))) if *answer)
    );
    scheduler.drive_all();
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
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
