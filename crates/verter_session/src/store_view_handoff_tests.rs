use super::*;
use std::task::{Poll, Waker};
use verter_execution::cancellation::CancellationToken;

#[test]
fn owned_store_view_transfers_cold_work_and_cancels_only_one_subscriber() {
    let host = Arc::new(VerterHost::new_standalone(
        crate::types::HostConfig::default(),
    ));
    let scope = host.scheduler.enter_execution_scope();
    let registry = scope.task().registry().clone();
    drop(scope);
    let mut leader = host.begin_store_view(registry.register_task(), CancellationToken::new());
    let producer = match leader.poll(Waker::noop()) {
        Poll::Ready(Ok(StoreViewStep::Produce(producer))) => producer,
        _ => panic!("cold work must transfer without running"),
    };
    assert!(!host.store_view_manager().is_populated());
    let stop = CancellationToken::new();
    let mut cancelled = host.begin_store_view(registry.register_task(), stop.clone());
    let mut sibling = host.begin_store_view(registry.register_task(), CancellationToken::new());
    assert!(cancelled.poll(Waker::noop()).is_pending());
    assert!(sibling.poll(Waker::noop()).is_pending());
    stop.cancel();
    assert!(matches!(
        cancelled.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Cancelled))
    ));
    std::thread::spawn(move || producer.run()).join().unwrap();
    for handoff in [&mut leader, &mut sibling] {
        assert!(
            matches!(handoff.poll(Waker::noop()), Poll::Ready(Ok(StoreViewStep::Value(value))) if value.is_current())
        );
    }
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

fn fixture() -> (Arc<VerterHost>, verter_execution::tasks::TaskRegistry) {
    let host = Arc::new(VerterHost::new_standalone(
        crate::types::HostConfig::default(),
    ));
    let scope = host.scheduler.enter_execution_scope();
    let registry = scope.task().registry().clone();
    drop(scope);
    (host, registry)
}
fn begin(
    host: &Arc<VerterHost>,
    registry: &verter_execution::tasks::TaskRegistry,
) -> StoreViewHandoff {
    host.begin_store_view(registry.register_task(), CancellationToken::new())
}
fn take_producer(handoff: &mut StoreViewHandoff) -> StoreViewProducer {
    match handoff.poll(Waker::noop()) {
        Poll::Ready(Ok(StoreViewStep::Produce(producer))) => producer,
        _ => panic!("cold capture must transfer its producer"),
    }
}
struct Notify {
    manager: StoreViewManager,
    wakes: std::sync::atomic::AtomicUsize,
    locked: std::sync::atomic::AtomicBool,
}
impl std::task::Wake for Notify {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.locked
            .fetch_or(self.manager.state.try_lock().is_none(), Ordering::SeqCst);
        self.wakes.fetch_add(1, Ordering::SeqCst);
    }
}
fn notify(host: &VerterHost) -> Arc<Notify> {
    Arc::new(Notify {
        manager: host.store_view_manager().clone(),
        wakes: 0.into(),
        locked: false.into(),
    })
}
#[test]
fn held_owned_store_view_poll_is_bounded_and_completion_wakes_outside_locks() {
    fn send<T: Send>() {}
    send::<StoreViewHandoff>();
    send::<StoreViewProducer>();
    let (host, registry) = fixture();
    let mut owner = begin(&host, &registry);
    let producer = take_producer(&mut owner);
    let mut follower = begin(&host, &registry);
    let signal = notify(&host);
    let waker = Waker::from(signal.clone());
    let (tx, rx) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let pending = follower.poll(&waker).is_pending();
        tx.send(pending).unwrap();
        follower
    });
    let pending = rx.recv_timeout(std::time::Duration::from_secs(5));
    // Always release the physical work and join before reporting a failed poll.
    producer.run();
    let mut follower = poller.join().unwrap();
    assert_eq!(
        pending.ok(),
        Some(true),
        "poll must not wait for producer execution"
    );
    assert!(
        signal.wakes.load(Ordering::SeqCst) > 0,
        "completion must wake the registered follower"
    );
    assert!(!signal.locked.load(Ordering::SeqCst));
    assert!(matches!(
        follower.poll(Waker::noop()),
        Poll::Ready(Ok(StoreViewStep::Value(_)))
    ));
    assert!(matches!(
        owner.poll(Waker::noop()),
        Poll::Ready(Ok(StoreViewStep::Value(_)))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
#[test]
fn store_view_withdraws_exact_subscriber_and_replaces_waker() {
    let (host, registry) = fixture();
    let mut owner = begin(&host, &registry);
    let producer = take_producer(&mut owner);
    let shared = notify(&host);
    let obsolete = notify(&host);
    let mut first = begin(&host, &registry);
    let mut sibling = begin(&host, &registry);
    assert!(first.poll(&Waker::from(shared.clone())).is_pending());
    assert!(sibling.poll(&Waker::from(obsolete.clone())).is_pending());
    assert!(sibling.poll(&Waker::from(shared.clone())).is_pending());
    drop(first);
    Waker::from(shared.clone()).wake_by_ref(); // Spurious wake changes no readiness.
    assert!(sibling.poll(&Waker::from(shared.clone())).is_pending());
    shared.wakes.store(0, Ordering::SeqCst);
    producer.run();
    assert_eq!(obsolete.wakes.load(Ordering::SeqCst), 0);
    assert_eq!(
        shared.wakes.load(Ordering::SeqCst),
        1,
        "dropping one same-waker registration must withdraw only it"
    );
    assert!(matches!(
        sibling.poll(Waker::noop()),
        Poll::Ready(Ok(StoreViewStep::Value(_)))
    ));
    drop(owner);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
#[test]
fn store_view_producer_drop_and_panic_settle_all_subscribers() {
    for panic in [false, true] {
        let (host, registry) = fixture();
        let mut owner = begin(&host, &registry);
        let producer = take_producer(&mut owner);
        let mut follower = begin(&host, &registry);
        assert!(follower.poll(Waker::noop()).is_pending());
        if panic {
            std::thread::spawn(move || {
                FORCE_BUILD_PANIC.with(|flag| flag.set(true));
                producer.run(); // Owned execution contains panic.
            })
            .join()
            .unwrap();
        } else {
            drop(producer);
        }
        let expected = if panic {
            StoreViewError::ProducerPanicked
        } else {
            StoreViewError::ProducerDropped
        };
        for handoff in [&mut owner, &mut follower] {
            assert!(
                matches!(handoff.poll(Waker::noop()), Poll::Ready(Err(error)) if error == expected)
            );
        }
        assert!(!host.store_view_manager().state.lock().building.is_claimed());
        assert_eq!(registry.active_task_count_for_tests(), 0);
        let mut successor = begin(&host, &registry);
        take_producer(&mut successor).run();
        assert!(matches!(
            successor.poll(Waker::noop()),
            Poll::Ready(Ok(StoreViewStep::Value(_)))
        ));
    }
}
#[test]
fn reset_revokes_unstarted_claim_without_touching_successor() {
    let (host, registry) = fixture();
    let mut old = begin(&host, &registry);
    let producer = take_producer(&mut old);
    host.store_view_manager().clear();
    assert!(matches!(
        old.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Reset))
    ));
    let mut successor = begin(&host, &registry);
    let next = take_producer(&mut successor);
    producer.run();
    assert!(successor.poll(Waker::noop()).is_pending());
    next.run();
    assert!(matches!(
        successor.poll(Waker::noop()),
        Poll::Ready(Ok(StoreViewStep::Value(_)))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
#[test]
fn reset_running_claim_stops_old_subscribers_but_new_subscriber_waits_for_physical_release() {
    let (host, registry) = fixture();
    let mut old = begin(&host, &registry);
    let producer = take_producer(&mut old);
    let mut old_follower = begin(&host, &registry);
    assert!(old_follower.poll(Waker::noop()).is_pending());
    TEST_BUILD_GATE.arm();
    let worker = std::thread::spawn(move || {
        TEST_BUILD_GATE.enroll_current_thread();
        producer.run();
    });
    let parked = TEST_BUILD_GATE.wait_for_builder_parked(std::time::Duration::from_secs(5));
    host.store_view_manager().clear();
    let old_reset = matches!(
        old.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Reset))
    );
    let follower_reset = matches!(
        old_follower.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Reset))
    );
    let mut successor = begin(&host, &registry);
    let waits_physical = successor.poll(Waker::noop()).is_pending();
    TEST_BUILD_GATE.release();
    worker.join().unwrap();
    assert!(parked && old_reset && follower_reset && waits_physical);
    take_producer(&mut successor).run();
    assert!(matches!(
        successor.poll(Waker::noop()),
        Poll::Ready(Ok(StoreViewStep::Value(_)))
    ));
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
#[test]
fn store_view_shared_task_graph_refuses_same_task_and_mixed_family_cycles() {
    use verter_execution::dependency::{DependencyPoll, DependencyStop, OwnedDependency};
    struct Held;
    impl DependencyPoll for Held {
        type Output = ();
        fn poll_dependency(&mut self, _: &Waker) -> Poll<()> {
            Poll::Pending
        }
    }
    let (host, registry) = fixture();
    let waiter = registry.register_task();
    let mut owner = host.begin_store_view(waiter.clone(), CancellationToken::new());
    let producer = take_producer(&mut owner);
    let task = producer
        .core
        .claim
        .as_ref()
        .unwrap()
        .claim
        .task
        .clone()
        .unwrap();
    let mut same = host.begin_store_view(task.clone(), CancellationToken::new());
    assert!(matches!(
        same.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Cycle))
    ));
    let mut mixed = OwnedDependency::new(Held, task, waiter, CancellationToken::new());
    assert!(matches!(
        mixed.poll(Waker::noop()),
        Poll::Ready(Err(DependencyStop::Cycle))
    ));
    drop(mixed);
    drop(producer);
    drop(owner);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

fn pending_maintenance() -> (Arc<VerterHost>, verter_execution::tasks::TaskRegistry) {
    let (host, registry) = fixture();
    let upsert = |source: &str| {
        let _ = host
            .upsert(crate::UpsertRequest {
                canonical_id: None,
                input_id: "/held.ts".to_string(),
                source: Arc::from(source),
                file_language: crate::types::FileLanguage::script_ts(),
                aliases: Vec::new(),
            })
            .unwrap();
        host.ensure_indexed_ready("/held.ts").unwrap();
    };
    upsert("export interface A { a: number }");
    let old = host.resolver_store_view_read();
    upsert("export interface A { a: string }");
    drop(host.resolver_store_view_read());
    let store = host.project_type_store().indexed();
    assert!(store.retained_retired_version_count() > 0);
    drop(old);
    assert!(store.root_release_reclamation_pending());
    (host, registry)
}
#[test]
fn store_view_completed_capture_revalidates_after_suspension() {
    for maintenance in [false, true] {
        let (host, registry) = if maintenance {
            pending_maintenance()
        } else {
            fixture()
        };
        let mut owner = begin(&host, &registry);
        let producer = take_producer(&mut owner);
        assert_eq!(producer.core.cached_read.is_some(), maintenance);
        producer.run();
        host.bump_store_view_epoch();
        let successor = match owner.poll(Waker::noop()) {
            Poll::Ready(Ok(StoreViewStep::Produce(producer))) => producer,
            _ => panic!("a completed capture superseded before resumption must re-probe, never expose old Current"),
        };
        successor.run();
        assert!(
            matches!(owner.poll(Waker::noop()), Poll::Ready(Ok(StoreViewStep::Value(value))) if value.is_current())
        );
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}
#[test]
fn store_view_warm_hit_transfers_only_pending_maintenance() {
    let (host, registry) = pending_maintenance();
    let store = host.project_type_store().indexed();
    let before = store.retained_retired_version_count();
    let mut owner = begin(&host, &registry);
    let producer = take_producer(&mut owner);
    assert!(
        producer.core.cached_read.is_some(),
        "warm maintenance must not rebuild the coherent snapshot"
    );
    assert_eq!(
        store.retained_retired_version_count(),
        before,
        "poll must not reclaim"
    );
    producer.run();
    assert!(store.retained_retired_version_count() < before);
    assert!(
        matches!(owner.poll(Waker::noop()), Poll::Ready(Ok(StoreViewStep::Value(value))) if value.is_current())
    );
    let mut warm = begin(&host, &registry);
    assert!(
        matches!(warm.poll(Waker::noop()), Poll::Ready(Ok(StoreViewStep::Value(value))) if value.is_current())
    );
}
#[test]
fn store_view_already_cancelled_never_claims_and_transferred_work_needs_no_new_root() {
    let (host, registry) = fixture();
    let stop = CancellationToken::new();
    stop.cancel();
    let mut cancelled = host.begin_store_view(registry.register_task(), stop);
    assert!(matches!(
        cancelled.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Cancelled))
    ));
    assert!(!host.store_view_manager().state.lock().building.is_claimed());
    // The test driver admits roots only during this initial fixture phase.
    let open = std::cell::Cell::new(true);
    let admit = |stop| {
        open.get()
            .then(|| host.begin_store_view(registry.register_task(), stop))
    };
    let stop = CancellationToken::new();
    let mut owner = admit(stop.clone()).unwrap();
    let producer = take_producer(&mut owner);
    let mut surviving = admit(CancellationToken::new()).unwrap();
    assert!(surviving.poll(Waker::noop()).is_pending());
    open.set(false);
    assert!(admit(CancellationToken::new()).is_none());
    stop.cancel();
    assert!(matches!(
        owner.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::Cancelled))
    ));
    producer.run(); // Only already-transferred work runs after admission closes.
    assert!(
        matches!(surviving.poll(Waker::noop()), Poll::Ready(Ok(StoreViewStep::Value(value))) if value.is_current())
    );
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn store_view_maintenance_preserves_return_only_reason() {
    let (host, registry) = pending_maintenance();
    host.bump_store_view_epoch();
    let mut owner = begin(&host, &registry);
    // The exhausted-waiter phase classifies an existing stale cache as a seed.
    owner.core.phase = Phase::Final;
    let producer = take_producer(&mut owner);
    assert!(matches!(
        producer.core.cached_read,
        Some(StoreViewRead::ReturnOnly {
            reason: StoreViewReturnOnlyReason::Superseded,
            ..
        })
    ));
    producer.run();
    let value = match owner.poll(Waker::noop()) {
        Poll::Ready(Ok(StoreViewStep::Value(value))) => value,
        _ => panic!("maintenance must preserve its captured return-only capability"),
    };
    assert!(!value.is_current());
    assert_eq!(
        value.into_read().return_only_reason_for_tests(),
        Some(StoreViewReturnOnlyReason::Superseded)
    );
}

#[test]
fn store_view_refuses_an_unowned_physical_claim() {
    let (host, registry) = fixture();
    let manager = host.store_view_manager();
    let (claim, _) = BuildClaimGuard::claim_for(manager, &mut manager.state.lock(), None, true);
    let mut demand = begin(&host, &registry);
    assert!(matches!(
        demand.poll(Waker::noop()),
        Poll::Ready(Err(StoreViewError::ProducerUnowned))
    ));
    drop(claim);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn completed_store_view_handoff_and_value_retain_no_host() {
    let (host, registry) = fixture();
    let mut demand = begin(&host, &registry);
    take_producer(&mut demand).run();
    let value = match demand.poll(Waker::noop()) {
        Poll::Ready(Ok(StoreViewStep::Value(value))) => value,
        _ => panic!("completed capture"),
    };
    let weak = Arc::downgrade(&host);
    drop(host);
    assert!(
        weak.upgrade().is_none(),
        "completed capability retains only its view"
    );
    assert!(value.is_current());
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn cancelled_store_view_releases_last_host_without_joining_held_lowering_workers() {
    last_host_with_held_lowering_workers(true);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn warm_store_view_releases_last_host_without_joining_held_lowering_workers() {
    last_host_with_held_lowering_workers(false);
}

#[cfg(not(target_arch = "wasm32"))]
fn last_host_with_held_lowering_workers(cancel: bool) {
    let (host, registry) = fixture();
    host.close();
    if !cancel {
        drop(host.resolver_store_view_read());
    }
    let mut held = host.decl_lowering.hold_workers_for_tests();
    let weak = Arc::downgrade(&host);
    let stop = CancellationToken::new();
    let mut demand = host.begin_store_view(registry.register_task(), stop.clone());
    drop(host);
    if cancel {
        stop.cancel();
    }

    let (tx, rx) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let terminal = match demand.poll(Waker::noop()) {
            Poll::Ready(Err(StoreViewError::Cancelled)) => cancel,
            Poll::Ready(Ok(StoreViewStep::Value(value))) => !cancel && value.is_current(),
            _ => false,
        };
        drop(demand);
        let _ = tx.send(terminal);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(5));
    let host_released = weak.strong_count() == 0;
    // Release and join even when cancellation or handoff drop tried to join.
    held.release();
    poller.join().unwrap();
    assert_eq!(
        result.ok(),
        Some(true),
        "terminal poll must return before worker release"
    );
    assert!(host_released, "a terminal handoff must release its host");
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn failed_store_view_releases_cached_artifacts_without_joining_held_lowering_workers() {
    let (host, registry) = pending_maintenance();
    let mut demand = begin(&host, &registry);
    let producer = take_producer(&mut demand);
    let mut held = host.decl_lowering.hold_workers_for_tests();
    drop(producer);
    let weak = Arc::downgrade(&host);
    drop(host);

    let (tx, rx) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let failed = matches!(
            demand.poll(Waker::noop()),
            Poll::Ready(Err(StoreViewError::ProducerDropped))
        );
        drop(demand);
        let _ = tx.send(failed);
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(5));
    let host_released = weak.strong_count() == 0;
    held.release();
    poller.join().unwrap();
    assert_eq!(
        result.ok(),
        Some(true),
        "failed poll must return before worker release"
    );
    assert!(host_released);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}
