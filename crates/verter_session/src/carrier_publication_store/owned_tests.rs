use super::*;
use crate::carrier_publication_store_tests::{accepted, authorities, request};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::task::Wake;
use verter_execution::tasks::TaskRegistry;

#[derive(Default)]
struct Count(AtomicUsize);
impl Wake for Count {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn producer(handoff: &mut OwnedPublication) -> PublicationProducer {
    let Poll::Ready(Ok(PublicationStep::Produce(producer))) = handoff.poll(Waker::noop()) else {
        panic!("cold publication transfers producer")
    };
    producer
}
fn work(producer: &mut PublicationProducer) -> PublicationWork {
    let Poll::Ready(PublicationProducerStep::Work(work)) = producer.poll(Waker::noop()) else {
        panic!("external phase expected")
    };
    work
}
fn drain(producer: &mut PublicationProducer) {
    for _ in 0..8 {
        match producer.poll(Waker::noop()) {
            Poll::Ready(PublicationProducerStep::Work(work)) => work.run(),
            Poll::Ready(PublicationProducerStep::Complete) => return,
            Poll::Pending => panic!("all dependencies have external work available"),
        }
    }
    panic!("bounded admitted phases must drain");
}
fn outcome(handoff: &mut OwnedPublication) -> PublicationOutcome {
    let Poll::Ready(Ok(PublicationStep::Outcome(outcome))) = handoff.poll(Waker::noop()) else {
        panic!("terminal publication expected")
    };
    outcome
}

#[test]
fn cancelling_the_initiator_does_not_cancel_admitted_publication_or_stable_work() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>admitted</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut context = request(1, &accepted);
    let stop = context.cancellation.clone();
    let mut admission_open = true;
    let admit = |context: PublicationRequestContext| {
        assert!(admission_open, "new roots stay closed during resumed work");
        store.begin_publication(accepted.clone(), context, registry.register_task())
    };
    let mut first = admit(context.clone());
    context.audit_request_id = AuditRequestId::new(2);
    context.cancellation = CancellationToken::new();
    let mut sibling = admit(context);
    admission_open = false;
    assert!(!admission_open);
    let mut producer = producer(&mut first);
    work(&mut producer).run(); // initial retained miss
    let stable = work(&mut producer); // S is admitted, still held
    let count = Arc::new(Count::default());
    let waker = Waker::from(count.clone());
    assert!(producer.poll(&waker).is_pending());
    assert!(first.poll(&waker).is_pending());
    assert!(sibling.poll(Waker::noop()).is_pending());
    assert_eq!(store.audit_snapshot().parser_started, 0);
    stop.cancel();
    assert!(count.0.load(Ordering::SeqCst) > 0);
    assert!(matches!(
        first.poll(Waker::noop()),
        Poll::Ready(Err(PublicationError::Cancelled))
    ));
    // A cancelled ambient caller must not poison P/S producer execution.
    let _ambient = verter_execution::cancellation::JobCancellationGuard::install(stop);
    stable.run();
    drain(&mut producer);
    assert!(outcome(&mut sibling).into_envelope().is_some());
    assert_eq!(store.audit_snapshot().parser_started, 1);
    assert!(store.stable_parses.lock().unwrap().is_empty());
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn a_sibling_can_claim_after_the_unpolled_initiator_detaches() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>sibling</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let first = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let mut sibling = store.begin_publication(
        accepted.clone(),
        request(2, &accepted),
        registry.register_task(),
    );
    drop(first);
    drain(&mut producer(&mut sibling));
    assert!(outcome(&mut sibling).into_envelope().is_some());
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn completed_publication_attachment_pins_delivery_until_its_first_poll() {
    for retain_previous in [true, false] {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>delivery</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::with_stable_unit_retention(
            source,
            grammar,
            0,
            registry.clone(),
        );
        let mut first = store.begin_publication(
            accepted.clone(),
            request(1, &accepted),
            registry.register_task(),
        );
        drain(&mut producer(&mut first));
        let envelope = outcome(&mut first).into_envelope().unwrap();
        let weak = Arc::downgrade(&envelope);
        let mut attached = store.begin_publication(
            accepted.clone(),
            request(2, &accepted),
            registry.register_task(),
        );
        let previous = retain_previous.then_some(envelope);
        let delivered = outcome(&mut attached)
            .into_envelope()
            .expect("attached subscriber owns a complete envelope even when retention is denied");
        assert!(std::sync::Weak::ptr_eq(&weak, &Arc::downgrade(&delivered)));
        drop(previous);
        drop(delivered);
        assert!(
            weak.upgrade().is_none(),
            "drained delivery must not retain the envelope"
        );
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);

        let mut retry = store.begin_publication(
            accepted.clone(),
            request(3, &accepted),
            registry.register_task(),
        );
        drain(&mut producer(&mut retry));
        assert!(
            outcome(&mut retry).into_envelope().is_some(),
            "an already expired publication must produce again"
        );
    }
}

#[test]
fn composed_registry_rejects_foreign_cold_waiters_but_ready_answers_need_no_edge() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>authority</template>");
    let registry = TaskRegistry::default();
    let alien = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut wrong = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        alien.register_task(),
    );
    assert!(matches!(
        wrong.poll(Waker::noop()),
        Poll::Ready(Err(PublicationError::Cycle))
    ));
    let mut right = store.begin_publication(
        accepted.clone(),
        request(2, &accepted),
        registry.register_task(),
    );
    drain(&mut producer(&mut right));
    let envelope = outcome(&mut right).into_envelope().unwrap();
    let mut ready = store.begin_publication(
        accepted.clone(),
        request(3, &accepted),
        alien.register_task(),
    );
    assert!(Arc::ptr_eq(
        &outcome(&mut ready).into_envelope().unwrap(),
        &envelope
    ));
    let mut wrong = store.begin_stable_parse(
        accepted.clone(),
        request(4, &accepted),
        alien.register_task(),
    );
    assert!(matches!(
        wrong.poll(Waker::noop()),
        Poll::Ready(Err(StableParseError::Cycle))
    ));
    let mut right = store.begin_stable_parse(
        accepted.clone(),
        request(5, &accepted),
        registry.register_task(),
    );
    let Poll::Ready(Ok(StableParseStep::Produce(producer))) = right.poll(Waker::noop()) else {
        panic!("stable producer")
    };
    producer.run();
    let Poll::Ready(Ok(StableParseStep::Parsed(value))) = right.poll(Waker::noop()) else {
        panic!("stable value")
    };
    let mut ready = store.begin_stable_parse(
        accepted.clone(),
        request(6, &accepted),
        alien.register_task(),
    );
    assert!(matches!(
        ready.poll(Waker::noop()),
        Poll::Ready(Ok(StableParseStep::Parsed(_)))
    ));
    drop(value);
    assert_eq!(registry.active_task_count_for_tests(), 0);
    assert_eq!(alien.active_task_count_for_tests(), 0);
}

#[test]
fn stable_retirement_moves_into_finish_work_and_cannot_remove_a_successor() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>finish</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut root = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let mut producer = producer(&mut root);
    work(&mut producer).run();
    work(&mut producer).run();
    let finish = work(&mut producer);
    let (key, old) = store
        .stable_parses
        .lock()
        .unwrap()
        .iter()
        .map(|(key, lane)| (key.clone(), lane.clone()))
        .next()
        .unwrap();
    drop(producer);
    assert_eq!(store.stable_parses.lock().unwrap().len(), 1);
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Err(PublicationError::ProducerDropped))
    ));
    finish.run();
    assert!(store.stable_parses.lock().unwrap().is_empty());
    // Install an independently admitted successor and replay old retirement.
    let mut successor = store.begin_stable_parse(
        accepted.clone(),
        request(2, &accepted),
        registry.register_task(),
    );
    let current = store
        .stable_parses
        .lock()
        .unwrap()
        .get(&key)
        .unwrap()
        .clone();
    assert!(!Arc::ptr_eq(&old, &current));
    retire_stable(&store, &key, &old);
    assert!(Arc::ptr_eq(
        store.stable_parses.lock().unwrap().get(&key).unwrap(),
        &current
    ));
    let Poll::Ready(Ok(StableParseStep::Produce(producer))) = successor.poll(Waker::noop()) else {
        panic!("successor producer")
    };
    drop(producer);
    assert!(matches!(
        successor.poll(Waker::noop()),
        Poll::Ready(Err(StableParseError::ProducerDropped))
    ));
    drop(old);
    drop(current);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn publication_and_stable_edges_refuse_same_path_and_mixed_family_cycles() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>cycle</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let root_task = registry.register_task();
    let mut root =
        store.begin_publication(accepted.clone(), request(1, &accepted), root_task.clone());
    let mut producer = producer(&mut root);
    let p = producer
        .core
        .as_ref()
        .unwrap()
        .lane
        .as_ref()
        .unwrap()
        .task
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    let mut recursive = store.begin_publication(accepted.clone(), request(2, &accepted), p);
    assert!(matches!(
        recursive.poll(Waker::noop()),
        Poll::Ready(Err(PublicationError::Cycle))
    ));
    work(&mut producer).run();
    let stable = work(&mut producer);
    let s = store
        .stable_parses
        .lock()
        .unwrap()
        .values()
        .next()
        .unwrap()
        .task
        .lock()
        .unwrap()
        .clone()
        .unwrap();
    assert!(
        s.wait_for(&root_task).is_err(),
        "W -> P -> S refuses S -> W"
    );
    let mut recursive =
        store.begin_stable_parse(accepted.clone(), request(3, &accepted), s.clone());
    assert!(matches!(
        recursive.poll(Waker::noop()),
        Poll::Ready(Err(StableParseError::Cycle))
    ));
    stable.run();
    drain(&mut producer);
    assert!(outcome(&mut root).into_envelope().is_some());
    drop(root_task);
    drop(s);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn cancelling_or_dropping_one_stable_waiter_withdraws_only_its_registration() {
    for cancel in [false, true] {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>withdraw</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::new(source, grammar, registry.clone());
        let context = request(1, &accepted);
        let stop = context.cancellation.clone();
        let mut first =
            store.begin_stable_parse(accepted.clone(), context, registry.register_task());
        let mut sibling = store.begin_stable_parse(
            accepted.clone(),
            request(2, &accepted),
            registry.register_task(),
        );
        let Poll::Ready(Ok(StableParseStep::Produce(producer))) = first.poll(Waker::noop()) else {
            panic!("stable producer")
        };
        let first_count = Arc::new(Count::default());
        let sibling_count = Arc::new(Count::default());
        assert!(first.poll(&Waker::from(first_count.clone())).is_pending());
        assert!(sibling
            .poll(&Waker::from(sibling_count.clone()))
            .is_pending());
        if cancel {
            stop.cancel();
            assert_eq!(first_count.0.load(Ordering::SeqCst), 1);
            assert!(matches!(
                first.poll(Waker::noop()),
                Poll::Ready(Err(StableParseError::Cancelled))
            ));
        }
        drop(first);
        let before = first_count.0.load(Ordering::SeqCst);
        producer.run();
        assert_eq!(first_count.0.load(Ordering::SeqCst), before);
        assert_eq!(sibling_count.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            sibling.poll(Waker::noop()),
            Poll::Ready(Ok(StableParseStep::Parsed(_)))
        ));
        assert!(store.stable_parses.lock().unwrap().is_empty());
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn held_publication_and_stable_polls_return_before_producer_release() {
    use std::time::Duration;
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>held</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut root = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let mut producer = producer(&mut root);
    let (send, receive) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let pending = root.poll(Waker::noop()).is_pending();
        send.send((root, pending))
            .unwrap_or_else(|_| panic!("receiver stays live"));
    });
    let result = receive.recv_timeout(Duration::from_secs(1));
    // Always release and join before asserting, including a planted blocking poll.
    drain(&mut producer);
    poller.join().unwrap();
    let (mut root, pending) = result.expect("held publication poll must return before release");
    assert!(pending);
    assert!(outcome(&mut root).into_envelope().is_some());

    let mut root = store.begin_stable_parse(
        accepted.clone(),
        request(2, &accepted),
        registry.register_task(),
    );
    let Poll::Ready(Ok(StableParseStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("stable producer")
    };
    let (send, receive) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let pending = root.poll(Waker::noop()).is_pending();
        send.send((root, pending))
            .unwrap_or_else(|_| panic!("receiver stays live"));
    });
    let result = receive.recv_timeout(Duration::from_secs(1));
    producer.run();
    poller.join().unwrap();
    let (mut root, pending) = result.expect("held stable poll must return before release");
    assert!(pending);
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Ok(StableParseStep::Parsed(_)))
    ));
}

#[test]
fn dropped_phase_work_wakes_its_producer_and_subscribers_with_typed_abandonment() {
    for phase in 0..3 {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>drop</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::new(source, grammar, registry.clone());
        let mut root = store.begin_publication(
            accepted.clone(),
            request(phase, &accepted),
            registry.register_task(),
        );
        let mut producer = producer(&mut root);
        for _ in 0..phase {
            work(&mut producer).run();
        }
        let work = work(&mut producer);
        let count = Arc::new(Count::default());
        let waker = Waker::from(count.clone());
        assert!(producer.poll(&waker).is_pending());
        assert!(root.poll(&waker).is_pending());
        drop(work);
        assert!(matches!(
            producer.poll(Waker::noop()),
            Poll::Ready(PublicationProducerStep::Complete)
        ));
        assert_eq!(
            count.0.load(Ordering::SeqCst),
            2,
            "both the producer and its subscriber wake after phase {phase} abandonment"
        );
        assert!(matches!(
            root.poll(Waker::noop()),
            Poll::Ready(Err(PublicationError::ProducerDropped))
        ));
        assert!(store.stable_parses.lock().unwrap().is_empty());
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn producer_notifications_run_after_execution_scopes_and_owner_locks_release() {
    struct Probe {
        registry: TaskRegistry,
        lane: Arc<StableParseLane>,
        calls: AtomicUsize,
        bad: AtomicUsize,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if ExecutionScope::current(&self.registry).is_some()
                || self.lane.state.try_lock().is_err()
            {
                self.bad.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>scope</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut root = store.begin_stable_parse(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let Poll::Ready(Ok(StableParseStep::Produce(producer))) = root.poll(Waker::noop()) else {
        panic!("stable producer")
    };
    let probe = Arc::new(Probe {
        registry,
        lane: root.interest.as_ref().unwrap().lane.clone(),
        calls: AtomicUsize::new(0),
        bad: AtomicUsize::new(0),
    });
    assert!(root.poll(&Waker::from(probe.clone())).is_pending());
    producer.run();
    assert_eq!(probe.calls.load(Ordering::SeqCst), 1);
    assert_eq!(probe.bad.load(Ordering::SeqCst), 0);
    assert!(matches!(
        root.poll(Waker::noop()),
        Poll::Ready(Ok(StableParseStep::Parsed(_)))
    ));
}

#[test]
fn abandoned_publication_releases_its_lane_and_unchanged_source_can_retry() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>retry</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut first = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let mut sibling = store.begin_publication(
        accepted.clone(),
        request(2, &accepted),
        registry.register_task(),
    );
    drop(producer(&mut first));
    for handoff in [&mut first, &mut sibling] {
        assert!(matches!(
            handoff.poll(Waker::noop()),
            Poll::Ready(Err(PublicationError::ProducerDropped))
        ));
    }
    assert!(
        store.lanes.lock().unwrap().is_empty(),
        "a drained abandoned flight retains no coordination state"
    );
    let mut next = store.begin_publication(
        accepted.clone(),
        request(3, &accepted),
        registry.register_task(),
    );
    drain(&mut producer(&mut next));
    assert!(outcome(&mut next).into_envelope().is_some());
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn abandoned_publication_retires_after_its_last_subscriber_already_dropped() {
    for transfer_work in [false, true] {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>abandoned</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::new(source, grammar, registry.clone());
        let mut root = store.begin_publication(
            accepted.clone(),
            request(1, &accepted),
            registry.register_task(),
        );
        let mut active = producer(&mut root);
        let external = transfer_work.then(|| work(&mut active));
        drop(root);
        if let Some(external) = external {
            drop(external);
            assert!(
                store.lanes.lock().unwrap().is_empty(),
                "abandoned work retires without a remaining subscriber poll"
            );
        }
        drop(active);
        assert!(
            store.lanes.lock().unwrap().is_empty(),
            "abandoned producer retires without a remaining subscriber poll"
        );
        assert_eq!(registry.active_task_count_for_tests(), 0);
        let mut next = store.begin_publication(
            accepted.clone(),
            request(2, &accepted),
            registry.register_task(),
        );
        drain(&mut producer(&mut next));
        assert!(outcome(&mut next).into_envelope().is_some());
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn publication_repoll_replaces_waker_and_spurious_wakes_execute_no_work() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>repoll</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let mut root = store.begin_publication(
        accepted.clone(),
        request(1, &accepted),
        registry.register_task(),
    );
    let mut producer = producer(&mut root);
    let old = Arc::new(Count::default());
    let new = Arc::new(Count::default());
    assert!(root.poll(&Waker::from(old.clone())).is_pending());
    let waker = Waker::from(new.clone());
    waker.wake_by_ref();
    assert!(root.poll(&waker).is_pending());
    assert!(root.poll(&waker).is_pending());
    assert_eq!(store.audit_snapshot().parser_started, 0);
    drain(&mut producer);
    assert_eq!(old.0.load(Ordering::SeqCst), 0);
    assert_eq!(new.0.load(Ordering::SeqCst), 2);
    assert!(outcome(&mut root).into_envelope().is_some());
    assert_eq!(registry.active_task_count_for_tests(), 0);
}

#[test]
fn publication_and_stable_completion_races_never_lose_registration() {
    for _ in 0..32 {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>race</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::new(source, grammar, registry.clone());
        let mut root = store.begin_publication(
            accepted.clone(),
            request(1, &accepted),
            registry.register_task(),
        );
        let mut producer = producer(&mut root);
        let count = Arc::new(Count::default());
        let start = Arc::new(std::sync::Barrier::new(2));
        let worker_start = start.clone();
        let worker = std::thread::spawn(move || {
            worker_start.wait();
            drain(&mut producer);
        });
        start.wait();
        let result = root.poll(&Waker::from(count.clone()));
        worker.join().unwrap();
        assert!(result.is_ready() || count.0.load(Ordering::SeqCst) == 1);
        match result {
            Poll::Ready(Ok(PublicationStep::Outcome(value))) => {
                assert!(value.into_envelope().is_some())
            }
            Poll::Pending => assert!(outcome(&mut root).into_envelope().is_some()),
            _ => panic!("publication completion expected"),
        }

        let mut stable = store.begin_stable_parse(
            accepted.clone(),
            request(2, &accepted),
            registry.register_task(),
        );
        let Poll::Ready(Ok(StableParseStep::Produce(producer))) = stable.poll(Waker::noop()) else {
            panic!("stable producer")
        };
        let count = Arc::new(Count::default());
        let start = Arc::new(std::sync::Barrier::new(2));
        let worker_start = start.clone();
        let worker = std::thread::spawn(move || {
            worker_start.wait();
            producer.run();
        });
        start.wait();
        let result = stable.poll(&Waker::from(count.clone()));
        worker.join().unwrap();
        assert!(result.is_ready() || count.0.load(Ordering::SeqCst) == 1);
        match result {
            Poll::Ready(Ok(StableParseStep::Parsed(value))) => drop(value),
            Poll::Pending => assert!(matches!(
                stable.poll(Waker::noop()),
                Poll::Ready(Ok(StableParseStep::Parsed(_)))
            )),
            _ => panic!("stable completion expected"),
        }
        assert!(store.stable_parses.lock().unwrap().is_empty());
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn stable_and_finish_panics_settle_siblings_and_release_owned_interests() {
    use persistence::{CarrierStableUnitStore, RetainedStableUnit};
    struct PanickingPhase {
        reads: AtomicUsize,
        stable: bool,
    }
    impl CarrierStableUnitStore for PanickingPhase {
        fn retained(
            &self,
            _: &FrameworkArtifactId,
            _: &AcceptedRegisteredCarrierSource,
        ) -> Option<RetainedStableUnit> {
            let read = self.reads.fetch_add(1, Ordering::SeqCst) + 1;
            assert!(!(self.stable && read == 2), "stable retained read panic");
            None
        }
        fn retain(
            &self,
            _: &FrameworkArtifactId,
            _: &AcceptedRegisteredCarrierSource,
            _: &Arc<FrameworkParseArtifact>,
            _: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
        ) {
            panic!("finish retention panic");
        }
        fn discard(&self, _: &FrameworkArtifactId, _: &AcceptedRegisteredCarrierSource) {}
    }
    for stable in [true, false] {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>panic</template>");
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::with_dependencies(
            source,
            grammar,
            Arc::new(PanickingPhase {
                reads: AtomicUsize::new(0),
                stable,
            }),
            Arc::new(crate::meta_provenance::MetaProvenance::default()),
            registry.clone(),
        );
        let mut first = store.begin_publication(
            accepted.clone(),
            request(1, &accepted),
            registry.register_task(),
        );
        let mut sibling = store.begin_publication(
            accepted.clone(),
            request(2, &accepted),
            registry.register_task(),
        );
        let mut producer = producer(&mut first);
        let first_wakes = Arc::new(Count::default());
        let sibling_wakes = Arc::new(Count::default());
        assert!(first.poll(&Waker::from(first_wakes.clone())).is_pending());
        assert!(sibling
            .poll(&Waker::from(sibling_wakes.clone()))
            .is_pending());
        drain(&mut producer);
        assert_eq!(first_wakes.0.load(Ordering::SeqCst), 1);
        assert_eq!(sibling_wakes.0.load(Ordering::SeqCst), 1);
        assert!(matches!(
            outcome(&mut first),
            PublicationOutcome::WinnerPanicked
        ));
        assert!(matches!(
            outcome(&mut sibling),
            PublicationOutcome::WinnerPanicked
        ));
        assert!(store.stable_parses.lock().unwrap().is_empty());
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}

#[test]
fn synchronous_publication_uses_its_store_authority_inside_an_unrelated_scope() {
    let (source, grammar) = authorities();
    let accepted = accepted(&source, &grammar, 1, "<template>independent</template>");
    let registry = TaskRegistry::default();
    let store = CarrierPublicationStore::new(source, grammar, registry.clone());
    let unrelated = TaskRegistry::default();
    let scope = ExecutionScope::enter(&unrelated);
    let outcome = store.publish_or_get(&accepted, request(1, &accepted));
    assert!(
        outcome.into_envelope().is_some(),
        "an unrelated host scope does not refuse complete publication"
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    assert_eq!(registry.active_task_count_for_tests(), 0);
    assert_eq!(unrelated.active_task_count_for_tests(), 1);
    drop(scope);
    assert_eq!(unrelated.active_task_count_for_tests(), 0);
}

#[test]
fn standalone_publication_refuses_recursion_during_both_retained_reads() {
    use persistence::{CarrierStableUnitStore, InMemoryStableUnitStore, RetainedStableUnit};
    struct ReentrantReads {
        inner: InMemoryStableUnitStore,
        store: Mutex<Option<CarrierPublicationStore>>,
        reads: AtomicUsize,
        at: usize,
        nested: std::sync::mpsc::Sender<PublicationOutcome>,
    }
    impl CarrierStableUnitStore for ReentrantReads {
        fn retained(
            &self,
            id: &FrameworkArtifactId,
            accepted: &AcceptedRegisteredCarrierSource,
        ) -> Option<RetainedStableUnit> {
            if self.reads.fetch_add(1, Ordering::SeqCst) + 1 == self.at {
                let store = self.store.lock().unwrap().clone().unwrap();
                let outcome = store.publish_or_get(accepted, request(99, accepted));
                self.nested.send(outcome).unwrap();
            }
            self.inner.retained(id, accepted)
        }
        fn retain(
            &self,
            id: &FrameworkArtifactId,
            accepted: &AcceptedRegisteredCarrierSource,
            artifact: &Arc<FrameworkParseArtifact>,
            cohort: crate::carrier_artifact_cohort::PersistedCarrierArtifactCohort,
        ) {
            self.inner.retain(id, accepted, artifact, cohort);
        }
        fn discard(&self, id: &FrameworkArtifactId, accepted: &AcceptedRegisteredCarrierSource) {
            self.inner.discard(id, accepted);
        }
    }
    for at in [1, 2] {
        let (source, grammar) = authorities();
        let accepted = accepted(&source, &grammar, 1, "<template>reentrant</template>");
        let (send, receive) = std::sync::mpsc::channel();
        let backend = Arc::new(ReentrantReads {
            inner: InMemoryStableUnitStore::default(),
            store: Mutex::new(None),
            reads: AtomicUsize::new(0),
            at,
            nested: send,
        });
        let registry = TaskRegistry::default();
        let store = CarrierPublicationStore::with_dependencies(
            source,
            grammar,
            backend.clone(),
            Arc::new(crate::meta_provenance::MetaProvenance::default()),
            registry.clone(),
        );
        *backend.store.lock().unwrap() = Some(store.clone());
        let worker_store = store.clone();
        let worker = std::thread::spawn(move || {
            worker_store.publish_or_get(&accepted, request(1, &accepted))
        });
        let nested = receive.recv_timeout(std::time::Duration::from_secs(1));
        if nested.is_err() {
            // Release a planted self-wait before joining and asserting. Never
            // notify while the table lock is held.
            let lanes: Vec<_> = store.lanes.lock().unwrap().values().cloned().collect();
            for lane in lanes {
                lane.settle(Err(PublicationError::ProducerDropped));
            }
        }
        let result = worker.join();
        let retained = backend.store.lock().unwrap().take();
        drop(retained);
        assert!(
            nested.is_ok(),
            "standalone recursive publication must refuse before producer release"
        );
        assert!(matches!(
            nested.unwrap(),
            PublicationOutcome::RetryExhausted
        ));
        assert!(
            result.unwrap().into_envelope().is_some(),
            "the outer complete answer survives the refused recursion"
        );
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        assert_eq!(registry.active_task_count_for_tests(), 0);
    }
}
