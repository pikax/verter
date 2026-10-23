use super::*;
use crate::driver::SUBMISSION_INBOX_CAPACITY;
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

fn request(file: &str) -> Request {
    Request {
        file_id: file.into(),
        target: TargetStage::Source,
        priority: Priority::Interactive,
        source: Some(Arc::from(format!("export const n = '{file}';"))),
        file_language: None,
        request_context: None,
    }
}

fn fill(scheduler: &Scheduler) {
    for _ in 0..SUBMISSION_INBOX_CAPACITY {
        scheduler.inbox.sender.try_send(Submission::Wake).unwrap();
    }
}

#[test]
fn owned_request_tracks_each_gating_prerequisite_before_its_own_stage() {
    use crate::dag::{DepKey, FileStageKey, WorkKind, WorkNodeIdentity};
    #[derive(Clone, Copy)]
    enum Release {
        Complete,
        Cancel,
        Fail,
    }
    for release in [Release::Complete, Release::Cancel, Release::Fail] {
        let scheduler = scheduler();
        let identity = |canonical: &str| WorkNodeIdentity::FileStage {
            canonical: Arc::from(canonical),
            incarnation: 1,
            generation: 1,
            stage: FileStageKey::Analysis,
        };
        let first = identity("/a.ts");
        let second = identity("/b.ts");
        let owner = identity("/owner.ts");
        let (handle, sender) = completion_pair();
        {
            let mut dag = scheduler.dag.lock();
            for dependency in [&first, &second] {
                dag.submit(
                    dependency.clone(),
                    WorkKind::Analysis,
                    Priority::Interactive,
                    vec![],
                    None,
                );
            }
            dag.submit(
                owner.clone(),
                WorkKind::Analysis,
                Priority::Interactive,
                vec![
                    DepKey::from_identity(&first),
                    DepKey::from_identity(&second),
                ],
                None,
            );
            let _ = dag.register_request(
                &Arc::from("/owner.ts"),
                1,
                TargetStage::Analysis,
                sender.clone(),
                None,
            );
            dag.bind_file_producer(&owner);
        }
        let waiter = scheduler.task_registry.register_task();
        let mut completion = handle
            .subscribe()
            .owned_producer(waiter.clone(), CancellationToken::new());
        for expected in [&first, &second, &owner] {
            assert!(completion.poll(Waker::noop()).is_pending());
            let job = scheduler
                .dag
                .lock()
                .next_ready()
                .expect("next prerequisite becomes ready");
            assert_eq!(&job.identity, expected);
            assert!(
                job.execution_task.wait_for(&waiter).is_err(),
                "the actual prerequisite cannot wait back on its root"
            );
            let mut dag = scheduler.dag.lock();
            if expected == &first {
                match release {
                    Release::Complete => {
                        dag.complete(expected);
                    }
                    Release::Cancel => {
                        dag.cancel(expected);
                    }
                    Release::Fail => {
                        dag.fanout_analysis_failure_to_waiters(
                            &Arc::from("/a.ts"),
                            1,
                            1,
                            &crate::job::SchedulerError::FileNotFound {
                                file_id: "/a.ts".into(),
                            },
                        );
                        dag.cancel(expected);
                    }
                }
            } else {
                dag.complete(expected);
            }
        }
        sender.send(CompletionState::Shutdown);
        assert!(matches!(
            completion.poll(Waker::noop()),
            Poll::Ready(Ok(CompletionState::Shutdown))
        ));
        assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
    }
}

#[test]
fn owned_request_tracks_source_before_a_transitive_analysis_producer_is_admitted() {
    use crate::dag::{DepKey, FileStageKey, WorkKind, WorkNodeIdentity};
    let scheduler = scheduler();
    let identity = |canonical: &str, stage| WorkNodeIdentity::FileStage {
        canonical: Arc::from(canonical),
        incarnation: 1,
        generation: 1,
        stage,
    };
    let source = identity("/dependency.ts", FileStageKey::Source);
    let analysis = identity("/dependency.ts", FileStageKey::Analysis);
    let owner = identity("/owner.ts", FileStageKey::Analysis);
    let (handle, sender) = completion_pair();
    {
        let mut dag = scheduler.dag.lock();
        dag.submit(
            source.clone(),
            WorkKind::Load,
            Priority::Interactive,
            vec![],
            None,
        );
        dag.submit(
            owner.clone(),
            WorkKind::Analysis,
            Priority::Interactive,
            vec![DepKey::from_identity(&analysis)],
            None,
        );
        let _ = dag.register_request(
            &Arc::from("/owner.ts"),
            1,
            TargetStage::Analysis,
            sender.clone(),
            None,
        );
        dag.bind_file_producer(&owner);
    }
    let waiter = scheduler.task_registry.register_task();
    let mut completion = handle
        .subscribe()
        .owned_producer(waiter.clone(), CancellationToken::new());
    assert!(completion.poll(Waker::noop()).is_pending());
    let source_job = scheduler
        .dag
        .lock()
        .next_ready()
        .expect("Source exists before Analysis");
    assert!(source_job.execution_task.wait_for(&waiter).is_err());
    {
        let mut dag = scheduler.dag.lock();
        dag.submit(
            analysis.clone(),
            WorkKind::Analysis,
            Priority::Interactive,
            vec![],
            None,
        );
        dag.bind_file_producer(&analysis);
        dag.complete(&source);
    }
    assert!(completion.poll(Waker::noop()).is_pending());
    let analysis_job = scheduler
        .dag
        .lock()
        .next_ready()
        .expect("late Analysis is ready");
    assert!(analysis_job.execution_task.wait_for(&waiter).is_err());
    scheduler.dag.lock().complete(&analysis);
    sender.send(CompletionState::Shutdown);
    assert!(matches!(
        completion.poll(Waker::noop()),
        Poll::Ready(Ok(CompletionState::Shutdown))
    ));
}

#[test]
fn owned_request_rebinds_after_scoped_producer_authority_is_installed() {
    use crate::dag::{DepKey, FileStageKey, WorkKind, WorkNodeIdentity};
    let scheduler = scheduler();
    let cache = WorkNodeIdentity::CacheNode {
        cache_id: crate::cache_id::SchedulerCacheId(17),
        key_hash: [1; 16],
        view_epoch: 1,
        snapshot_pin_id: crate::dag::PinId(1),
    };
    let owner = WorkNodeIdentity::FileStage {
        canonical: Arc::from("/owner.ts"),
        incarnation: 1,
        generation: 1,
        stage: FileStageKey::Analysis,
    };
    let (handle, sender) = completion_pair();
    {
        let mut dag = scheduler.dag.lock();
        dag.submit_cache(cache.clone(), Priority::Interactive, None);
        dag.submit(
            owner.clone(),
            WorkKind::Analysis,
            Priority::Interactive,
            vec![DepKey::from_identity(&cache)],
            None,
        );
        let _ = dag.register_request(
            &Arc::from("/owner.ts"),
            1,
            TargetStage::Analysis,
            sender.clone(),
            None,
        );
        dag.bind_file_producer(&owner);
    }
    let waiter = scheduler.task_registry.register_task();
    let mut completion = handle
        .subscribe()
        .owned_producer(waiter.clone(), CancellationToken::new());
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(counter.clone());
    assert!(completion.poll(&waker).is_pending());
    let producer = scheduler.task_registry.register_task();
    scheduler
        .dag
        .lock()
        .set_execution_task(&cache, producer.clone());
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        1,
        "installing the real authority wakes dependent roots"
    );
    assert!(completion.poll(&waker).is_pending());
    assert!(
        producer.wait_for(&waiter).is_err(),
        "the real scoped producer owns the dependency cycle edge"
    );
    sender.send(CompletionState::Shutdown);
    assert!(matches!(
        completion.poll(&waker),
        Poll::Ready(Ok(CompletionState::Shutdown))
    ));
}

#[test]
fn owned_request_cancellation_detaches_before_admission_without_driving() {
    use verter_execution::dependency::DependencyStop;
    let scheduler = scheduler();
    fill(&scheduler);
    let waiter = scheduler.task_registry.register_task();
    let stop = CancellationToken::new();
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut request = scheduler.begin_owned_request(request("/owned.ts"), waiter, stop.clone());
    let (send, receive) = std::sync::mpsc::channel();
    let poll_waker = waker.clone();
    let poller = std::thread::spawn(move || {
        let result = request.poll(&poll_waker);
        let _ = send.send((request, result));
    });
    let polled = receive.recv_timeout(std::time::Duration::from_secs(1));
    // Release before joining on every assertion path, including a planted
    // blocking offer. Capture the depth before releasing the held dependency.
    let depth = scheduler.inbox.receiver.len();
    if polled.is_err() {
        scheduler.drain_inbox();
    }
    poller.join().unwrap();
    let (mut request, result) =
        polled.expect("owned admission must return before capacity release");
    assert!(result.is_pending());
    assert_eq!(depth, SUBMISSION_INBOX_CAPACITY);
    stop.cancel();
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    assert!(matches!(
        request.poll(&waker),
        Poll::Ready(Err(DependencyStop::Cancelled))
    ));
    scheduler.drain_inbox();
    assert!(scheduler.try_get_source("/owned.ts").is_none());
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
    assert_eq!(scheduler.inbox.space.lock().len(), 0);
}

#[test]
fn owned_request_drop_withdraws_capacity_subscription_while_handle_remains_live() {
    let scheduler = scheduler();
    fill(&scheduler);
    let mut request = scheduler.begin_owned_request(
        request("/drop-owned.ts"),
        scheduler.task_registry.register_task(),
        CancellationToken::new(),
    );
    assert!(request.poll(Waker::noop()).is_pending());
    assert_eq!(scheduler.inbox.space.lock().len(), 1);
    drop(request);
    assert_eq!(scheduler.inbox.space.lock().len(), 0);
    scheduler.drain_inbox();
    assert!(scheduler.try_get_source("/drop-owned.ts").is_none());
}

#[test]
fn owned_request_capacity_wake_precedes_producer_and_completion_wakes() {
    let scheduler = scheduler();
    fill(&scheduler);
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut request = scheduler.begin_owned_request(
        request("/wake-owned.ts"),
        scheduler.task_registry.register_task(),
        CancellationToken::new(),
    );
    assert!(request.poll(&waker).is_pending());
    scheduler.drain_inbox();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        1,
        "capacity release wakes the retained offer"
    );
    assert!(request.poll(&waker).is_pending());
    scheduler.drain_inbox();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        2,
        "real producer binding wakes completion"
    );
    assert!(request.poll(&waker).is_pending());
    assert!(scheduler.drive_one());
    scheduler.drain_inbox();
    assert_eq!(
        counter.0.load(Ordering::SeqCst),
        3,
        "terminal completion wakes the waiter"
    );
    assert!(matches!(
        request.poll(&waker),
        Poll::Ready(Ok(CompletionState::Ready(_)))
    ));
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn owned_request_source_executes_as_the_waited_producer_and_refuses_a_mixed_cycle() {
    use verter_execution::tasks::{ExecutionScope, ExecutionTask};
    struct CycleLoader {
        parent: ExecutionTask,
    }
    impl SourceLoader for CycleLoader {
        fn load(&self, _: &str) -> Option<Arc<str>> {
            let producer = ExecutionScope::current(self.parent.registry())
                .expect("stage installs its actual execution task");
            assert!(!producer.same_task(&self.parent));
            assert!(
                producer.wait_for(&self.parent).is_err(),
                "parent is already waiting on this stage"
            );
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
    let registry = verter_execution::tasks::TaskRegistry::default();
    let parent = registry.register_task();
    let scheduler = Scheduler::new_sync_with_executor_and_registry(
        SchedulerConfig::default(),
        Arc::new(CycleLoader {
            parent: parent.clone(),
        }),
        Arc::new(DefaultExecutor),
        #[cfg(not(target_arch = "wasm32"))]
        crate::SchedulerCpuPool::new(
            1,
            SchedulerConfig::default().resolved_dag_budget().cpu as usize,
        ),
        #[cfg(not(target_arch = "wasm32"))]
        crate::SchedulerIoPool::new(
            1,
            SchedulerConfig::default().resolved_dag_budget().io as usize,
        ),
        registry,
    );
    let mut input = request("/mixed.ts");
    input.source = None;
    let mut request = scheduler.begin_owned_request(input, parent, CancellationToken::new());
    assert!(request.poll(Waker::noop()).is_pending());
    scheduler.drain_inbox();
    assert!(request.poll(Waker::noop()).is_pending());
    assert!(scheduler.drive_one());
    scheduler.drain_inbox();
    assert!(matches!(
        request.poll(Waker::noop()),
        Poll::Ready(Ok(CompletionState::Ready(_)))
    ));
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn owned_request_tracks_real_stage_owners_and_preserves_a_live_sibling() {
    use verter_execution::dependency::DependencyStop;
    let scheduler = scheduler();
    let first = scheduler.task_registry.register_task();
    let second = scheduler.task_registry.register_task();
    let stop = CancellationToken::new();
    let mut a = scheduler.begin_owned_request(request("/owned.ts"), first, stop.clone());
    let mut sibling_request = request("/owned.ts");
    sibling_request.source = None;
    let mut b = scheduler.begin_owned_request(sibling_request, second, CancellationToken::new());
    assert!(a.poll(Waker::noop()).is_pending());
    assert!(b.poll(Waker::noop()).is_pending());
    scheduler.drain_inbox();
    assert!(a.poll(Waker::noop()).is_pending());
    assert!(b.poll(Waker::noop()).is_pending());
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 2);
    stop.cancel();
    assert!(matches!(
        a.poll(Waker::noop()),
        Poll::Ready(Err(DependencyStop::Cancelled))
    ));
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 1);
    assert!(scheduler.drive_one());
    scheduler.drain_inbox();
    assert!(matches!(
        b.poll(Waker::noop()),
        Poll::Ready(Ok(CompletionState::Ready(_)))
    ));
    assert_eq!(scheduler.task_registry.wait_graph_occupancy().edges, 0);
}

fn finish(
    scheduler: &Arc<Scheduler>,
    subscription: &mut CompletionSubscription<RequestResult>,
) -> CompletionState<RequestResult> {
    loop {
        if let Poll::Ready(answer) = subscription.poll(Waker::noop()) {
            return answer;
        }
        if !scheduler.drive_one() {
            return match subscription.poll(Waker::noop()) {
                Poll::Ready(answer) => answer,
                Poll::Pending => panic!("the admitted dependency must make progress"),
            };
        }
    }
}

#[test]
fn submission_yields_before_capacity_is_released_and_resumes_on_another_thread() {
    let scheduler = scheduler();
    fill(&scheduler);
    let mut submission = scheduler.begin_submit_request(request("/admit.ts"));
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    let (reported, receive) = std::sync::mpsc::channel();
    let poller = std::thread::spawn(move || {
        let result = submission.poll(&waker);
        reported
            .send((submission, result))
            .unwrap_or_else(|_| panic!("receiver stays live"));
    });
    let result = receive.recv_timeout(std::time::Duration::from_secs(1));
    let depth_before_release = scheduler.inbox.receiver.len();
    // Always free the queue before asserting or joining, including when a
    // planted blocking send prevents the poll from returning on time.
    scheduler.drain_inbox();
    poller.join().unwrap();
    let (mut submission, result) = result.expect("poll must yield before capacity is released");
    assert!(result.is_pending());
    assert_eq!(depth_before_release, SUBMISSION_INBOX_CAPACITY);
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(mut completion) = submission.poll(Waker::noop()) else {
        panic!("freed capacity admits the retained request");
    };
    assert!(
        completion.poll(Waker::noop()).is_pending(),
        "admission does not execute stages"
    );
    assert!(finish(&scheduler, &mut completion).is_ready());
}

#[test]
fn dropping_a_pending_submission_withdraws_only_its_own_wake() {
    let scheduler = scheduler();
    fill(&scheduler);
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut dropped = scheduler.begin_submit_request(request("/dropped.ts"));
    let mut kept = scheduler.begin_submit_request(request("/kept.ts"));
    assert!(dropped.poll(&waker).is_pending());
    assert!(kept.poll(&waker).is_pending());
    assert!(kept.poll(&waker).is_pending());
    drop(dropped);
    scheduler.drain_inbox();
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(mut completion) = kept.poll(&waker) else {
        panic!("sibling remains admissible");
    };
    assert!(finish(&scheduler, &mut completion).is_ready());
    assert!(scheduler.try_get_source("/dropped.ts").is_none());
}

#[test]
fn an_atomic_batch_retains_input_order_across_submission_backpressure() {
    let scheduler = scheduler();
    fill(&scheduler);
    let mut batch = scheduler.begin_submit_batch_atomic(vec![request("/a.ts"), request("/b.ts")]);
    assert!(batch.poll(Waker::noop()).is_pending());
    assert_eq!(scheduler.inbox.receiver.len(), SUBMISSION_INBOX_CAPACITY);
    scheduler.drain_inbox();
    let Poll::Ready(mut batch) = batch.poll(Waker::noop()) else {
        panic!("batch fits one freed slot");
    };
    assert_eq!(scheduler.inbox.receiver.len(), 1, "one atomic inbox item");
    assert_eq!(batch.len(), 2);
    for (subscription, canonical) in batch.iter_mut().zip(["/a.ts", "/b.ts"]) {
        let CompletionState::Ready(RequestResult::Source(snapshot)) =
            finish(&scheduler, subscription)
        else {
            panic!("each source request completes");
        };
        assert_eq!(
            snapshot.source.as_ref(),
            format!("export const n = '{canonical}';")
        );
    }
}

#[test]
fn capacity_release_racing_registration_does_not_lose_the_wake() {
    for _ in 0..32 {
        let scheduler = scheduler();
        fill(&scheduler);
        let mut submission = scheduler.begin_submit_request(request("/race.ts"));
        let counter = Arc::new(CountWake::default());
        let waker = Waker::from(Arc::clone(&counter));
        let barrier = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let releaser = scope.spawn(|| {
                barrier.wait();
                scheduler.drain_inbox();
            });
            barrier.wait();
            let polled = submission.poll(&waker);
            releaser.join().unwrap();
            assert!(polled.is_ready() || counter.0.load(Ordering::SeqCst) == 1);
        });
    }
}

#[test]
fn reset_wakes_a_pending_submission_and_rejects_its_retired_lifetime() {
    let scheduler = scheduler();
    fill(&scheduler);
    let counter = Arc::new(CountWake::default());
    let waker = Waker::from(Arc::clone(&counter));
    let mut submission = scheduler.begin_submit_request(request("/reset.ts"));
    assert!(submission.poll(&waker).is_pending());
    scheduler.reset();
    assert_eq!(counter.0.load(Ordering::SeqCst), 1);
    let Poll::Ready(mut completion) = submission.poll(&waker) else {
        panic!("reset released inbox capacity");
    };
    assert!(matches!(
        finish(&scheduler, &mut completion),
        CompletionState::Shutdown
    ));
    assert!(scheduler.try_get_source("/reset.ts").is_none());
}

#[test]
fn quiesce_wakes_capacity_waiters_after_unlock_and_empty_batches_need_no_slot() {
    struct UnlockedWake {
        scheduler: std::sync::Weak<Scheduler>,
        count: AtomicUsize,
    }
    impl Wake for UnlockedWake {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().expect("handoff retains scheduler");
            assert!(scheduler.inbox.space.try_lock().is_some());
            assert!(scheduler.dag.try_lock().is_some());
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }
    let scheduler = scheduler();
    fill(&scheduler);
    let mut empty = scheduler.begin_submit_batch_atomic(Vec::new());
    assert!(matches!(empty.poll(Waker::noop()), Poll::Ready(batch) if batch.is_empty()));
    assert_eq!(scheduler.inbox.receiver.len(), SUBMISSION_INBOX_CAPACITY);
    let counter = Arc::new(UnlockedWake {
        scheduler: Arc::downgrade(&scheduler),
        count: AtomicUsize::new(0),
    });
    let waker = Waker::from(Arc::clone(&counter));
    let mut submission = scheduler.begin_submit_request(request("/quiesce.ts"));
    assert!(submission.poll(&waker).is_pending());
    scheduler.quiesce();
    assert_eq!(counter.count.load(Ordering::SeqCst), 1);
    assert!(submission.poll(&waker).is_ready());
}

#[test]
fn completion_wakes_run_after_admission_completion_and_retirement_locks_release() {
    struct Probe {
        scheduler: std::sync::Weak<Scheduler>,
        locked: AtomicUsize,
        wakes: AtomicUsize,
    }
    impl Wake for Probe {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().expect("scheduler remains live");
            let mut locked = scheduler.dag.try_lock().is_none();
            locked |= matches!(
                scheduler.nodes.try_get_mut("/completion.ts"),
                dashmap::try_result::TryResult::Locked
            );
            #[cfg(not(target_arch = "wasm32"))]
            {
                locked |= scheduler.reset_serial.try_lock().is_none();
            }
            if locked {
                self.locked.fetch_add(1, Ordering::SeqCst);
            }
            self.wakes.fetch_add(1, Ordering::SeqCst);
        }
    }
    let scheduler = scheduler();
    let probe = Arc::new(Probe {
        scheduler: Arc::downgrade(&scheduler),
        locked: AtomicUsize::new(0),
        wakes: AtomicUsize::new(0),
    });
    let waker = Waker::from(Arc::clone(&probe));
    for terminal in [
        "stale",
        "cold",
        "warm",
        "superseded",
        "failed",
        "removed",
        "reset",
    ] {
        let mut input = request("/completion.ts");
        if terminal == "warm" {
            input.source = None;
        }
        if terminal == "failed" {
            input.file_id = "/missing.ts".into();
            input.source = None;
        }
        let mut admission = scheduler.begin_submit_request(input);
        let Poll::Ready(mut completion) = admission.poll(&waker) else {
            panic!("empty inbox admits immediately");
        };
        assert!(completion.poll(&waker).is_pending());
        match terminal {
            "stale" => {
                scheduler.remove("/completion.ts");
                let _replacement = scheduler.submit_request(request("/completion.ts"));
                scheduler.drain_inbox();
            }
            "cold" | "warm" | "failed" => scheduler.drive_all(),
            "superseded" => {
                scheduler.drain_inbox();
                let _replacement = scheduler.submit_request(request("/completion.ts"));
                scheduler.drain_inbox();
            }
            "removed" => {
                scheduler.drain_inbox();
                scheduler.remove("/completion.ts");
            }
            "reset" => {
                scheduler.drain_inbox();
                scheduler.reset();
            }
            _ => unreachable!(),
        }
        let answer = completion.poll(&waker);
        assert!(
            match terminal {
                "cold" | "warm" => matches!(answer, Poll::Ready(CompletionState::Ready(_))),
                "superseded" => matches!(answer, Poll::Ready(CompletionState::Superseded)),
                "failed" => matches!(answer, Poll::Ready(CompletionState::Failed(_))),
                "stale" | "removed" | "reset" =>
                    matches!(answer, Poll::Ready(CompletionState::Shutdown)),
                _ => unreachable!(),
            },
            "{terminal}: wrong completion {answer:?}"
        );
        assert_eq!(probe.wakes.load(Ordering::SeqCst), 1, "{terminal}");
        assert_eq!(
            probe.locked.load(Ordering::SeqCst),
            0,
            "{terminal}: wake under scheduler lock"
        );
        probe.wakes.store(0, Ordering::SeqCst);
    }
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn a_driver_reset_does_not_wait_for_the_reset_that_may_be_joining_it() {
    let scheduler = scheduler();
    let serial = scheduler.reset_serial.lock();
    let (done, completed) = std::sync::mpsc::channel();
    let resetting = Arc::clone(&scheduler);
    let driver = std::thread::spawn(move || {
        let _role = caller_kind::CallerKindGuard::install(caller_kind::CallerKind::Driver);
        // Model the identity retained while this scheduler's reset joins
        // its driver. The role alone would also match another scheduler.
        *resetting.driver_thread_id.lock() = Some(std::thread::current().id());
        resetting.reset();
        *resetting.driver_thread_id.lock() = None;
        done.send(()).unwrap();
    });
    let returned_before_join = completed.recv_timeout(std::time::Duration::from_secs(1));
    // Release even if a planted blocking serialization hold prevents return.
    drop(serial);
    driver.join().unwrap();
    returned_before_join.expect("the driver must leave the active reset to its external owner");
}

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn another_schedulers_driver_still_waits_for_this_schedulers_reset_barrier() {
    let scheduler = scheduler();
    let serial = scheduler.reset_serial.lock();
    let (done, completed) = std::sync::mpsc::channel();
    let resetting = Arc::clone(&scheduler);
    let driver = std::thread::spawn(move || {
        let _role = caller_kind::CallerKindGuard::install(caller_kind::CallerKind::Driver);
        resetting.reset();
        done.send(()).unwrap();
    });
    let premature = completed.recv_timeout(std::time::Duration::from_millis(100));
    drop(serial);
    driver.join().unwrap();
    assert!(
        matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "a foreign driver must honor this reset barrier"
    );
}
