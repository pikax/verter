//! Returns the server's idle memory to the OS once per idle period.
//!
//! mimalloc purges only inside its own allocator calls, so a server that has
//! gone quiet keeps the pages its last burst freed committed.
//! [`spawn_idle_collect`] runs one task that waits, on the server's own
//! [`HandlerActivity`], for a full quiet window with no handler starting or
//! finishing, collects once, then sleeps until the next handler event. There
//! is no polling: both waits are woken by the handler guard.
//!
//! Never on the request path: the collect runs on a blocking-pool thread that
//! only this task awaits, so collections never overlap, and a request arriving
//! mid-collect starts immediately; the only coupling is that its allocations
//! may briefly contend on the arena locks the collect holds. The task is owned
//! by the server through [`IdleCollectTask`] and ends with it.

use std::sync::Arc;
use std::time::Duration;

use crate::server::HandlerActivity;

/// The running idle-collect task; aborted when dropped, so it cannot outlive
/// the server that owns it.
pub(crate) struct IdleCollectTask(tokio::task::JoinHandle<()>);

impl Drop for IdleCollectTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Spawn the idle-collect task. `collect` is
/// `verter_alloc::collect_idle_memory` in production; tests inject a probe.
pub(crate) fn spawn_idle_collect(
    activity: Arc<HandlerActivity>,
    delay: Duration,
    collect: fn(),
) -> IdleCollectTask {
    IdleCollectTask(tokio::spawn(async move {
        loop {
            let epoch = activity.wait_quiet_unbounded(delay).await;
            // Awaited here, not on any handler path; a panic in `collect`
            // only ends this one collection.
            let _ = tokio::task::spawn_blocking(collect).await;
            activity.wait_for_activity_after(epoch).await;
        }
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::HandlerGuard;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static COLLECTS: AtomicUsize = AtomicUsize::new(0);
    static EVENTS: tokio::sync::Notify = tokio::sync::Notify::const_new();

    fn probe() {
        COLLECTS.fetch_add(1, Ordering::SeqCst);
        EVENTS.notify_one();
    }

    /// The only test touching the statics, so counts are exact. The virtual
    /// clock drives every delay; the collect itself runs on a real
    /// blocking thread and is awaited through `EVENTS`.
    #[tokio::test(start_paused = true)]
    async fn collects_once_after_idle_and_not_while_requests_arrive() {
        let activity = Arc::new(HandlerActivity::default());
        let delay = Duration::from_millis(100);
        let task = spawn_idle_collect(Arc::clone(&activity), delay, probe);
        tokio::task::yield_now().await;

        // Requests arrive every 40 ms, so no window ever completes.
        for _ in 0..20 {
            drop(HandlerGuard::new(&activity, "test"));
            tokio::time::advance(Duration::from_millis(40)).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(COLLECTS.load(Ordering::SeqCst), 0, "busy server collected");

        // A handler still running holds the window open however long it takes.
        let held = HandlerGuard::new(&activity, "test");
        tokio::time::advance(delay * 5).await;
        tokio::task::yield_now().await;
        assert_eq!(
            COLLECTS.load(Ordering::SeqCst),
            0,
            "collected under a handler"
        );
        drop(held);

        // A handler that starts and finishes mid-window restarts it from its
        // own end: the collect is due one full delay later, not at the old
        // deadline plus another delay.
        tokio::task::yield_now().await;
        tokio::time::advance(delay * 6 / 10).await;
        drop(HandlerGuard::new(&activity, "test"));
        tokio::time::advance(delay - Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(
            COLLECTS.load(Ordering::SeqCst),
            0,
            "collected before a full quiet window"
        );

        // Idle: exactly one collect, however long the idle period lasts.
        tokio::time::advance(Duration::from_millis(1)).await;
        EVENTS.notified().await;
        assert_eq!(COLLECTS.load(Ordering::SeqCst), 1);
        tokio::time::advance(delay * 50).await;
        tokio::task::yield_now().await;
        assert_eq!(COLLECTS.load(Ordering::SeqCst), 1, "repeated while idle");

        // New activity, then idle again: one more.
        drop(HandlerGuard::new(&activity, "test"));
        tokio::time::advance(delay).await;
        EVENTS.notified().await;
        assert_eq!(COLLECTS.load(Ordering::SeqCst), 2);

        drop(task);
    }
}
