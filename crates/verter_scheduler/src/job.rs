//! Job types: completion handles, request results, and scheduler errors.
//!
//! Every [`CompletionHandle`] resolves to exactly one [`CompletionState`]:
//! `Ready`, `Failed`, `Superseded`, or `Shutdown`.

use std::fmt;
use std::sync::Arc;

use parking_lot::{Condvar, Mutex};

/// Errors originating from the scheduler.
#[derive(Clone, Debug)]
pub enum SchedulerError {
    /// Stage execution failed (e.g., parse error that prevents progress).
    StageFailed {
        file_id: String,
        stage: String,
        message: String,
    },
    /// File not found in scheduler or via source loader.
    FileNotFound { file_id: String },
    /// The scheduler was shut down before the request completed.
    Shutdown,
}

impl fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchedulerError::StageFailed {
                file_id,
                stage,
                message,
            } => write!(f, "stage {stage} failed for {file_id}: {message}"),
            SchedulerError::FileNotFound { file_id } => write!(f, "file not found: {file_id}"),
            SchedulerError::Shutdown => write!(f, "scheduler shut down"),
        }
    }
}

impl std::error::Error for SchedulerError {}

/// Terminal state of a request. Every handle resolves to exactly one of these.
///
/// `Clone` is required for `signal_all()` to fan out to multiple senders.
#[derive(Clone, Debug)]
pub enum CompletionState<T: Clone> {
    /// The requested stage completed successfully.
    Ready(T),
    /// Stage execution failed.
    Failed(SchedulerError),
    /// A newer generation invalidated this request.
    Superseded,
    /// The scheduler was dropped before the request completed.
    Shutdown,
}

impl<T: Clone> CompletionState<T> {
    /// Returns `true` if this is a `Ready` variant.
    pub fn is_ready(&self) -> bool {
        matches!(self, CompletionState::Ready(_))
    }

    /// Returns `true` if this is a terminal error state (not `Ready`).
    pub fn is_terminal_error(&self) -> bool {
        !self.is_ready()
    }
}

/// Canonical result type for all request completions.
///
/// Uses a single enum to avoid heterogeneous typing — the caller knows
/// which variant to expect based on their `TargetStage`.
///
/// All payloads are `Arc`-wrapped, so `Clone` is cheap.
#[derive(Clone, Debug)]
pub enum RequestResult {
    /// Source snapshot (parse result).
    Source(Arc<crate::node::SourceSnapshot>),
    /// Analysis snapshot (imports, bindings, macros).
    Analysis(Arc<crate::node::AnalysisSnapshot>),
    /// Artifact snapshot (compiled virtual files).
    Artifact(Arc<crate::node::ArtifactSnapshot>),
}

/// Type alias for the concrete completion handle returned to callers.
pub type RequestHandle = CompletionHandle<RequestResult>;

// ── CompletionHandle / CompletionSender ──

/// Internal shared state between handle and sender.
struct CompletionInner<T: Clone> {
    state: Mutex<Option<CompletionState<T>>>,
    condvar: Condvar,
}

/// A handle that the caller waits on. Resolves to exactly one [`CompletionState`].
pub struct CompletionHandle<T: Clone> {
    inner: Arc<CompletionInner<T>>,
}

/// The sender side — workers and the scheduler use this to signal completion.
///
/// First-writer-wins: subsequent `send()` calls are no-ops.
pub struct CompletionSender<T: Clone> {
    inner: Arc<CompletionInner<T>>,
}

/// Create a paired `(handle, sender)`.
///
/// The handle blocks on `wait()` until the sender signals a terminal state.
pub fn completion_pair<T: Clone>() -> (CompletionHandle<T>, CompletionSender<T>) {
    let inner = Arc::new(CompletionInner {
        state: Mutex::new(None),
        condvar: Condvar::new(),
    });
    (
        CompletionHandle {
            inner: inner.clone(),
        },
        CompletionSender { inner },
    )
}

impl<T: Clone> CompletionSender<T> {
    /// Signal completion. First-writer-wins: subsequent calls are no-ops.
    ///
    /// Returns `true` if this was the first write, `false` if already signaled.
    pub fn send(&self, state: CompletionState<T>) -> bool {
        let mut guard = self.inner.state.lock();
        if guard.is_some() {
            return false; // already signaled — enforce exactly-once
        }
        *guard = Some(state);
        self.inner.condvar.notify_all();
        true
    }
}

impl<T: Clone> CompletionHandle<T> {
    /// Block until the handle resolves to a terminal state.
    pub fn wait(&self) -> CompletionState<T> {
        let mut guard = self.inner.state.lock();
        while guard.is_none() {
            self.inner.condvar.wait(&mut guard);
        }
        guard.clone().unwrap()
    }

    /// Non-blocking check. Returns `Some` if already resolved.
    pub fn try_get(&self) -> Option<CompletionState<T>> {
        self.inner.state.lock().clone()
    }

    /// Returns `true` if already resolved.
    pub fn is_resolved(&self) -> bool {
        self.inner.state.lock().is_some()
    }
}

impl<T: Clone> Clone for CompletionHandle<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: Clone> Clone for CompletionSender<T> {
    fn clone(&self) -> Self {
        Self {
            inner: Arc::clone(&self.inner),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_handle_resolves_on_send() {
        let (handle, sender) = completion_pair::<u32>();

        // Not yet resolved
        assert!(!handle.is_resolved());
        assert!(handle.try_get().is_none());

        // Signal ready
        assert!(sender.send(CompletionState::Ready(42)));

        // Now resolved
        assert!(handle.is_resolved());
        let state = handle.try_get().unwrap();
        assert!(state.is_ready());
        match state {
            CompletionState::Ready(v) => assert_eq!(v, 42),
            _ => panic!("expected Ready"),
        }
    }

    #[test]
    fn completion_sender_first_writer_wins() {
        let (handle, sender) = completion_pair::<u32>();

        // First send succeeds
        assert!(sender.send(CompletionState::Ready(1)));
        // Second send is a no-op
        assert!(!sender.send(CompletionState::Ready(2)));

        // Value is from first send
        match handle.try_get().unwrap() {
            CompletionState::Ready(v) => assert_eq!(v, 1),
            _ => panic!("expected Ready(1)"),
        }
    }

    #[test]
    fn completion_handle_wait_blocks_until_signal() {
        let (handle, sender) = completion_pair::<String>();

        let handle2 = handle.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            sender.send(CompletionState::Ready("done".to_string()));
        });

        let state = handle2.wait();
        match state {
            CompletionState::Ready(v) => assert_eq!(v, "done"),
            _ => panic!("expected Ready"),
        }
        t.join().unwrap();
    }

    #[test]
    fn completion_handle_multiple_waiters() {
        let (handle, sender) = completion_pair::<u32>();

        let h1 = handle.clone();
        let h2 = handle.clone();

        let t1 = std::thread::spawn(move || h1.wait());
        let t2 = std::thread::spawn(move || h2.wait());

        std::thread::sleep(std::time::Duration::from_millis(20));
        sender.send(CompletionState::Ready(99));

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();
        match (r1, r2) {
            (CompletionState::Ready(a), CompletionState::Ready(b)) => {
                assert_eq!(a, 99);
                assert_eq!(b, 99);
            }
            _ => panic!("both should be Ready(99)"),
        }
    }

    #[test]
    fn completion_state_variants() {
        let ready: CompletionState<u32> = CompletionState::Ready(1);
        assert!(ready.is_ready());
        assert!(!ready.is_terminal_error());

        let failed: CompletionState<u32> = CompletionState::Failed(SchedulerError::Shutdown);
        assert!(!failed.is_ready());
        assert!(failed.is_terminal_error());

        let superseded: CompletionState<u32> = CompletionState::Superseded;
        assert!(!superseded.is_ready());
        assert!(superseded.is_terminal_error());

        let shutdown: CompletionState<u32> = CompletionState::Shutdown;
        assert!(!shutdown.is_ready());
        assert!(shutdown.is_terminal_error());
    }

    #[test]
    fn scheduler_error_display() {
        let e = SchedulerError::StageFailed {
            file_id: "foo.vue".into(),
            stage: "Analysis".into(),
            message: "parse error".into(),
        };
        assert_eq!(
            e.to_string(),
            "stage Analysis failed for foo.vue: parse error"
        );

        let e = SchedulerError::FileNotFound {
            file_id: "bar.vue".into(),
        };
        assert_eq!(e.to_string(), "file not found: bar.vue");

        let e = SchedulerError::Shutdown;
        assert_eq!(e.to_string(), "scheduler shut down");
    }
}
