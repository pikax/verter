//! Job types: completion handles, request results, and scheduler errors.
//!
//! Every [`CompletionHandle`] resolves to exactly one [`CompletionState`]:
//! `Ready`, `Failed`, `Superseded`, or `Shutdown`.

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::{Condvar, Mutex};

use crate::dag::{DepKey, WorkNodeIdentity};
use crate::stage::TargetStage;

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
    /// A prerequisite that this work was gating on failed terminally,
    /// so this work cannot run.
    ///
    /// `dep_key` identifies the failed prerequisite — NOT the current
    /// (downstream) work. It is the [`DepKey`] that was recorded on
    /// the waiter's `failed_blocker_deps` set. For
    /// [`DepKey::FileStage`] the key carries the failed dep's
    /// canonical id, generation, and stage. Both fan-out paths
    /// (Source-stage failure and Analysis-stage failure) and the
    /// admission-time attach use the Analysis-stage DepKey for the
    /// failed prerequisite, because Analysis is the gating stage
    /// that downstream consumers (Analysis, Artifact, CacheNode
    /// waiters) wait on; a Source failure transitively poisons the
    /// Analysis stage at the same `(canonical, generation)`. The
    /// implementation does NOT pin the stage to "Source" or to
    /// "Analysis" — it preserves whatever stage the failed `DepKey`
    /// carried. For [`DepKey::Artifact`] the key carries the failed
    /// dep's canonical id, generation, profile hash, and content
    /// hash; for [`DepKey::CacheNode`] the pre-dispatch chokepoint
    /// debug-asserts that this variant does not appear in
    /// `failed_blocker_deps`.
    ///
    /// `cause` carries the producer's terminal [`SchedulerError`]
    /// verbatim — typically [`SchedulerError::FileNotFound`] for a
    /// missing source file or [`SchedulerError::StageFailed`] for an
    /// executor error. Consumers can disambiguate failure kinds
    /// (FileNotFound vs StageFailed) directly off this field instead
    /// of reconstructing the original failure from the dep key
    /// alone. Boxed because [`SchedulerError`] is recursive through
    /// this variant.
    ///
    /// Two producer paths populate the waiter's
    /// `failed_blocker_deps`, symmetric across both Source- and
    /// Analysis-stage producers:
    ///
    /// 1. **Fan-out at terminal-failure time** — the producer
    ///    terminalized AFTER the consumer admitted. A Source-stage
    ///    failure fans out via
    ///    [`crate::dag::SchedulerDag::fanout_source_failure_to_analysis_waiters`];
    ///    an Analysis-stage failure fans out via
    ///    [`crate::dag::SchedulerDag::fanout_analysis_failure_to_waiters`].
    ///    Both helpers route through the shared Analysis-DepKey
    ///    waiter sweep and record a [`crate::dag::FailedDepRecord`]
    ///    on every admitted waiter.
    /// 2. **Admission-time attach** — the producer terminalized
    ///    BEFORE the consumer admitted, observed via the persistent
    ///    `terminal_dep_failures` store. A late dispatcher trying
    ///    to admit a blocker whose dep already terminalized routes
    ///    through [`crate::dag::SchedulerDag::attach_failed_dep`],
    ///    which records the same [`crate::dag::FailedDepRecord`]
    ///    shape on the freshly-admitted waiter. This path is
    ///    used for both Source- and Analysis-stage admissions.
    ///
    /// The pre-dispatch chokepoint in `execute_stage_on_worker`
    /// extracts the first record and surfaces this variant — once,
    /// in one place, regardless of task kind — so a
    /// dependency-failure never silently resolves as `Ready`.
    DependencyFailed {
        dep_key: DepKey,
        cause: Box<SchedulerError>,
    },
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
            SchedulerError::DependencyFailed { dep_key, cause } => {
                write!(f, "dependency failed: {dep_key:?} (cause: {cause})")
            }
        }
    }
}

impl std::error::Error for SchedulerError {
    /// Expose the inner producer cause for
    /// [`SchedulerError::DependencyFailed`] so standard error-chain
    /// consumers (anyhow's `source()` walk, `std::error::Error`
    /// chain iterators, format-renderers that follow `Error::source`)
    /// can disambiguate the underlying failure mode without
    /// pattern-matching the envelope. Other variants carry no
    /// wrapped error and return `None`.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SchedulerError::DependencyFailed { cause, .. } => Some(cause.as_ref()),
            SchedulerError::StageFailed { .. }
            | SchedulerError::FileNotFound { .. }
            | SchedulerError::Shutdown => None,
        }
    }
}

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

/// Identity of the work a [`CompletionHandle`] is waiting on.
///
/// The variant distinguishes:
///
/// - [`CompletionTarget::Work`] — a specific DAG identity. Used for
///   same-path self-await detection in cooperative pumps: a worker
///   that is executing `work_node` and then calls `wait_or_drive`
///   on a handle pointing at the SAME `work_node` is waiting on
///   itself; the cooperative pump returns
///   [`CompletionState::Failed`] with stage `"wait_or_drive"`
///   rather than blocking forever.
/// - [`CompletionTarget::Request`] — a session-level request that
///   may admit one or more DAG identities at unknown future
///   generations (the canonical id is known but the generation is
///   not yet decided). Same-path detection on a request target
///   matches by `(canonical, target)` against the active-path
///   frame, covering the full prerequisite-stage chain:
///   - `target = Source` matches an active `FileStage{Source}`
///     frame on the same canonical.
///   - `target = Analysis` matches an active `FileStage{Source}`
///     OR `FileStage{Analysis}` frame on the same canonical
///     (Analysis admission gates on Source completion).
///   - `target = Artifact{ profile_hash }` matches an active
///     `FileStage{Source}` or `FileStage{Analysis}` frame on the
///     same canonical (Artifact admission gates on Analysis which
///     gates on Source), OR an active `Artifact` frame on the same
///     canonical AND the same `profile_hash`. Two Artifact frames
///     for the same canonical with DIFFERENT profile_hash values
///     are independent work units (they share only the upstream
///     Analysis gate, not the Artifact slot itself) and must NOT
///     collapse into a same-path match.
///
///   This is the fallback shape used between `submit_request`
///   (stamps `Request{..}`) and `handle_new_request` (overwrites
///   with the concrete `Work` identity). Once admission lands the
///   concrete `Work` identity, the more precise
///   [`CompletionTarget::Work`] matching takes over.
#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CompletionTarget {
    /// A specific DAG identity (file-stage, artifact, or cache-node).
    Work(WorkNodeIdentity),
    /// A session-level request whose DAG identity is not yet known
    /// at handle-construction time. Resolved by `wait_or_drive`
    /// using `(canonical, target)` against the active-path frame
    /// across the full prerequisite-stage chain.
    Request {
        /// Canonical file id the request was submitted against.
        canonical: Arc<str>,
        /// Target stage the request will resolve to.
        target: TargetStage,
    },
}

// ── CompletionHandle / CompletionSender ──

/// Internal shared state between handle and sender.
struct CompletionInner<T: Clone> {
    state: Mutex<Option<CompletionState<T>>>,
    condvar: Condvar,
    /// Optional target identity for same-path self-await detection
    /// by the cooperative pump.
    target: Mutex<Option<CompletionTarget>>,
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
        target: Mutex::new(None),
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

    /// Attach a [`CompletionTarget`] so cooperative-pump callers
    /// (`Scheduler::wait_or_drive`) can detect same-path self-await
    /// against the handle's pending work.
    ///
    /// Production stamping sequence:
    ///
    /// 1. `Scheduler::submit_request` stamps
    ///    `CompletionTarget::Request { canonical, target }` when
    ///    the handle is first constructed — the concrete DAG
    ///    identity is not yet known.
    /// 2. `Scheduler::handle_new_request` runs at admission and
    ///    overwrites the slot with the concrete
    ///    `CompletionTarget::Work(first_missing_identity)`.
    ///
    /// The slot is last-writer-wins — there is no internal
    /// coordination preventing a third caller from overwriting
    /// the admission stamp, but production code does not do so.
    pub(crate) fn set_target(&self, target: CompletionTarget) {
        *self.inner.target.lock() = Some(target);
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

    /// Block until the handle resolves OR `timeout` elapses. Returns
    /// `Some(state)` if the handle resolved within the budget,
    /// `None` if the timeout expired with the handle still pending.
    ///
    /// Used by the cooperative pump to bound how long a single
    /// iteration sleeps on the condvar before re-checking the inbox
    /// and ready queue.
    pub(crate) fn wait_timeout(&self, timeout: Duration) -> Option<CompletionState<T>> {
        let mut guard = self.inner.state.lock();
        if guard.is_some() {
            return Some(guard.clone().unwrap());
        }
        let result = self.inner.condvar.wait_for(&mut guard, timeout);
        if result.timed_out() && guard.is_none() {
            None
        } else {
            guard.clone()
        }
    }

    /// Non-blocking check. Returns `Some` if already resolved.
    pub fn try_get(&self) -> Option<CompletionState<T>> {
        self.inner.state.lock().clone()
    }

    /// Returns `true` if already resolved.
    pub fn is_resolved(&self) -> bool {
        self.inner.state.lock().is_some()
    }

    /// Snapshot the target identity attached by the sender, if any.
    /// Returns `None` if the sender never called `set_target`.
    pub(crate) fn target(&self) -> Option<CompletionTarget> {
        self.inner.target.lock().clone()
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

    #[cfg(not(target_arch = "wasm32"))]
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

    #[cfg(not(target_arch = "wasm32"))]
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

    /// `Error::source()` for `DependencyFailed` must expose the
    /// inner producer cause so standard error-chain consumers can
    /// reach `FileNotFound`/`StageFailed` underneath. Other variants
    /// return `None`.
    #[test]
    fn error_source_for_dependency_failed_exposes_inner_cause() {
        use crate::dag::{DepKey, FileStageKey};
        use std::error::Error;

        let inner = SchedulerError::FileNotFound {
            file_id: "/dep.ts".into(),
        };
        let outer = SchedulerError::DependencyFailed {
            dep_key: DepKey::FileStage {
                canonical: std::sync::Arc::from("/dep.ts"),
                generation: 1,
                stage: FileStageKey::Analysis,
            },
            cause: Box::new(inner.clone()),
        };
        let source = outer
            .source()
            .expect("DependencyFailed must expose inner cause via Error::source()");
        // The source is the FileNotFound variant — verify by
        // downcast to SchedulerError and matching.
        let down = source
            .downcast_ref::<SchedulerError>()
            .expect("source must downcast to SchedulerError");
        match down {
            SchedulerError::FileNotFound { file_id } => assert_eq!(file_id, "/dep.ts"),
            other => panic!("expected FileNotFound inner, got {other:?}"),
        }

        // Other variants carry no wrapped error: source() must
        // return None.
        assert!(
            SchedulerError::FileNotFound {
                file_id: "/x.vue".into()
            }
            .source()
            .is_none(),
            "FileNotFound has no inner cause",
        );
        assert!(
            SchedulerError::StageFailed {
                file_id: "/x.vue".into(),
                stage: "Source".into(),
                message: "err".into(),
            }
            .source()
            .is_none(),
            "StageFailed has no inner cause",
        );
        assert!(
            SchedulerError::Shutdown.source().is_none(),
            "Shutdown has no inner cause",
        );
    }

    #[test]
    fn wait_timeout_returns_none_when_handle_unsignaled() {
        let (handle, _sender) = completion_pair::<u32>();
        let observed = handle.wait_timeout(Duration::from_millis(20));
        assert!(observed.is_none(), "unsignaled handle must time out");
    }

    #[test]
    fn wait_timeout_returns_state_when_signaled_before_timeout() {
        let (handle, sender) = completion_pair::<u32>();
        sender.send(CompletionState::Ready(7));
        let observed = handle.wait_timeout(Duration::from_secs(5));
        match observed {
            Some(CompletionState::Ready(v)) => assert_eq!(v, 7),
            other => panic!("expected Ready(7), got {other:?}"),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn wait_timeout_wakes_when_signal_arrives_mid_wait() {
        let (handle, sender) = completion_pair::<&'static str>();
        let h2 = handle.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(15));
            sender.send(CompletionState::Ready("ok"));
        });
        let observed = h2.wait_timeout(Duration::from_secs(5));
        match observed {
            Some(CompletionState::Ready(v)) => assert_eq!(v, "ok"),
            other => panic!("expected Ready(ok) within budget, got {other:?}"),
        }
        t.join().unwrap();
    }

    #[test]
    fn completion_target_round_trip() {
        let (handle, sender) = completion_pair::<u32>();
        assert!(handle.target().is_none(), "fresh handle exposes no target",);
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            generation: 3,
            stage: crate::dag::FileStageKey::Analysis,
        };
        sender.set_target(CompletionTarget::Work(id.clone()));
        match handle.target() {
            Some(CompletionTarget::Work(observed)) => assert_eq!(observed, id),
            other => panic!("expected Work target, got {other:?}"),
        }
        // Last-writer-wins — the request-stage target overwrites
        // even when a Work target is already attached.
        let req_target = CompletionTarget::Request {
            canonical: Arc::from("/x.vue"),
            target: TargetStage::Analysis,
        };
        sender.set_target(req_target.clone());
        assert_eq!(handle.target(), Some(req_target));
    }
}
