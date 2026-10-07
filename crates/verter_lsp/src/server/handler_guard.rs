// ── Handler activity: interactive-lane admission for background work ────

/// The interactive-handler activity of ONE server: the in-flight handler
/// count, the idle wake, and the activity epoch the coarse quiet window
/// reads.
///
/// Owned by the server instance and shared, by `Arc`, with that server's
/// handlers, background scanner and heartbeat — never process-global, so two
/// servers in one process never hold back each other's background work. The
/// count is current occupancy (a handler adds itself for its own lifetime);
/// the epoch only distinguishes "no handler started or finished since" for
/// the quiet window.
#[derive(Debug, Default)]
pub struct HandlerActivity {
    active: std::sync::atomic::AtomicU32,
    idle: tokio::sync::Notify,
    epoch: std::sync::atomic::AtomicU64,
}

impl HandlerActivity {
    /// In-flight handlers right now — a diagnostic read with no production
    /// consumer in default builds (the heartbeat and handler trace lines
    /// that report it compile away), so the accessor exists only for the
    /// observation feature and the crate's own tests.
    #[cfg(any(test, feature = "test-support", feature = "semantic-observe"))]
    pub fn active(&self) -> u32 {
        self.active.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Admit one unit of background CPU work only while no handler of this
    /// server is active.
    ///
    /// The check is intentionally repeated before every scanner item. A
    /// request may race immediately after this returns, but then competes
    /// with at most one carrier compile; it can never sit behind the
    /// remainder of a workspace pass.
    pub async fn wait_idle(&self) {
        wait_for_idle_counter(&self.active, &self.idle).await;
    }

    /// Admit coarse background work only after this server's interactive
    /// lane has remained idle for a complete quiet window. This is used
    /// before non-preemptible units such as a filesystem discovery walk;
    /// per-file scanner work continues to use [`Self::wait_idle`].
    ///
    /// Gives coarse background work a quiet-window preference without
    /// allowing continuous interactive traffic to starve correctness work
    /// forever. The return value distinguishes a genuine quiet-window
    /// admission from the fairness deadline, which is useful to callers that
    /// want to trace the latter.
    pub async fn wait_quiet(
        &self,
        quiet: std::time::Duration,
        max_defer: std::time::Duration,
    ) -> bool {
        wait_for_quiet_counter_bounded(&self.active, &self.idle, &self.epoch, quiet, max_defer)
            .await
    }
}

async fn wait_for_idle_counter(active: &std::sync::atomic::AtomicU32, idle: &tokio::sync::Notify) {
    wait_for_idle_counter_on_gap(active, idle, |_| {}).await;
}

/// Subscribe to `notify_waiters` BEFORE the idle re-check. Creating
/// `Notify::notified()` does not register a waiter; `enable()` does. The
/// producer uses `notify_waiters` (no stored permit), so a notify landing
/// between an un-enabled future and `.await` is lost.
async fn wait_for_idle_counter_on_gap(
    active: &std::sync::atomic::AtomicU32,
    idle: &tokio::sync::Notify,
    mut on_gap: impl FnMut(&std::sync::atomic::AtomicU32),
) {
    loop {
        let notified = idle.notified();
        tokio::pin!(notified);
        notified.as_mut().enable();
        if active.load(std::sync::atomic::Ordering::Acquire) == 0 {
            return;
        }
        on_gap(active);
        notified.await;
    }
}

async fn wait_for_quiet_counter(
    active: &std::sync::atomic::AtomicU32,
    idle: &tokio::sync::Notify,
    activity_epoch: &std::sync::atomic::AtomicU64,
    quiet: std::time::Duration,
) {
    loop {
        wait_for_idle_counter(active, idle).await;
        let epoch = activity_epoch.load(std::sync::atomic::Ordering::Acquire);
        tokio::time::sleep(quiet).await;
        if active.load(std::sync::atomic::Ordering::Acquire) == 0
            && activity_epoch.load(std::sync::atomic::Ordering::Acquire) == epoch
        {
            return;
        }
    }
}

async fn wait_for_quiet_counter_bounded(
    active: &std::sync::atomic::AtomicU32,
    idle: &tokio::sync::Notify,
    activity_epoch: &std::sync::atomic::AtomicU64,
    quiet: std::time::Duration,
    max_defer: std::time::Duration,
) -> bool {
    tokio::time::timeout(
        max_defer,
        wait_for_quiet_counter(active, idle, activity_epoch, quiet),
    )
    .await
    .is_ok()
}

/// RAII guard that tracks one handler's lifetime on its server's
/// [`HandlerActivity`]: REQUIRED admission bookkeeping (count, wake, epoch)
/// always, and — compiled in only under the default-off `semantic-observe`
/// feature — the freeze-diagnosis enter/exit trace lines with the name,
/// entry timestamp and thread id only those lines consume.
pub struct HandlerGuard<'a> {
    activity: &'a HandlerActivity,
    #[cfg(feature = "semantic-observe")]
    name: &'static str,
    #[cfg(feature = "semantic-observe")]
    start: std::time::Instant,
    #[cfg(feature = "semantic-observe")]
    thread_id: std::thread::ThreadId,
}

impl<'a> HandlerGuard<'a> {
    pub fn new(activity: &'a HandlerActivity, name: &'static str) -> Self {
        activity
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        #[cfg(feature = "semantic-observe")]
        let prev = activity
            .active
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        #[cfg(not(feature = "semantic-observe"))]
        activity
            .active
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        #[cfg(feature = "semantic-observe")]
        {
            let thread_id = std::thread::current().id();
            tracing::info!(
                "HANDLER_ENTER {name} active={} thread={thread_id:?}",
                prev + 1
            );
            Self {
                activity,
                name,
                start: std::time::Instant::now(),
                thread_id,
            }
        }
        #[cfg(not(feature = "semantic-observe"))]
        {
            let _ = name;
            Self { activity }
        }
    }
}

impl Drop for HandlerGuard<'_> {
    fn drop(&mut self) {
        let activity = self.activity;
        let remaining = activity
            .active
            .fetch_sub(1, std::sync::atomic::Ordering::AcqRel)
            - 1;
        if remaining == 0 {
            activity.idle.notify_waiters();
        }
        activity
            .epoch
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        #[cfg(feature = "semantic-observe")]
        {
            let elapsed = self.start.elapsed();
            tracing::info!(
                "HANDLER_EXIT {} active={remaining} elapsed={elapsed:?} thread={:?}",
                self.name,
                self.thread_id,
            );
        }
    }
}

pub(crate) fn block_in_place_if_available<R>(f: impl FnOnce() -> R) -> R {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(f)
        }
        _ => f(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Admission is an event, so it is proven by polling, not by racing two
    /// wall-clock timeouts: `Pending` while a handler is active, `Ready` on
    /// the notify, and the virtual clock never moves.
    #[tokio::test(start_paused = true)]
    async fn background_admission_waits_for_the_last_handler() {
        use std::task::Poll;

        let active = std::sync::atomic::AtomicU32::new(1);
        let idle = tokio::sync::Notify::new();
        let start = tokio::time::Instant::now();
        let mut waiter = Box::pin(wait_for_idle_counter(&active, &idle));

        assert!(
            matches!(futures_util::poll!(&mut waiter), Poll::Pending),
            "background CPU work must not be admitted while a handler is active"
        );

        active.store(0, std::sync::atomic::Ordering::Release);
        idle.notify_waiters();
        waiter.await;
        assert_eq!(
            tokio::time::Instant::now(),
            start,
            "dropping the last handler must wake background work through the \
             notify, without any timer participating"
        );
    }

    /// The quiet window is semantic time, so it is driven on the paused
    /// clock and read as exact virtual instants. The previous shape raced a
    /// 15ms real sleep against a 30ms window and a 25ms timeout — three
    /// margins that collapse into each other on a loaded machine.
    #[tokio::test(start_paused = true)]
    async fn coarse_background_admission_restarts_its_quiet_window_on_activity() {
        use std::sync::atomic::Ordering;
        use std::task::Poll;

        let active = std::sync::atomic::AtomicU32::new(0);
        let idle = tokio::sync::Notify::new();
        let epoch = std::sync::atomic::AtomicU64::new(0);
        let quiet = std::time::Duration::from_millis(30);
        let start = tokio::time::Instant::now();
        let mut waiter = Box::pin(wait_for_quiet_counter(&active, &idle, &epoch, quiet));

        // Enter the wait: idle, so it arms the quiet-window timer.
        assert!(matches!(futures_util::poll!(&mut waiter), Poll::Pending));

        // Activity halfway through the window.
        tokio::time::advance(quiet / 2).await;
        epoch.fetch_add(1, Ordering::AcqRel);
        active.store(1, Ordering::Release);

        // Crossing the ORIGINAL boundary must not admit — the window restarts.
        tokio::time::advance(quiet / 2).await;
        assert!(
            matches!(futures_util::poll!(&mut waiter), Poll::Pending),
            "activity inside the quiet window must defer coarse background work"
        );
        assert_eq!(tokio::time::Instant::now(), start + quiet);

        // Idle again: only a COMPLETE window from the new stamp admits.
        active.store(0, Ordering::Release);
        epoch.fetch_add(1, Ordering::AcqRel);
        idle.notify_waiters();
        assert!(matches!(futures_util::poll!(&mut waiter), Poll::Pending));
        tokio::time::advance(quiet).await;
        waiter.await;
        assert_eq!(
            tokio::time::Instant::now(),
            start + quiet * 2,
            "admission must land exactly one full quiet window after the last activity"
        );
    }

    /// A request stream can remain continuously active for the lifetime of an
    /// editor session. Coarse correctness work must still receive a bounded
    /// admission slot instead of waiting forever for an idle transition.
    #[tokio::test(start_paused = true)]
    async fn coarse_background_admission_has_a_fairness_deadline() {
        use std::task::Poll;

        let active = std::sync::atomic::AtomicU32::new(1);
        let idle = tokio::sync::Notify::new();
        let epoch = std::sync::atomic::AtomicU64::new(1);
        let quiet = std::time::Duration::from_millis(30);
        let max_defer = std::time::Duration::from_secs(1);
        let start = tokio::time::Instant::now();
        let mut waiter = Box::pin(wait_for_quiet_counter_bounded(
            &active, &idle, &epoch, quiet, max_defer,
        ));

        assert!(matches!(futures_util::poll!(&mut waiter), Poll::Pending));
        tokio::time::advance(max_defer).await;
        assert!(
            !waiter.await,
            "continuous handler traffic must take the bounded fairness path"
        );
        assert_eq!(
            tokio::time::Instant::now(),
            start + max_defer,
            "background admission must not be deferred past its fairness deadline"
        );
    }

    /// Force the producer's OWN wake primitive — `notify_waiters`, the one
    /// `HandlerGuard::drop` uses — into the former check-to-await gap.
    ///
    /// This is NOT an `enable()` discriminator, and says so: on Tokio 1.52
    /// `notified()` snapshots the `notify_waiters` counter at CONSTRUCTION,
    /// so removing `enable()` leaves the test green (planted and observed).
    /// What it DOES discriminate is the ORDERING — `notify_waiters` stores
    /// no permit, so constructing the future AFTER the idle re-check loses
    /// the wake and hangs. Using `notify_one` here would not even prove
    /// that, because a stored permit survives the gap either way. The
    /// production pin+enable stays as defense in depth matching
    /// `RegistrationSignal`. The ordering half was planted and observed
    /// red at all three gap sites.
    #[tokio::test(start_paused = true)]
    async fn idle_wait_does_not_lose_notify_waiters_in_the_check_to_await_gap() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let active = std::sync::atomic::AtomicU32::new(1);
        let idle = tokio::sync::Notify::new();
        let fired = AtomicBool::new(false);
        let start = tokio::time::Instant::now();
        tokio::time::timeout(
            std::time::Duration::from_secs(5),
            wait_for_idle_counter_on_gap(&active, &idle, |active| {
                if !fired.swap(true, Ordering::SeqCst) {
                    active.store(0, Ordering::Release);
                    idle.notify_waiters();
                }
            }),
        )
        .await
        .expect("notify_waiters in the check-to-await gap must resume the waiter");
        assert!(
            fired.load(Ordering::SeqCst),
            "the gap hook must have fired — otherwise the wait took the idle fast path"
        );
        assert_eq!(
            tokio::time::Instant::now(),
            start,
            "a captured notify_waiters wake must not consume virtual time"
        );
    }
}
