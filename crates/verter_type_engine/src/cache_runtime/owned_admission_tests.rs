use super::*;
use crate::cache_runtime::NonAdmissionReason;
use dashmap::DashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use verter_execution::cancellation::CancellationToken;
use verter_execution::tasks::{ExecutionScope, TaskRegistry};

#[test]
fn an_owned_follower_refuses_an_unowned_pending_producer_without_claiming_a_cycle() {
    let registry = TaskRegistry::default();
    let flights = InflightTable::new();
    let map = Arc::new(DashMap::new());
    let (entered_tx, entered_rx) = std::sync::mpsc::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let answer = std::thread::scope(|scope| {
        let producer_map = map.clone();
        let producer_flights = &flights;
        let producer = scope.spawn(move || {
            cooperative_admit_with_post_publish(
                &producer_map,
                producer_flights,
                1u32,
                |v: &u32| Some(*v),
                || {
                    entered_tx.send(()).unwrap();
                    release_rx
                        .recv_timeout(std::time::Duration::from_secs(5))
                        .unwrap();
                    ComputeAdmission::Cacheable(42)
                },
                |v| *v,
                |_| true,
                |_, _| {},
                |_, _| {},
                None,
            )
        });
        let entered = entered_rx.recv_timeout(std::time::Duration::from_secs(5));
        let answer = entered.as_ref().ok().map(|_| {
            let mut follower = artifact(map.clone(), &flights, &registry, || {
                panic!("the unowned producer must continue")
            });
            follower.poll(Waker::noop())
        });
        let _ = release_tx.send(());
        let produced = producer.join();
        assert!(entered.is_ok());
        assert_eq!(produced.unwrap(), Some(42));
        answer.unwrap()
    });
    assert!(matches!(
        answer,
        Poll::Ready(Err(OwnedAdmissionError::ProducerUnowned))
    ));
    assert_eq!(registry.wait_count_for_tests(), 0);
    assert!(matches!(
        artifact(map, &flights, &registry, || panic!(
            "completed value is reusable"
        ))
        .poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(42))))
    ));
}

#[test]
fn a_follower_observing_completion_before_its_first_join_poll_can_resume() {
    for owned_producer in [false, true] {
        let registry = TaskRegistry::default();
        let map = Arc::new(DashMap::new());
        let flights = InflightTable::new();
        // Completion precedes retirement. A follower can acquire this exact
        // terminal slot before the producer removes it from the table.
        let slot = Arc::new(InflightSlot::new());
        {
            let mut state = slot.state.lock();
            state.claimed = true;
            state.completed = true;
            state.failure = Some(InflightFailureKind::ComputeFailed);
            state.producer = owned_producer.then(|| registry.register_task());
        }
        flights.table.lock().insert(1u32, slot);
        let mut follower = artifact(map, &flights, &registry, || {
            panic!("failed artifact flight is terminal")
        });
        let answer = match follower.poll(Waker::noop()) {
            Poll::Pending => follower.poll(Waker::noop()),
            answer => answer,
        };
        assert!(matches!(
            answer,
            Poll::Ready(Ok(OwnedAdmissionStep::Value(None)))
        ));
        assert_eq!(registry.wait_count_for_tests(), 0);
        drop(flights);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn a_completed_admission_releases_its_table_and_key_while_the_handle_survives() {
    fn assert_send<T: Send>() {}
    assert_send::<OwnedAdmission<u32, u32>>();
    let registry = TaskRegistry::default();
    let flights = InflightTable::new();
    let table = Arc::downgrade(&flights.table);
    let map = Arc::new(DashMap::new());
    let key: Arc<str> = Arc::from("unretained-owner");
    let key_lifetime = Arc::downgrade(&key);
    let mut admission = owned_admit_with_post_publish(
        map.clone(),
        &flights,
        key.clone(),
        |v: &u32| Some(*v),
        || ComputeAdmission::ReturnOnly {
            value: 31,
            reason: NonAdmissionReason::ForcedTestRefusal,
        },
        |v| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        registry.register_task(),
        CancellationToken::new(),
    );
    drop(key);
    let Poll::Ready(Ok(OwnedAdmissionStep::Produce(producer))) = admission.poll(Waker::noop())
    else {
        panic!("cold admission transfers its producer");
    };
    producer.run();
    assert!(matches!(
        admission.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(31))))
    ));
    drop(flights);
    drop(map);
    assert!(
        table.upgrade().is_none(),
        "a completed subscriber must release the table owner"
    );
    assert!(
        key_lifetime.upgrade().is_none(),
        "a completed subscriber must release its owned key"
    );
    assert_eq!(registry.active_task_count_for_tests(), 0);
    drop(admission);
}

fn artifact(
    map: Arc<DashMap<u32, Arc<u32>>>,
    flights: &InflightTable<u32>,
    registry: &TaskRegistry,
    compute: impl FnOnce() -> ComputeAdmission<u32, u32> + Send + 'static,
) -> OwnedAdmission<u32, u32> {
    owned_admit_with_post_publish(
        map,
        flights,
        1,
        |v: &u32| Some(*v),
        compute,
        |v| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        registry.register_task(),
        CancellationToken::new(),
    )
}

fn take_producer(
    admission: &mut OwnedAdmission<u32, u32>,
    waker: &Waker,
) -> crate::cache_runtime::OwnedAdmissionProducer<u32, u32> {
    match admission.poll(waker) {
        Poll::Ready(Ok(OwnedAdmissionStep::Produce(producer))) => producer,
        _ => panic!("expected sole producer"),
    }
}

struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn owned_artifact_admission_transfers_compute_and_cancels_only_one_waiter() {
    let registry = TaskRegistry::default();
    let map = Arc::new(DashMap::new());
    let flights = InflightTable::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let producer_calls = calls.clone();
    let observed = Arc::new(std::sync::Mutex::new(None));
    let observed_task = observed.clone();
    let waiter = registry.register_task();
    let waiter_id = waiter.id();
    let mut winner = owned_admit_with_post_publish(
        map.clone(),
        &flights,
        1u32,
        |v: &u32| Some(*v),
        move || {
            assert!(
                !verter_execution::cancellation::current_job_cancellation_token()
                    .is_some_and(|token| token.is_cancelled())
            );
            producer_calls.fetch_add(1, Ordering::SeqCst);
            *observed_task.lock().unwrap() = ExecutionScope::current_task().map(|t| t.id());
            ComputeAdmission::Cacheable(42)
        },
        |v: &u32| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        waiter,
        CancellationToken::new(),
    );
    let wake = Arc::new(CountWake(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let producer = match winner.poll(&waker) {
        Poll::Ready(Ok(OwnedAdmissionStep::Produce(p))) => p,
        _ => panic!("cold admission must transfer its producer"),
    };
    assert_eq!(calls.load(Ordering::SeqCst), 0, "poll must not compute");
    let stop = CancellationToken::new();
    let mut follower = owned_admit_with_post_publish(
        map.clone(),
        &flights,
        1u32,
        |v: &u32| Some(*v),
        || panic!("a cancelled follower must not compute"),
        |v: &u32| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        registry.register_task(),
        stop.clone(),
    );
    assert!(follower.poll(&waker).is_pending());
    assert_eq!(registry.wait_count_for_tests(), 2);
    let before_cancel = wake.0.load(Ordering::SeqCst);
    stop.cancel();
    assert_eq!(
        wake.0.load(Ordering::SeqCst),
        before_cancel + 1,
        "one subscriber owns one stop registration"
    );
    assert!(matches!(
        follower.poll(&waker),
        Poll::Ready(Err(OwnedAdmissionError::Cancelled))
    ));
    assert_eq!(registry.wait_count_for_tests(), 1);
    std::thread::spawn(move || {
        let _scope = verter_execution::cancellation::JobCancellationGuard::install(stop);
        producer.run();
        assert!(
            verter_execution::cancellation::current_job_cancellation_token()
                .unwrap()
                .is_cancelled()
        );
    })
    .join()
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_ne!(*observed.lock().unwrap(), Some(waiter_id));
    assert!(matches!(
        winner.poll(&waker),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(42))))
    ));
    assert_eq!(registry.wait_count_for_tests(), 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_artifact_producer_drop_and_panic_settle_winner_and_joiners() {
    for panic in [false, true] {
        let registry = TaskRegistry::default();
        let map = Arc::new(DashMap::new());
        let flights = InflightTable::new();
        let wake = Arc::new(CountWake(AtomicUsize::new(0)));
        let waker = Waker::from(wake.clone());
        let mut winner = artifact(map.clone(), &flights, &registry, move || {
            assert!(!panic, "injected compute failure");
            ComputeAdmission::Cacheable(99)
        });
        let producer = take_producer(&mut winner, &waker);
        let mut follower = artifact(map.clone(), &flights, &registry, || {
            panic!("artifact follower must surface failure")
        });
        assert!(follower.poll(&waker).is_pending());
        assert!(winner.poll(&waker).is_pending());
        if panic {
            producer.run();
        } else {
            drop(producer);
        }
        assert!(wake.0.load(Ordering::SeqCst) >= 2);
        let expected = if panic {
            OwnedAdmissionError::ProducerPanicked
        } else {
            OwnedAdmissionError::ProducerDropped
        };
        assert!(matches!(winner.poll(&waker), Poll::Ready(Err(error)) if error == expected));
        assert!(matches!(
            follower.poll(&waker),
            Poll::Ready(Ok(OwnedAdmissionStep::Value(None)))
        ));
        assert!(map.is_empty());
        assert!(flights.table.lock().is_empty());
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn owned_artifact_withdraws_replaced_and_dropped_subscribers() {
    let registry = TaskRegistry::default();
    let map = Arc::new(DashMap::new());
    let flights = InflightTable::new();
    let old = Arc::new(CountWake(AtomicUsize::new(0)));
    let current = Arc::new(CountWake(AtomicUsize::new(0)));
    let winner_current = Arc::new(CountWake(AtomicUsize::new(0)));
    let dropped = Arc::new(CountWake(AtomicUsize::new(0)));
    let mut winner = artifact(map.clone(), &flights, &registry, || {
        ComputeAdmission::Cacheable(19)
    });
    let producer = take_producer(&mut winner, &Waker::from(old.clone()));
    assert!(winner
        .poll(&Waker::from(winner_current.clone()))
        .is_pending());
    let mut follower = artifact(map.clone(), &flights, &registry, || panic!("shared result"));
    assert!(follower.poll(&Waker::from(old.clone())).is_pending());
    assert!(follower.poll(&Waker::from(current.clone())).is_pending());
    let mut withdrawn = artifact(map, &flights, &registry, || panic!("dropped follower"));
    assert!(withdrawn.poll(&Waker::from(dropped.clone())).is_pending());
    drop(withdrawn);
    producer.run();
    assert_eq!(current.0.load(Ordering::SeqCst), 1);
    assert_eq!(dropped.0.load(Ordering::SeqCst), 0);
    assert_eq!(old.0.load(Ordering::SeqCst), 0);
    assert_eq!(winner_current.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        winner.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(19))))
    ));
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(19))))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_artifact_retains_parent_edge_and_refuses_same_task_recursion() {
    let registry = TaskRegistry::default();
    let waiter = registry.register_task();
    let parent = waiter.clone();
    let map = Arc::new(DashMap::new());
    let flights = Arc::new(InflightTable::new());
    let nested_map = map.clone();
    let nested_flights = flights.clone();
    let mut winner = owned_admit_with_post_publish(
        map,
        &flights,
        1u32,
        |v: &u32| Some(*v),
        move || {
            let task = ExecutionScope::current_task().expect("producer scope");
            assert!(
                task.wait_for(&parent).is_err(),
                "parent still awaits this producer"
            );
            let mut nested = owned_admit_with_post_publish(
                nested_map,
                &nested_flights,
                1,
                |v: &u32| Some(*v),
                || panic!("recursive producer"),
                |v| *v,
                |_| true,
                |_, _| {},
                |_, _| {},
                None,
                task,
                CancellationToken::new(),
            );
            assert!(matches!(
                nested.poll(Waker::noop()),
                Poll::Ready(Err(OwnedAdmissionError::Cycle))
            ));
            ComputeAdmission::Cacheable(13)
        },
        |v| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        waiter,
        CancellationToken::new(),
    );
    take_producer(&mut winner, Waker::noop()).run();
    assert!(matches!(
        winner.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(13))))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_artifact_ready_hit_and_pre_cancel_do_not_register_wait_edges() {
    let registry = TaskRegistry::default();
    let map = Arc::new(DashMap::new());
    let flights = InflightTable::new();
    map.insert(1, Arc::new(6));
    let mut ready = artifact(map.clone(), &flights, &registry, || panic!("warm hit"));
    assert!(matches!(
        ready.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(6))))
    ));
    let stop = CancellationToken::new();
    stop.cancel();
    let mut stopped = owned_admit_with_post_publish(
        map,
        &flights,
        1,
        |v: &u32| Some(*v),
        || panic!("pre-cancelled admission"),
        |v| *v,
        |_| true,
        |_, _| {},
        |_, _| {},
        None,
        registry.register_task(),
        stop,
    );
    assert!(matches!(
        stopped.poll(Waker::noop()),
        Poll::Ready(Err(OwnedAdmissionError::Cancelled))
    ));
    assert_eq!(registry.wait_count_for_tests(), 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn abandoning_a_follower_withdraws_edges_before_dropping_callback_captures() {
    struct ObserveDrop(TaskRegistry, Arc<AtomicUsize>);
    impl Drop for ObserveDrop {
        fn drop(&mut self) {
            self.1
                .store(self.0.wait_count_for_tests(), Ordering::SeqCst);
        }
    }
    let registry = TaskRegistry::default();
    let flights = InflightTable::new();
    let map = Arc::new(DashMap::new());
    let mut leader = artifact(map.clone(), &flights, &registry, || {
        ComputeAdmission::Cacheable(7)
    });
    let producer = take_producer(&mut leader, Waker::noop());
    let observed = Arc::new(AtomicUsize::new(usize::MAX));
    let capture = ObserveDrop(registry.clone(), observed.clone());
    let mut follower = artifact(map, &flights, &registry, move || {
        drop(capture);
        panic!("follower must not compute")
    });
    assert!(follower.poll(Waker::noop()).is_pending());
    assert_eq!(registry.wait_count_for_tests(), 2);
    drop(follower);
    let at_drop = observed.load(Ordering::SeqCst);
    drop((producer, leader));
    assert_eq!(
        at_drop, 1,
        "only the leader edge survives capture destruction"
    );
    assert_eq!(registry.wait_count_for_tests(), 0);
}

#[test]
fn stale_artifact_poll_transfers_cleanup_while_the_retention_writer_is_held() {
    for stale in [false, true] {
        let registry = TaskRegistry::default();
        let flights = InflightTable::new();
        let map = Arc::new(DashMap::new());
        map.insert(1u32, Arc::new(6u32));
        let fence = Arc::new(parking_lot::RwLock::new(()));
        let removals = Arc::new(AtomicUsize::new(0));
        let cleaned = removals.clone();
        let mut admission = owned_admit_with_post_publish(
            map.clone(),
            &flights,
            1u32,
            move |v: &u32| (!stale).then_some(*v),
            || ComputeAdmission::Cacheable(7),
            |v| *v,
            |_| true,
            move |_, _| {
                cleaned.fetch_add(1, Ordering::SeqCst);
            },
            |_, _| {},
            Some(fence.clone()),
            registry.register_task(),
            CancellationToken::new(),
        );
        let held = fence.write();
        let (tx, rx) = std::sync::mpsc::channel();
        let poller = std::thread::spawn(move || {
            let step = admission.poll(Waker::noop());
            tx.send(()).unwrap();
            (admission, step)
        });
        let returned = rx.recv_timeout(std::time::Duration::from_secs(2));
        drop(held);
        let (mut admission, step) = poller.join().unwrap();
        assert!(
            returned.is_ok(),
            "poll must return while the lifecycle writer is held (stale={stale})"
        );
        assert_eq!(
            removals.load(Ordering::SeqCst),
            0,
            "poll never cleans stale state"
        );
        if stale {
            let Poll::Ready(Ok(OwnedAdmissionStep::Produce(producer))) = step else {
                panic!("stale lookup transfers work")
            };
            producer.run();
            assert!(matches!(
                admission.poll(Waker::noop()),
                Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(7))))
            ));
            assert_eq!(removals.load(Ordering::SeqCst), 1);
            assert_eq!(**map.get(&1).unwrap(), 7);
        } else {
            assert!(matches!(
                step,
                Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(6))))
            ));
        }
        assert_eq!(registry.wait_count_for_tests(), 0);
    }
}

#[test]
fn owned_artifact_refuses_foreign_authority_without_stopping_the_producer() {
    let registry = TaskRegistry::default();
    let foreign = TaskRegistry::default();
    let map = Arc::new(DashMap::new());
    let flights = InflightTable::new();
    let mut winner = artifact(map.clone(), &flights, &registry, || {
        ComputeAdmission::Cacheable(31)
    });
    let producer = take_producer(&mut winner, Waker::noop());
    let mut other = artifact(map, &flights, &foreign, || {
        panic!("foreign authority cannot claim this flight")
    });
    assert!(matches!(
        other.poll(Waker::noop()),
        Poll::Ready(Err(OwnedAdmissionError::Cycle))
    ));
    assert_eq!(foreign.active_task_count_for_tests(), 0);
    producer.run();
    assert!(matches!(
        winner.poll(Waker::noop()),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(31))))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_query_return_only_and_rejected_admission_fork_for_the_followers_view() {
    use crate::cache_runtime::lookup_publish::owned_admit_with_lookup_publish;
    for rejected in [false, true] {
        let registry = TaskRegistry::default();
        let flights = InflightTable::new();
        let published = Arc::new(AtomicUsize::new(0));
        let winner_published = published.clone();
        let follower_published = published.clone();
        let mut winner = owned_admit_with_lookup_publish(
            &flights,
            1u32,
            || None,
            move || {
                if rejected {
                    ComputeAdmission::Cacheable(7u32)
                } else {
                    ComputeAdmission::ReturnOnly {
                        value: 7u32,
                        reason: NonAdmissionReason::ForcedTestRefusal,
                    }
                }
            },
            |v| *v,
            |v| Some(*v),
            move |_| !rejected,
            move |_| {
                winner_published.fetch_add(1, Ordering::SeqCst);
            },
            |_: ()| {},
            None,
            registry.register_task(),
            CancellationToken::new(),
        );
        let producer = take_producer(&mut winner, Waker::noop());
        let mut follower = owned_admit_with_lookup_publish(
            &flights,
            1u32,
            || None,
            || ComputeAdmission::<u32, u32>::ReturnOnly {
                value: 9,
                reason: NonAdmissionReason::ForcedTestRefusal,
            },
            |v| *v,
            |_| None,
            |_| true,
            move |_| {
                follower_published.fetch_add(1, Ordering::SeqCst);
            },
            |_: ()| {},
            None,
            registry.register_task(),
            CancellationToken::new(),
        );
        assert!(follower.poll(Waker::noop()).is_pending());
        producer.run();
        assert!(matches!(
            winner.poll(Waker::noop()),
            Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(7))))
        ));
        take_producer(&mut follower, Waker::noop()).run();
        assert!(matches!(
            follower.poll(Waker::noop()),
            Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(9))))
        ));
        assert_eq!(published.load(Ordering::SeqCst), 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn owned_query_published_joiner_validates_its_own_view_and_cancel_keeps_sibling() {
    use crate::cache_runtime::lookup_publish::owned_admit_with_lookup_publish;
    let registry = TaskRegistry::default();
    let flights = InflightTable::new();
    let cache = Arc::new(std::sync::Mutex::new(None::<u32>));
    let bubbles = Arc::new(AtomicUsize::new(0));
    let begin = |expected, stop: CancellationToken| {
        let read = cache.clone();
        let write = cache.clone();
        let bubble = bubbles.clone();
        owned_admit_with_lookup_publish(
            &flights,
            1u32,
            move || {
                let value = *read.lock().unwrap();
                value.filter(|value| *value == expected).inspect(|_| {
                    bubble.fetch_add(1, Ordering::SeqCst);
                })
            },
            move || {
                assert!(
                    !verter_execution::cancellation::current_job_cancellation_token()
                        .is_some_and(|token| token.is_cancelled())
                );
                assert!(
                    !crate::resolver_core::request_ports::CancellationCheckpoint::new()
                        .is_cancelled()
                );
                ComputeAdmission::Cacheable(expected)
            },
            |v| *v,
            |_| None,
            |_| true,
            move |entry| {
                *write.lock().unwrap() = Some(entry);
            },
            |_: ()| {},
            None,
            registry.register_task(),
            stop,
        )
    };
    let wake = Arc::new(CountWake(AtomicUsize::new(0)));
    let waker = Waker::from(wake.clone());
    let stop = CancellationToken::new();
    let mut winner = begin(7, stop.clone());
    let producer = take_producer(&mut winner, &waker);
    let mut stopped = begin(7, stop.clone());
    let mut sibling = begin(7, CancellationToken::new());
    let mut other_view = begin(9, CancellationToken::new());
    assert!(stopped.poll(&waker).is_pending());
    assert!(sibling.poll(&waker).is_pending());
    assert!(other_view.poll(&waker).is_pending());
    stop.cancel();
    assert!(matches!(
        stopped.poll(&waker),
        Poll::Ready(Err(OwnedAdmissionError::Cancelled))
    ));
    assert!(matches!(
        winner.poll(&waker),
        Poll::Ready(Err(OwnedAdmissionError::Cancelled))
    ));
    {
        let _request = crate::request_context::install_test_request_for("/shared-producer.ts");
        crate::request_context::current_request_context()
            .unwrap()
            .cancel();
        let _scope = verter_execution::cancellation::JobCancellationGuard::install(stop);
        producer.run();
        assert!(
            verter_execution::cancellation::current_job_cancellation_token()
                .unwrap()
                .is_cancelled()
        );
    }
    assert!(matches!(
        sibling.poll(&waker),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(7))))
    ));
    assert_eq!(
        bubbles.load(Ordering::SeqCst),
        1,
        "winner projection must not replace follower validation"
    );
    take_producer(&mut other_view, &waker).run();
    assert!(matches!(
        other_view.poll(&waker),
        Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(9))))
    ));
    assert_eq!(*cache.lock().unwrap(), Some(9));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn owned_query_retries_abandoned_producer_but_preserves_compute_failed_policy() {
    use crate::cache_runtime::lookup_publish::owned_admit_with_lookup_publish;
    for abandon in [true, false] {
        let registry = TaskRegistry::default();
        let flights = InflightTable::new();
        let mut winner = owned_admit_with_lookup_publish(
            &flights,
            1u32,
            || None::<u32>,
            || ComputeAdmission::<u32, u32>::Failed,
            |v| *v,
            |_| None,
            |_| true,
            |_| panic!("failed producer cannot publish"),
            |_: ()| {},
            None,
            registry.register_task(),
            CancellationToken::new(),
        );
        let producer = take_producer(&mut winner, Waker::noop());
        let mut follower = owned_admit_with_lookup_publish(
            &flights,
            1u32,
            || None,
            || ComputeAdmission::<u32, u32>::ReturnOnly {
                value: 23,
                reason: NonAdmissionReason::ForcedTestRefusal,
            },
            |v| *v,
            |_| None,
            |_| true,
            |_| panic!("return-only cannot publish"),
            |_: ()| {},
            None,
            registry.register_task(),
            CancellationToken::new(),
        );
        assert!(follower.poll(Waker::noop()).is_pending());
        if abandon {
            drop(producer);
            assert!(matches!(
                winner.poll(Waker::noop()),
                Poll::Ready(Err(OwnedAdmissionError::ProducerDropped))
            ));
            take_producer(&mut follower, Waker::noop()).run();
            assert!(matches!(
                follower.poll(Waker::noop()),
                Poll::Ready(Ok(OwnedAdmissionStep::Value(Some(23))))
            ));
        } else {
            producer.run();
            assert!(matches!(
                winner.poll(Waker::noop()),
                Poll::Ready(Ok(OwnedAdmissionStep::Value(None)))
            ));
            assert!(matches!(
                follower.poll(Waker::noop()),
                Poll::Ready(Ok(OwnedAdmissionStep::Value(None)))
            ));
        }
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}
