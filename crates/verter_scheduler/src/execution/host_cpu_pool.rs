//! Host/runtime batch-coordinator CPU pool.
//!
//! Distinct from the scheduler's own CPU pool (`Scheduler::cpu_pool`).
//! Coordinator-only: the external host/runtime layer owns it and runs
//! the outer collect/order/finalise wait of EVERY host batch API on it —
//! both the component-meta batch and the SFC compile batch (and any
//! future host batch fan-out) share this one pool. Parse and cache-node
//! work never lands here; it goes through the scheduler and runs on the
//! scheduler's own stage pool.
//!
//! Dual-pool isolation eliminates the deadlock class where a saturated
//! scheduler stage pool could starve a batch coordinator while parse
//! tasks await CPU availability.
//!
//! Workers register as [`CallerKind::External`] in TLS — `wait_or_drive`
//! parks rather than executes inline when the caller is `External` with
//! a live driver, and `dispatch_ready_job`'s inline-execute branch
//! excludes `External` entirely, so host pool workers never run
//! scheduler CPU tasks (preserves the dual-pool isolation invariant).
//!
//! 8 MiB worker stack matches the existing per-call Rayon pool —
//! the public host API must not regress stack capacity for deeply-
//! nested compile inputs.
//!
//! Build-count atomic exposes pool-construction observability for
//! discriminating tests: a singleton host pool reports `1`; a regressed
//! per-call rebuild would report `n`.
//!
//! Spawn timing is policy-driven. [`HostCpuPool::new`] spawns the worker
//! threads EAGERLY at construction (the default / `lsp_interactive`
//! resource policy); [`HostCpuPool::new_lazy`] defers the spawn to the
//! first [`HostCpuPool::install`] (the `batch_typecheck` policy), behind a
//! `OnceLock` so cold construction of a one-shot batch host creates zero
//! host-pool threads. Either way the pool is a singleton owned by the host
//! and reused across every batch. `pool_thread_count` / `pool_spawned`
//! observe the spawn transition (0 / `false` before the first lazy
//! `install`, non-zero / `true` after).
//!
//! Per-pool identity token (test-only): every successful
//! [`HostCpuPool::new`] assigns a process-unique `pool_id`. Workers
//! stash the id into a thread-local on `start_handler`, exposed via
//! [`host_cpu_pool_token`]. Integration tests assert that a
//! `compile_many` worker's observed token equals
//! `host.host_cpu_pool().pool_id()` — proving the worker is THIS host
//! pool's worker, not some other `External`-defaulting thread.

#[cfg(any(test, feature = "test-support"))]
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use crate::caller_kind::CallerKind;

/// Process-wide build counter. Incremented every time [`HostCpuPool::new`]
/// successfully constructs a new pool. The counter is exposed via
/// [`HostCpuPool::build_count`] for the
/// `two_back_to_back_compile_many_share_host_pool` test: a properly
/// host-owned pool reports a stable count across `compile_many` calls,
/// while a per-call rebuild would advance the count on every call.
///
/// Test-only — gated behind `cfg(any(test, feature = "test-support"))`
/// so production binaries never link the counter (or the
/// `HostCpuPool::build_count` accessor that reads it).
#[cfg(any(test, feature = "test-support"))]
static BUILD_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Process-wide pool-id counter. Every successful [`HostCpuPool::new`]
/// claims a unique id by `fetch_add`. Used by the test-only TLS token
/// so a `compile_many` worker can prove it is running on a SPECIFIC
/// host pool (rather than any `External`-defaulting thread).
///
/// Test-only — gated behind `cfg(any(test, feature = "test-support"))`
/// so production builds neither allocate the counter nor pay the
/// per-pool `fetch_add` cost.
#[cfg(any(test, feature = "test-support"))]
static NEXT_POOL_ID: AtomicUsize = AtomicUsize::new(1);

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Per-pool identity token installed by each [`HostCpuPool`]'s
    /// `start_handler` and read by integration tests through
    /// [`host_cpu_pool_token`]. `None` on threads that never ran on
    /// a `HostCpuPool`; `Some(pool_id)` on workers spawned by a
    /// `HostCpuPool` whose `pool_id == pool_id`.
    ///
    /// Test-only — gated behind `cfg(any(test, feature =
    /// "test-support"))`. Cross-crate tests in `verter_session` enable
    /// the `test-support` feature in `[dev-dependencies]`; production
    /// binaries do not, so the TLS cell, the worker store, and the
    /// [`host_cpu_pool_token`] reader are all compiled out.
    static HOST_CPU_POOL_TOKEN: std::cell::Cell<Option<usize>> =
        const { std::cell::Cell::new(None) };
}

/// Read the host-CPU-pool identity token from the current thread's
/// TLS. Returns `Some(pool_id)` on a worker spawned by the matching
/// `HostCpuPool`; `None` on any other thread (including external
/// callers that never ran on a host pool, and scheduler workers).
///
/// Guard against forward regressions where a worker's `External`
/// caller-kind is inherited *by default* (not via a host-pool
/// `start_handler`) — for example, a regression that re-routes
/// `compile_many` back onto a per-call Rayon pool. Pair with
/// [`HostCpuPool::pool_id`] to discriminate the SPECIFIC pool a
/// worker came from.
///
/// Test-targeted: production code does not depend on the token.
/// Cross-crate integration tests (e.g. in `verter_session`) enable
/// the `test-support` feature in `[dev-dependencies]` to use it.
#[cfg(any(test, feature = "test-support"))]
pub fn host_cpu_pool_token() -> Option<usize> {
    HOST_CPU_POOL_TOKEN.with(|c| c.get())
}

/// Host/runtime batch-coordinator CPU pool, owned by the external
/// host/runtime layer and shared by every host batch API's outer
/// coordinator (component-meta batch, SFC compile batch, and any future
/// host batch fan-out). See module documentation for the dual-pool
/// isolation invariant.
pub struct HostCpuPool {
    /// The underlying rayon pool, behind a `OnceLock` so the worker
    /// threads can spawn LAZILY on the first [`Self::install`] (the
    /// `batch_typecheck` resource policy) instead of EAGERLY at
    /// construction (the default / `lsp_interactive` policy, where
    /// [`Self::new`] forces the spawn immediately). The spawn point is the
    /// single `get_or_init` in [`Self::ensure_pool`].
    pool: OnceLock<rayon::ThreadPool>,
    /// Resolved worker count, captured at construction and used when the
    /// pool actually spawns (eagerly in `new`, or on first `install` for
    /// `new_lazy`).
    threads: usize,
    /// Process-unique id for this pool. Workers stash this into
    /// `HOST_CPU_POOL_TOKEN` on `start_handler` so tests can assert
    /// a worker is THIS pool's worker, not just any `External`
    /// thread.
    ///
    /// Test-only field — only allocated and populated when
    /// `cfg(any(test, feature = "test-support"))`. Production builds
    /// elide the field entirely so there is no per-pool memory cost
    /// or `NEXT_POOL_ID` fetch_add.
    #[cfg(any(test, feature = "test-support"))]
    pool_id: usize,
}

impl HostCpuPool {
    /// Build a new EAGER host CPU pool with `threads` workers, each with
    /// an 8 MiB stack: the worker threads spawn immediately at
    /// construction. `threads == 0` is rejected (callers resolve
    /// `Option<usize>` to a positive count before calling). This is the
    /// default / `lsp_interactive` resource policy — WHEN the pool spawns
    /// is unchanged from before lazy spawning existed, so Full-mode
    /// construction is timing-identical.
    pub fn new(threads: usize) -> Arc<Self> {
        let this = Self::alloc(threads);
        // Eager policy: force the worker threads to spawn now.
        this.ensure_pool();
        this
    }

    /// Build a new LAZY host CPU pool with `threads` workers: the worker
    /// threads do NOT spawn until the first [`Self::install`]. This is the
    /// `batch_typecheck` resource policy — cold host construction spawns
    /// ZERO host-pool threads, dropping a cost a one-shot batch never
    /// amortises.
    pub fn new_lazy(threads: usize) -> Arc<Self> {
        // Lazy policy: the `OnceLock` stays empty until first demand.
        Self::alloc(threads)
    }

    /// Allocate the pool handle (and, in test builds, its identity)
    /// WITHOUT spawning workers. The eager constructor forces the spawn
    /// immediately via [`Self::ensure_pool`]; the lazy constructor defers
    /// it to the first `install`.
    fn alloc(threads: usize) -> Arc<Self> {
        assert!(
            threads > 0,
            "HostCpuPool requires at least one worker thread"
        );
        #[cfg(any(test, feature = "test-support"))]
        let pool_id = NEXT_POOL_ID.fetch_add(1, Ordering::Relaxed);
        // `BUILD_COUNT` counts pool OBJECTS (one per `new` / `new_lazy`) —
        // the signal the back-to-back `compile_many` test uses to prove
        // the host owns ONE pool across batches, independent of WHEN the
        // worker threads actually spawn (which `pool_thread_count`
        // observes separately).
        #[cfg(any(test, feature = "test-support"))]
        BUILD_COUNT.fetch_add(1, Ordering::Relaxed);
        Arc::new(Self {
            pool: OnceLock::new(),
            threads,
            #[cfg(any(test, feature = "test-support"))]
            pool_id,
        })
    }

    /// Spawn (once) and return the underlying rayon pool. The first caller
    /// builds the 8 MiB worker threads; concurrent callers block on the
    /// `OnceLock` until that build completes. This is the SINGLE spawn
    /// point — `new` calls it eagerly at construction, `new_lazy` defers
    /// it to the first `install`. Spawning only creates OS threads (no
    /// host re-entry, no lock acquisition), so a lazy spawn under a batch
    /// demand site cannot deadlock.
    fn ensure_pool(&self) -> &rayon::ThreadPool {
        self.pool.get_or_init(|| {
            #[cfg(any(test, feature = "test-support"))]
            let pool_id = self.pool_id;
            rayon::ThreadPoolBuilder::new()
                .num_threads(self.threads)
                .stack_size(8 * 1024 * 1024)
                .thread_name(|i| format!("verter-host-cpu-{i}"))
                .start_handler(move |_| {
                    // Workers register as `External` so `wait_or_drive`
                    // parks on the completion handle rather than inline-
                    // executing scheduler CPU tasks. `dispatch_ready_job`'s
                    // inline branch excludes `External`, so host workers
                    // never run scheduler CPU work — this is the dual-pool
                    // isolation invariant.
                    //
                    // `External` is the default TLS state for un-marked
                    // threads, but the explicit handler documents the
                    // contract and survives any future change to the
                    // default initialiser.
                    let _ = CallerKind::set(CallerKind::External);
                    // Stash the pool-id token so integration tests can
                    // prove a worker is THIS pool's worker (not any
                    // `External`-defaulting thread). Gated behind
                    // `cfg(any(test, feature = "test-support"))` so the
                    // TLS write only runs in builds that expose the
                    // matching reader — production builds skip it.
                    #[cfg(any(test, feature = "test-support"))]
                    HOST_CPU_POOL_TOKEN.with(|c| c.set(Some(pool_id)));
                })
                .build()
                .expect("failed to build host CPU pool")
        })
    }

    /// Run `f` on the host CPU pool, blocking the caller until `f`
    /// returns. Same semantics as `rayon::ThreadPool::install`. The first
    /// call on a lazily-constructed pool ([`Self::new_lazy`]) spawns the
    /// worker threads here.
    pub fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.ensure_pool().install(f)
    }

    /// Cumulative count of pool OBJECTS built across the host process — one
    /// per [`HostCpuPool::new`] OR [`HostCpuPool::new_lazy`], since
    /// `BUILD_COUNT` increments in the shared `alloc()` constructor
    /// regardless of WHEN the worker threads actually spawn. Exposed for the
    /// back-to-back compile_many test that asserts host-owned (singleton)
    /// ownership instead of per-call rebuilds.
    ///
    /// Test-only — gated behind `cfg(any(test, feature =
    /// "test-support"))`. Production binaries do not link this accessor
    /// or the underlying `BUILD_COUNT` static.
    #[cfg(any(test, feature = "test-support"))]
    pub fn build_count() -> usize {
        BUILD_COUNT.load(Ordering::Relaxed)
    }

    /// Process-unique identity of this pool. Pair with
    /// [`host_cpu_pool_token`] read from inside an `install`'d closure
    /// to assert a worker is running on THIS specific host pool — the
    /// strongest discriminator the test suite has against a forward
    /// regression where `compile_many` would re-route onto an
    /// alternate `External`-defaulting thread (a per-call Rayon pool,
    /// a global Rayon, etc).
    ///
    /// Test-only — gated behind `cfg(any(test, feature =
    /// "test-support"))`. Production binaries do not link this
    /// accessor or the underlying `pool_id` field.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_id(&self) -> usize {
        self.pool_id
    }

    /// Number of worker threads in the underlying `rayon::ThreadPool`.
    ///
    /// Returns `0` until the pool's worker threads have actually spawned —
    /// i.e. a [`Self::new_lazy`] pool reports `0` at construction and a
    /// non-zero count only after the first [`Self::install`]. An eager
    /// [`Self::new`] pool reports the resolved worker count immediately.
    /// This 0-vs-N transition is the discriminating observable the
    /// lazy-resource-policy tests assert on (construction does NOT spawn
    /// batch-pool threads; first demand does).
    ///
    /// On a spawned pool the count reflects the resolved worker count after
    /// `HostConfig`'s `Option<usize>` → `usize` resolution: `None` and
    /// `Some(0)` both land on `available_parallelism()` (final-fallback
    /// `1`), and `Some(n)` for `n >= 1` lands on exactly `n` — the
    /// discriminator for `host_cpu_threads_some_zero_constructs_default_pool`.
    ///
    /// Test-only — gated behind `cfg(any(test, feature =
    /// "test-support"))`. Production binaries do not link this
    /// accessor.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_thread_count(&self) -> usize {
        self.pool
            .get()
            .map(|pool| pool.current_num_threads())
            .unwrap_or(0)
    }

    /// Whether the underlying rayon pool's worker threads have spawned
    /// yet. `false` for a freshly-constructed [`Self::new_lazy`] pool;
    /// `true` after the first [`Self::install`], and always `true` for an
    /// eager [`Self::new`] pool. The boolean form of [`Self::pool_thread_count`]
    /// `> 0` for tests that only care about the spawn transition.
    ///
    /// Test-only — gated behind `cfg(any(test, feature =
    /// "test-support"))`.
    #[cfg(any(test, feature = "test-support"))]
    pub fn pool_spawned(&self) -> bool {
        self.pool.get().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Discriminating test for the dual-pool isolation invariant: a
    /// `HostCpuPool` worker MUST report `CallerKind::External`.
    /// Without the host pool the assertion has nothing to run against
    /// (the type does not exist); with the host pool the
    /// `start_handler` installs the `External` tag so `wait_or_drive`
    /// parks (it would inline-execute for a `CpuWorker` tag).
    #[test]
    fn host_cpu_pool_workers_run_external_caller_kind() {
        let pool = HostCpuPool::new(2);
        let observed = pool.install(CallerKind::current);
        assert_eq!(
            observed,
            CallerKind::External,
            "HostCpuPool workers must register as `External` so \
             `wait_or_drive` parks instead of inline-executing scheduler \
             CPU tasks (dual-pool isolation invariant)"
        );
    }

    /// Discriminating test for the 8 MiB worker stack. A recursion
    /// depth that comfortably overflows a 1 MiB stack must still
    /// succeed on the host pool.
    ///
    /// The recursion uses a moderately-sized stack frame so the
    /// per-frame consumption is observable; 8000 levels at ~256 bytes
    /// each is ~2 MiB, well above the 1 MiB Windows default and well
    /// below the 8 MiB configured ceiling.
    #[test]
    fn host_cpu_pool_has_8mib_stack() {
        // Each frame allocates a ~256-byte buffer so per-frame stack
        // consumption is observable. 8000 frames ≈ 2 MiB — overflows a
        // 1 MiB default Windows stack; succeeds on the 8 MiB host pool
        // configured by `HostCpuPool::new`.
        fn deep_recurse(level: usize, max: usize) -> usize {
            // Force a non-trivial stack frame: a ~256-byte local
            // buffer that survives the recursion (no tail-call
            // elimination) and a use of the level value so the
            // compiler cannot fold the call away.
            let buf = [level as u8; 256];
            if level >= max {
                return buf[0] as usize;
            }
            buf[0] as usize ^ deep_recurse(level + 1, max)
        }

        let pool = HostCpuPool::new(1);
        let result = pool.install(|| deep_recurse(0, 8000));
        // The result is opaque (XOR of 8001 single bytes); the
        // discriminator is that the call returns at all without a
        // stack overflow panic. A 1 MiB stack would have unwound long
        // before reaching this assertion.
        let _ = result;
    }

    /// Discriminating test for the `install` blocking semantics: the
    /// closure's side effect MUST be observable in the caller's thread
    /// after `install` returns. A non-blocking `install` (or any
    /// async-style submit-and-forget regression) would let the assertion
    /// race with the worker.
    #[test]
    fn host_cpu_pool_install_blocks_until_closure_returns() {
        let pool = HostCpuPool::new(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = Arc::clone(&counter);

        pool.install(move || {
            // A small sleep so a non-blocking `install` would
            // observe the assertion fire BEFORE the increment lands.
            std::thread::sleep(std::time::Duration::from_millis(20));
            counter_clone.fetch_add(1, Ordering::SeqCst);
        });

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "`install` must block the caller until the closure has \
             returned (synchronous semantics matching \
             `rayon::ThreadPool::install`)"
        );
    }

    /// Build-count atomic must advance on every successful `new`
    /// call. The counter is process-global, so concurrent tests on
    /// the same module may interleave their increments — the
    /// discriminator is *strict monotonicity per local call*, not an
    /// exact global delta. The downstream `compile_many_*` test
    /// asserts a stable count across `compile_many` calls (single
    /// host pool reused) and does its own paired-sample check; this
    /// unit test characterises only the per-call increment contract.
    #[test]
    fn host_cpu_pool_new_advances_build_count() {
        let before = HostCpuPool::build_count();
        let _p1 = HostCpuPool::new(1);
        let after_one = HostCpuPool::build_count();
        let _p2 = HostCpuPool::new(1);
        let after_two = HostCpuPool::build_count();
        assert!(
            after_one > before,
            "`HostCpuPool::new` must advance the build counter \
             (saw {before} -> {after_one})"
        );
        assert!(
            after_two > after_one,
            "a second `HostCpuPool::new` must advance the counter \
             again (saw {after_one} -> {after_two})"
        );
    }

    /// Per-pool identity token: workers running on a `HostCpuPool`
    /// observe `host_cpu_pool_token() == Some(pool.pool_id())`. The
    /// calling test thread itself (which never ran a host-pool
    /// `start_handler`) observes `None`.
    ///
    /// Discriminator: the former per-call Rayon pool installed no
    /// per-pool token — its workers inherited `External` by *default*.
    /// A regression that re-routes `compile_many` onto a non-host-pool
    /// thread (any Rayon global, any per-call pool) would yield `None`
    /// inside the closure — the strict-equality assertion catches that
    /// class.
    #[test]
    fn host_cpu_pool_workers_carry_pool_id_token() {
        // Calling thread never ran a host-pool start_handler.
        assert_eq!(
            host_cpu_pool_token(),
            None,
            "calling thread must not carry a stale host-pool token"
        );
        let pool = HostCpuPool::new(2);
        let pool_id = pool.pool_id();
        let observed = pool.install(host_cpu_pool_token);
        assert_eq!(
            observed,
            Some(pool_id),
            "worker token must equal pool.pool_id() (saw {observed:?}, \
             expected Some({pool_id}))"
        );
    }

    /// Two distinct pools claim DIFFERENT process-unique ids. A
    /// regression that re-used or hard-coded an id would let a test
    /// false-positive a worker that was actually on the wrong pool.
    #[test]
    fn host_cpu_pool_ids_are_distinct() {
        let a = HostCpuPool::new(1);
        let b = HostCpuPool::new(1);
        assert_ne!(
            a.pool_id(),
            b.pool_id(),
            "two distinct HostCpuPool instances must claim different pool_id values"
        );
    }

    /// Discriminating test for the LAZY spawn policy (`new_lazy`): the
    /// worker threads MUST NOT spawn at construction, and MUST spawn on the
    /// first `install`. A regression that reverted the laziness (built the
    /// rayon pool in `new_lazy`/`alloc` instead of deferring to
    /// `ensure_pool`) would observe a non-zero thread count BEFORE the
    /// install and fail the construction-time assertions.
    #[test]
    fn host_cpu_pool_new_lazy_defers_thread_spawn_until_first_install() {
        let pool = HostCpuPool::new_lazy(2);
        // Construction must spawn ZERO worker threads.
        assert!(
            !pool.pool_spawned(),
            "new_lazy must NOT spawn worker threads at construction"
        );
        assert_eq!(
            pool.pool_thread_count(),
            0,
            "new_lazy thread count must be 0 before the first install \
             (workers spawn lazily on demand)"
        );

        // First demand spawns the workers.
        let ran = pool.install(|| 7usize);
        assert_eq!(ran, 7, "install must run the closure on the spawned pool");
        assert!(
            pool.pool_spawned(),
            "the first install must spawn the lazy pool's worker threads"
        );
        assert_eq!(
            pool.pool_thread_count(),
            2,
            "after the first install the lazy pool must report its resolved \
             worker count (2)"
        );
    }

    /// Pins the EAGER spawn policy (`new`): worker threads spawn at
    /// construction, so the count is non-zero BEFORE any install. This is
    /// the Full / `lsp_interactive` invariant — reverting `new` to lazy
    /// would flip these to 0 and fail here.
    #[test]
    fn host_cpu_pool_new_spawns_threads_eagerly_at_construction() {
        let pool = HostCpuPool::new(2);
        assert!(
            pool.pool_spawned(),
            "eager `new` must spawn worker threads at construction"
        );
        assert_eq!(
            pool.pool_thread_count(),
            2,
            "eager `new` must report its resolved worker count (2) before \
             any install"
        );
    }
}
