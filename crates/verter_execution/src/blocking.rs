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

/// Wakes the thread that created it.
struct UnparkThread(std::thread::Thread);

impl Wake for UnparkThread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

thread_local! {
    /// This thread's driving waker, built once per thread.
    static PARK_WAKER: Waker = Waker::from(Arc::new(UnparkThread(std::thread::current())));
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
    PARK_WAKER.with(|waker| loop {
        match poll(waker) {
            Poll::Ready(outcome) => return outcome,
            Poll::Pending => match slice {
                Some(slice) => std::thread::park_timeout(slice),
                None => std::thread::park(),
            },
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

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
