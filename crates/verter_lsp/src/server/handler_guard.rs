// ── Handler tracking for freeze diagnosis ──────────────────────────────

/// Global counter of in-flight LSP request handlers. When this reaches the tokio
/// worker thread count, the runtime is saturated and timers/heartbeats can't fire.
pub(crate) static ACTIVE_HANDLERS: std::sync::atomic::AtomicU32 =
    std::sync::atomic::AtomicU32::new(0);
static HANDLERS_IDLE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static HANDLER_ACTIVITY_EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Admit one unit of background CPU work only while no LSP handler is active.
///
/// The check is intentionally repeated before every scanner item. A request may
/// race immediately after this returns, but then competes with at most one
/// carrier compile; it can never sit behind the remainder of a workspace pass.
pub(crate) async fn wait_for_handlers_idle() {
    wait_for_idle_counter(&ACTIVE_HANDLERS, &HANDLERS_IDLE).await;
}

/// Admit coarse background work only after the interactive lane has remained
/// idle for a complete quiet window. This is used before non-preemptible units
/// such as a filesystem discovery walk; per-file scanner work continues to use
/// [`wait_for_handlers_idle`].
/// Give coarse background work a quiet-window preference without allowing
/// continuous interactive traffic to starve correctness work forever.
///
/// The return value distinguishes a genuine quiet-window admission from the
/// fairness deadline, which is useful to callers that want to trace the latter.
pub(crate) async fn wait_for_handlers_quiet(
    quiet: std::time::Duration,
    max_defer: std::time::Duration,
) -> bool {
    wait_for_quiet_counter_bounded(
        &ACTIVE_HANDLERS,
        &HANDLERS_IDLE,
        &HANDLER_ACTIVITY_EPOCH,
        quiet,
        max_defer,
    )
    .await
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

/// RAII guard that tracks handler lifecycle. Logs entry (with thread ID and active
/// handler count) on creation, logs exit (with duration) on drop.
pub(crate) struct HandlerGuard {
    name: &'static str,
    start: std::time::Instant,
    thread_id: std::thread::ThreadId,
}

impl HandlerGuard {
    pub(crate) fn new(name: &'static str) -> Self {
        HANDLER_ACTIVITY_EPOCH.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let prev = ACTIVE_HANDLERS.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let thread_id = std::thread::current().id();
        tracing::info!(
            "HANDLER_ENTER {name} active={} thread={thread_id:?}",
            prev + 1
        );
        Self {
            name,
            start: std::time::Instant::now(),
            thread_id,
        }
    }
}

impl Drop for HandlerGuard {
    fn drop(&mut self) {
        let remaining = ACTIVE_HANDLERS.fetch_sub(1, std::sync::atomic::Ordering::AcqRel) - 1;
        if remaining == 0 {
            HANDLERS_IDLE.notify_waiters();
        }
        HANDLER_ACTIVITY_EPOCH.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        let elapsed = self.start.elapsed();
        tracing::info!(
            "HANDLER_EXIT {} active={remaining} elapsed={elapsed:?} thread={:?}",
            self.name,
            self.thread_id,
        );
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
