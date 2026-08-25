//! Event-driven wait for a document to become registered.
//!
//! tower-lsp dispatches `did_open` and a request for the same document
//! concurrently, so a completion can arrive before the open has registered. The
//! request must wait for the registration; waiting by re-checking on a timer
//! costs it the remainder of whichever poll step it landed in, so a document
//! that registers 1ms after a check is still held for the rest of the interval.
//! Waiting on the registration event costs only the time the registration
//! actually took.

/// A one-line notify plus the event-driven wait over it. Owned by the document
/// registry; signalled on every registration.
#[derive(Default)]
pub(crate) struct RegistrationSignal {
    notify: tokio::sync::Notify,
    /// TEST-ONLY proof that the fast path was (or was not) taken: incremented
    /// each time [`Self::wait_until`] actually arms the notify/timeout race,
    /// so a test can assert "the already-present case never entered the wait
    /// machinery" structurally instead of on a wall-clock elapsed bound (a
    /// tight ceiling flips under machine load; this counter cannot).
    #[cfg(test)]
    timeout_arm_count: std::sync::atomic::AtomicUsize,
    /// TEST-ONLY: wakes when [`Self::timeout_arm_count`] increments, so a
    /// test can wait for the race to be armed without a yield-count loop.
    #[cfg(test)]
    armed: tokio::sync::Notify,
    /// TEST-ONLY proof of which branch resolved a wait: incremented each time
    /// the race's timeout arm actually fires (budget exhausted) rather than
    /// the notify arm. A test proving "resumed via the signal, not a budget
    /// fall-through" reads this instead of comparing elapsed time against a
    /// ceiling comfortably below the budget — a margin that narrows to
    /// nothing under machine load.
    #[cfg(test)]
    budget_exhausted_count: std::sync::atomic::AtomicUsize,
}

impl RegistrationSignal {
    /// Wake every waiter. Called after a document registers.
    pub(crate) fn signal(&self) {
        self.notify.notify_waiters();
    }

    /// Resolve once `present()` holds, or `budget` elapses. Returns the final
    /// value of `present()`. Interest is registered BEFORE each presence
    /// re-check, so a signal landing between the two cannot be missed.
    pub(crate) async fn wait_until(
        &self,
        budget: std::time::Duration,
        mut present: impl FnMut() -> bool,
    ) -> bool {
        if present() {
            return true;
        }
        let deadline = tokio::time::Instant::now() + budget;
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if present() {
                return true;
            }
            #[cfg(test)]
            {
                self.timeout_arm_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.armed.notify_waiters();
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                #[cfg(test)]
                self.budget_exhausted_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                return present();
            }
        }
    }

    /// TEST-ONLY: how many times [`Self::wait_until`] armed the
    /// notify/timeout race on this signal. Zero means every call so far took
    /// the synchronous already-present fast path.
    #[cfg(test)]
    pub(crate) fn timeout_arm_count(&self) -> usize {
        self.timeout_arm_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// TEST-ONLY: how many times [`Self::wait_until`] gave up on its budget
    /// (the timeout arm fired) on this signal. Zero means every wait so far
    /// resolved via the notify signal, never a fall-through.
    #[cfg(test)]
    pub(crate) fn budget_exhausted_count(&self) -> usize {
        self.budget_exhausted_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// TEST-ONLY: resolve once the notify/timeout race has been armed.
    #[cfg(test)]
    pub(crate) async fn wait_until_timeout_armed(&self) {
        loop {
            let notified = self.armed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            if self.timeout_arm_count() >= 1 {
                return;
            }
            notified.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    // All three tests below run on tokio's PAUSED virtual clock
    // (`start_paused = true`): the clock advances only when explicitly
    // advanced or when every runnable task is blocked on a timer with
    // nothing left to do. That turns "resolved via the signal, immediately"
    // from a wall-clock claim (approximate, flips under machine load) into
    // an exact, deterministic claim: `Instant::now()` provably did not move
    // at all, because nothing that ran needed a timer tick. A poll-based
    // fallback masquerading as event-driven (the regression class this
    // guards against) would still need *some* timer to re-check on, and
    // would move the virtual clock to prove it.

    #[tokio::test(start_paused = true)]
    async fn returns_immediately_when_already_present() {
        let signal = RegistrationSignal::default();
        let start = tokio::time::Instant::now();
        let ok = signal
            .wait_until(std::time::Duration::from_secs(5), || true)
            .await;
        assert!(ok);
        // The fast path returns before the loop (and its notify/timeout
        // race) is ever entered, so the virtual clock cannot have moved —
        // not "probably fast", provably zero elapsed ticks.
        assert_eq!(
            tokio::time::Instant::now(),
            start,
            "an already-present condition must resolve without the virtual \
             clock advancing at all"
        );
        assert_eq!(
            signal.timeout_arm_count(),
            0,
            "an already-present condition must not enter the wait machinery"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn wakes_on_signal_without_advancing_the_clock() {
        let present = Arc::new(AtomicBool::new(false));
        let signal = Arc::new(RegistrationSignal::default());
        let budget = std::time::Duration::from_secs(5);

        // The setter never sleeps — it only yields once, a pure scheduling
        // handoff that costs zero virtual time — before storing and
        // signalling. If the waiter resolves at all here, it can only be via
        // the notify wake, since nothing anywhere in this test ever arms a
        // timer that could fire.
        let setter = tokio::spawn({
            let present = Arc::clone(&present);
            let signal = Arc::clone(&signal);
            async move {
                tokio::task::yield_now().await;
                present.store(true, Ordering::SeqCst);
                signal.signal();
            }
        });

        let start = tokio::time::Instant::now();
        let ok = signal
            .wait_until(budget, || present.load(Ordering::SeqCst))
            .await;
        assert!(ok, "the wait must observe the signalled condition");
        // The strongest possible form of "resolved via the signal, not the
        // budget": the virtual clock did not advance by even one tick. A
        // budget-polling fallback that happens to notice `present` quickly
        // would still need a timer to re-check on and would move the clock;
        // this cannot pass by accident.
        assert_eq!(
            tokio::time::Instant::now(),
            start,
            "a notify-driven wake never needs a timer tick — any clock \
             movement here means the wait fell back to polling/timeout"
        );
        assert_eq!(
            signal.budget_exhausted_count(),
            0,
            "the wait must wake on the signal, not fall through to the budget"
        );
        setter.await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn gives_up_on_its_budget_when_never_signalled() {
        let signal = RegistrationSignal::default();
        let budget = std::time::Duration::from_millis(40);
        let start = tokio::time::Instant::now();
        let ok = signal.wait_until(budget, || false).await;
        assert!(!ok, "an unmet condition returns false");
        // With the clock paused and nothing else scheduled, tokio can only
        // resolve this by fast-forwarding to the exact deadline of the one
        // pending timer (`timeout_at(deadline, ...)`) — an EXACT equality,
        // not a floor, and not subject to any real scheduling slop because
        // no real time passes at all.
        assert_eq!(
            tokio::time::Instant::now(),
            start + budget,
            "an unmet condition must consume exactly its budget, no more and \
             no less"
        );
        assert_eq!(
            signal.budget_exhausted_count(),
            1,
            "an unmet condition must resolve through the budget's own timeout arm"
        );
    }
}
