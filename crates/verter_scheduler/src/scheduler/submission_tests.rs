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
    // A real external reset posts shutdown before joining its driver.
    scheduler.shutdown.store(true, Ordering::Release);
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
    let (boundary, observed) = std::sync::mpsc::channel();
    *scheduler.lifecycle_boundary.lock() = Some(boundary);
    let (done, completed) = std::sync::mpsc::channel();
    let resetting = Arc::clone(&scheduler);
    let driver = std::thread::spawn(move || {
        let _role = caller_kind::CallerKindGuard::install(caller_kind::CallerKind::Driver);
        resetting.reset();
        done.send(()).unwrap();
    });
    let attempted = observed.recv_timeout(std::time::Duration::from_secs(1));
    let premature = completed.recv_timeout(std::time::Duration::from_millis(100));
    drop(serial);
    driver.join().unwrap();
    assert_eq!(attempted.unwrap(), "reset-contended");
    assert!(
        matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "a foreign driver must honor this reset barrier"
    );
}
