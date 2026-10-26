//! Owned workspace continuations. Only transferred work advances the resolver;
//! polling a subscriber or producer only observes readiness and registers wakes.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};

use parking_lot::Mutex;
use verter_execution::cancellation::CancellationToken;
use verter_execution::dependency::{DependencyPoll, DependencyStop, OwnedDependency};
use verter_execution::tasks::{ExecutionScope, ExecutionTask, TaskRegistry};
use verter_execution::wake::{WakeRegistration, WakerSet};

use crate::resolution_currency::ResolutionOutcome;

/// A terminal stop never carries an admitted or partial resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionError {
    Cancelled,
    Cycle,
    ProducerDropped,
    ProducerPanicked,
    Unsupported,
}

/// External work boundaries. All input loading and filesystem discovery run
/// outside poll, on the existing caller/owner execution service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolutionPhase {
    Capture,
    Resolve,
    Preflight,
    Load,
    Discover,
    Freeze,
    Validate,
    Publish,
}

type Answer = Result<ResolutionOutcome, ResolutionError>;
type OwnedFuture = Pin<Box<dyn Future<Output = Answer> + Send>>;

/// The input to a workspace resolution, including its immutable request view.
#[derive(Clone)]
pub struct ResolutionRequest {
    pub importer: String,
    pub specifier: String,
    pub context: verter_session_query::resolution::ResolutionContext,
    pub published: Option<Arc<crate::published_state::PublishedRoot>>,
    pub overlay: Option<crate::resolution_currency::ResolutionOverlaySnapshot>,
    pub project: Option<verter_session_query::resolution::ProjectOwnership>,
}

/// A lookup rooted at an explicit project, without a fabricated importer.
pub struct ProjectResolutionRequest {
    pub project: verter_session_query::resolution::ProjectOwnership,
    pub specifier: String,
    pub context: verter_session_query::resolution::ResolutionContext,
}

/// Borrowed coordinates of one immutable importer request. Keeping them
/// together leaves the continuation's execution and admission owners explicit.
#[derive(Clone, Copy)]
pub(crate) struct ImportRequest<'a> {
    pub(crate) importer: &'a str,
    pub(crate) specifier: &'a str,
    pub(crate) context: verter_session_query::resolution::ResolutionContext,
}
impl<'a> ImportRequest<'a> {
    pub(crate) fn new(
        importer: &'a str,
        specifier: &'a str,
        context: verter_session_query::resolution::ResolutionContext,
    ) -> Self {
        Self {
            importer,
            specifier,
            context,
        }
    }
}

/// Exclusive poll-only subscriber. Dropping or cancelling it leaves transferred
/// producer work and other demands alive.
pub struct WorkspaceResolution {
    core: Core<OwnedFuture>,
}
/// Readiness either transfers the independent producer or delivers the answer.
pub enum ResolutionStep {
    Produce(ResolutionProducer),
    Value(ResolutionOutcome),
}
/// An independently owned continuation; it can move between owner executions.
pub struct ResolutionProducer {
    core: Producer<OwnedFuture>,
}
/// One external continuation slice. Dropping it publishes abandonment.
pub struct ResolutionWork {
    core: Work<OwnedFuture>,
}
impl WorkspaceResolution {
    pub(crate) fn unsupported(waiter: ExecutionTask, stop: CancellationToken) -> Self {
        Self::new(waiter.registry().clone(), waiter, stop, |_, _| {
            Box::pin(async { Err(ResolutionError::Unsupported) })
        })
    }
    pub(crate) fn new(
        registry: TaskRegistry,
        waiter: ExecutionTask,
        stop: CancellationToken,
        build: impl FnOnce(ExecutionTask, Progress) -> OwnedFuture,
    ) -> Self {
        Self {
            core: Core::new(registry, waiter, stop, build),
        }
    }
    pub fn poll(&mut self, waker: &Waker) -> Poll<Result<ResolutionStep, ResolutionError>> {
        self.core.poll(waker).map(|r| {
            r.map(|s| match s {
                Step::Produce(core) => ResolutionStep::Produce(ResolutionProducer { core }),
                Step::Value(value) => ResolutionStep::Value(value),
            })
        })
    }
}
impl ResolutionProducer {
    pub fn poll(&mut self, waker: &Waker) -> Poll<Option<ResolutionWork>> {
        self.core
            .poll(waker)
            .map(|work| work.map(|core| ResolutionWork { core }))
    }
}
impl ResolutionWork {
    pub fn phase(&self) -> ResolutionPhase {
        self.core.progress.phase()
    }
    /// Perform one admitted owner slice externally. This creates no thread,
    /// executor, scheduler, pool or CPU permit.
    pub fn run(self) {
        self.core.run();
    }
}

#[derive(Clone)]
pub(crate) struct Progress(
    Arc<Mutex<ResolutionPhase>>,
    crate::filesystem::DirectoryEvidence,
);
impl Progress {
    pub(crate) fn new() -> Self {
        Self(
            Arc::new(Mutex::new(ResolutionPhase::Capture)),
            crate::filesystem::DirectoryEvidence::default(),
        )
    }
    pub(crate) async fn at(&self, phase: ResolutionPhase) {
        *self.0.lock() = phase;
        let mut yielded = false;
        std::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
    }
    fn phase(&self) -> ResolutionPhase {
        *self.0.lock()
    }
}

struct State<F> {
    future: Option<F>,
    answer: Option<Answer>,
    terminal: bool,
    attached: bool,
    running: bool,
    signal: Arc<Mutex<Signal>>,
    task: Option<ExecutionTask>,
}
#[derive(Default)]
struct Signal {
    runnable: bool,
    terminal: bool,
    wakes: u64,
    wakers: WakerSet,
}
struct Reply<F>(Arc<Mutex<State<F>>>, WakeRegistration);
impl<F> DependencyPoll for Reply<F> {
    type Output = Answer;
    fn poll_dependency(&mut self, waker: &Waker) -> Poll<Answer> {
        let mut state = self.0.lock();
        if let Some(answer) = state.answer.take() {
            return Poll::Ready(answer);
        }
        state.signal.lock().wakers.register(&mut self.1, waker);
        Poll::Pending
    }
}
impl<F> Drop for Reply<F> {
    fn drop(&mut self) {
        let answer = {
            let mut state = self.0.lock();
            state.signal.lock().wakers.withdraw(&mut self.1);
            state.attached = false;
            state.answer.take()
        };
        drop(answer);
    }
}
pub(crate) struct Core<F> {
    producer: Option<Producer<F>>,
    reply: Option<OwnedDependency<Reply<F>>>,
}
impl<F> Drop for Core<F> {
    fn drop(&mut self) {
        // Withdraw the subscriber before abandoning untransferred producer
        // work, so its notification cannot observe a stale outgoing edge.
        self.reply = None;
        self.producer = None;
    }
}
pub(crate) enum Step<F> {
    Produce(Producer<F>),
    Value(ResolutionOutcome),
}
impl<F: Future<Output = Answer> + Unpin> Core<F> {
    pub(crate) fn new(
        registry: TaskRegistry,
        waiter: ExecutionTask,
        stop: CancellationToken,
        build: impl FnOnce(ExecutionTask, Progress) -> F,
    ) -> Self {
        let task = registry.register_task();
        let progress = Progress::new();
        let future = build(task.clone(), progress.clone());
        let state = Arc::new(Mutex::new(State {
            future: Some(future),
            answer: None,
            terminal: false,
            attached: true,
            running: false,
            signal: Arc::new(Mutex::new(Signal {
                runnable: true,
                ..Signal::default()
            })),
            task: Some(task.clone()),
        }));
        Self {
            producer: Some(Producer {
                state: state.clone(),
                progress,
                registration: WakeRegistration::default(),
            }),
            reply: Some(OwnedDependency::new(
                Reply(state, WakeRegistration::default()),
                waiter,
                task,
                stop,
            )),
        }
    }
    pub(crate) fn poll(&mut self, waker: &Waker) -> Poll<Result<Step<F>, ResolutionError>> {
        match self.reply.as_mut().expect("live subscriber").poll(waker) {
            Poll::Ready(Ok(answer)) => {
                self.producer = None;
                Poll::Ready(answer.map(Step::Value))
            }
            Poll::Ready(Err(stop)) => {
                self.producer = None;
                Poll::Ready(Err(match stop {
                    DependencyStop::Cancelled => ResolutionError::Cancelled,
                    DependencyStop::Cycle => ResolutionError::Cycle,
                }))
            }
            Poll::Pending => match self.producer.take() {
                Some(producer) => Poll::Ready(Ok(Step::Produce(producer))),
                None => Poll::Pending,
            },
        }
    }
}

pub(crate) struct Producer<F> {
    state: Arc<Mutex<State<F>>>,
    progress: Progress,
    registration: WakeRegistration,
}
impl<F: Future<Output = Answer> + Unpin> Producer<F> {
    pub(crate) fn poll(&mut self, waker: &Waker) -> Poll<Option<Work<F>>> {
        let mut state = self.state.lock();
        if state.terminal {
            return Poll::Ready(None);
        }
        let signal = state.signal.clone();
        let mut signal = signal.lock();
        signal.wakers.register(&mut self.registration, waker);
        if signal.runnable && !state.running {
            signal.runnable = false;
            state.running = true;
            Poll::Ready(Some(Work {
                future: state.future.take(),
                state: self.state.clone(),
                task: state.task.clone().expect("live producer"),
                generation: signal.wakes,
                progress: self.progress.clone(),
                done: false,
            }))
        } else {
            Poll::Pending
        }
    }
}
fn settle<F>(state: &Mutex<State<F>>, answer: Answer) {
    let (wakes, future, task, discarded) = {
        let mut state = state.lock();
        if state.terminal {
            return;
        }
        state.terminal = true;
        let discarded = if state.attached {
            state.answer = Some(answer);
            None
        } else {
            Some(answer)
        };
        (
            {
                let mut signal = state.signal.lock();
                signal.terminal = true;
                signal.wakers.take()
            },
            state.future.take(),
            state.task.take(),
            discarded,
        )
    };
    drop(future);
    drop(task);
    drop(discarded);
    wakes.wake();
}
impl<F> Drop for Producer<F> {
    fn drop(&mut self) {
        self.state
            .lock()
            .signal
            .lock()
            .wakers
            .withdraw(&mut self.registration);
        settle(&self.state, Err(ResolutionError::ProducerDropped));
    }
}
struct Resume(std::sync::Weak<Mutex<Signal>>);
impl Wake for Resume {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        let Some(shared) = self.0.upgrade() else {
            return;
        };
        let wakes = {
            let mut state = shared.lock();
            if state.terminal {
                return;
            }
            state.runnable = true;
            state.wakes += 1;
            state.wakers.take()
        };
        wakes.wake();
    }
}
pub(crate) struct Work<F> {
    future: Option<F>,
    state: Arc<Mutex<State<F>>>,
    task: ExecutionTask,
    generation: u64,
    progress: Progress,
    done: bool,
}
impl<F: Future<Output = Answer> + Unpin> Work<F> {
    pub(crate) fn run(mut self) {
        if self.state.lock().terminal {
            self.done = true;
            return;
        }
        let waker = Waker::from(Arc::new(Resume(Arc::downgrade(&self.state.lock().signal))));
        let result = {
            let _directories = self.progress.1.install();
            let _scope = ExecutionScope::install(self.task.registry(), &self.task);
            // The subscriber's cancellation is deliberately not installed on
            // an independent producer serving other demands.
            let _stop = verter_execution::cancellation::JobCancellationGuard::replace(None);
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Pin::new(self.future.as_mut().expect("owned continuation"))
                    .poll(&mut Context::from_waker(&waker))
            }))
        };
        self.done = true;
        match result {
            Ok(Poll::Ready(answer)) => {
                self.future = None;
                settle(&self.state, answer);
            }
            Err(_) => {
                self.future = None;
                settle(&self.state, Err(ResolutionError::ProducerPanicked));
            }
            Ok(Poll::Pending) => {
                let wakes = {
                    let mut state = self.state.lock();
                    if state.terminal {
                        None
                    } else {
                        state.future = self.future.take();
                        state.running = false;
                        let mut signal = state.signal.lock();
                        signal.runnable = signal.wakes != self.generation;
                        // A scheduled poll may have re-registered while this
                        // slice still owned the future. Publish readiness and
                        // notify that poller together, then wake after unlock.
                        signal.runnable.then(|| signal.wakers.take())
                    }
                };
                if let Some(wakes) = wakes {
                    wakes.wake();
                }
            }
        }
    }
}
impl<F> Drop for Work<F> {
    fn drop(&mut self) {
        if !self.done {
            self.future = None;
            settle(
                &self.state,
                Err(if std::thread::panicking() {
                    ResolutionError::ProducerPanicked
                } else {
                    ResolutionError::ProducerDropped
                }),
            );
        }
    }
}

/// External compatibility boundary. Legacy boolean cancellation is sampled
/// here only; production owned subscribers use token wake registration.
pub(crate) fn drive_compat<F: Future<Output = Answer> + Unpin>(
    waiter: ExecutionTask,
    cancelled: Option<&(dyn Fn() -> bool + Sync)>,
    build: impl FnOnce(ExecutionTask, Progress) -> F,
) -> ResolutionOutcome {
    let stop = CancellationToken::default();
    let mut core = Core::new(waiter.registry().clone(), waiter, stop.clone(), build);
    let mut producer = None;
    let answer = verter_execution::blocking::block_on_when_pending_with_interval(
        verter_execution::blocking::WaitFamily::WorkspaceResolution,
        cancelled.map(|_| std::time::Duration::from_millis(5)),
        |waker| loop {
            if cancelled.is_some_and(|f| f()) {
                stop.cancel();
            }
            match core.poll(waker) {
                Poll::Ready(Ok(Step::Value(value))) => return Poll::Ready(Ok(value)),
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(Step::Produce(work))) => producer = Some(work),
                Poll::Pending => {}
            }
            if let Some(producer) = producer.as_mut() {
                match producer.poll(waker) {
                    Poll::Ready(Some(work)) => {
                        work.run();
                        continue;
                    }
                    Poll::Ready(None) => continue,
                    Poll::Pending => return Poll::Pending,
                }
            }
            return Poll::Pending;
        },
    );
    answer.unwrap_or_else(|error| {
        ResolutionOutcome::refused(
            None,
            match error {
                ResolutionError::Cancelled => verter_audit::NonAdmissionReason::Cancelled,
                ResolutionError::Cycle => verter_audit::NonAdmissionReason::UnresolvedProvenance,
                ResolutionError::ProducerDropped
                | ResolutionError::ProducerPanicked
                | ResolutionError::Unsupported => verter_audit::NonAdmissionReason::ComputeFailed,
            },
        )
    })
}
