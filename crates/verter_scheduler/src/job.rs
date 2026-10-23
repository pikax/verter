//! Job types: completion handles, request results, and scheduler errors.
//!
//! Every [`CompletionHandle`] resolves to exactly one [`CompletionState`]:
//! `Ready`, `Failed`, `Superseded`, or `Shutdown`.

use std::fmt;
use std::sync::Arc;
// Named only by `wait_timeout`, which is native-only.
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

use parking_lot::{Condvar, Mutex};
use verter_execution::wake::{WakeRegistration, WakerSet};

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
    /// The file's language row has no registered carrier implementation
    /// behind it (a known-but-unsupported framework language). Typed so
    /// consumers distinguish it structurally — never by parsing a
    /// message string. Produced when a stage executor returns
    /// [`crate::execution::executor::StageErrorKind::UnsupportedLanguage`].
    UnsupportedLanguage {
        file_id: String,
        adapter_id: verter_language::FrameworkAdapterId,
    },
    /// The file's source nests deeper than a stack this host can provide:
    /// its parse, or the walk-stack lease of its program's walks, was
    /// refused, and nothing was published for it. Typed operational
    /// incompleteness, produced when a stage executor returns
    /// [`crate::execution::executor::StageErrorKind::StackUnavailable`].
    StackUnavailable { file_id: String, needed: usize },
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
            SchedulerError::UnsupportedLanguage {
                file_id,
                adapter_id,
            } => write!(
                f,
                "unsupported framework language for {file_id}: no carrier \
                 implementation is registered for adapter '{adapter_id}'"
            ),
            SchedulerError::StackUnavailable { file_id, needed } => write!(
                f,
                "the source of {file_id} nests deeper than a stack this host can provide \
                 ({needed} bytes needed)"
            ),
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
            | SchedulerError::UnsupportedLanguage { .. }
            | SchedulerError::StackUnavailable { .. }
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

/// A handle's terminal state and the wakers of the subscriptions polling it.
struct CompletionSlot<T: Clone> {
    state: Option<CompletionState<T>>,
    /// Each pending subscription's registration; taken by the completion
    /// that resolves the slot.
    wakers: WakerSet,
}

/// Internal shared state between handle and sender.
struct CompletionInner<T: Clone> {
    slot: Mutex<CompletionSlot<T>>,
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
        slot: Mutex::new(CompletionSlot {
            state: None,
            wakers: WakerSet::default(),
        }),
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
    ///
    /// Blocking waiters are notified, and every subscription that polled the
    /// handle while it was pending is woken once, after the slot's lock is
    /// released.
    pub fn send(&self, state: CompletionState<T>) -> bool {
        let Some(wakers) = self.send_deferred(state) else {
            return false;
        };
        wakers.wake();
        true
    }

    /// Publish under an enclosing scheduler lock, retaining callback delivery
    /// for that lock's owner. State publication and first-writer semantics are
    /// identical to `send`; only polling wakes are deferred.
    pub(crate) fn send_deferred(
        &self,
        state: CompletionState<T>,
    ) -> Option<verter_execution::wake::TakenWakers> {
        let wakers = {
            let mut slot = self.inner.slot.lock();
            if slot.state.is_some() {
                return None;
            }
            slot.state = Some(state);
            slot.wakers.take()
        };
        self.inner.condvar.notify_all();
        Some(wakers)
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
        let mut slot = self.inner.slot.lock();
        while slot.state.is_none() {
            self.inner.condvar.wait(&mut slot);
        }
        slot.state.clone().unwrap()
    }

    /// Block until the handle resolves OR `timeout` elapses. Returns
    /// `Some(state)` if the handle resolved within the budget,
    /// `None` if the timeout expired with the handle still pending.
    ///
    /// Used by the cooperative pump to bound how long a single
    /// iteration sleeps on the condvar before re-checking the inbox
    /// and ready queue. The pump is native-only, so this is too.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn wait_timeout(&self, timeout: Duration) -> Option<CompletionState<T>> {
        let mut slot = self.inner.slot.lock();
        if slot.state.is_some() {
            return slot.state.clone();
        }
        let result = self.inner.condvar.wait_for(&mut slot, timeout);
        if result.timed_out() && slot.state.is_none() {
            None
        } else {
            slot.state.clone()
        }
    }

    /// Non-blocking check. Returns `Some` if already resolved.
    pub fn try_get(&self) -> Option<CompletionState<T>> {
        self.inner.slot.lock().state.clone()
    }

    /// Returns `true` if already resolved.
    pub fn is_resolved(&self) -> bool {
        self.inner.slot.lock().state.is_some()
    }

    /// A nonblocking subscription to this handle's completion, for a
    /// pollable computation that must not park its thread on it.
    pub fn subscribe(&self) -> CompletionSubscription<T> {
        CompletionSubscription {
            inner: Arc::clone(&self.inner),
            registration: WakeRegistration::default(),
        }
    }

    /// How many pending subscriptions have a waker registered on the handle.
    #[cfg(any(test, feature = "test-support"))]
    pub fn registered_wakers(&self) -> usize {
        self.inner.slot.lock().wakers.len()
    }

    /// Snapshot the target identity attached by the sender, if any.
    /// Returns `None` if the sender never called `set_target`.
    pub(crate) fn target(&self) -> Option<CompletionTarget> {
        self.inner.target.lock().clone()
    }
}

/// A pollable view of one [`CompletionHandle`]'s completion.
///
/// [`Self::poll`] checks the handle and, while it is pending, registers the
/// poll's waker in the same critical section, so a completion that lands
/// between the check and the registration still wakes it. The subscription
/// holds at most one registration: a re-poll with a waker that wakes the
/// same task keeps it, any other waker replaces it, and dropping the
/// subscription withdraws it without touching any other subscription's,
/// even one registered with the same executor waker. A wake only signals
/// that a later poll will answer; it never runs the waiting computation.
/// The answer is the handle's terminal state, cloned for every poll that
/// observes it.
pub struct CompletionSubscription<T: Clone> {
    inner: Arc<CompletionInner<T>>,
    /// This subscription's registration while it is pending.
    registration: WakeRegistration,
}

impl<T: Clone> CompletionSubscription<T> {
    /// Attach a completion wait to its execution owners and request stop.
    /// Neither this capability nor its result exposes the blocking handle.
    pub fn owned(
        self,
        waiter: verter_execution::tasks::ExecutionTask,
        producer: verter_execution::tasks::ExecutionTask,
        cancellation: verter_execution::cancellation::CancellationToken,
    ) -> verter_execution::dependency::OwnedDependency<Self> {
        verter_execution::dependency::OwnedDependency::new(self, waiter, producer, cancellation)
    }
    /// `Ready` with the handle's terminal state once it resolved, else
    /// `Pending` with `waker` registered to fire when it does.
    pub fn poll(&mut self, waker: &std::task::Waker) -> std::task::Poll<CompletionState<T>> {
        let mut slot = self.inner.slot.lock();
        if let Some(state) = &slot.state {
            // The completion took every registration when it resolved.
            return std::task::Poll::Ready(state.clone());
        }
        slot.wakers.register(&mut self.registration, waker);
        std::task::Poll::Pending
    }
}

impl<T: Clone> verter_execution::dependency::DependencyPoll for CompletionSubscription<T> {
    type Output = CompletionState<T>;

    fn poll_dependency(&mut self, waker: &std::task::Waker) -> std::task::Poll<Self::Output> {
        self.poll(waker)
    }
}

impl<T: Clone> Drop for CompletionSubscription<T> {
    fn drop(&mut self) {
        if self.registration.is_registered() {
            self.inner
                .slot
                .lock()
                .wakers
                .withdraw(&mut self.registration);
        }
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
                incarnation: 1,
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

    /// A waker that counts how often it was woken.
    struct CountingWake(std::sync::atomic::AtomicUsize);

    impl std::task::Wake for CountingWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }

    fn counting_waker() -> (Arc<CountingWake>, std::task::Waker) {
        let counter = Arc::new(CountingWake(std::sync::atomic::AtomicUsize::new(0)));
        (Arc::clone(&counter), std::task::Waker::from(counter))
    }

    fn wakes(counter: &CountingWake) -> usize {
        counter.0.load(std::sync::atomic::Ordering::SeqCst)
    }

    #[test]
    fn subscription_to_a_resolved_handle_answers_without_registering() {
        let (handle, sender) = completion_pair::<u32>();
        sender.send(CompletionState::Ready(5));
        let (counter, waker) = counting_waker();
        let mut subscription = handle.subscribe();
        match subscription.poll(&waker) {
            std::task::Poll::Ready(CompletionState::Ready(5)) => {}
            other => panic!("expected Ready(5), got {other:?}"),
        }
        assert_eq!(handle.registered_wakers(), 0);
        assert_eq!(wakes(&counter), 0);
    }

    #[test]
    fn pending_subscription_is_woken_once_by_completion_then_ready() {
        let (handle, sender) = completion_pair::<u32>();
        let (counter, waker) = counting_waker();
        let mut subscription = handle.subscribe();
        assert!(subscription.poll(&waker).is_pending());
        assert_eq!(handle.registered_wakers(), 1);
        assert_eq!(wakes(&counter), 0, "registering never wakes");

        sender.send(CompletionState::Superseded);
        assert_eq!(wakes(&counter), 1, "completion wakes the registered poller");
        assert_eq!(
            handle.registered_wakers(),
            0,
            "completion takes the registration"
        );
        assert!(matches!(
            subscription.poll(&waker),
            std::task::Poll::Ready(CompletionState::Superseded)
        ));
        // A second send is a no-op: no second wake.
        sender.send(CompletionState::Shutdown);
        assert_eq!(wakes(&counter), 1);
        // Spurious re-polls after the answer keep answering it.
        assert!(matches!(
            subscription.poll(&waker),
            std::task::Poll::Ready(CompletionState::Superseded)
        ));
    }

    #[test]
    fn owned_completion_stop_wakes_and_detaches_without_resolving_its_sibling() {
        use std::task::Poll;
        use verter_execution::cancellation::CancellationToken;
        use verter_execution::dependency::DependencyStop;
        use verter_execution::tasks::TaskRegistry;
        let registry = TaskRegistry::default();
        let producer = registry.register_task();
        let (handle, sender) = completion_pair::<u32>();
        let token = CancellationToken::new();
        let mut first =
            handle
                .subscribe()
                .owned(registry.register_task(), producer.clone(), token.clone());
        let mut sibling =
            handle
                .subscribe()
                .owned(registry.register_task(), producer, CancellationToken::new());
        let (counter, waker) = counting_waker();
        assert!(first.poll(&waker).is_pending());
        assert!(sibling.poll(&waker).is_pending());
        assert_eq!(handle.registered_wakers(), 2);
        token.cancel();
        assert_eq!(wakes(&counter), 1);
        assert!(matches!(
            first.poll(&waker),
            Poll::Ready(Err(DependencyStop::Cancelled))
        ));
        assert_eq!(handle.registered_wakers(), 1);
        assert_eq!(registry.wait_graph_occupancy().edges, 1);
        assert!(!handle.is_resolved());
        sender.send(CompletionState::Ready(51));
        assert!(matches!(
            sibling.poll(&waker),
            Poll::Ready(Ok(CompletionState::Ready(51)))
        ));
        assert_eq!(registry.wait_graph_occupancy().edges, 0);
    }

    #[test]
    fn repolling_keeps_one_registration_and_a_new_waker_replaces_it() {
        let (handle, sender) = completion_pair::<u32>();
        let (first_counter, first) = counting_waker();
        let (second_counter, second) = counting_waker();
        let mut subscription = handle.subscribe();
        assert!(subscription.poll(&first).is_pending());
        assert!(subscription.poll(&first).is_pending());
        assert_eq!(
            handle.registered_wakers(),
            1,
            "same waker is not re-registered"
        );
        assert!(subscription.poll(&second).is_pending());
        assert_eq!(
            handle.registered_wakers(),
            1,
            "a new waker replaces the old"
        );

        sender.send(CompletionState::Ready(1));
        assert_eq!(wakes(&first_counter), 0, "the replaced waker is not woken");
        assert_eq!(wakes(&second_counter), 1);
    }

    #[test]
    fn dropping_a_pending_subscription_withdraws_only_its_own_registration() {
        let (handle, sender) = completion_pair::<u32>();
        let (dropped_counter, dropped_waker) = counting_waker();
        let (kept_counter, kept_waker) = counting_waker();
        let mut dropped = handle.subscribe();
        let mut kept = handle.subscribe();
        assert!(dropped.poll(&dropped_waker).is_pending());
        assert!(kept.poll(&kept_waker).is_pending());
        assert_eq!(handle.registered_wakers(), 2);

        drop(dropped);
        assert_eq!(
            handle.registered_wakers(),
            1,
            "drop withdraws its registration"
        );

        sender.send(CompletionState::Ready(3));
        assert_eq!(wakes(&dropped_counter), 0);
        assert_eq!(
            wakes(&kept_counter),
            1,
            "a sibling subscriber stays registered"
        );
        assert!(matches!(
            kept.poll(&kept_waker),
            std::task::Poll::Ready(CompletionState::Ready(3))
        ));
    }

    #[test]
    fn one_executor_waker_shared_by_two_subscriptions_is_withdrawn_per_subscription() {
        let (handle, sender) = completion_pair::<u32>();
        let (counter, waker) = counting_waker();
        let mut first = handle.subscribe();
        let mut second = handle.subscribe();
        assert!(first.poll(&waker).is_pending());
        assert!(second.poll(&waker).is_pending());
        drop(first);
        assert_eq!(handle.registered_wakers(), 1);
        sender.send(CompletionState::Ready(4));
        assert_eq!(wakes(&counter), 1);
        assert!(second.poll(&waker).is_ready());
    }

    /// A completion racing a subscription's registration is never lost:
    /// either the poll that registers sees the answer, or the registered
    /// waker fires.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn completion_racing_registration_never_loses_the_wake() {
        for round in 0..256u32 {
            let (handle, sender) = completion_pair::<u32>();
            let (counter, waker) = counting_waker();
            let start = Arc::new(std::sync::Barrier::new(2));
            let sending = {
                let start = Arc::clone(&start);
                std::thread::spawn(move || {
                    start.wait();
                    sender.send(CompletionState::Ready(round));
                })
            };
            let mut subscription = handle.subscribe();
            start.wait();
            let answered_at_poll = subscription.poll(&waker).is_ready();
            sending.join().expect("the sender thread completes");
            assert!(
                answered_at_poll || wakes(&counter) == 1,
                "round {round}: a pending poll must be woken by the completion"
            );
            assert!(matches!(
                subscription.poll(&waker),
                std::task::Poll::Ready(CompletionState::Ready(answer)) if answer == round
            ));
            assert_eq!(handle.registered_wakers(), 0);
        }
    }

    #[test]
    fn completion_target_round_trip() {
        let (handle, sender) = completion_pair::<u32>();
        assert!(handle.target().is_none(), "fresh handle exposes no target",);
        let id = WorkNodeIdentity::FileStage {
            canonical: Arc::from("/x.vue"),
            incarnation: 1,
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
