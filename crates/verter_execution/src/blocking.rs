//! The external-boundary compatibility driver for pollable computations.
//!
//! A pollable computation answers `Ready` or returns `Pending` after
//! registering the poll's waker with whatever it waits on. A synchronous
//! public entry that must hand its caller a finished value drives such a
//! computation here: it polls with a waker that unparks the driving thread
//! and parks the thread whenever the poll is pending.
//!
//! This is the only place a pollable computation is allowed to block, and
//! it is reached only from a synchronous boundary outside the computation:
//! no poll-facing capability hands a computation a way to call it. A
//! computation that needs another one returns `Pending` instead.

use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use std::time::Duration;

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
/// A spurious unpark only costs one more poll. `slice`, when given, bounds
/// each park so a computation whose stop is observed only at its next poll
/// (a cancelled request) is re-polled at least that often even when no
/// producer wakes it.
pub fn block_on<T>(slice: Option<Duration>, mut poll: impl FnMut(&Waker) -> Poll<T>) -> T {
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
        let answer = block_on(None, |_| {
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
        let answer = block_on(None, |waker| {
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
}
