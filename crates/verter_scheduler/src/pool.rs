//! Host-constructed scheduler worker pools.
//!
//! The scheduler owns no pool construction. The host builds two native
//! scheduler pools and injects them as `Arc<…>` into every
//! [`Scheduler`](crate::scheduler::Scheduler) constructor:
//!
//! - [`SchedulerCpuPool`] — scheduler stage CPU work (Parse / Analysis /
//!   Artifact). Wraps a `rayon::ThreadPool` whose fire-and-forget
//!   [`try_submit`](SchedulerCpuPool::try_submit) is bounded by a
//!   [`CpuConcurrencySemaphore`](crate::cpu_concurrency::CpuConcurrencySemaphore)
//!   sized to dominate the DAG CPU budget. Workers register as
//!   [`CallerKind::CpuWorker`](crate::caller_kind::CallerKind) so the
//!   cooperative pump may inline-execute ready CPU dependencies on the
//!   same worker. Submit takes [`OwnerCommand<Cpu>`](crate::owner_command::OwnerCommand).
//! - [`SchedulerIoPool`] — scheduler source/load work. Owns a fixed-size
//!   crossbeam-channel worker topology, separate from the CPU pool so
//!   blocking disk reads cannot starve parse/analyze work. Workers
//!   register as [`CallerKind::IoWorker`](crate::caller_kind::CallerKind).
//!
//! Both pools coexist with [`HostCpuPool`](crate::host_cpu_pool::HostCpuPool)
//! (which tags `CallerKind::External`) under the dual-pool isolation
//! invariant: host-coordinator work never runs scheduler stage work, and
//! scheduler stage workers never run the outer batch fan-out.
//!
//! Neither pool is available on WASM — the WASM scheduler runs stages
//! inline on the calling thread.
//!
//! # Nonblocking dispatch ([`try_submit`](SchedulerIoPool::try_submit))
//!
//! Both pools expose a single nonblocking submit primitive:
//!
//! ```ignore
//! pub fn try_submit(&self, command: OwnerCommand<Cpu /* or Io */>)
//!     -> Result<SchedulerPoolSubmitResult, SchedulerPoolSubmitError>;
//! ```
//!
//! The driver dispatch loop NEVER blocks on a full pool. The DAG
//! capacity ledger reserves the CPU/IO permit in `next_ready_for_pump`
//! BEFORE a `ReadyJob` exists, and each transport is sized to dominate
//! the matching `dag_budget` (CPU → `cpu`, IO → `io`). A `Full`/`Closed`
//! result during scheduler dispatch is treated as an INVARIANT VIOLATION,
//! not backpressure — the caller `debug_assert!`s and terminalizes the
//! job through the normal DAG cancel path. Ledger release and transport
//! permit drop are not atomic: `Dag::complete` can free a ledger permit
//! before the worker drops the matching transport permit, so a genuine
//! `Full` remains possible in that window and is still fail-closed.
//!
//! To keep the invariant true for an explicit `dag_budget`, the host
//! sizes each pool's transport capacity to dominate the matching budget
//! (see [`SchedulerCpuPool::new`] / [`SchedulerIoPool::new`]).

use crossbeam_channel::{bounded, Sender, TrySendError};

/// A unit of fire-and-forget work submitted to a scheduler pool.
///
/// Completion does not flow back through the pool — stage completion is
/// published onto the scheduler inbox by the closure itself. The pool is
/// a pure execution substrate, so a boxed `FnOnce` is the whole surface.
#[cfg(not(target_arch = "wasm32"))]
pub type SchedulerPoolTask = Box<dyn FnOnce() + Send + 'static>;

/// Successful outcome of [`SchedulerCpuPool::try_submit`] /
/// [`SchedulerIoPool::try_submit`]: the task was enqueued onto the pool.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub enum SchedulerPoolSubmitResult {
    /// The task was accepted by the pool and will run on a worker.
    Submitted,
}

/// Failure outcome of a nonblocking pool submit.
///
/// During scheduler dispatch either variant is an invariant violation
/// (the DAG ledger reserved the permit before producing the job), not a
/// backpressure signal. The dispatch path `debug_assert!`s on it and
/// terminalizes the job rather than blocking, dropping, or requeueing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(target_arch = "wasm32"))]
pub enum SchedulerPoolSubmitError {
    /// The pool's transport is at capacity. Dispatch fail-closes this as
    /// an invariant violation; it is not a backpressure signal.
    Full,
    /// The pool is shutting down (all workers gone / receiver dropped).
    Closed,
}

/// Process-wide pool-id counter shared by both scheduler pool types.
/// Every successful `new` claims a unique id by `fetch_add`. Used by the
/// test-only diagnostics so a test can prove a worker is running on a
/// SPECIFIC injected scheduler pool (mirrors `HostCpuPool::pool_id`).
///
/// Test-only — gated behind `cfg(any(test, feature = "test-support"))`
/// so production builds neither allocate the counter nor pay the
/// per-pool `fetch_add` cost.
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
static NEXT_POOL_ID: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);

/// Scheduler-owned CPU pool for stage work (Parse / Analysis / Artifact).
///
/// Named charter boundary [`CpuPool`]. Wraps a `rayon::ThreadPool` for
/// scoped [`Self::install`] (non-`'static` waiter-side producers) and a
/// bounded [`CpuConcurrencySemaphore`](crate::cpu_concurrency::CpuConcurrencySemaphore)
/// for fire-and-forget [`Self::try_submit`]. The semaphore is the CPU
/// transport — it is sized to dominate `dag_budget.cpu` so the DAG
/// ledger remains the sole admission gate, matching
/// [`SchedulerIoPool`]. Workers register as
/// [`CallerKind::CpuWorker`](crate::caller_kind::CallerKind) so the
/// cooperative pump may inline-execute ready CPU-bound dependencies on
/// the same worker. Distinct from
/// [`HostCpuPool`](crate::host_cpu_pool::HostCpuPool) (which tags
/// `External`) — host-coordinator work never lands here.
#[cfg(not(target_arch = "wasm32"))]
pub struct SchedulerCpuPool {
    pool: rayon::ThreadPool,
    /// Bounded CPU transport. Each successful `try_submit` holds one
    /// owned permit until the spawned task drops it.
    slots: std::sync::Arc<crate::cpu_concurrency::CpuConcurrencySemaphore>,
    /// Resolved transport capacity after the same
    /// `max(threads*4, requested, 1)` floor as [`SchedulerIoPool`].
    #[cfg(any(test, feature = "test-support"))]
    transport_capacity: usize,
    /// Process-unique id for this pool. Workers stash this into
    /// `SCHEDULER_CPU_POOL_TOKEN` on `start_handler` so tests can assert
    /// a worker is running on THIS injected pool.
    ///
    /// Test-only field — only allocated and populated when
    /// `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pool_id: usize,
}

/// Named charter boundary for the bounded scheduler CPU pool.
#[cfg(not(target_arch = "wasm32"))]
pub type CpuPool = SchedulerCpuPool;

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
thread_local! {
    /// Per-pool identity token installed by each [`SchedulerCpuPool`]'s
    /// `start_handler` and read by [`scheduler_cpu_pool_token`].
    static SCHEDULER_CPU_POOL_TOKEN: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Read the scheduler-CPU-pool identity token from the current thread's
/// TLS. Returns `Some(pool_id)` on a worker spawned by the matching
/// [`SchedulerCpuPool`]; `None` on any other thread.
///
/// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub fn scheduler_cpu_pool_token() -> Option<usize> {
    SCHEDULER_CPU_POOL_TOKEN.with(|c| c.get())
}

#[cfg(not(target_arch = "wasm32"))]
impl SchedulerCpuPool {
    /// Build a new scheduler CPU pool with `threads` workers and a
    /// transport bound of `transport_capacity` in-flight+queued tasks.
    ///
    /// `transport_capacity` MUST dominate the resolved `dag_budget.cpu`
    /// so the DAG capacity ledger remains the sole admission gate — if
    /// the bound could fill before the ledger's `cpu` permits are
    /// exhausted, the pool would become a second admission authority
    /// and `try_submit` could observe `Full` at the dispatch site (an
    /// invariant violation). The host derives the capacity via
    /// [`SchedulerConfig::resolved_dag_budget`](crate::scheduler::SchedulerConfig::resolved_dag_budget).
    /// Each worker registers as [`CallerKind::CpuWorker`].
    pub fn new(threads: usize, transport_capacity: usize) -> std::sync::Arc<Self> {
        let threads = threads.max(1);
        let capacity = transport_capacity.max(threads * 4).max(1);
        #[cfg(any(test, feature = "test-support"))]
        let pool_id = NEXT_POOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .stack_size(WORKER_STACK_BYTES)
            .thread_name(|i| format!("verter-cpu-{i}"))
            .start_handler(move |_| {
                // Mark every rayon worker as a scheduler CPU worker so
                // cooperative-pump callers reached via the session-side
                // `wait_or_drive` entry can detect that they are running
                // inside the scheduler's owned pool. `dispatch_ready_job`
                // inline-executes only `CpuWorker`×non-Source and
                // `IoWorker`×Source — `External` is excluded, so host
                // workers never run scheduler CPU work (dual-pool
                // isolation invariant).
                let _ =
                    crate::caller_kind::CallerKind::set(crate::caller_kind::CallerKind::CpuWorker);
                #[cfg(any(test, feature = "test-support"))]
                SCHEDULER_CPU_POOL_TOKEN.with(|c| c.set(Some(pool_id)));
            })
            .build()
            .expect("failed to build scheduler CPU pool");
        std::sync::Arc::new(Self {
            pool,
            slots: std::sync::Arc::new(crate::cpu_concurrency::CpuConcurrencySemaphore::new(
                capacity,
            )),
            #[cfg(any(test, feature = "test-support"))]
            transport_capacity: capacity,
            #[cfg(any(test, feature = "test-support"))]
            pool_id,
        })
    }

    /// Nonblocking owner-affine submit. Takes a CPU-owned command and
    /// reserves one transport slot before `rayon::spawn`. Returns
    /// [`SchedulerPoolSubmitError::Full`] when the bound is saturated
    /// rather than enqueueing unbounded work.
    ///
    /// Dispatch treats `Full` as an invariant violation (fail-closed
    /// terminalization) because transport capacity is sized to dominate
    /// `dag_budget.cpu`. That sizing does not cover the window between
    /// `Dag::complete` releasing the ledger permit on the driver and the
    /// worker dropping this transport permit: a waiter admitted in that
    /// window can observe `Full`. The site still fail-closes rather than
    /// growing an unbounded queue.
    pub fn try_submit(
        &self,
        command: crate::owner_command::OwnerCommand<crate::owner_command::Cpu>,
    ) -> Result<SchedulerPoolSubmitResult, SchedulerPoolSubmitError> {
        let Some(permit) = self.slots.try_acquire_owned() else {
            return Err(SchedulerPoolSubmitError::Full);
        };
        let task = command.into_task();
        self.pool.spawn(move || {
            let _permit = permit;
            task();
        });
        Ok(SchedulerPoolSubmitResult::Submitted)
    }

    /// Transport bound (resolved after the
    /// `max(threads*4, transport_capacity, 1)` floor). Exposed so tests
    /// can assert the bound dominates `dag_budget.cpu`.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn transport_capacity(&self) -> usize {
        self.transport_capacity
    }

    /// Execute a non-`'static` scoped operation on this scheduler CPU pool.
    ///
    /// Unlike [`Self::try_submit`], `install` blocks until the operation
    /// returns, so the closure may borrow caller-owned state. This is the
    /// execution substrate for request-scoped semantic producers whose
    /// resolver context cannot be promoted to `'static`.
    pub fn install<OP, R>(&self, operation: OP) -> R
    where
        OP: FnOnce() -> R + Send,
        R: Send,
    {
        self.pool.install(operation)
    }

    /// Process-unique identity of this pool. Pair with
    /// [`scheduler_cpu_pool_token`] read from inside a submitted task to
    /// assert a worker is running on THIS specific injected pool.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    /// Number of worker threads in the underlying `rayon::ThreadPool`.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_thread_count(&self) -> usize {
        self.pool.current_num_threads()
    }
}

/// The native stack of every thread Verter runs analysis on: the
/// scheduler's CPU and I/O workers, the host CPU pool, the
/// declaration-lowering workers and the entry threads and runtimes of the
/// LSP, MCP and `verter-tsc` binaries. A source job parses and analyzes the
/// file it loads (an I/O worker runs it inline), and the parser, the
/// syntax-tree clone, the slice lowering and the flow evaluation still
/// recurse once per level of nesting in places. The platform defaults
/// (1 MiB for a Windows main thread, 2 MiB for std, tokio and rayon threads)
/// held about a quarter of what the analysis of a 256-deep nest of arrow
/// functions takes unoptimized on the calling thread (11.4 MB; 2.6 MB
/// optimized). Unoptimized frames are four to five times larger, so an
/// unoptimized build reserves 32 MiB. Stacks are reserved, not committed,
/// so the reservation costs address space only.
pub const WORKER_STACK_BYTES: usize = if cfg!(debug_assertions) {
    32 * 1024 * 1024
} else {
    8 * 1024 * 1024
};

/// Scheduler-owned I/O pool for source/load work.
///
/// Fixed-size pool with a bounded crossbeam channel. Separate from the
/// CPU pool so I/O storms (dependency-heavy miss storms loading many
/// files from disk) cannot starve CPU-bound parse/analyze/compile work.
/// Workers register as [`CallerKind::IoWorker`](crate::caller_kind::CallerKind).
///
/// The transport is a single bounded crossbeam channel — NOT a second
/// admission budget or ready queue. Admission concurrency is owned
/// solely by the DAG capacity ledger; the channel's capacity is sized to
/// dominate `dag_budget.io` so the ledger remains the only gate.
#[cfg(not(target_arch = "wasm32"))]
pub struct SchedulerIoPool {
    sender: Sender<SchedulerPoolTask>,
    _threads: Vec<std::thread::JoinHandle<()>>,
    /// Test-only process-unique identity token (see [`SchedulerCpuPool`]).
    #[cfg(any(test, feature = "test-support"))]
    pool_id: usize,
}

#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
thread_local! {
    /// Per-pool identity token installed by each [`SchedulerIoPool`]'s
    /// worker thread and read by [`scheduler_io_pool_token`].
    static SCHEDULER_IO_POOL_TOKEN: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Read the scheduler-IO-pool identity token from the current thread's
/// TLS. Returns `Some(pool_id)` on a worker spawned by the matching
/// [`SchedulerIoPool`]; `None` on any other thread.
///
/// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
#[cfg(all(not(target_arch = "wasm32"), any(test, feature = "test-support")))]
pub fn scheduler_io_pool_token() -> Option<usize> {
    SCHEDULER_IO_POOL_TOKEN.with(|c| c.get())
}

#[cfg(not(target_arch = "wasm32"))]
impl SchedulerIoPool {
    /// Create a new I/O pool with `threads` workers and a transport
    /// channel sized to `transport_capacity`.
    ///
    /// `transport_capacity` MUST dominate the resolved `dag_budget.io`
    /// so the DAG capacity ledger remains the sole admission gate — if
    /// the channel could fill before the ledger's `io` permits are
    /// exhausted, the channel would become a second admission authority
    /// and `try_submit` could observe `Full` at the dispatch site (an
    /// invariant violation). The host derives the capacity via
    /// [`SchedulerConfig::resolved_dag_budget`](crate::scheduler::SchedulerConfig::resolved_dag_budget).
    pub fn new(threads: usize, transport_capacity: usize) -> std::sync::Arc<Self> {
        let threads = threads.max(1);
        // The transport must hold at least one slot and at least the
        // legacy `threads * 4` headroom, and must dominate the resolved
        // DAG io budget passed by the host.
        let capacity = transport_capacity.max(threads * 4).max(1);
        #[cfg(any(test, feature = "test-support"))]
        let pool_id = NEXT_POOL_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let (sender, receiver) = bounded::<SchedulerPoolTask>(capacity);
        let mut handles = Vec::with_capacity(threads);

        for i in 0..threads {
            let rx = receiver.clone();
            handles.push(
                std::thread::Builder::new()
                    .name(format!("verter-io-{i}"))
                    .stack_size(WORKER_STACK_BYTES)
                    .spawn(move || {
                        // Mark this thread as an I/O worker so
                        // cooperative-pump callers reached via the
                        // session-side `wait_or_drive` entry can detect
                        // that they are running inside the scheduler's
                        // owned pool.
                        let _ = crate::caller_kind::CallerKind::set(
                            crate::caller_kind::CallerKind::IoWorker,
                        );
                        #[cfg(any(test, feature = "test-support"))]
                        SCHEDULER_IO_POOL_TOKEN.with(|c| c.set(Some(pool_id)));
                        while let Ok(task) = rx.recv() {
                            task();
                        }
                    })
                    .expect("failed to spawn scheduler I/O pool thread"),
            );
        }

        std::sync::Arc::new(Self {
            sender,
            _threads: handles,
            #[cfg(any(test, feature = "test-support"))]
            pool_id,
        })
    }

    /// Nonblocking submit. Uses `try_send` — NEVER the blocking `send`.
    /// Returns [`SchedulerPoolSubmitError::Full`] if the transport is at
    /// capacity, [`SchedulerPoolSubmitError::Closed`] if all workers
    /// have gone. Under the DAG ledger invariant neither is reachable at
    /// the dispatch site.
    pub fn try_submit(
        &self,
        command: crate::owner_command::OwnerCommand<crate::owner_command::Io>,
    ) -> Result<SchedulerPoolSubmitResult, SchedulerPoolSubmitError> {
        match self.sender.try_send(command.into_task()) {
            Ok(()) => Ok(SchedulerPoolSubmitResult::Submitted),
            Err(TrySendError::Full(_)) => Err(SchedulerPoolSubmitError::Full),
            Err(TrySendError::Disconnected(_)) => Err(SchedulerPoolSubmitError::Closed),
        }
    }

    /// Transport channel capacity (resolved after the
    /// `max(threads*4, transport_capacity, 1)` floor). Exposed so tests
    /// can assert the capacity dominates `dag_budget.io`.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn transport_capacity(&self) -> usize {
        self.sender
            .capacity()
            .expect("scheduler IO transport is bounded")
    }

    /// Process-unique identity of this pool.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    /// Number of worker threads.
    ///
    /// Test-only — gated behind `cfg(any(test, feature = "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn worker_count(&self) -> usize {
        self._threads.len()
    }
}

#[cfg(test)]
#[cfg(not(target_arch = "wasm32"))]
mod tests {
    use super::*;
    use crate::owner_command::OwnerCommand;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn cpu(task: SchedulerPoolTask) -> OwnerCommand<crate::owner_command::Cpu> {
        OwnerCommand::cpu(task)
    }
    fn io(task: SchedulerPoolTask) -> OwnerCommand<crate::owner_command::Io> {
        OwnerCommand::io(task)
    }

    #[test]
    fn scheduler_io_pool_runs_submitted_tasks() {
        // Transport sized to comfortably hold all 10 in-flight tasks so
        // every `try_submit` succeeds regardless of worker drain timing.
        let pool = SchedulerIoPool::new(2, 32);
        let counter = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = crossbeam_channel::unbounded::<()>();

        for _ in 0..10 {
            let c = Arc::clone(&counter);
            let tx = tx.clone();
            pool.try_submit(io(Box::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            })))
            .expect("submit must succeed under capacity");
        }
        for _ in 0..10 {
            rx.recv().expect("worker ran task");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[test]
    fn scheduler_cpu_pool_runs_submitted_tasks() {
        let pool = SchedulerCpuPool::new(2, 32);
        let counter = Arc::new(AtomicUsize::new(0));
        let (tx, rx) = crossbeam_channel::bounded::<()>(10);

        for _ in 0..10 {
            let c = Arc::clone(&counter);
            let tx = tx.clone();
            let r = pool.try_submit(cpu(Box::new(move || {
                c.fetch_add(1, Ordering::SeqCst);
                let _ = tx.send(());
            })));
            assert_eq!(r, Ok(SchedulerPoolSubmitResult::Submitted));
        }
        for _ in 0..10 {
            rx.recv().expect("worker ran task");
        }
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    /// G3-AC1: `SchedulerCpuPool::try_submit` reports `Full` once the
    /// bounded transport is saturated — the displaced unbounded rayon
    /// spawn queue is gone.
    #[test]
    fn scheduler_cpu_pool_try_submit_reports_full_without_blocking() {
        let pool = SchedulerCpuPool::new(1, 1);
        let cap = pool.transport_capacity();
        let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(0);
        pool.try_submit(cpu(Box::new(move || {
            let _ = release_rx.recv();
        })))
        .expect("first submit ok");
        let mut saw_full = false;
        for _ in 0..(cap + 8) {
            match pool.try_submit(cpu(Box::new(|| {}))) {
                Ok(_) => {}
                Err(SchedulerPoolSubmitError::Full) => {
                    saw_full = true;
                    break;
                }
                Err(SchedulerPoolSubmitError::Closed) => panic!("pool not closed"),
            }
        }
        assert!(
            saw_full,
            "try_submit must report Full on a saturated CPU transport \
             (capacity {cap}) instead of enqueueing unbounded rayon work"
        );
        let _ = release_tx.send(());
    }

    #[test]
    fn scheduler_cpu_transport_capacity_dominates_request_and_floor() {
        let big = SchedulerCpuPool::new(1, 64);
        assert!(
            big.transport_capacity() >= 64,
            "CPU transport capacity must dominate the requested capacity (saw {})",
            big.transport_capacity()
        );
        let small = SchedulerCpuPool::new(4, 1);
        assert!(
            small.transport_capacity() >= 4 * 4,
            "CPU transport capacity must not drop below the threads*4 floor (saw {})",
            small.transport_capacity()
        );
    }

    /// Transport capacity must dominate the requested capacity AND the
    /// legacy `threads*4` floor (so a small explicit capacity never
    /// shrinks the channel below the worker-count headroom).
    #[test]
    fn scheduler_io_transport_capacity_dominates_request_and_floor() {
        // Explicit request larger than the floor → request wins.
        let big = SchedulerIoPool::new(1, 64);
        assert!(
            big.transport_capacity() >= 64,
            "transport capacity must dominate the requested capacity (saw {})",
            big.transport_capacity()
        );
        // Explicit request below the floor → floor (threads*4) wins.
        let small = SchedulerIoPool::new(4, 1);
        assert!(
            small.transport_capacity() >= 4 * 4,
            "transport capacity must not drop below the threads*4 floor (saw {})",
            small.transport_capacity()
        );
    }

    /// `try_submit` reports `Full` once the bounded transport is
    /// saturated — proving it uses `try_send`, not the blocking `send`.
    /// (At the dispatch site the ledger makes this unreachable; here we
    /// force it with a tiny channel + a parked worker to characterize
    /// the nonblocking contract.)
    #[test]
    fn scheduler_io_try_submit_reports_full_without_blocking() {
        // 1 worker, capacity floored to 4 (threads*4). Park the worker,
        // then fill the 4 transport slots; the next submit must return
        // Full immediately rather than block the caller.
        let pool = SchedulerIoPool::new(1, 1);
        let cap = pool.transport_capacity();
        let (release_tx, release_rx) = crossbeam_channel::bounded::<()>(0);
        // Park the single worker on the first task.
        pool.try_submit(io(Box::new(move || {
            let _ = release_rx.recv();
        })))
        .expect("first submit ok");
        // Fill the transport. The worker is parked, so queued tasks pile
        // up to `cap`. The exact count that fits before Full depends on
        // whether the worker has dequeued the first task yet, so submit
        // until we observe Full within a bounded budget — the
        // discriminator is that Full is OBSERVED (a blocking send would
        // hang here and the test would time out).
        let mut saw_full = false;
        for _ in 0..(cap + 8) {
            match pool.try_submit(io(Box::new(|| {}))) {
                Ok(_) => {}
                Err(SchedulerPoolSubmitError::Full) => {
                    saw_full = true;
                    break;
                }
                Err(SchedulerPoolSubmitError::Closed) => panic!("pool not closed"),
            }
        }
        assert!(
            saw_full,
            "try_submit must report Full on a saturated bounded transport \
             (capacity {cap}) instead of blocking"
        );
        let _ = release_tx.send(());
    }

    /// Workers report the correct `CallerKind` tag (isolation basis).
    #[test]
    fn scheduler_cpu_pool_workers_tag_cpu_worker() {
        let pool = SchedulerCpuPool::new(1, 8);
        let (tx, rx) = crossbeam_channel::bounded::<crate::caller_kind::CallerKind>(1);
        pool.try_submit(cpu(Box::new(move || {
            let _ = tx.send(crate::caller_kind::CallerKind::current());
        })))
        .expect("submit ok");
        assert_eq!(
            rx.recv().unwrap(),
            crate::caller_kind::CallerKind::CpuWorker,
            "SchedulerCpuPool workers must tag CpuWorker"
        );
    }

    #[test]
    fn scheduler_io_pool_workers_tag_io_worker() {
        let pool = SchedulerIoPool::new(1, 8);
        let (tx, rx) = crossbeam_channel::bounded::<crate::caller_kind::CallerKind>(1);
        pool.try_submit(io(Box::new(move || {
            let _ = tx.send(crate::caller_kind::CallerKind::current());
        })))
        .expect("submit ok");
        assert_eq!(
            rx.recv().unwrap(),
            crate::caller_kind::CallerKind::IoWorker,
            "SchedulerIoPool workers must tag IoWorker"
        );
    }

    /// Distinct pools claim distinct process-unique ids (so a test can
    /// prove a worker is on a SPECIFIC injected pool, not just any
    /// CpuWorker/IoWorker-tagged thread).
    #[test]
    fn scheduler_pool_ids_are_distinct() {
        let a = SchedulerCpuPool::new(1, 8);
        let b = SchedulerCpuPool::new(1, 8);
        assert_ne!(a.pool_id(), b.pool_id());
        let c = SchedulerIoPool::new(1, 8);
        let d = SchedulerIoPool::new(1, 8);
        assert_ne!(c.pool_id(), d.pool_id());
    }

    /// A CPU pool worker carries its pool's id token; the calling thread
    /// does not (mirrors `HostCpuPool::workers_carry_pool_id_token`).
    #[test]
    fn scheduler_cpu_pool_worker_carries_pool_id_token() {
        assert_eq!(scheduler_cpu_pool_token(), None);
        let pool = SchedulerCpuPool::new(1, 8);
        let id = pool.pool_id();
        let (tx, rx) = crossbeam_channel::bounded::<Option<usize>>(1);
        pool.try_submit(cpu(Box::new(move || {
            let _ = tx.send(scheduler_cpu_pool_token());
        })))
        .expect("submit ok");
        assert_eq!(rx.recv().unwrap(), Some(id));
    }
}
