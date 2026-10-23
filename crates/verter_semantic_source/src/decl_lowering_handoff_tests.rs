//! Pollable owner-shard handoffs: a submitter whose worker's queue is full
//! yields instead of blocking, the worker's freed space and its answer each
//! wake the registered poll exactly when they happen, and a dropped handoff
//! leaves no registration and no retained parse behind.
//!
//! The single worker is held by an event-controlled job (it waits on its own
//! channel), so "the queue is full" and "the answer has not arrived" are
//! states the test creates, never timings it hopes for. A held job is
//! released when its sender drops, so a failed assertion still frees the
//! worker, and dropping the service joins it.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use std::time::Duration;

use verter_session_query::source::snapshot::SnapshotKey;

use super::{DeclLoweringService, LeaseHandoff, LeasedBatchHandoff, LeasedJob, LeasedRunHandoff};

/// Every bounded wait in this suite: long enough for a loaded machine, and
/// only ever spent when the code under test is wrong.
const DEADLINE: Duration = Duration::from_secs(10);

fn key(canonical: &str, hash_byte: u8) -> SnapshotKey {
    SnapshotKey {
        canonical: Arc::from(canonical),
        whole_hash: [hash_byte; 16],
        parse_env_hash: [0u8; 16],
    }
}

fn source() -> Arc<str> {
    Arc::from("type A = { a: 1 };\ntype B = { b: 2 };\n")
}

/// One worker; a queue of `capacity` jobs.
fn service(capacity: usize) -> Arc<DeclLoweringService> {
    let mut service = DeclLoweringService::new_with(/* lazy = */ true, 1);
    service.queue_capacity = capacity;
    Arc::new(service)
}

/// A waker that counts its wakes and reports each on a channel.
struct ReportingWake {
    wakes: AtomicUsize,
    report: parking_lot::Mutex<Sender<()>>,
}

impl Wake for ReportingWake {
    fn wake(self: Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        let _ = self.report.lock().send(());
    }
}

fn reporting_waker() -> (Arc<ReportingWake>, Waker, Receiver<()>) {
    let (report, reports) = channel();
    let wake = Arc::new(ReportingWake {
        wakes: AtomicUsize::new(0),
        report: parking_lot::Mutex::new(report),
    });
    (Arc::clone(&wake), Waker::from(wake), reports)
}

fn wakes(wake: &ReportingWake) -> usize {
    wake.wakes.load(Ordering::SeqCst)
}

/// Hold the service's single worker on a job until the returned sender
/// sends or drops; with `fill`, also fill its queue behind that job.
fn hold_worker(service: &DeclLoweringService, fill: bool) -> Sender<()> {
    let (started_tx, started) = channel::<()>();
    let (release, released) = channel::<()>();
    let worker = &service.workers()[0];
    worker
        .jobs
        .send(Box::new(move |_| {
            started_tx.send(()).unwrap();
            let _ = released.recv();
        }))
        .expect("the worker takes the holding job");
    started
        .recv_timeout(DEADLINE)
        .expect("the worker starts the holding job");
    if fill {
        loop {
            match worker.jobs.try_send(Box::new(|_| {})) {
                Ok(()) => {}
                Err(crossbeam_channel::TrySendError::Full(_)) => break,
                Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                    panic!("the worker is alive")
                }
            }
        }
    }
    release
}

/// A lease on `key`, acquired through the bounded driver.
fn acquire(service: &Arc<DeclLoweringService>, key: &SnapshotKey) -> super::LeaseOutcome {
    let mut handoff = service.begin_acquire_lease(key, &source(), oxc_span::SourceType::ts());
    finish(|waker| handoff.poll(waker)).0
}

/// Whether `key`'s retained parse is live, through the bounded driver:
/// `None` on a lease miss.
fn run_leased(service: &Arc<DeclLoweringService>, key: &SnapshotKey) -> Option<bool> {
    let mut handoff = service.begin_run_leased(key, |program| program.is_some());
    finish(|waker| handoff.poll(waker))
}

fn space_waiters(service: &DeclLoweringService) -> usize {
    service.workers()[0].space.lock().len()
}

/// Poll to completion, failing — never hanging — when a pending poll is not
/// woken within the deadline.
fn finish<T>(mut poll: impl FnMut(&Waker) -> Poll<T>) -> T {
    let (_wake, waker, reports) = reporting_waker();
    loop {
        match poll(&waker) {
            Poll::Ready(answer) => return answer,
            Poll::Pending => reports
                .recv_timeout(DEADLINE)
                .expect("a pending handoff is woken by what it waits on"),
        }
    }
}

/// A full queue never blocks the submitter: its poll returns `Pending`
/// holding the job and one registration, re-polling keeps that one
/// registration, and the slot the worker frees wakes it. A submitter that
/// failed to register for the space would never be woken here.
#[test]
fn a_full_worker_queue_yields_until_the_worker_frees_space() {
    let service = service(1);
    let k = key("/ws/backpressure.ts", 1);
    let lease = acquire(&service, &k);
    let release = hold_worker(&service, true);
    let (wake, waker, reports) = reporting_waker();

    let mut handoff: LeasedRunHandoff<bool> =
        service.begin_run_leased(&k, |program| program.is_some());
    assert!(handoff.poll(&waker).is_pending());
    assert!(
        !handoff.handoff.admitted(),
        "a full queue leaves the job with its submitter"
    );
    assert!(handoff.poll(&waker).is_pending());
    assert_eq!(
        space_waiters(&service),
        1,
        "re-polling keeps one registration"
    );
    assert_eq!(wakes(&wake), 0, "registering never wakes");

    release.send(()).unwrap();
    reports
        .recv_timeout(DEADLINE)
        .expect("the slot the worker frees wakes the waiting submitter");
    assert_eq!(finish(|waker| handoff.poll(waker)), Some(true));
    assert_eq!(
        space_waiters(&service),
        0,
        "admission withdraws the registration"
    );
    drop(lease.lease);
}

/// An admitted job's answer wakes the poll that registered for it, once,
/// and the next poll takes it.
#[test]
fn the_workers_answer_wakes_the_registered_poll() {
    let service = service(4);
    let k = key("/ws/answer.ts", 2);
    let lease = acquire(&service, &k);
    let release = hold_worker(&service, false);
    let (wake, waker, reports) = reporting_waker();

    let mut handoff = service.begin_run_leased(&k, |program| {
        program
            .expect("parse must succeed")
            .borrow_dependent()
            .body
            .len()
    });
    assert!(handoff.poll(&waker).is_pending());
    assert!(
        handoff.handoff.admitted(),
        "a queue with space admits at once"
    );
    assert!(handoff.poll(&waker).is_pending());
    assert_eq!(wakes(&wake), 0);

    release.send(()).unwrap();
    reports
        .recv_timeout(DEADLINE)
        .expect("the worker's answer wakes the registered poll");
    assert_eq!(handoff.poll(&waker), Poll::Ready(Some(2)));
    assert_eq!(wakes(&wake), 1, "one answer, one wake");
    drop(lease.lease);
}

/// Dropping a submitter that waits for queue space withdraws its
/// registration: nothing of it outlives it on the worker.
#[test]
fn dropping_a_submitter_waiting_for_space_withdraws_its_registration() {
    let service = service(1);
    let k = key("/ws/withdrawn.ts", 3);
    let release = hold_worker(&service, true);
    let (wake, waker, _reports) = reporting_waker();

    let mut handoff = service.begin_run_leased(&k, |_| ());
    assert!(handoff.poll(&waker).is_pending());
    assert_eq!(space_waiters(&service), 1);
    drop(handoff);
    assert_eq!(space_waiters(&service), 0, "a dropped submitter withdraws");

    release.send(()).unwrap();
    service
        .release_completion()
        .wait()
        .expect("the worker drains its queue");
    assert_eq!(wakes(&wake), 0, "a withdrawn submitter is never woken");
}

/// An acquisition abandoned after the worker admitted it pins nothing: the
/// worker releases the lease it acquired for it on its own shard, so no
/// live lease and no retained parse is left behind.
#[test]
fn an_abandoned_acquisition_releases_its_lease_on_the_worker() {
    let service = service(4);
    let k = key("/ws/abandoned.ts", 4);
    let release = hold_worker(&service, false);
    let (_wake, waker, _reports) = reporting_waker();

    let mut handoff: LeaseHandoff<()> =
        service.begin_acquire_lease(&k, &source(), oxc_span::SourceType::ts());
    assert!(handoff.poll(&waker).is_pending());
    assert!(handoff.handoff.admitted());
    drop(handoff);

    release.send(()).unwrap();
    service
        .release_completion()
        .wait()
        .expect("the worker applies the abandoned lease's release");
    assert_eq!(
        service.live_lease_count(),
        0,
        "no lease outlives its handoff"
    );
    assert!(
        run_leased(&service, &k).is_none(),
        "the abandoned acquisition retains no parse"
    );
}

#[test]
fn stopping_an_owned_acquisition_detaches_before_queue_or_reply_release() {
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::dependency::DependencyStop;
    use verter_execution::tasks::TaskRegistry;

    for full in [false, true] {
        let service = service(2);
        let k = key("/ws/stopped.ts", 21);
        let registry = TaskRegistry::default();
        let release = hold_worker(&service, full);
        let token = CancellationToken::new();
        let mut cancelled = service
            .begin_acquire_lease(&k, &source(), oxc_span::SourceType::ts())
            .owned(registry.register_task(), token.clone());
        let mut sibling = service
            .begin_acquire_lease(&k, &source(), oxc_span::SourceType::ts())
            .owned(registry.register_task(), CancellationToken::new());
        let (wake, waker, _) = reporting_waker();
        assert!(cancelled.poll(&waker).is_pending());
        assert!(sibling.poll(&waker).is_pending());
        assert_eq!(registry.wait_graph_occupancy().edges, 2);
        token.cancel();
        assert_eq!(wakes(&wake), 1);
        assert!(matches!(
            cancelled.poll(&waker),
            Poll::Ready(Err(DependencyStop::Cancelled))
        ));
        assert_eq!(registry.wait_graph_occupancy().edges, 1);
        if full {
            assert_eq!(space_waiters(&service), 1);
        }
        release.send(()).unwrap();
        let outcome = finish_owned(|waker| sibling.poll(waker))
            .unwrap()
            .unwrap()
            .0;
        assert!(run_leased(&service, &k).is_some());
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        drop(outcome);
        service.release_completion().wait().unwrap();
        assert_eq!(service.live_lease_count(), 0);
        assert!(run_leased(&service, &k).is_none());
    }
}

#[test]
fn an_owned_lowering_contains_job_panic_and_installs_its_producer_task() {
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::tasks::{ExecutionScope, TaskRegistry};
    let service = service(2);
    let k = key("/ws/task.ts", 22);
    let registry = TaskRegistry::default();
    let from_job = registry.clone();
    let task_present = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed_task = Arc::clone(&task_present);
    let mut handoff = service
        .begin_acquire_lease_and_run(&k, &source(), oxc_span::SourceType::ts(), move |_| {
            observed_task.store(
                ExecutionScope::current(&from_job).is_some(),
                Ordering::SeqCst,
            );
            panic!("lowering failed");
        })
        .owned(registry.register_task(), CancellationToken::new());
    assert!(matches!(
        finish_owned(|waker| handoff.poll(waker)),
        Ok(Err(super::LoweringFailure::Panicked))
    ));
    assert!(task_present.load(Ordering::SeqCst));
    service.release_completion().wait().unwrap();
    assert_eq!(service.live_lease_count(), 0);
}

#[test]
fn an_owned_run_reports_a_closed_reply_and_an_owned_batch_preserves_order() {
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::tasks::TaskRegistry;
    let service = service(2);
    let k = key("/ws/closed-reply.ts", 23);
    let lease = acquire(&service, &k);
    let registry = TaskRegistry::default();
    let mut handoff = service.begin_run_leased(&k, |_| 1_u64);
    let discarded = handoff.handoff.job.take().unwrap();
    // The owner drops the retained operation without producing its reply.
    handoff.handoff.job = Some(Box::new(move |_| drop(discarded)));
    let mut handoff = handoff.owned(registry.register_task(), CancellationToken::new());
    assert!(matches!(
        finish_owned(|waker| handoff.poll(waker)),
        Ok(Err(super::LoweringFailure::WorkerStopped))
    ));
    let jobs = vec![
        LeasedJob {
            key: k.clone(),
            run: Box::new(|_| 11_u64),
        },
        LeasedJob {
            key: key("/ws/missing-batch.ts", 24),
            run: Box::new(|_| 22_u64),
        },
        LeasedJob {
            key: k,
            run: Box::new(|_| 33_u64),
        },
    ];
    let mut batch = service
        .begin_run_leased_batch(jobs)
        .owned(registry.register_task(), CancellationToken::new());
    assert_eq!(
        finish_owned(|waker| batch.poll(waker)),
        Ok(Ok(vec![Some(11), None, Some(33)]))
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    drop(lease);
}

/// A handoff is owned, `Send` state: suspended on one thread, it resumes
/// and completes on another, and the answer is the one the synchronous
/// entry gives.
#[test]
fn a_pending_handoff_resumes_on_another_thread() {
    fn assert_send<T: Send>(_: &T) {}

    let service = service(4);
    let k = key("/ws/moved.ts", 5);
    let release = hold_worker(&service, false);
    let (_wake, waker, reports) = reporting_waker();

    let mut handoff =
        service.begin_acquire_lease_and_run(&k, &source(), oxc_span::SourceType::ts(), |program| {
            program.is_some()
        });
    assert_send(&handoff);
    assert!(handoff.poll(&waker).is_pending());

    let resumed = std::thread::spawn(move || {
        reports
            .recv_timeout(DEADLINE)
            .expect("the answer wakes the suspended handoff");
        match handoff.poll(&waker) {
            Poll::Ready((outcome, ran)) => (outcome.parsed_now, ran, outcome.lease),
            Poll::Pending => panic!("a woken handoff answers"),
        }
    });
    release.send(()).unwrap();
    let (parsed_now, ran, lease) = resumed.join().expect("the resuming thread completes");
    assert!(parsed_now, "the first acquisition parses");
    assert_eq!(ran, Some(true));
    assert!(
        run_leased(&service, &k) == Some(true),
        "the lease the moved handoff answered pins the parse"
    );
    drop(lease);
}

/// A batch offers every shard's share on its first poll and answers in input
/// order once every share has answered.
#[test]
fn a_batch_handoff_answers_in_input_order_after_every_share() {
    let service = service(4);
    let leased = key("/ws/batch-leased.ts", 6);
    let missing = key("/ws/batch-missing.ts", 7);
    let lease = acquire(&service, &leased);
    let release = hold_worker(&service, false);
    let (_wake, waker, reports) = reporting_waker();

    let jobs = vec![
        LeasedJob {
            key: missing.clone(),
            run: Box::new(|_| 0usize),
        },
        LeasedJob {
            key: leased.clone(),
            run: Box::new(|program| {
                program
                    .expect("parse must succeed")
                    .borrow_dependent()
                    .body
                    .len()
            }),
        },
    ];
    let mut handoff: LeasedBatchHandoff<usize> = service.begin_run_leased_batch(jobs);
    assert!(handoff.poll(&waker).is_pending());
    release.send(()).unwrap();
    reports
        .recv_timeout(DEADLINE)
        .expect("the share's answer wakes the batch");
    assert_eq!(finish(|waker| handoff.poll(waker)), vec![None, Some(2)]);
    drop(lease.lease);
}

#[test]
fn owned_lowering_refuses_same_shard_reentry_before_admitting_any_job() {
    use verter_execution::cancellation::CancellationToken;
    use verter_execution::tasks::{ExecutionScope, TaskRegistry};
    for operation in 0..3 {
        let service = service(2);
        let k = key("/ws/outer.ts", 25);
        // Different keys still share this one worker and cannot re-enter it.
        let nested_key = key("/ws/nested.ts", 26);
        let registry = TaskRegistry::default();
        let nested_service = service.clone();
        let ran = Arc::new(AtomicUsize::new(0));
        let nested_ran = ran.clone();
        let mut outer = service
            .begin_acquire_lease_and_run(&k, &source(), oxc_span::SourceType::ts(), move |_| {
                let waiter = ExecutionScope::current_task().expect("actual worker task");
                if operation == 1 {
                    let mut nested = nested_service
                        .begin_run_leased_batch(vec![LeasedJob {
                            key: nested_key,
                            run: Box::new(move |_| {
                                nested_ran.fetch_add(1, Ordering::SeqCst);
                                1
                            }),
                        }])
                        .owned(waiter, CancellationToken::new());
                    matches!(
                        nested.poll(Waker::noop()),
                        Poll::Ready(Ok(super::LoweringStep::Value(Err(
                            super::LoweringFailure::Reentrant
                        ))))
                    )
                } else if operation == 2 {
                    let mut nested = nested_service
                        .begin_acquire_lease_and_run(
                            &nested_key,
                            &source(),
                            oxc_span::SourceType::ts(),
                            move |_| {
                                nested_ran.fetch_add(1, Ordering::SeqCst);
                                1
                            },
                        )
                        .owned(waiter, CancellationToken::new());
                    matches!(
                        nested.poll(Waker::noop()),
                        Poll::Ready(Ok(super::LoweringStep::Value(Err(
                            super::LoweringFailure::Reentrant
                        ))))
                    )
                } else {
                    let mut nested = nested_service
                        .begin_run_leased(&nested_key, move |_| {
                            nested_ran.fetch_add(1, Ordering::SeqCst);
                            1
                        })
                        .owned(waiter, CancellationToken::new());
                    matches!(
                        nested.poll(Waker::noop()),
                        Poll::Ready(Ok(super::LoweringStep::Value(Err(
                            super::LoweringFailure::Reentrant
                        ))))
                    )
                }
            })
            .owned(registry.register_task(), CancellationToken::new());
        let (outcome, refused) = finish_owned(|waker| outer.poll(waker)).unwrap().unwrap();
        drop(outcome);
        service.release_completion().wait().unwrap();
        assert_eq!(
            refused,
            Some(true),
            "a worker must refuse its own shard before queue admission"
        );
        assert_eq!(ran.load(Ordering::SeqCst), 0);
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
    }
}

#[test]
fn inline_lowering_poll_transfers_work_instead_of_running_it() {
    let ran = Arc::new(AtomicUsize::new(0));
    let executed = ran.clone();
    let mut reply = super::inline::InlineWork::new(Box::new(move || {
        executed.fetch_add(1, Ordering::SeqCst);
        42_usize
    }));
    let (wake, waker, _) = reporting_waker();
    assert!(reply.poll(&waker).is_pending());
    assert_eq!(ran.load(Ordering::SeqCst), 0);
    let work = reply.take_work().expect("explicit independent work token");
    assert!(reply.poll(&waker).is_pending());
    std::thread::spawn(move || work.run()).join().unwrap();
    assert!(wakes(&wake) > 0);
    assert_eq!(reply.poll(&waker), Poll::Ready(42));
}

fn finish_owned<T>(
    mut poll: impl FnMut(
        &Waker,
    ) -> Poll<
        Result<super::LoweringStep<T>, verter_execution::dependency::DependencyStop>,
    >,
) -> Result<T, verter_execution::dependency::DependencyStop> {
    loop {
        match finish(|waker| poll(waker))? {
            super::LoweringStep::Work(work) => work.run(),
            super::LoweringStep::Value(value) => return Ok(value),
        }
    }
}

struct InlineProbe(super::inline::InlineWork<usize>);
impl verter_execution::dependency::DependencyPoll for InlineProbe {
    type Output = Result<usize, super::LoweringFailure>;
    fn poll_dependency(&mut self, waker: &Waker) -> Poll<Self::Output> {
        super::poll_lowering(|| self.0.poll(waker))
    }
}
fn owned_inline(
    operation: Box<dyn FnOnce() -> usize + Send>,
    registry: &verter_execution::tasks::TaskRegistry,
    stop: verter_execution::cancellation::CancellationToken,
) -> super::OwnedLoweringHandoff<InlineProbe> {
    let mut reply = super::inline::InlineWork::new(operation);
    let work = reply.take_work();
    let dependency = verter_execution::dependency::OwnedDependency::new(
        InlineProbe(reply),
        registry.register_task(),
        registry.register_task(),
        stop,
    );
    super::OwnedLoweringHandoff::new(dependency, work)
}
#[test]
fn owned_inline_work_preserves_edge_until_answer_and_contains_drop_and_panic() {
    use super::{LoweringFailure, LoweringStep};
    use verter_execution::{cancellation::CancellationToken, tasks::TaskRegistry};
    for panic in [false, true] {
        let registry = TaskRegistry::default();
        let mut demand = owned_inline(
            Box::new(move || {
                if panic {
                    panic!("inline failed");
                }
                5
            }),
            &registry,
            CancellationToken::new(),
        );
        let (wake, waker, _) = reporting_waker();
        let work = match demand.poll(&waker) {
            Poll::Ready(Ok(LoweringStep::Work(work))) => work,
            _ => panic!("must transfer"),
        };
        assert_eq!(registry.wait_graph_occupancy().edges, 1);
        assert!(demand.poll(&waker).is_pending());
        if panic {
            work.run();
        } else {
            drop(work);
        }
        assert!(wakes(&wake) > 0);
        let expected = if panic {
            LoweringFailure::Panicked
        } else {
            LoweringFailure::ProducerDropped
        };
        assert!(
            matches!(demand.poll(&waker), Poll::Ready(Ok(LoweringStep::Value(Err(error)))) if error == expected)
        );
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
    }
}
#[test]
fn owned_inline_stop_before_and_after_transfer_preserves_independent_work_and_withdraws() {
    use super::LoweringStep;
    use verter_execution::{
        cancellation::CancellationToken, dependency::DependencyStop, tasks::TaskRegistry,
    };
    for before in [false, true] {
        let registry = TaskRegistry::default();
        let stop = CancellationToken::new();
        let ran = Arc::new(AtomicUsize::new(0));
        let observed = ran.clone();
        let mut demand = owned_inline(
            Box::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                7
            }),
            &registry,
            stop.clone(),
        );
        let (wake, waker, _) = reporting_waker();
        let work = if before {
            None
        } else {
            match demand.poll(&waker) {
                Poll::Ready(Ok(LoweringStep::Work(work))) => Some(work),
                _ => panic!("must transfer"),
            }
        };
        stop.cancel();
        assert!(matches!(
            demand.poll(&waker),
            Poll::Ready(Err(DependencyStop::Cancelled))
        ));
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
        let wakes_after_stop = wakes(&wake);
        if let Some(work) = work {
            work.run();
        }
        assert_eq!(
            wakes(&wake),
            wakes_after_stop,
            "abandoned readiness registration must be gone"
        );
        assert_eq!(ran.load(Ordering::SeqCst), usize::from(!before));
    }
}
#[test]
fn inline_work_completion_before_repoll_and_abandoned_reply_cleanup_are_exact() {
    use super::LoweringStep;
    use verter_execution::{cancellation::CancellationToken, tasks::TaskRegistry};
    let registry = TaskRegistry::default();
    let mut demand = owned_inline(Box::new(|| 9), &registry, CancellationToken::new());
    let work = match demand.poll(Waker::noop()) {
        Poll::Ready(Ok(LoweringStep::Work(work))) => work,
        _ => panic!("must transfer"),
    };
    fn send<T: Send>(_: &T) {}
    send(&demand);
    send(&work);
    std::thread::spawn(move || work.run()).join().unwrap();
    assert!(matches!(
        demand.poll(Waker::noop()),
        Poll::Ready(Ok(LoweringStep::Value(Ok(9))))
    ));
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
    let mut abandoned = owned_inline(Box::new(|| 10), &registry, CancellationToken::new());
    let (wake, waker, _) = reporting_waker();
    let work = match abandoned.poll(&waker) {
        Poll::Ready(Ok(LoweringStep::Work(work))) => work,
        _ => panic!("must transfer"),
    };
    drop(abandoned);
    work.run();
    assert_eq!(wakes(&wake), 0);
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn owned_batch_preflight_refuses_all_shares_before_admitting_a_free_shard() {
    use verter_execution::{
        cancellation::CancellationToken,
        tasks::{ExecutionScope, TaskRegistry},
    };
    let service = Arc::new(DeclLoweringService::new_with(true, 2));
    let for_shard = |shard| {
        (0..100)
            .map(|i| key(&format!("/ws/shard-{i}.ts"), i as u8))
            .find(|key| super::shard_index(key, 2) == shard)
            .unwrap()
    };
    let free = for_shard(0);
    let held = for_shard(1);
    let lease = acquire(&service, &free);
    let nested_service = service.clone();
    let nested_held = held.clone();
    let ran = Arc::new(AtomicUsize::new(0));
    let observed = ran.clone();
    let registry = TaskRegistry::default();
    let mut outer = service
        .begin_acquire_lease_and_run(&held, &source(), oxc_span::SourceType::ts(), move |_| {
            let waiter = ExecutionScope::current_task().unwrap();
            let mut nested = nested_service
                .begin_run_leased_batch(vec![
                    LeasedJob {
                        key: free,
                        run: Box::new(move |_| {
                            observed.fetch_add(1, Ordering::SeqCst);
                            1
                        }),
                    },
                    LeasedJob {
                        key: nested_held,
                        run: Box::new(|_| 2),
                    },
                ])
                .owned(waiter, CancellationToken::new());
            matches!(
                nested.poll(Waker::noop()),
                Poll::Ready(Ok(super::LoweringStep::Value(Err(
                    super::LoweringFailure::Reentrant
                ))))
            )
        })
        .owned(registry.register_task(), CancellationToken::new());
    let (outcome, refused) = finish_owned(|waker| outer.poll(waker)).unwrap().unwrap();
    drop(outcome);
    drop(lease);
    service.release_completion().wait().unwrap();
    assert_eq!(refused, Some(true));
    assert_eq!(
        ran.load(Ordering::SeqCst),
        0,
        "preflight must not admit the earlier free shard"
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
}

#[test]
fn held_inline_reply_poll_is_bounded_and_completion_wakes() {
    let mut reply = super::inline::InlineWork::new(Box::new(|| 42_usize));
    let work = reply.take_work().unwrap();
    let (wake, waker, _) = reporting_waker();
    let (tx, rx) = channel();
    let poller = std::thread::spawn(move || {
        tx.send(reply.poll(&waker).is_pending()).unwrap();
        reply
    });
    let pending = rx.recv_timeout(DEADLINE);
    // Always release and join before asserting, even for a blocking mutant.
    work.run();
    let mut reply = poller.join().unwrap();
    assert_eq!(pending.ok(), Some(true));
    assert!(
        wakes(&wake) > 0,
        "completion must wake readiness registration"
    );
    assert_eq!(reply.poll(Waker::noop()), Poll::Ready(42));
}

#[test]
fn an_owned_empty_batch_is_immediate_without_work() {
    use verter_execution::{cancellation::CancellationToken, tasks::TaskRegistry};
    let service = service(1);
    let registry = TaskRegistry::default();
    let mut batch = service
        .begin_run_leased_batch(Vec::<LeasedJob<usize>>::new())
        .owned(registry.register_task(), CancellationToken::new());
    assert!(
        matches!(batch.poll(Waker::noop()), Poll::Ready(Ok(super::LoweringStep::Value(Ok(values)))) if values.is_empty())
    );
    assert_eq!(registry.wait_graph_occupancy().edges, 0);
}
