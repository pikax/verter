//! The semantic continuation runtime.
//!
//! One [`SemanticExecution`] evaluates demands for one request. A computation
//! that needs another semantic demand does not call its evaluator: its frame
//! returns [`EvalStep::Need`] and stays parked in the execution's frame arena.
//! The drive loop starts the needed demand (or joins it, or answers it from
//! the execution's completed results) and, when that demand completes, puts
//! every waiting frame on the ready queue with the outcome as its delivery.
//! Waking a waiter only enqueues it; no frame is ever resumed from inside
//! another frame's step, so semantic dependency depth is heap-owned frame
//! state and never native stack depth.
//!
//! The runtime owns scheduling, demand identity and completion. Everything
//! semantic — what a demand means, how its frame evaluates, how a cycle is
//! answered, which completions a later need may reuse — belongs to the
//! [`Program`] the execution drives. A frame owns its own state (cursors,
//! inference context, taint, checker state, evidence); the runtime only moves
//! frames between the arena and the ready queue.
//!
//! # Lifecycle of a demand
//!
//! A demand is opened by a drive's root or by a frame's `Need`:
//!
//! - a completed result the execution holds answers it at once;
//! - an open demand of the execution is on the needing frame's own chain
//!   (see below), so needing it closes a cycle, which the program answers
//!   ([`Program::close_cycle`]) for the needing frame alone — the open
//!   demand keeps evaluating;
//! - otherwise the program starts it ([`Program::start`]): an immediate
//!   answer, a frame that produces it, or a subscription to a producer
//!   outside the execution.
//!
//! Completion delivers the outcome to every waiter in the order they began
//! waiting. A completion the program declares reusable
//! ([`Program::reusable`]) stays in the demand table and answers every later
//! need of the same demand within the execution — the execution's transaction
//! results, independent of any retention decision the program makes for the
//! value outside it. Any other completion is delivered and forgotten, so a
//! later need starts the demand again.
//!
//! # One chain
//!
//! A drive has one root, and a frame waits on at most one demand, so the
//! frames form a single chain from the root to the one frame that can run.
//! When that frame's demand waits on a producer outside the execution nothing
//! else can run. Every open demand is therefore on the chain of the frame
//! that needs it.
//!
//! # Polling
//!
//! An execution is driven by polls ([`SemanticExecution::poll`]): a poll
//! runs every frame that can run and returns `Ready` with the root's
//! outcome, or `Pending` once the chain's tip waits on an outside producer.
//! The program polls that producer's subscription with the poll's waker
//! ([`Program::poll_subscription`]), registering it atomically with the
//! readiness check, so a producer that resolves between the two is never
//! missed: the waker fires and the next poll finds it resolved. A pending
//! execution keeps everything it suspended — frames, demand records,
//! completed results and the one subscription — and dropping it releases
//! each on its own. [`SemanticExecution::drive`] is the blocking driver:
//! it polls with a waker that unparks the driving thread.
//!
//! One poller drives an execution at a time (`poll` takes it exclusively);
//! its state is `Send` whenever the program's frames, demands, values and
//! subscriptions are, so a pending execution may resume on another thread.
//! The request context bound to an execution
//! ([`SemanticExecution::bind_request_context`]) is installed for each step
//! and restored after it, on whichever thread the step runs.
//!
//! # Nesting
//!
//! A frame's step must never drive an execution: bridging code that is not a
//! continuation through a nested drive would put the nested evaluation back
//! on the native stack. A drive refuses to begin inside a running step.
//!
//! # Retention
//!
//! An execution is request-scoped. It holds its frames, demand records and
//! completed results until it is dropped with its request; nothing it owns is
//! resident. Its frames and records live in flat vectors, so dropping it —
//! after completion or a stop — drops each on its own.

use std::cell::Cell;
use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};

use rustc_hash::FxHashMap;
use smallvec::SmallVec;

/// The outcome a completed demand delivers: its value, or the operational
/// failure that stopped it.
pub(crate) type Outcome<P> = Result<<P as Program>::Value, <P as Program>::Failure>;

/// Identity of one demand within one execution. Records are never reused
/// within an execution, so the index alone is exact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DemandId(u32);

/// Identity of one continuation frame: an arena slot plus the generation of
/// its occupant, so a stale id never addresses a later frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ContinuationId {
    slot: u32,
    generation: u32,
}

/// What one step of a frame asks of the runtime.
pub(crate) enum EvalStep<P: Program + ?Sized> {
    /// Park this frame until `demand` completes; its outcome is the frame's
    /// next delivery. `role` tells the program how a cycle through this edge
    /// is answered.
    Need { demand: P::Demand, role: P::Role },
    /// Park this frame until `frame` — a private child computation with no
    /// demand identity of its own — completes; its outcome is the parked
    /// frame's next delivery. A child is never shared, reused or part of a
    /// cycle: it is the caller's own sub-computation (one level of a
    /// structural traversal, a fixation, a reconstruction) held as a frame
    /// instead of a native call.
    Spawn(P::Frame),
    /// The frame's demand completed with this value.
    Complete(P::Value),
    /// The frame's demand stopped with an operational failure.
    Incomplete(P::Failure),
    /// Run again after the frames already ready.
    Yield,
    /// Continue the same demand with this frame instead: a tail transition
    /// that keeps nothing of the frame it replaces.
    Replace(P::Frame),
}

/// How the program begins a demand the execution has not opened.
pub(crate) enum Start<P: Program + ?Sized> {
    /// The demand is answered without a frame (a validated reuse, a refusal).
    Answer(Outcome<P>),
    /// This frame produces the demand.
    Produce(P::Frame),
    /// A producer outside this execution owns the demand; wait on it.
    Subscribe(P::Subscription),
}

/// How a subscription to an outside producer resolved.
pub(crate) enum External<P: Program + ?Sized> {
    /// The producer delivered this outcome.
    Complete(Outcome<P>),
    /// The producer went away without a usable outcome; start the demand
    /// again (the program may now claim it, reuse a result or subscribe
    /// anew).
    Restart,
}

/// The state of one demand in the demand table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DemandState {
    /// The program is being asked to start it.
    Starting,
    /// Its producing frame is on the ready queue.
    Runnable(ContinuationId),
    /// Its producing frame is stepping.
    Running(ContinuationId),
    /// Its producing frame is parked on a demand it needs.
    Waiting(ContinuationId),
    /// A producer outside the execution owns it; the drive waits on it.
    Subscribed,
    /// It completed with a reusable value, held in the table.
    Complete,
}

/// The semantics an execution drives.
pub(crate) trait Program {
    /// Full identity of a demand; equal demands are one computation.
    type Demand: Clone + Eq + Hash;
    /// Heap-owned state of one suspended computation.
    type Frame;
    /// A completed demand's value.
    type Value: Clone;
    /// An operational failure (cancellation, an exhausted resource).
    type Failure: Clone;
    /// The kind of dependency a need is, for cycle answers.
    type Role: Copy;
    /// A handle on a producer outside the execution.
    type Subscription;

    /// Begin a demand the execution has not opened (or whose outside
    /// producer went away).
    fn start(&mut self, demand: &Self::Demand) -> Start<Self>;

    /// Advance `frame` by one step. `delivery` is the outcome of the demand
    /// the frame last needed: `None` on its first step, after a yield and
    /// after a replacement.
    fn step(&mut self, frame: &mut Self::Frame, delivery: Option<Outcome<Self>>) -> EvalStep<Self>;

    /// Answer a need of `demand`, which is open on the needing frame's own
    /// chain. The answer goes to the needing frame only; the open demand
    /// keeps evaluating.
    fn close_cycle(&mut self, demand: &Self::Demand, role: Self::Role) -> Outcome<Self>;

    /// Whether a completed value may answer later needs of its demand within
    /// this execution.
    fn reusable(&self, demand: &Self::Demand, value: &Self::Value) -> bool;

    /// Poll the outside producer of a subscription: `Ready` once it resolved,
    /// else `Pending` with `waker` registered to fire when it does. Checking
    /// and registering are one atomic step against the producer, so a
    /// resolution between them still wakes. Called only when no frame of the
    /// execution can run; a resolved subscription is dropped by the runtime.
    fn poll_subscription(
        &mut self,
        subscription: &mut Self::Subscription,
        waker: &Waker,
    ) -> Poll<External<Self>>;

    /// The failure that stops the whole drive, when it must stop
    /// (cancellation). Checked before every step.
    fn stop(&self) -> Option<Self::Failure>;
}

struct DemandRecord<P: Program> {
    /// The demand's identity; `None` for a spawned child's private demand.
    demand: Option<P::Demand>,
    state: DemandState,
    /// Frames waiting on this demand, in the order they began waiting.
    waiters: SmallVec<[ContinuationId; 1]>,
    /// The reusable completed value once `state` is `Complete`.
    value: Option<P::Value>,
}

struct FrameRecord<P: Program> {
    frame: P::Frame,
    /// The demand this frame produces.
    produces: DemandId,
    /// The outcome of the demand it last needed, until its next step.
    delivery: Option<Outcome<P>>,
}

struct ArenaSlot<P: Program> {
    generation: u32,
    record: Option<FrameRecord<P>>,
}

/// Counters describing one execution's work.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ExecutionCounters {
    /// Frame steps taken.
    pub(crate) steps: u64,
    /// Demands the program was asked to start.
    pub(crate) starts: u64,
    /// Needs answered from the execution's completed results.
    pub(crate) reuses: u64,
    /// Needs that closed a cycle.
    pub(crate) cycles: u64,
    /// Outside producers' subscriptions resolved.
    pub(crate) external_waits: u64,
    /// Polls that returned `Pending`, waiting on an outside producer.
    /// Observation only.
    #[cfg(any(test, feature = "semantic-observe"))]
    pub(crate) pending_polls: u64,
    /// Most frames alive at once.
    pub(crate) peak_frames: usize,
    /// Subscription slots inspected to find the one to wait on. Test-only.
    #[cfg(any(test, feature = "test-support"))]
    pub(crate) subscription_inspections: u64,
}

thread_local! {
    /// Whether a frame step is running on this thread. A drive that begins
    /// while it is set would nest an execution on the native stack.
    static STEP_RUNNING: Cell<bool> = const { Cell::new(false) };

    /// How many drives are running on this thread. Code a frame's step
    /// reaches synchronously sees it set, and evaluates in place what it
    /// would otherwise drive.
    static DRIVES_RUNNING: Cell<u32> = const { Cell::new(0) };
}

/// How long a blocking drive parks before it looks for a stop again when
/// no producer woke it.
const PARK_SLICE: std::time::Duration = std::time::Duration::from_millis(2);

/// Wakes the thread that created it: the waker a blocking drive polls with.
struct UnparkThread(std::thread::Thread);

impl Wake for UnparkThread {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

thread_local! {
    /// This thread's blocking-drive waker, built once per thread.
    static PARK_WAKER: Waker = Waker::from(Arc::new(UnparkThread(std::thread::current())));
}

/// Marks a drive as running for its extent, unwinding included.
struct DriveScope;

impl DriveScope {
    fn enter() -> Self {
        DRIVES_RUNNING.with(|running| running.set(running.get() + 1));
        DriveScope
    }
}

impl Drop for DriveScope {
    fn drop(&mut self) {
        DRIVES_RUNNING.with(|running| running.set(running.get() - 1));
    }
}

/// Whether a drive is running on this thread: a synchronous entry reached
/// from one of its steps (or from the program it drives) must not drive
/// another.
pub(crate) fn drive_running() -> bool {
    DRIVES_RUNNING.with(Cell::get) > 0
}

/// Marks a frame step as running for its extent, unwinding included.
struct StepScope {
    outer: bool,
}

impl StepScope {
    fn enter() -> Self {
        Self {
            outer: STEP_RUNNING.with(|running| running.replace(true)),
        }
    }
}

impl Drop for StepScope {
    fn drop(&mut self) {
        STEP_RUNNING.with(|running| running.set(self.outer));
    }
}

/// Whether a frame step of some execution is running on this thread.
pub(crate) fn step_running() -> bool {
    STEP_RUNNING.with(Cell::get)
}

/// One request-scoped execution: its demand table (which holds the
/// transaction's completed results), frame arena, ready queue and the
/// outside subscription its chain waits on.
pub(crate) struct SemanticExecution<P: Program> {
    demands: Vec<DemandRecord<P>>,
    index: FxHashMap<P::Demand, DemandId>,
    frames: Vec<ArenaSlot<P>>,
    free_frames: Vec<u32>,
    live_frames: usize,
    ready: VecDeque<ContinuationId>,
    /// The subscription to an outside producer the chain's tip waits on. A
    /// drive runs one chain, and a demand is subscribed only when that
    /// chain's tip needs it, so at most one is live; a wait takes it.
    subscription: Option<(DemandId, P::Subscription)>,
    /// The open drive's root, from its first poll until it is ready.
    root: Option<DemandId>,
    /// The drive root's outcome once it completes.
    root_outcome: Option<Outcome<P>>,
    /// The request context each step runs under, when the execution may be
    /// stepped away from the thread that began it.
    request_context: Option<verter_execution::request_context::OpaqueRequestContext>,
    counters: ExecutionCounters,
}

impl<P: Program> Default for SemanticExecution<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P: Program> SemanticExecution<P> {
    pub(crate) fn new() -> Self {
        Self {
            demands: Vec::new(),
            index: FxHashMap::default(),
            frames: Vec::new(),
            free_frames: Vec::new(),
            live_frames: 0,
            ready: VecDeque::new(),
            subscription: None,
            root: None,
            root_outcome: None,
            request_context: None,
            counters: ExecutionCounters::default(),
        }
    }

    pub(crate) fn counters(&self) -> ExecutionCounters {
        self.counters
    }

    /// The state of `demand`, when the execution has it open or holds its
    /// completed result.
    pub(crate) fn demand_state(&self, demand: &P::Demand) -> Option<DemandState> {
        self.index
            .get(demand)
            .map(|id| self.demands[id.0 as usize].state)
    }

    /// Frames currently alive in the arena.
    pub(crate) fn live_frames(&self) -> usize {
        self.live_frames
    }

    /// Run every step under `context`: it is installed into the stepping
    /// thread's request TLS before each step and the thread's own restored
    /// after it, so a pending execution resumed on another thread steps
    /// under its own request.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "bound by the poller that resumes an execution away from the thread that began it"
        )
    )]
    pub(crate) fn bind_request_context(
        &mut self,
        context: verter_execution::request_context::OpaqueRequestContext,
    ) {
        self.request_context = Some(context);
    }

    /// Whether a drive is open: polled at least once and not yet ready.
    #[cfg(test)]
    pub(crate) fn is_pending(&self) -> bool {
        self.root.is_some()
    }

    /// Evaluate `root` to completion, blocking the calling thread whenever
    /// the chain waits on an outside producer until that producer wakes it.
    /// A stop abandons every open demand; completed results stay for a
    /// later drive.
    pub(crate) fn drive(&mut self, program: &mut P, root: P::Demand) -> Outcome<P> {
        let mut root = Some(root);
        PARK_WAKER.with(|waker| loop {
            match self.poll(program, root.take(), waker) {
                Poll::Ready(outcome) => return outcome,
                // Timed parking keeps a stop observable while the producer
                // has not woken this thread.
                Poll::Pending => std::thread::park_timeout(PARK_SLICE),
            }
        })
    }

    /// Advance the drive of `root` as far as it can go. The first poll of a
    /// drive names its root; later polls of the same drive pass `None`.
    /// `Ready` carries the root's outcome (or the stop that abandoned every
    /// open demand); `Pending` means the chain's tip waits on an outside
    /// producer that will wake `waker`. The only way an execution runs:
    /// frames never drive, and a poll never runs inside a frame's step.
    pub(crate) fn poll(
        &mut self,
        program: &mut P,
        root: Option<P::Demand>,
        waker: &Waker,
    ) -> Poll<Outcome<P>> {
        verter_debug_assert!(
            !step_running(),
            "a frame step must not drive a nested execution"
        );
        let _drive = DriveScope::enter();
        let root = match (self.root, root) {
            (Some(open), None) => open,
            (None, Some(root)) => match self.open(program, root) {
                Ok(open) => open,
                Err(outcome) => return Poll::Ready(outcome),
            },
            (Some(_), Some(_)) => unreachable!("a pending drive is polled without a new root"),
            (None, None) => unreachable!("a drive's first poll names its root"),
        };
        loop {
            if let Some(outcome) = self.root_outcome.take() {
                self.root = None;
                return Poll::Ready(outcome);
            }
            if let Some(failure) = program.stop() {
                self.abandon();
                self.root = None;
                return Poll::Ready(Err(failure));
            }
            if let Some(frame) = self.ready.pop_front() {
                self.run(program, frame, root);
                continue;
            }
            // Nothing can run: the chain's tip waits on an outside producer.
            #[cfg(any(test, feature = "test-support"))]
            {
                self.counters.subscription_inspections += 1;
            }
            let Some((demand, subscription)) = self.subscription.as_mut() else {
                unreachable!("an open drive always has a runnable frame or a subscription");
            };
            let demand = *demand;
            let external = match program.poll_subscription(subscription, waker) {
                Poll::Pending => {
                    #[cfg(any(test, feature = "semantic-observe"))]
                    {
                        self.counters.pending_polls += 1;
                    }
                    return Poll::Pending;
                }
                Poll::Ready(external) => external,
            };
            self.subscription = None;
            self.counters.external_waits += 1;
            match external {
                External::Complete(outcome) => self.complete(program, demand, outcome, root),
                External::Restart => self.begin(program, demand, root),
            }
        }
    }

    /// Open a drive of `root`: begin it, or answer it from the execution's
    /// completed result for it.
    fn open(&mut self, program: &mut P, root: P::Demand) -> Result<DemandId, Outcome<P>> {
        verter_debug_assert!(
            self.live_frames == 0 && self.ready.is_empty() && self.subscription.is_none(),
            "a drive begins with no open demand"
        );
        if let Some(&id) = self.index.get(&root) {
            let record = &self.demands[id.0 as usize];
            verter_debug_assert!(record.state == DemandState::Complete);
            self.counters.reuses += 1;
            return Err(Ok(record
                .value
                .clone()
                .expect("a complete demand holds its value")));
        }
        self.root_outcome = None;
        let root = self.insert_demand(root);
        self.root = Some(root);
        self.begin(program, root, root);
        Ok(root)
    }

    /// Ask the program to start the demand `id` and install what it returns.
    fn begin(&mut self, program: &mut P, id: DemandId, root: DemandId) {
        self.counters.starts += 1;
        self.demands[id.0 as usize].state = DemandState::Starting;
        let start = program.start(
            self.demands[id.0 as usize]
                .demand
                .as_ref()
                .expect("only an identified demand is started"),
        );
        match start {
            Start::Answer(outcome) => self.complete(program, id, outcome, root),
            Start::Produce(frame) => {
                let frame = self.alloc_frame(frame, id);
                self.demands[id.0 as usize].state = DemandState::Runnable(frame);
                self.ready.push_back(frame);
            }
            Start::Subscribe(subscription) => {
                verter_debug_assert!(
                    self.subscription.is_none(),
                    "a drive runs one chain; a second live subscription breaks it"
                );
                self.subscription = Some((id, subscription));
                self.demands[id.0 as usize].state = DemandState::Subscribed;
            }
        }
    }

    fn run(&mut self, program: &mut P, id: ContinuationId, root: DemandId) {
        let (produces, delivery) = {
            let record = self.frame_mut(id);
            (record.produces, record.delivery.take())
        };
        self.demands[produces.0 as usize].state = DemandState::Running(id);
        self.counters.steps += 1;
        let step = {
            // The bound request context for this step alone; dropping the
            // handle restores the thread's own, unwinding included.
            let _context = self
                .request_context
                .as_ref()
                .map(|context| Arc::clone(&context.0).install_tls());
            let record = self.frame_mut(id);
            let _scope = StepScope::enter();
            program.step(&mut record.frame, delivery)
        };
        match step {
            EvalStep::Need { demand, role } => {
                self.demands[produces.0 as usize].state = DemandState::Waiting(id);
                self.need(program, id, demand, role, root);
            }
            EvalStep::Complete(value) => {
                self.free_frame(id);
                self.complete(program, produces, Ok(value), root);
            }
            EvalStep::Incomplete(failure) => {
                self.free_frame(id);
                self.complete(program, produces, Err(failure), root);
            }
            EvalStep::Yield => {
                self.demands[produces.0 as usize].state = DemandState::Runnable(id);
                self.ready.push_back(id);
            }
            EvalStep::Spawn(frame) => {
                self.demands[produces.0 as usize].state = DemandState::Waiting(id);
                let child =
                    DemandId(u32::try_from(self.demands.len()).expect("demand table exhausted"));
                self.demands.push(DemandRecord {
                    demand: None,
                    state: DemandState::Starting,
                    waiters: SmallVec::from_elem(id, 1),
                    value: None,
                });
                let frame = self.alloc_frame(frame, child);
                self.demands[child.0 as usize].state = DemandState::Runnable(frame);
                self.ready.push_back(frame);
            }
            EvalStep::Replace(frame) => {
                self.frame_mut(id).frame = frame;
                self.demands[produces.0 as usize].state = DemandState::Runnable(id);
                self.ready.push_back(id);
            }
        }
    }

    fn need(
        &mut self,
        program: &mut P,
        waiter: ContinuationId,
        demand: P::Demand,
        role: P::Role,
        root: DemandId,
    ) {
        let Some(&id) = self.index.get(&demand) else {
            let id = self.insert_demand(demand);
            self.demands[id.0 as usize].waiters.push(waiter);
            self.begin(program, id, root);
            return;
        };
        match self.demands[id.0 as usize].state {
            DemandState::Complete => {
                self.counters.reuses += 1;
                let value = self.demands[id.0 as usize]
                    .value
                    .clone()
                    .expect("a complete demand holds its value");
                self.deliver(waiter, Ok(value));
            }
            DemandState::Subscribed => self.demands[id.0 as usize].waiters.push(waiter),
            DemandState::Starting
            | DemandState::Runnable(_)
            | DemandState::Running(_)
            | DemandState::Waiting(_) => {
                // One chain: an open demand the running frame needs is the
                // frame's own demand or one of its ancestors.
                verter_debug_assert!(
                    self.ready.is_empty(),
                    "a drive runs one chain; a ready frame beside the running one breaks the cycle rule"
                );
                self.counters.cycles += 1;
                let outcome = program.close_cycle(&demand, role);
                self.deliver(waiter, outcome);
            }
        }
    }

    /// Complete `id` with `outcome`: keep a reusable value as the execution's
    /// result for it, and enqueue every waiter with the outcome.
    fn complete(&mut self, program: &mut P, id: DemandId, outcome: Outcome<P>, root: DemandId) {
        let record = &mut self.demands[id.0 as usize];
        let waiters = std::mem::take(&mut record.waiters);
        let keep = match &outcome {
            Ok(value) => record
                .demand
                .as_ref()
                .is_some_and(|demand| program.reusable(demand, value)),
            Err(_) => false,
        };
        if keep {
            record.state = DemandState::Complete;
            record.value = outcome.as_ref().ok().cloned();
        } else {
            // Forget the demand: a later need starts it again.
            if let Some(demand) = &record.demand {
                self.index.remove(demand);
            }
        }
        if id == root {
            self.root_outcome = Some(outcome.clone());
        }
        for waiter in waiters {
            self.deliver(waiter, outcome.clone());
        }
    }

    fn deliver(&mut self, waiter: ContinuationId, outcome: Outcome<P>) {
        let record = self.frame_mut(waiter);
        verter_debug_assert!(record.delivery.is_none(), "a frame waits on one demand");
        record.delivery = Some(outcome);
        let produces = record.produces;
        self.demands[produces.0 as usize].state = DemandState::Runnable(waiter);
        self.ready.push_back(waiter);
    }

    fn insert_demand(&mut self, demand: P::Demand) -> DemandId {
        let id = DemandId(u32::try_from(self.demands.len()).expect("demand table exhausted"));
        self.index.insert(demand.clone(), id);
        self.demands.push(DemandRecord {
            demand: Some(demand),
            state: DemandState::Starting,
            waiters: SmallVec::new(),
            value: None,
        });
        id
    }

    fn alloc_frame(&mut self, frame: P::Frame, produces: DemandId) -> ContinuationId {
        let record = FrameRecord {
            frame,
            produces,
            delivery: None,
        };
        self.live_frames += 1;
        self.counters.peak_frames = self.counters.peak_frames.max(self.live_frames);
        if let Some(slot) = self.free_frames.pop() {
            let entry = &mut self.frames[slot as usize];
            entry.generation = entry.generation.wrapping_add(1);
            entry.record = Some(record);
            return ContinuationId {
                slot,
                generation: entry.generation,
            };
        }
        let slot = u32::try_from(self.frames.len()).expect("frame arena exhausted");
        self.frames.push(ArenaSlot {
            generation: 0,
            record: Some(record),
        });
        ContinuationId {
            slot,
            generation: 0,
        }
    }

    fn free_frame(&mut self, id: ContinuationId) {
        let entry = &mut self.frames[id.slot as usize];
        verter_debug_assert!(entry.generation == id.generation, "stale continuation id");
        entry.record = None;
        self.free_frames.push(id.slot);
        self.live_frames -= 1;
    }

    fn frame_mut(&mut self, id: ContinuationId) -> &mut FrameRecord<P> {
        let entry = &mut self.frames[id.slot as usize];
        verter_debug_assert!(entry.generation == id.generation, "stale continuation id");
        entry
            .record
            .as_mut()
            .expect("continuation id names a live frame")
    }

    /// Drop every frame, subscription and open demand after a stop, each on
    /// its own. Completed results stay.
    fn abandon(&mut self) {
        self.ready.clear();
        self.frames.clear();
        self.free_frames.clear();
        self.live_frames = 0;
        self.subscription = None;
        let demands = &mut self.demands;
        self.index.retain(|_, id| {
            let record = &mut demands[id.0 as usize];
            record.waiters.clear();
            record.state == DemandState::Complete
        });
    }
}

#[cfg(test)]
#[path = "semantic_execution_tests.rs"]
mod tests;
