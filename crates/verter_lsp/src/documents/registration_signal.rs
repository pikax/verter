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
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return present();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn returns_immediately_when_already_present() {
        let signal = RegistrationSignal::default();
        let started = std::time::Instant::now();
        let ok = signal
            .wait_until(std::time::Duration::from_secs(5), || true)
            .await;
        assert!(ok);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(2),
            "an already-present condition must not wait"
        );
    }

    #[tokio::test]
    async fn wakes_on_signal_well_before_the_budget() {
        let present = Arc::new(AtomicBool::new(false));
        let signal = Arc::new(RegistrationSignal::default());
        let budget = std::time::Duration::from_secs(5);

        let waiter = {
            let present = Arc::clone(&present);
            let signal = Arc::clone(&signal);
            async move {
                signal
                    .wait_until(budget, || present.load(Ordering::SeqCst))
                    .await
            }
        };
        let setter = {
            let present = Arc::clone(&present);
            let signal = Arc::clone(&signal);
            async move {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                present.store(true, Ordering::SeqCst);
                signal.signal();
            }
        };

        let started = std::time::Instant::now();
        let (ok, ()) = tokio::join!(waiter, setter);
        assert!(ok, "the wait must observe the signalled condition");
        assert!(
            started.elapsed() < budget / 2,
            "the wait must wake on the signal, not fall through to the budget"
        );
    }

    #[tokio::test]
    async fn gives_up_on_its_budget_when_never_signalled() {
        let signal = RegistrationSignal::default();
        let budget = std::time::Duration::from_millis(40);
        let started = std::time::Instant::now();
        let ok = signal.wait_until(budget, || false).await;
        let elapsed = started.elapsed();
        assert!(!ok, "an unmet condition returns false");
        assert!(elapsed >= budget, "the wait uses its whole budget");
        assert!(
            elapsed < budget * 4,
            "the wait gives up ON its budget, not well past it"
        );
    }
}
