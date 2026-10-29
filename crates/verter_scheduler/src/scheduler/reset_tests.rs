use super::*;
use crate::source_loader::MemorySourceLoader;
use std::task::{Poll, Wake, Waker};

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
        source: Some(Arc::from("export const n = 1;")),
        file_language: None,
        request_context: None,
    }
}

struct StopDriver(Arc<Scheduler>);

impl Drop for StopDriver {
    fn drop(&mut self) {
        self.0.shutdown.store(true, Ordering::Release);
        let _ = self.0.driver_teardown.0.try_send(());
        let handle = self.0.driver_handle.lock().take();
        if let Some(handle) = handle {
            let joined = handle.join();
            if !std::thread::panicking() {
                joined.unwrap();
            }
        }
    }
}

#[test]
fn native_driver_work_waits_for_identity_and_lifecycle_publication() {
    struct Completed(std::sync::mpsc::Sender<()>);
    impl Wake for Completed {
        fn wake(self: Arc<Self>) {
            let _ = self.0.send(());
        }
    }
    let scheduler = scheduler();
    let _cleanup = StopDriver(Arc::clone(&scheduler));
    let (done, completed) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(Completed(done)));
    let mut admission = scheduler.begin_submit_request(request("/startup.ts"));
    let Poll::Ready(mut completion) = admission.poll(Waker::noop()) else {
        panic!("empty inbox admits the request");
    };
    assert!(completion.poll(&waker).is_pending());
    let start = scheduler.spawn_native_driver();
    let before_start = completed.recv_timeout(std::time::Duration::from_millis(100));
    assert!(scheduler.driver_thread_id.lock().is_some());
    assert!(scheduler.driver_handle.lock().is_some());
    // Open before asserting, so a broken early publication still leaves the
    // owned driver joinable and a failing test cannot strand its startup wait.
    start.send(()).unwrap();
    assert!(
        matches!(
            before_start,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "no callback may run before lifecycle publication completes"
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match completion.poll(&waker) {
            Poll::Ready(state) => {
                assert!(matches!(state, CompletionState::Ready(_)));
                break;
            }
            // Producer publication is an advisory wake too. Keep the same
            // subscription armed and wait for terminal readiness.
            Poll::Pending => completed
                .recv_timeout(deadline.saturating_duration_since(std::time::Instant::now()))
                .expect("the admitted request completes"),
        }
    }
}

#[test]
fn reset_and_restart_in_a_completion_wake_reuse_one_driver() {
    struct ResetRestart {
        scheduler: std::sync::Weak<Scheduler>,
        done: std::sync::mpsc::Sender<(std::thread::ThreadId, std::thread::ThreadId)>,
    }
    impl Wake for ResetRestart {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().unwrap();
            let original = std::thread::current().id();
            scheduler.reset();
            scheduler.restart_driver();
            self.done
                .send((
                    original,
                    scheduler
                        .driver_handle
                        .lock()
                        .as_ref()
                        .unwrap()
                        .thread()
                        .id(),
                ))
                .unwrap();
        }
    }
    let scheduler = scheduler();
    let _cleanup = StopDriver(Arc::clone(&scheduler));
    let (done, completed) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(ResetRestart {
        scheduler: Arc::downgrade(&scheduler),
        done,
    }));
    let mut admission = scheduler.begin_submit_request(request("/reset-restart.ts"));
    let Poll::Ready(mut completion) = admission.poll(Waker::noop()) else {
        panic!("empty inbox admits the request");
    };
    assert!(completion.poll(&waker).is_pending());
    scheduler.restart_driver();
    let (original, restarted) = completed
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        original, restarted,
        "a callback must not spawn a second driver"
    );
    let fresh = scheduler.submit_request(request("/after-local-restart.ts"));
    assert!(matches!(
        fresh.wait_timeout(std::time::Duration::from_secs(1)),
        Some(CompletionState::Ready(_))
    ));
    assert!(!scheduler.has_node("/reset-restart.ts"));
}

#[test]
fn a_shutdown_completion_can_reopen_its_retained_driver() {
    struct Restart {
        scheduler: std::sync::Weak<Scheduler>,
        done: std::sync::mpsc::Sender<()>,
    }
    impl Wake for Restart {
        fn wake(self: Arc<Self>) {
            self.scheduler.upgrade().unwrap().restart_driver();
            self.done.send(()).unwrap();
        }
    }
    let scheduler = scheduler();
    let _cleanup = StopDriver(Arc::clone(&scheduler));
    let (done, completed) = std::sync::mpsc::channel();
    let mut admission = scheduler.begin_submit_request(request("/shutdown-drain.ts"));
    let Poll::Ready(mut completion) = admission.poll(Waker::noop()) else {
        panic!("empty inbox admits the request");
    };
    let waker = Waker::from(Arc::new(Restart {
        scheduler: Arc::downgrade(&scheduler),
        done,
    }));
    assert!(completion.poll(&waker).is_pending());
    // Model a prepared submission arriving after reset's final drain. Its
    // shutdown completion is published by the outgoing driver's final pump.
    scheduler.shutdown.store(true, Ordering::Release);
    scheduler.spawn_native_driver().send(()).unwrap();
    completed
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();
    assert!(matches!(
        completion.poll(Waker::noop()),
        Poll::Ready(CompletionState::Shutdown)
    ));
    let fresh = scheduler.submit_request(request("/after-shutdown-drain.ts"));
    assert!(matches!(
        fresh.wait_timeout(std::time::Duration::from_secs(1)),
        Some(CompletionState::Ready(_))
    ));
}

#[test]
fn a_completion_wake_can_reset_its_own_driver_without_leaving_it_running() {
    struct ResetWake {
        scheduler: std::sync::Weak<Scheduler>,
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
        reset: std::sync::mpsc::Sender<()>,
    }
    impl Wake for ResetWake {
        fn wake(self: Arc<Self>) {
            self.entered.send(()).unwrap();
            self.release.lock().recv().unwrap();
            self.scheduler.upgrade().unwrap().reset();
            self.reset.send(()).unwrap();
        }
    }
    struct DriverGuard {
        scheduler: Arc<Scheduler>,
        release: Option<std::sync::mpsc::Sender<()>>,
        handle: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for DriverGuard {
        fn drop(&mut self) {
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            // Also stop the unfixed driver, and join it before any assertion
            // reports a failure. Teardown has its own channel, not the inbox
            // a cooperative consumer may already have drained.
            self.scheduler.shutdown.store(true, Ordering::Release);
            let _ = self.scheduler.driver_teardown.0.try_send(());
            if let Some(handle) = self
                .handle
                .take()
                .or_else(|| self.scheduler.driver_handle.lock().take())
            {
                let joined = handle.join();
                if !std::thread::panicking() {
                    joined.unwrap();
                }
            }
        }
    }

    let scheduler = scheduler();
    let (entered, entering) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let (reset, resetting) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(ResetWake {
        scheduler: Arc::downgrade(&scheduler),
        entered,
        release: Mutex::new(released),
        reset,
    }));
    let mut admission = scheduler.begin_submit_request(request("/self-reset.ts"));
    let Poll::Ready(mut completion) = admission.poll(Waker::noop()) else {
        panic!("empty inbox admits the request");
    };
    assert!(completion.poll(&waker).is_pending());
    let mut guard = DriverGuard {
        scheduler: Arc::clone(&scheduler),
        release: Some(release),
        handle: None,
    };
    scheduler.restart_driver();
    entering
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the production driver must publish completion");
    // Keep the real thread joinable even if a broken reset discards its
    // scheduler-owned handle. Its registered driver identity is unchanged.
    guard.handle = scheduler.driver_handle.lock().take();
    guard.release.take().unwrap().send(()).unwrap();
    resetting
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("the wake must return from its own reset");
    // A joiner reports the driver's exit; the timeout is a hang guard only.
    // A driver still running at the deadline is stopped by the guard's drop,
    // which then lets the joiner finish.
    let driver = guard.handle.take().unwrap();
    let (exited_tx, exited) = std::sync::mpsc::channel();
    let joiner = std::thread::spawn(move || {
        let joined = driver.join();
        let _ = exited_tx.send(());
        joined
    });
    let stopped = exited
        .recv_timeout(std::time::Duration::from_secs(1))
        .is_ok();
    let shutdown = scheduler.shutdown.load(Ordering::Acquire);
    let cleared = scheduler.node_ids().is_empty();
    drop(guard);
    joiner
        .join()
        .expect("the joiner thread must not panic")
        .expect("the driver thread must not panic");
    assert!(stopped, "a self-reset driver must exit before any restart");
    assert!(shutdown, "a self-reset alone must leave admission stopped");
    assert!(cleared, "self-reset still retires the published file");
}

#[test]
fn external_restart_joins_a_self_reset_driver_before_reopening_work() {
    struct ResetWake {
        scheduler: std::sync::Weak<Scheduler>,
        reset: std::sync::mpsc::Sender<bool>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }
    impl Wake for ResetWake {
        fn wake(self: Arc<Self>) {
            let scheduler = self.scheduler.upgrade().unwrap();
            scheduler.reset();
            self.reset
                .send(scheduler.driver_handle.lock().is_some())
                .unwrap();
            self.release.lock().recv().unwrap();
        }
    }
    struct Cleanup {
        scheduler: Arc<Scheduler>,
        release: Option<std::sync::mpsc::Sender<()>>,
        restarter: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if let Some(release) = self.release.take() {
                let _ = release.send(());
            }
            let restart_panic = self
                .restarter
                .take()
                .and_then(|restarter| restarter.join().err());
            self.scheduler.shutdown.store(true, Ordering::Release);
            let _ = self.scheduler.driver_teardown.0.try_send(());
            let handle = self.scheduler.driver_handle.lock().take();
            let driver_panic = handle.and_then(|handle| handle.join().err());
            if !std::thread::panicking() {
                if let Some(panic) = restart_panic.or(driver_panic) {
                    std::panic::resume_unwind(panic);
                }
            }
        }
    }

    let scheduler = scheduler();
    let (reset, resetting) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    let waker = Waker::from(Arc::new(ResetWake {
        scheduler: Arc::downgrade(&scheduler),
        reset,
        release: Mutex::new(released),
    }));
    let mut admission = scheduler.begin_submit_request(request("/startup-reset.ts"));
    let Poll::Ready(mut completion) = admission.poll(Waker::noop()) else {
        panic!("empty inbox admits the request");
    };
    assert!(completion.poll(&waker).is_pending());
    let mut cleanup = Cleanup {
        scheduler: Arc::clone(&scheduler),
        release: Some(release),
        restarter: None,
    };
    // No gate delays the wake until after restart returns: startup must
    // publish the real identity and handle before invoking this callback.
    scheduler.restart_driver();
    let retained = resetting.recv_timeout(std::time::Duration::from_secs(1));
    let (boundary, observed) = std::sync::mpsc::channel();
    *scheduler.lifecycle_boundary.lock() = Some(boundary);
    let (done, completed) = std::sync::mpsc::channel();
    let restarting = Arc::clone(&scheduler);
    cleanup.restarter = Some(std::thread::spawn(move || {
        restarting.restart_driver();
        done.send(()).unwrap();
    }));
    let attempted = observed.recv_timeout(std::time::Duration::from_secs(1));
    let before_release = completed.recv_timeout(std::time::Duration::from_millis(100));
    cleanup.release.take().unwrap().send(()).unwrap();
    cleanup.restarter.take().unwrap().join().unwrap();
    assert_eq!(attempted.unwrap(), "restart-join");
    assert!(retained.unwrap(), "reset must retain the join handle");
    assert!(
        matches!(
            before_release,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "restart must not overlap the outgoing callback"
    );
    completed
        .recv_timeout(std::time::Duration::from_secs(1))
        .expect("restart completes once the outgoing driver exits");
    let fresh = scheduler.submit_request(request("/after-self-reset.ts"));
    assert!(matches!(
        fresh.wait_timeout(std::time::Duration::from_secs(1)),
        Some(CompletionState::Ready(_))
    ));
    assert!(!scheduler.shutdown.load(Ordering::Acquire));
}

#[test]
fn a_driver_reset_waits_for_a_healthy_lifecycle_owner_instead_of_losing_the_reset() {
    let scheduler = scheduler();
    let _request = scheduler.submit_request(request("/must-retire.ts"));
    scheduler.drive_all();
    let serial = scheduler.reset_serial.lock();
    let (boundary, observed) = std::sync::mpsc::channel();
    *scheduler.lifecycle_boundary.lock() = Some(boundary);
    let (done, completed) = std::sync::mpsc::channel();
    let resetting = Arc::clone(&scheduler);
    let driver = std::thread::spawn(move || {
        *resetting.driver_thread_id.lock() = Some(std::thread::current().id());
        resetting.reset();
        *resetting.driver_thread_id.lock() = None;
        done.send(()).unwrap();
    });
    let attempted = observed.recv_timeout(std::time::Duration::from_secs(1));
    let before_release = completed.recv_timeout(std::time::Duration::from_millis(100));
    drop(serial);
    driver.join().unwrap();
    assert_eq!(attempted.unwrap(), "reset-contended");
    assert!(
        matches!(
            before_release,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ),
        "a healthy lifecycle owner has not supplied reset semantics"
    );
    assert!(!scheduler.has_node("/must-retire.ts"));
}
