//! The external-boundary compatibility driver for pollable computations,
//! and the probe that tells which synchronous waits a computation reached.
//!
//! A pollable computation answers `Ready` or returns `Pending` after
//! registering the poll's waker with whatever it waits on. A synchronous
//! public entry that must hand its caller a finished value drives such a
//! computation here: it polls with a waker that unparks the driving thread
//! and parks the thread whenever the poll is pending.
//!
//! A computation that needs another one should return `Pending` instead of
//! driving it here. That is not yet true of the whole call graph: the
//! synchronous host roots still reach this driver, and several other
//! synchronous waits, from deep inside their own computation. Every such
//! wait announces itself through [`enter_wait`] with its [`WaitFamily`], so
//! a [`WaitProbe`] installed around a computation can list the waits it
//! reached ([`WaitProbe::record`]) or refuse them outright
//! ([`WaitProbe::forbid`]). A poll that must never block runs under a
//! forbidding probe: a wait it reaches fails at once, before it parks the
//! thread or touches the dependency.
//!
//! The probe is test evidence, compiled only under `test` or the
//! `test-support` feature. A production build keeps every announcement as
//! an empty inline call.

#[cfg(any(test, feature = "test-support"))]
use std::cell::RefCell;
#[cfg(any(test, feature = "test-support"))]
use std::collections::BTreeSet;
#[cfg(any(test, feature = "test-support"))]
use std::marker::PhantomData;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use std::time::Duration;

/// A dependency family whose synchronous wait can hold the calling thread.
///
/// Each variant names one owner's wait, so a probe's report says which
/// dependencies a computation still reaches synchronously.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WaitFamily {
    /// A synchronous query joining a semantic memo flight another task
    /// produces.
    SemanticFlight,
    /// The semantic runtime driving a task's frames to completion.
    SemanticDrive,
    /// An artifact-cache or query-identity candidate singleflight joiner.
    CacheFlight,
    /// The resolver request singleflight follower.
    ResolverFlight,
    /// The store-view build park.
    StoreViewBuild,
    /// A carrier publication lane or its stable shared parse.
    CarrierPublication,
    /// A scheduler completion awaited, or driven inline, to its terminal
    /// state.
    SchedulerCompletion,
    /// Scheduler inbox admission waiting for capacity.
    SchedulerAdmission,
    /// A scoped cache-node flight driven to its value.
    ScopedCacheNode,
    /// A workspace import-resolution flight.
    WorkspaceResolution,
    /// A declaration-lowering owner-shard submission or reply.
    DeclLowering,
    /// Work installed on a CPU pool, blocking its caller until it returns.
    CpuPool,
    /// A synchronous public entry driving a host root computation to its
    /// answer. This is the external boundary itself: a root's own poll
    /// never enters it.
    Root,
}

/// The probe installed on this thread, if any.
#[cfg(any(test, feature = "test-support"))]
struct ProbeState {
    /// Reaching a wait fails instead of being recorded.
    forbid: bool,
    /// The waits reached, in order.
    reached: Vec<WaitFamily>,
}

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    static PROBE: RefCell<Option<ProbeState>> = const { RefCell::new(None) };
}

/// Announce that this thread is entering a synchronous wait of `family`.
///
/// Every synchronous wait on a dependency calls this on entry, before it
/// checks readiness, takes a lock or parks: entering the wait at all is
/// what a non-blocking poll must not do, whether or not this particular
/// wait would have parked. Under a recording [`WaitProbe`] the wait is
/// listed; under a forbidding one this panics, naming the family, before
/// the wait has any effect. Without the probe compiled in it does nothing.
///
/// # Panics
///
/// When a `WaitProbe::forbid` probe is installed on this thread.
#[cfg(any(test, feature = "test-support"))]
pub fn enter_wait(family: WaitFamily) {
    let forbidden = PROBE.with(|probe| match probe.borrow_mut().as_mut() {
        Some(probe) if probe.forbid => true,
        Some(probe) => {
            probe.reached.push(family);
            false
        }
        None => false,
    });
    assert!(
        !forbidden,
        "a computation that must not block reached a synchronous {family:?} wait",
    );
}

/// Announce that this thread is entering a synchronous wait of `family`.
/// Without the probe compiled in, there is nothing to tell.
#[cfg(not(any(test, feature = "test-support")))]
#[inline(always)]
pub fn enter_wait(_family: WaitFamily) {}

/// Lists, or refuses, the synchronous waits this thread enters while the
/// probe is installed.
///
/// A probe covers the installing thread only: work another thread runs on
/// its behalf is that thread's own. Probes nest. An inner probe inside a
/// forbidding one forbids too, and the waits an inner recording probe saw
/// are added to an enclosing recording probe when it is removed. The probe
/// is removed, and the enclosing one restored, when it drops.
#[cfg(any(test, feature = "test-support"))]
#[must_use = "the probe is removed when it drops"]
pub struct WaitProbe {
    previous: Option<ProbeState>,
    /// The probe belongs to the thread that installed it.
    _thread: PhantomData<*const ()>,
}

#[cfg(any(test, feature = "test-support"))]
impl WaitProbe {
    /// Install a probe that records every wait this thread enters.
    pub fn record() -> Self {
        Self::install(false)
    }

    /// Install a probe under which entering any wait panics.
    pub fn forbid() -> Self {
        Self::install(true)
    }

    fn install(forbid: bool) -> Self {
        let previous = PROBE.with(|probe| probe.borrow_mut().take());
        let forbid = forbid || previous.as_ref().is_some_and(|previous| previous.forbid);
        PROBE.with(|probe| {
            *probe.borrow_mut() = Some(ProbeState {
                forbid,
                reached: Vec::new(),
            });
        });
        Self {
            previous,
            _thread: PhantomData,
        }
    }

    /// The waits entered since this probe was installed, in order.
    #[must_use]
    pub fn reached(&self) -> Vec<WaitFamily> {
        PROBE.with(|probe| {
            probe
                .borrow()
                .as_ref()
                .map(|probe| probe.reached.clone())
                .unwrap_or_default()
        })
    }

    /// The distinct families entered since this probe was installed.
    #[must_use]
    pub fn families(&self) -> BTreeSet<WaitFamily> {
        self.reached().into_iter().collect()
    }
}

#[cfg(any(test, feature = "test-support"))]
impl Drop for WaitProbe {
    fn drop(&mut self) {
        let mut previous = self.previous.take();
        let ours = PROBE.with(|probe| probe.borrow_mut().take());
        if let (Some(previous), Some(ours)) = (previous.as_mut(), ours) {
            if !previous.forbid {
                previous.reached.extend(ours.reached);
            }
        }
        PROBE.with(|probe| *probe.borrow_mut() = previous);
    }
}

/// One drive's notification, independent of nested drives on the same thread.
struct UnparkThread {
    thread: std::thread::Thread,
    notified: AtomicBool,
}

impl UnparkThread {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            thread: std::thread::current(),
            notified: AtomicBool::new(false),
        })
    }

    fn park(&self, slice: Option<Duration>) {
        // Another drive can consume the thread's park token, but cannot
        // consume this drive's notification. A wake after this check leaves
        // a park token, so the check-to-park race is safe as well.
        if !self.notified.swap(false, Ordering::AcqRel) {
            match slice {
                Some(slice) => std::thread::park_timeout(slice),
                None => std::thread::park(),
            }
        }
    }
}

impl Wake for UnparkThread {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.notified.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

/// Poll `poll` to completion on this thread, parking whenever it is pending
/// until the producer it waits on wakes the thread.
///
/// `family` names the dependency the drive waits on; the drive enters that
/// wait ([`enter_wait`]) before its first poll. A spurious unpark only costs
/// one more poll. `slice`, when given, bounds each park so a computation
/// whose stop is observed only at its next poll (a cancelled request) is
/// re-polled at least that often even when no producer wakes it.
///
/// # Panics
///
/// Under a forbidding `WaitProbe`, before polling.
pub fn block_on<T>(
    family: WaitFamily,
    slice: Option<Duration>,
    mut poll: impl FnMut(&Waker) -> Poll<T>,
) -> T {
    enter_wait(family);
    let notification = UnparkThread::new();
    let waker = Waker::from(Arc::clone(&notification));
    loop {
        match poll(&waker) {
            Poll::Ready(outcome) => return outcome,
            Poll::Pending => notification.park(slice),
        }
    }
}

thread_local! {
    /// The family the pollable boundary that last returned `Pending` on
    /// this thread waits on, for the synchronous driver of the computation
    /// that reached it.
    static PENDING_ON: std::cell::Cell<Option<WaitFamily>> = const { std::cell::Cell::new(None) };
}

/// Record that the dependency boundary returning `Pending` from this poll
/// waits on `family`. A synchronous driver over the computation that
/// reached it ([`drive_pending`]) announces that family when it parks.
pub fn note_pending(family: WaitFamily) {
    PENDING_ON.with(|slot| slot.set(Some(family)));
}

/// Drive `computation` to completion on this thread: the synchronous form of
/// a pollable computation, for a caller that is not itself pollable yet.
///
/// Each poll that returns `Pending` enters the wait of the family its
/// pending boundary noted ([`note_pending`]), or `fallback` when none did,
/// and parks until the computation's waker fires. A computation that
/// answers without waiting enters no wait. The caller pins the computation
/// where it builds it (`std::pin::pin!`), so its state is never copied
/// into the driver's frame.
///
/// # Panics
///
/// Under a forbidding `WaitProbe`, once a poll is pending.
pub fn drive_pending<F: std::future::Future + ?Sized>(
    fallback: WaitFamily,
    mut computation: std::pin::Pin<&mut F>,
) -> F::Output {
    let notification = UnparkThread::new();
    let waker = Waker::from(Arc::clone(&notification));
    let mut cx = std::task::Context::from_waker(&waker);
    loop {
        PENDING_ON.with(|slot| slot.set(None));
        if let Poll::Ready(outcome) = computation.as_mut().poll(&mut cx) {
            return outcome;
        }
        enter_wait(PENDING_ON.with(std::cell::Cell::take).unwrap_or(fallback));
        notification.park(None);
    }
}

/// The computation `build` returns, placed on the heap from a frame that
/// ends here. For a computation whose state is large: the frame that builds
/// it does not stay on the stack while it runs, so neither does the state.
#[inline(never)]
pub fn on_heap<F>(build: impl FnOnce() -> F) -> std::pin::Pin<Box<F>> {
    Box::pin(build())
}

/// The answer of a computation that never suspends: one written in the
/// pollable form whose every step here runs synchronously, so its first
/// poll answers.
///
/// # Panics
///
/// If the computation's first poll is pending: it reached a dependency
/// that suspends, which a caller of this function cannot wait on.
pub fn complete_now<F: std::future::Future + ?Sized>(
    computation: std::pin::Pin<&mut F>,
) -> F::Output {
    match computation.poll(&mut std::task::Context::from_waker(Waker::noop())) {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => panic!("a computation completed in place suspended"),
    }
}

/// Poll `poll` to completion on this thread, entering the `family` wait only
/// if its first poll is pending.
///
/// For a synchronous entry over a computation that usually answers at once
/// and waits only behind another caller's work (a store-view build another
/// thread holds): the answer it gives without waiting is no wait, and the
/// wait it does enter is announced before the thread parks. Under a
/// forbidding `WaitProbe` the pending first poll's registration is withdrawn
/// as the refusal unwinds through whatever owns it.
///
/// # Panics
///
/// Under a forbidding `WaitProbe`, once the first poll is pending.
pub fn block_on_when_pending<T>(family: WaitFamily, poll: impl FnMut(&Waker) -> Poll<T>) -> T {
    block_on_when_pending_with_interval(family, None, poll)
}

/// Drive an eager compatibility entry, announcing a wait only after `Pending`.
/// The optional interval preserves a legacy cancellation or worker-pump cadence
/// while parked; an immediately ready answer never enters that wait.
pub fn block_on_when_pending_with_interval<T>(
    family: WaitFamily,
    interval: Option<Duration>,
    mut poll: impl FnMut(&Waker) -> Poll<T>,
) -> T {
    let notification = UnparkThread::new();
    let waker = Waker::from(Arc::clone(&notification));
    if let Poll::Ready(outcome) = poll(&waker) {
        return outcome;
    }
    enter_wait(family);
    loop {
        notification.park(interval);
        if let Poll::Ready(outcome) = poll(&waker) {
            return outcome;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each compatibility driver must retain its own notification even if
    /// a nested pending drive consumes the thread's unpark token.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn nested_drives_preserve_the_outer_notification() {
        #[derive(Clone, Copy, Debug)]
        enum Driver {
            Blocking,
            WhenPending,
            Future,
        }
        fn drive(driver: Driver, mut poll: impl FnMut(&Waker) -> Poll<usize>) -> usize {
            match driver {
                Driver::Blocking => block_on(WaitFamily::SemanticDrive, None, poll),
                Driver::WhenPending => block_on_when_pending(WaitFamily::SemanticDrive, poll),
                Driver::Future => drive_pending(
                    WaitFamily::SemanticDrive,
                    std::pin::pin!(std::future::poll_fn(|cx| poll(cx.waker()))),
                ),
            }
        }

        for outer in [Driver::Blocking, Driver::WhenPending, Driver::Future] {
            for inner in [Driver::Blocking, Driver::WhenPending, Driver::Future] {
                let (wake, waking) = std::sync::mpsc::channel();
                let (done, completed) = std::sync::mpsc::channel();
                let worker = std::thread::spawn(move || {
                    let mut polls = 0;
                    let answer = drive(outer, |waker| {
                        polls += 1;
                        if polls > 1 {
                            return Poll::Ready(polls);
                        }
                        wake.send(waker.clone()).unwrap();
                        waker.wake_by_ref();
                        let mut nested_polls = 0;
                        assert_eq!(
                            drive(inner, |nested_waker| {
                                nested_polls += 1;
                                if nested_polls == 1 {
                                    nested_waker.wake_by_ref();
                                    Poll::Pending
                                } else {
                                    Poll::Ready(nested_polls)
                                }
                            }),
                            2
                        );
                        Poll::Pending
                    });
                    done.send(answer).unwrap();
                });
                let waker = waking.recv_timeout(Duration::from_secs(1));
                let answer = completed.recv_timeout(Duration::from_secs(1));
                // Rescue a lost notification before joining, including on failure.
                if let Ok(waker) = waker {
                    waker.wake();
                }
                worker.join().unwrap();
                assert_eq!(
                    answer.unwrap_or_else(|error| panic!("{outer:?}/{inner:?}: {error}")),
                    2
                );
            }
        }
    }

    #[test]
    fn a_ready_poll_answers_without_parking() {
        let mut polls = 0;
        let answer = block_on(WaitFamily::SchedulerCompletion, None, |_| {
            polls += 1;
            Poll::Ready(7)
        });
        assert_eq!((answer, polls), (7, 1));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_pending_poll_is_resumed_by_its_producer_wake() {
        let slot: Arc<parking_lot::Mutex<(Option<u32>, Option<Waker>)>> =
            Arc::new(parking_lot::Mutex::new((None, None)));
        let producer = {
            let slot = Arc::clone(&slot);
            std::thread::spawn(move || loop {
                let waker = {
                    let mut state = slot.lock();
                    match state.1.take() {
                        Some(waker) => {
                            state.0 = Some(11);
                            waker
                        }
                        None => {
                            drop(state);
                            std::thread::yield_now();
                            continue;
                        }
                    }
                };
                waker.wake();
                break;
            })
        };
        let answer = block_on(WaitFamily::SchedulerCompletion, None, |waker| {
            let mut state = slot.lock();
            match state.0 {
                Some(answer) => Poll::Ready(answer),
                None => {
                    state.1 = Some(waker.clone());
                    Poll::Pending
                }
            }
        });
        producer.join().expect("the producer completes");
        assert_eq!(answer, 11);
    }

    /// A recording probe lists every wait entered, a ready one included,
    /// and a nested recording probe's waits reach the enclosing probe.
    #[test]
    fn a_recording_probe_lists_the_waits_entered_in_order() {
        let outer = WaitProbe::record();
        block_on(WaitFamily::DeclLowering, None, |_| Poll::Ready(()));
        {
            let inner = WaitProbe::record();
            enter_wait(WaitFamily::SemanticFlight);
            assert_eq!(inner.reached(), [WaitFamily::SemanticFlight]);
        }
        enter_wait(WaitFamily::CpuPool);
        assert_eq!(
            outer.reached(),
            [
                WaitFamily::DeclLowering,
                WaitFamily::SemanticFlight,
                WaitFamily::CpuPool,
            ],
        );
        drop(outer);
        // Removed: entering a wait is no longer observed or refused.
        enter_wait(WaitFamily::CpuPool);
        assert!(WaitProbe::record().reached().is_empty());
    }

    /// A forbidding probe refuses a drive before its first poll, so the
    /// refused wait never touches the dependency, and a recording probe
    /// nested inside it cannot lift the refusal.
    #[test]
    fn a_forbidding_probe_refuses_a_wait_before_it_polls() {
        let _forbid = WaitProbe::forbid();
        let mut polled = false;
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            block_on(WaitFamily::ResolverFlight, None, |_| {
                polled = true;
                Poll::Ready(())
            });
        }));
        assert!(refused.is_err(), "the forbidden drive panics");
        assert!(!polled, "the refused drive never polled its dependency");
        let _nested = WaitProbe::record();
        assert!(
            std::panic::catch_unwind(|| enter_wait(WaitFamily::CpuPool)).is_err(),
            "a nested recording probe still forbids",
        );
    }

    /// A drive that answers on its first poll enters no wait; one whose
    /// first poll is pending enters its family's wait before parking, and a
    /// forbidding probe refuses it there.
    #[test]
    fn a_drive_enters_its_wait_only_once_its_first_poll_is_pending() {
        let probe = WaitProbe::record();
        let answer = block_on_when_pending(WaitFamily::StoreViewBuild, |_| Poll::Ready(3));
        assert_eq!(answer, 3);
        assert!(probe.reached().is_empty(), "a ready drive waits on nothing");

        let mut polls = 0;
        let answer = block_on_when_pending(WaitFamily::StoreViewBuild, |waker| {
            polls += 1;
            if polls == 1 {
                waker.wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(5)
            }
        });
        assert_eq!((answer, polls), (5, 2));
        assert_eq!(probe.reached(), [WaitFamily::StoreViewBuild]);
        drop(probe);

        let _forbid = WaitProbe::forbid();
        let mut polls = 0;
        let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            block_on_when_pending(WaitFamily::StoreViewBuild, |_| {
                polls += 1;
                Poll::<()>::Pending
            })
        }));
        assert!(refused.is_err(), "a pending drive is refused");
        assert_eq!(polls, 1, "the refusal comes before the thread parks");
    }

    /// A driven computation enters the wait its pending boundary noted, the
    /// fallback when none did, and nothing when it answers at once.
    #[test]
    fn a_driven_computation_enters_the_wait_its_pending_boundary_noted() {
        struct PendingOnce {
            noted: Option<WaitFamily>,
            polls: usize,
        }
        impl std::future::Future for PendingOnce {
            type Output = usize;
            fn poll(
                mut self: std::pin::Pin<&mut Self>,
                cx: &mut std::task::Context<'_>,
            ) -> Poll<usize> {
                self.polls += 1;
                if self.polls > 1 {
                    return Poll::Ready(self.polls);
                }
                if let Some(family) = self.noted {
                    note_pending(family);
                }
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
        let probe = WaitProbe::record();
        assert_eq!(
            drive_pending(WaitFamily::CpuPool, std::pin::pin!(std::future::ready(1))),
            1
        );
        assert!(
            probe.reached().is_empty(),
            "a ready computation waits on nothing"
        );
        let noted = PendingOnce {
            noted: Some(WaitFamily::DeclLowering),
            polls: 0,
        };
        assert_eq!(drive_pending(WaitFamily::CpuPool, std::pin::pin!(noted)), 2);
        let unnoted = PendingOnce {
            noted: None,
            polls: 0,
        };
        assert_eq!(
            drive_pending(WaitFamily::CpuPool, std::pin::pin!(unnoted)),
            2
        );
        assert_eq!(
            probe.reached(),
            [WaitFamily::DeclLowering, WaitFamily::CpuPool]
        );
    }

    /// A computation completed in place answers its first poll, and one
    /// that suspends instead fails rather than being left unpolled.
    #[test]
    fn a_computation_completed_in_place_must_answer_its_first_poll() {
        assert_eq!(complete_now(std::pin::pin!(async { 7 })), 7);
        let suspended = std::panic::catch_unwind(|| {
            complete_now(std::pin::pin!(std::future::pending::<()>()));
        });
        assert!(suspended.is_err(), "a suspending computation is refused");
    }

    /// A probe observes its own thread only.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn a_probe_covers_only_the_installing_thread() {
        let probe = WaitProbe::forbid();
        std::thread::spawn(|| enter_wait(WaitFamily::CpuPool))
            .join()
            .expect("another thread's wait is its own");
        assert!(probe.reached().is_empty());
    }
}
