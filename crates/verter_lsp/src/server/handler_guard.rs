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
pub(crate) async fn wait_for_handlers_quiet(quiet: std::time::Duration) {
    wait_for_quiet_counter(
        &ACTIVE_HANDLERS,
        &HANDLERS_IDLE,
        &HANDLER_ACTIVITY_EPOCH,
        quiet,
    )
    .await;
}

async fn wait_for_idle_counter(active: &std::sync::atomic::AtomicU32, idle: &tokio::sync::Notify) {
    loop {
        // Register the waiter before checking the counter so the transition to
        // zero cannot be missed between the check and `.await`.
        let notified = idle.notified();
        if active.load(std::sync::atomic::Ordering::Acquire) == 0 {
            return;
        }
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

    #[tokio::test]
    async fn background_admission_waits_for_the_last_handler() {
        let active = std::sync::atomic::AtomicU32::new(1);
        let idle = tokio::sync::Notify::new();
        let mut waiter = Box::pin(wait_for_idle_counter(&active, &idle));

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), &mut waiter)
                .await
                .is_err(),
            "background CPU work must not be admitted while a handler is active"
        );

        active.store(0, std::sync::atomic::Ordering::Release);
        idle.notify_waiters();
        tokio::time::timeout(std::time::Duration::from_millis(200), waiter)
            .await
            .expect("dropping the last handler must wake background work");
    }

    #[tokio::test]
    async fn coarse_background_admission_restarts_its_quiet_window_on_activity() {
        let active = std::sync::atomic::AtomicU32::new(0);
        let idle = tokio::sync::Notify::new();
        let epoch = std::sync::atomic::AtomicU64::new(0);
        let quiet = std::time::Duration::from_millis(30);
        let mut waiter = Box::pin(wait_for_quiet_counter(&active, &idle, &epoch, quiet));

        tokio::time::sleep(std::time::Duration::from_millis(15)).await;
        epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        active.store(1, std::sync::atomic::Ordering::Release);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(25), &mut waiter)
                .await
                .is_err(),
            "activity inside the quiet window must defer coarse background work"
        );

        active.store(0, std::sync::atomic::Ordering::Release);
        epoch.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        idle.notify_waiters();
        tokio::time::timeout(std::time::Duration::from_millis(100), waiter)
            .await
            .expect("a complete idle window should admit background work");
    }
}
