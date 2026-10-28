use super::*;
use crate::dag::ResourceClass;
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

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn completed_file_stage_releases_its_task_before_worker_context_cleanup() {
    use std::sync::mpsc;
    use std::time::Duration;
    use verter_execution::request_context::{
        OpaqueRequestContext, RequestContextLike, TlsUninstall,
    };

    struct HeldCleanup {
        posted: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        released: mpsc::Sender<()>,
    }
    impl TlsUninstall for HeldCleanup {
        fn uninstall(self: Box<Self>) {}
    }
    impl Drop for HeldCleanup {
        fn drop(&mut self) {
            if self.posted.send(()).is_err() {
                return;
            }
            match self.release.recv_timeout(Duration::from_secs(5)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => panic!("consumer did not finish"),
            }
            let _ = self.released.send(());
        }
    }
    struct Context(Mutex<Option<HeldCleanup>>);
    impl RequestContextLike for Context {
        fn request_id(&self) -> u64 {
            1
        }
        fn install_tls(self: Arc<Self>) -> Box<dyn TlsUninstall + Send> {
            Box::new(self.0.lock().take().expect("one source execution"))
        }
    }

    for caller in [
        caller_kind::CallerKind::Driver,
        caller_kind::CallerKind::IoWorker,
    ] {
        let scheduler = scheduler();
        let (posted_tx, posted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (released_tx, released_rx) = mpsc::channel();
        let context = OpaqueRequestContext(Arc::new(Context(Mutex::new(Some(HeldCleanup {
            posted: posted_tx,
            release: release_rx,
            released: released_tx,
        })))));
        let mut root = scheduler.begin_owned_request(
            Request {
                file_id: "/completed.ts".into(),
                target: TargetStage::Source,
                priority: Priority::Interactive,
                source: Some(Arc::from("export const n = 41;")),
                file_language: Some(verter_language::FileLanguage::script_ts()),
                request_context: Some(context),
            },
            scheduler.task_registry.register_task(),
            CancellationToken::new(),
        );
        assert!(root.poll(Waker::noop()).is_pending());
        std::thread::scope(|scope| {
            let dispatch = scope.spawn(|| {
                scheduler.pump_ready(PumpReason::DriverLoop, caller);
            });
            // The stage has posted its terminal envelope, but its worker is
            // still held in unrelated request-context cleanup.
            posted_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            scheduler.drain_inbox();
            assert!(matches!(
                root.poll(Waker::noop()),
                Poll::Ready(Ok(CompletionState::Ready(RequestResult::Source(_))))
            ));
            drop(root);
            assert_eq!(
                scheduler.task_registry.active_task_count_for_tests(),
                0,
                "execution task retained after {caller:?} completion"
            );
            assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
            drop(release_tx);
            released_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            dispatch.join().unwrap();
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn scoped_dispatch_releases_its_task_before_waking_the_consumer() {
    use std::sync::mpsc;
    use std::time::Duration;

    struct HeldWake {
        channels: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>,
    }
    impl Wake for HeldWake {
        fn wake(self: Arc<Self>) {
            let channels = self.channels.lock().take();
            if let Some((ready, release)) = channels {
                ready.send(()).unwrap();
                // Disconnection also releases dispatch during assertion unwind.
                match release.recv_timeout(Duration::from_secs(5)) {
                    Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
                    Err(mpsc::RecvTimeoutError::Timeout) => panic!("consumer did not finish"),
                }
            }
        }
    }

    for native in [true, false] {
        let scheduler = scheduler();
        let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let waker = Waker::from(Arc::new(HeldWake {
            channels: Mutex::new(Some((ready_tx, release_rx))),
        }));
        assert!(root.poll(&waker).is_pending());
        std::thread::scope(|scope| {
            let dispatch = scope.spawn(|| {
                if native {
                    scheduler.pump_ready(PumpReason::DriverLoop, caller_kind::CallerKind::Driver);
                } else {
                    scheduler.drive_all();
                }
            });
            ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            // Dispatch is still inside its readiness callback while another thread
            // can claim, execute and consume the complete answer.
            let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop())
            else {
                panic!("dispatch transfers the producer");
            };
            producer.run(|_| 41);
            assert!(matches!(
                root.poll(Waker::noop()),
                Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 41
            ));
            drop(root);
            assert_eq!(scheduler.task_registry.active_task_count_for_tests(), 0);
            assert_eq!(scheduler.task_registry.wait_count_for_tests(), 0);
            drop(release_tx);
            dispatch.join().unwrap();
        });
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
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
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
    let producer_task = flight.state.lock().producer.as_ref().unwrap().0.clone();
    let mut follower = {
        let _scope = ExecutionScope::install(&scheduler.task_registry, &waiter);
        scheduler.begin_scoped_cache_node::<u64>(request()).unwrap()
    };
    assert!(follower.poll(Waker::noop()).is_pending());
    drop(root);
    let observed = Arc::new(AtomicUsize::new(0));
    let waker = Waker::from(Arc::new(Probe {
        waiter,
        producer: producer_task,
        observed: observed.clone(),
    }));
    let mut stop = aggregate.subscribe();
    assert!(stop.poll(&waker).is_pending());
    drop(follower);
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
fn terminal_scoped_flight_releases_tasks_with_a_late_submission_still_queued() {
    let scheduler = scheduler();
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    assert!(scheduler.drive_one());
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("dispatch transfers the producer");
    };
    let mut follower = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(follower.poll(Waker::noop()).is_pending());
    producer.run(|_| 42);
    for handoff in [&mut root, &mut follower] {
        assert!(matches!(
            handoff.poll(Waker::noop()),
            Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 42
        ));
    }
    assert!(!scheduler.inbox.receiver.is_empty());
    assert_eq!(
        scheduler.task_registry.active_task_count_for_tests(),
        0,
        "terminal flight envelopes must not retain execution tasks"
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
        .state
        .lock()
        .producer
        .as_ref()
        .unwrap()
        .0
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
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 1);
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
    scheduler.quiesce();
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

#[test]
fn settling_a_scoped_producer_notifies_capacity_blocked_dispatch() {
    use crate::caller_kind::CallerKind;
    for terminal in ["success", "cancel", "drop"] {
        let scheduler = Scheduler::test_new_sync(
            SchedulerConfig {
                cpu_threads: 1,
                dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
                ..SchedulerConfig::default()
            },
            Arc::new(MemorySourceLoader::new()),
        );
        let mut first = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
        assert!(first.poll(Waker::noop()).is_pending());
        scheduler.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = first.poll(Waker::noop())
        else {
            panic!("first flight dispatches");
        };
        let mut second_request = request();
        second_request.key_hash = [0xB6; 16];
        let mut second = scheduler
            .begin_scoped_cache_node::<u64>(second_request)
            .unwrap();
        assert!(second.poll(Waker::noop()).is_pending());
        scheduler.pump_ready(PumpReason::DriverLoop, CallerKind::Driver);
        assert!(!scheduler
            .pump_ready(PumpReason::DriverLoop, CallerKind::Driver)
            .made_progress());
        assert!(
            scheduler.inbox.receiver.is_empty(),
            "driver exhausted available work"
        );
        assert!(second.poll(Waker::noop()).is_pending());
        match terminal {
            "success" => producer.run(|_| 41),
            "cancel" => {
                drop(first);
                drop(producer);
            }
            "drop" => drop(producer),
            _ => unreachable!(),
        }
        assert_eq!(scheduler.dag.lock().in_flight_cpu_permits(), 0);
        assert!(
            matches!(scheduler.inbox.receiver.try_recv(), Ok(Submission::Wake)),
            "{terminal}: released capacity must explicitly notify the idle driver"
        );
        scheduler.pump_ready(PumpReason::DriverWake, CallerKind::Driver);
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = second.poll(Waker::noop())
        else {
            panic!("{terminal}: notified driver dispatches the queued flight");
        };
        producer.run(|_| 42);
        assert!(
            matches!(second.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 42)
        );
    }
}

#[test]
fn a_pending_scoped_dependency_borrows_its_active_workers_capacity() {
    let scheduler = Scheduler::test_new_sync(
        SchedulerConfig {
            cpu_threads: 1,
            dag_budget: Some(DagCapacityBudget { cpu: 1, io: 1 }),
            ..SchedulerConfig::default()
        },
        Arc::new(MemorySourceLoader::new()),
    );
    let mut root = scheduler.begin_scoped_cache_node::<u64>(request()).unwrap();
    assert!(root.poll(Waker::noop()).is_pending());
    scheduler.pump_ready(PumpReason::DriverLoop, caller_kind::CallerKind::Driver);
    let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("outer flight dispatches");
    };
    producer.run(|_| {
        let _worker = caller_kind::CallerKindGuard::install(caller_kind::CallerKind::CpuWorker);
        let mut nested = request();
        nested.key_hash = [0xB6; 16];
        let mut child = scheduler.begin_scoped_cache_node::<u64>(nested).unwrap();
        assert!(child.poll(Waker::noop()).is_pending());
        scheduler.dispatch_for_pending_dependency();
        let Poll::Ready(Ok(ScopedCacheNodeStep::Produce(producer))) = child.poll(Waker::noop())
        else {
            panic!("a saturated worker must loan capacity using its active path");
        };
        assert_eq!(scheduler.dag.lock().in_flight_cpu_permits(), 2);
        producer.run(|_| 42);
        let Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) = child.poll(Waker::noop()) else {
            panic!("nested producer completes");
        };
        *value
    });
    assert!(
        matches!(root.poll(Waker::noop()), Poll::Ready(Ok(ScopedCacheNodeStep::Value(value))) if *value == 42)
    );
    assert_eq!(scheduler.dag.lock().in_flight_cpu_permits(), 0);
}

#[test]
fn final_capacity_release_wakes_progress_subscribers_after_lifecycle_unlock() {
    struct Probe {
        scheduler: std::sync::Weak<Scheduler>,
        calls: AtomicUsize,
        locked: AtomicBool,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().unwrap();
            if scheduler.dag.try_lock().is_none()
                || scheduler.scoped_cache_gate.try_lock().is_none()
            {
                self.locked.store(true, Ordering::SeqCst);
            }
            self.calls.fetch_add(1, Ordering::SeqCst);
        }
    }
    let scheduler = scheduler();
    let probe = Arc::new(Probe {
        scheduler: Arc::downgrade(&scheduler),
        calls: AtomicUsize::new(0),
        locked: AtomicBool::new(false),
    });
    let waker = Waker::from(Arc::clone(&probe));
    let mut dropped = scheduler.subscribe_progress();
    let mut live = scheduler.subscribe_progress();
    dropped.register(&waker);
    live.register(&waker);
    drop(dropped);
    let reservation = Arc::new(
        scheduler
            .dag
            .lock()
            .try_reserve_for_class(ResourceClass::Cpu)
            .unwrap(),
    );
    let final_share = Arc::clone(&reservation);
    drop(reservation);
    assert_eq!(
        probe.calls.load(Ordering::SeqCst),
        0,
        "a shared permit remains held"
    );
    {
        let _wakes = scheduler.dag.defer_wakes();
        let _gate = scheduler.scoped_cache_gate.lock();
        let _dag = scheduler.dag.lock();
        drop(final_share);
        assert_eq!(
            probe.calls.load(Ordering::SeqCst),
            0,
            "publication cannot run a callback under lifecycle holds"
        );
    }
    assert_eq!(
        probe.calls.load(Ordering::SeqCst),
        1,
        "only the surviving subscriber wakes"
    );
    assert!(
        !probe.locked.load(Ordering::SeqCst),
        "wake is reentrant after unlock"
    );
    assert!(matches!(
        scheduler.inbox.receiver.try_recv(),
        Ok(Submission::Wake)
    ));
    assert_eq!(scheduler.dag.lock().in_flight_cpu_permits(), 0);

    // Reset invalidates an old reservation's release; it must neither debit
    // a new admission nor advertise newly available capacity.
    let old = scheduler
        .dag
        .lock()
        .try_reserve_for_class(ResourceClass::Cpu)
        .unwrap();
    scheduler.reset();
    scheduler.drain_inbox();
    live.register(&waker);
    let current = scheduler
        .dag
        .lock()
        .try_reserve_for_class(ResourceClass::Cpu)
        .unwrap();
    drop(old);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(scheduler.dag.lock().in_flight_cpu_permits(), 1);
    drop(current);
    assert_eq!(probe.calls.load(Ordering::SeqCst), 2);
}

/// Dispatch can select an admitted node before its submission binds the
/// node's job token to the flight. The owners that arrived before either
/// still own the job: a later follower's cancellation must not cancel it,
/// while every owner's cancellation still does.
#[test]
fn dispatch_before_aggregate_binding_keeps_earlier_owners_on_the_job() {
    let scheduler = scheduler();
    let flight = ScopedCacheFlight::new(
        Arc::clone(&scheduler.dag),
        scheduler.task_registry.register_task(),
    );
    let root = CancellationToken::new();
    let follower = CancellationToken::new();
    assert!(flight.try_add_owner(1, Some(root.clone())));
    let aggregate = CancellationToken::aggregate();
    assert!(flight.mark_dispatched(aggregate.clone()));
    assert!(flight.attach_aggregate(aggregate.clone()));
    assert!(flight.try_add_owner(2, Some(follower.clone())));
    follower.cancel();
    assert!(
        !aggregate.is_cancelled(),
        "the earlier owner still wants the job"
    );
    root.cancel();
    assert!(aggregate.is_cancelled(), "no owner wants the job any more");
}

fn bind_with_a_cancelled_owner(dispatch_first: bool) {
    let scheduler = scheduler();
    let flight = ScopedCacheFlight::new(
        Arc::clone(&scheduler.dag),
        scheduler.task_registry.register_task(),
    );
    assert!(flight.try_add_owner(1, Some(CancellationToken::new())));
    assert!(flight.try_add_owner(2, Some(CancellationToken::new())));
    // Cancel the first owner in this map's iteration order, after insertion
    // but before binding, so the regression is independent of the hash seed.
    let live_owner_id = {
        let state = flight.state.lock();
        let mut owners = state.owners.iter();
        owners.next().unwrap().1.request.as_ref().unwrap().cancel();
        *owners.next().unwrap().0
    };
    let aggregate = CancellationToken::aggregate();
    let wake = Arc::new(CountWake::default());
    let mut waiting = aggregate.subscribe();
    assert!(waiting.poll(&Waker::from(Arc::clone(&wake))).is_pending());
    if dispatch_first {
        assert!(flight.mark_dispatched(aggregate.clone()));
        assert!(flight.attach_aggregate(aggregate.clone()));
    } else {
        assert!(flight.attach_aggregate(aggregate.clone()));
        assert!(flight.mark_dispatched(aggregate.clone()));
    }
    assert!(!aggregate.is_cancelled(), "the live owner retains the job");
    assert!(flight.terminal().is_none());
    assert_eq!(wake.0.load(Ordering::SeqCst), 0);
    assert!(flight.detach_owner(live_owner_id));
    assert!(aggregate.is_cancelled());
    assert!(matches!(
        flight.terminal(),
        Some(ScopedCacheTerminal::Cancelled)
    ));
    assert_eq!(wake.0.load(Ordering::SeqCst), 1);
}

#[test]
fn admission_binding_keeps_a_live_owner_after_a_cancelled_owner() {
    bind_with_a_cancelled_owner(false);
}

#[test]
fn dispatch_binding_keeps_a_live_owner_after_a_cancelled_owner() {
    bind_with_a_cancelled_owner(true);
}
