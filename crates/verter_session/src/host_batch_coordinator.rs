//! Host batch-coordinator primitive.
//!
//! Every host/runtime API that fans a batch of independent items out
//! across the host-owned coordinator pool routes through
//! [`HostBatchCoordinator::run_batch`]. It is the single owner of the
//! shared outer-coordinator concerns; call sites supply only the
//! per-item work and (for clients that can panic) the domain-specific
//! panic→result conversion.
//!
//! What the coordinator owns (one coordination rule for every client):
//!
//! - **Pool ownership.** The parallel `install` runs on the host's
//!   dedicated coordinator pool ([`verter_scheduler::HostCpuPool`]),
//!   never on the scheduler's stage-execution `cpu_pool`. This is the
//!   load-bearing isolation: a coordinator job may park waiting for
//!   scheduler-owned `Load`/`Parse` work without ever occupying a
//!   worker that the scheduler's driver needs to dispatch that very
//!   work. Routing the outer wait onto the stage pool is exactly the
//!   starvation-deadlock class this primitive exists to prevent.
//! - **Submission accounting.** When a client's [`BatchPolicy`] carries
//!   a scheduler handle, the coordinator performs the per-batch
//!   `Scheduler::account_batch_submission` bump exactly once per
//!   non-empty batch (the N items share one submission context). The
//!   accounting is pool-free; it lives here so neither client hand-rolls
//!   it at the call site.
//! - **Per-item panic isolation.** Each item closure runs inside a
//!   generic `catch_unwind` boundary. A panic in ONE item is caught and
//!   handed to the client's [`BatchPolicy::on_item_panic`] converter,
//!   which maps it to that client's domain result — so one panicking
//!   item never aborts the whole batch or poisons sibling results. The
//!   generic catch/isolation is centralized here; only the domain
//!   payload→result conversion stays at the call site (it is genuinely
//!   client-specific: a compile maps it to an error `CompileBatchEntry`,
//!   a meta query maps it to a missing slot).
//! - **Empty / single-item fast path.** Zero items allocate and run
//!   nothing; a single item runs on the coordinator pool but skips the
//!   parallel fan-out machinery (no `par_iter`, no index/sort). The
//!   work stays on a coordinator-pool worker so the outer-wait
//!   semantics are uniform across batch sizes.
//! - **Deterministic ordering.** Exactly one result per input, in
//!   input order, regardless of completion order.
//! - **Tracing.** Each batch opens one `debug_span!` carrying the
//!   client's [`BatchPolicy::label`] and the item count, so a batch's
//!   fan-out is attributable in a trace without each client wiring its
//!   own span at the boundary.
//! - **Non-reentrant host-batch policy.** A `run_batch` invoked while
//!   the calling thread is ALREADY inside a host-batch fan-out runs the
//!   nested batch INLINE / sequentially on the current worker rather
//!   than issuing a fresh nested `install` on the coordinator pool.
//!   Nesting a second outer wait onto the same finite coordinator pool
//!   would reintroduce the starvation class one layer up; collapsing it
//!   inline keeps the contract: an outer fan-out blocks only on
//!   scheduler-owned work, and host-batch fan-out never recurses onto
//!   itself.
//! - **wasm / sync fallback.** With no coordinator pool (wasm32) the
//!   batch runs inline / sequentially with identical observable
//!   ordering, the same accounting, and the same per-item panic
//!   isolation.
//!
//! What the coordinator deliberately does NOT own: there is no
//! cancellation or shutdown facility for these batches in the current
//! runtime (the scheduler exposes no batch-cancellation token), so the
//! coordinator does not pretend to provide one. A batch runs to
//! completion. The shared concerns above are exactly the ones that
//! genuinely exist and were previously duplicated at the call sites.
//!
//! New invariant (host batch coordination): outer API fan-out may block
//! only on scheduler-owned work; scheduler-owned work must never
//! require host-batch workers; nested host-batch fan-out is collapsed
//! inline by this coordinator.

use std::cell::Cell;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use verter_scheduler::scheduler::Scheduler;
#[cfg(not(target_arch = "wasm32"))]
use verter_scheduler::HostCpuPool;

thread_local! {
    /// Set while the calling thread is executing a host-batch item
    /// closure. `run_batch` installs this marker per item, around each
    /// item closure on whichever pool worker runs that item, so a
    /// nested `run_batch` reached from inside the closure observes the
    /// flag and collapses to the inline / sequential path instead of
    /// issuing a fresh coordinator-pool `install`.
    ///
    /// Per-item (not per-install) scoping is required because
    /// `par_iter` distributes items across the pool's workers via
    /// work-stealing — a marker set only on the install-entry thread
    /// would leave stolen items running un-marked.
    static IN_HOST_BATCH: Cell<bool> = const { Cell::new(false) };
}

/// True iff the calling thread is currently executing inside a
/// host-batch fan-out. Used by the discriminating reentrancy tests to
/// assert the non-reentrant guard collapsed a nested batch inline
/// rather than re-installing on the coordinator pool.
#[cfg(test)]
fn in_host_batch() -> bool {
    IN_HOST_BATCH.with(Cell::get)
}

/// RAII guard that marks the current thread in-batch for the duration
/// of a coordinator-pool fan-out and restores the prior value on drop
/// (normal return AND panic unwind).
struct InHostBatchGuard {
    previous: bool,
}

impl InHostBatchGuard {
    fn enter() -> Self {
        let previous = IN_HOST_BATCH.with(|c| c.replace(true));
        Self { previous }
    }
}

impl Drop for InHostBatchGuard {
    fn drop(&mut self) {
        IN_HOST_BATCH.with(|c| c.set(self.previous));
    }
}

/// The opaque payload of a caught item panic, handed to
/// [`BatchPolicy::on_item_panic`] so the client can render it into its
/// domain result. Carries the unwind payload (for message extraction)
/// and a reference to the item whose closure panicked (so the converter
/// can attribute the failure — e.g. include the canonical id — without
/// re-deriving it).
pub(crate) struct BatchItemPanic<'a, T> {
    /// The panic payload captured by `catch_unwind`. Use
    /// [`BatchItemPanic::message`] to extract a human-readable string.
    pub payload: Box<dyn std::any::Any + Send>,
    /// The item that panicked, so the converter can attribute the
    /// failure (e.g. include the canonical id) without re-deriving it.
    pub item: &'a T,
}

impl<T> BatchItemPanic<'_, T> {
    /// Best-effort human-readable message from the unwind payload
    /// (`&str` / `String` panics; otherwise a generic marker).
    pub fn message(&self) -> String {
        if let Some(s) = self.payload.downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = self.payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic payload was not a string".to_string()
        }
    }
}

/// Per-client batch coordination policy.
///
/// Captures the genuinely client-specific coordination differences so
/// the [`HostBatchCoordinator`] can own the shared rule while each
/// client supplies only what truly differs:
///
/// - whether the batch performs scheduler submission accounting (the
///   component-meta batch shares one scheduler submission context per
///   batch; the SFC compile batch does not account at all);
/// - the tracing label for the batch span;
/// - how a caught item panic converts into the client's domain result.
///
/// The closure `on_item_panic` is the ONLY place domain-specific panic
/// handling lives: the coordinator catches the panic generically, the
/// client decides what `R` a panicked item produces.
pub(crate) struct BatchPolicy<'p, T, R> {
    /// When `Some`, the coordinator calls
    /// [`Scheduler::account_batch_submission`] exactly once for a
    /// non-empty batch. `None` for clients (e.g. compile) that perform
    /// no per-batch scheduler accounting. The accounting is pool-free,
    /// so it is performed identically on the native and wasm/sync paths.
    pub scheduler: Option<&'p Scheduler>,
    /// Static label naming this batch in the per-batch tracing span.
    pub label: &'static str,
    /// Converts a caught per-item panic into the client's domain result
    /// for that slot, keeping the batch's input→output alignment intact
    /// while isolating the panic from sibling items.
    pub on_item_panic: &'p (dyn Fn(BatchItemPanic<'_, T>) -> R + Sync + Send),
}

/// Coordinator for host/runtime batch fan-out over the host-owned CPU
/// pool. Borrows the host's coordinator pool; cheap to construct per
/// batch call.
///
/// On wasm32 the type carries no pool handle (there is none) and every
/// batch runs inline.
pub(crate) struct HostBatchCoordinator<'a> {
    #[cfg(not(target_arch = "wasm32"))]
    pool: &'a Arc<HostCpuPool>,
    /// Per-instance count of actual coordinator-pool `install` calls
    /// made through THIS coordinator. Bumped immediately before each
    /// `self.pool.install(...)`. Per-instance (not a process-global
    /// static) so a test's install-count assertion is isolated from
    /// every other coordinator the suite builds concurrently — the
    /// production `new` hands in a throwaway counter; only a test that
    /// needs the observation constructs the coordinator with its own.
    /// Test-only.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    install_count: Arc<std::sync::atomic::AtomicUsize>,
    #[cfg(target_arch = "wasm32")]
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> HostBatchCoordinator<'a> {
    /// Construct a coordinator bound to the host's coordinator pool.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn new(pool: &'a Arc<HostCpuPool>) -> Self {
        Self {
            pool,
            #[cfg(test)]
            install_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    /// Construct a coordinator that reports its coordinator-pool install
    /// count through `install_count`, so a test can assert the
    /// non-reentrant guard collapsed nested batches inline (no extra
    /// installs) in isolation from the rest of the suite. Test-only.
    #[cfg(all(test, not(target_arch = "wasm32")))]
    fn new_with_install_counter(
        pool: &'a Arc<HostCpuPool>,
        install_count: Arc<std::sync::atomic::AtomicUsize>,
    ) -> Self {
        Self {
            pool,
            install_count,
        }
    }

    /// Construct a coordinator on wasm32 (no coordinator pool; all
    /// batches run inline / sequentially).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Run one item closure inside the generic per-item panic boundary,
    /// converting a caught panic into the client's domain result via
    /// `policy.on_item_panic`. The in-batch marker is set for the
    /// duration of the closure so a nested `run_batch` reached from
    /// inside it collapses inline.
    #[cfg(not(target_arch = "wasm32"))]
    fn run_item<T, R, F>(policy: &BatchPolicy<'_, T, R>, item: &T, f: &F) -> R
    where
        F: Fn(&T) -> R + Sync + Send,
    {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let _guard = InHostBatchGuard::enter();
            f(item)
        }));
        match outcome {
            Ok(r) => r,
            Err(payload) => (policy.on_item_panic)(BatchItemPanic { payload, item }),
        }
    }

    /// Fan `items` out across the host coordinator pool under `policy`,
    /// applying `f` to each item, and return one result per item in
    /// input order.
    ///
    /// Routing rules (single source of truth for host batch fan-out):
    ///
    /// - `items.is_empty()` → no pool work, no accounting, empty `Vec`.
    /// - otherwise the per-batch scheduler submission accounting (if
    ///   `policy.scheduler` is `Some`) runs exactly once.
    /// - already inside a host-batch fan-out on this thread → run
    ///   inline / sequentially on the current coordinator worker
    ///   (non-reentrant guard; never a nested coordinator-pool
    ///   `install`). Per-item panic isolation still applies.
    /// - exactly one item (cold) → `install` on the coordinator pool
    ///   and run the single item there, skipping the parallel fan-out
    ///   machinery. The work still runs on a coordinator-pool worker so
    ///   the `External` wait semantics are uniform across batch sizes.
    /// - otherwise → `install` on the coordinator pool and fan out with
    ///   rayon, preserving input order via an indexed parallel map.
    ///
    /// Each item closure runs inside a generic `catch_unwind` boundary;
    /// a panic in one item is converted to a domain result through
    /// `policy.on_item_panic` and does not abort the batch.
    ///
    /// `f` must be `Sync + Send` (it runs on multiple pool workers) and
    /// each result `R` must be `Send`. The closure may freely call
    /// scalar host/scheduler operations and may even call back into
    /// `run_batch` (the nested call collapses inline), but it must not
    /// rely on a nested call running in parallel.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_batch<T, R, F>(
        &self,
        items: &[T],
        policy: &BatchPolicy<'_, T, R>,
        f: F,
    ) -> Vec<R>
    where
        T: Sync,
        R: Send,
        F: Fn(&T) -> R + Sync + Send,
    {
        // Empty: nothing to fan out, and no accounting (submit_count
        // stays O(1) per *non-empty* batch).
        if items.is_empty() {
            return Vec::new();
        }

        // One per-batch tracing span carrying the client's label + the
        // fan-out width, so the batch is attributable without each call
        // site wiring its own span.
        let _span =
            tracing::debug_span!("host_batch", label = policy.label, items = items.len()).entered();

        // Per-batch scheduler submission accounting: the N items share
        // one submission context, so the counter bumps exactly once per
        // non-empty batch. Pool-free; it never installs an outer wait.
        // Skipped on the nested path below — a nested batch is part of
        // the same outer submission and must not double-count.
        if !IN_HOST_BATCH.with(Cell::get) {
            if let Some(scheduler) = policy.scheduler {
                scheduler.account_batch_submission();
            }
        }

        // Nested host-batch fan-out (any size): the non-reentrant guard.
        // Run inline / sequentially on the CURRENT coordinator worker —
        // a fresh `install` here would stack a second outer wait on the
        // same finite coordinator pool, reintroducing the starvation
        // class one level up. The thread is already in-batch (we only
        // reach a nested call from inside an outer item closure), so the
        // marker is already set; `run_item` re-asserts it per item and
        // still applies the per-item panic boundary.
        if IN_HOST_BATCH.with(Cell::get) {
            return items
                .iter()
                .map(|item| Self::run_item(policy, item, &f))
                .collect();
        }

        // Single item (cold, not nested): run it ON the coordinator pool
        // — every host/runtime batch executes on the coordinator pool so
        // its `External`-tagged workers (which park rather than
        // inline-execute scheduler CPU work) own the outer wait
        // uniformly, regardless of batch size. The fast path skips only
        // the parallel fan-out machinery (no `par_iter`, no index/sort),
        // not the pool. The per-item boundary in `run_item` sets the
        // in-batch marker and isolates a panic.
        if items.len() == 1 {
            #[cfg(test)]
            self.install_count
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return self
                .pool
                .install(|| vec![Self::run_item(policy, &items[0], &f)]);
        }

        // Cold outer fan-out: run the parallel map on the coordinator
        // pool. The in-batch marker is set PER ITEM (inside `run_item`),
        // NOT once around the whole install: `par_iter` distributes
        // items across every worker in the pool via work-stealing, so a
        // single marker on the install-entry thread would leave stolen
        // items running un-marked — and a nested `run_batch` reached
        // from one of those would wrongly re-install. Scoping the marker
        // to each item's closure guarantees every worker running an item
        // is marked for the duration of that item, regardless of which
        // worker stole it.
        #[cfg(test)]
        self.install_count
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pool.install(|| {
            use rayon::prelude::*;
            // Index-paired parallel map keeps results in input order
            // independent of completion order, then strips the index.
            let mut indexed: Vec<(usize, R)> = items
                .par_iter()
                .enumerate()
                .map(|(idx, item)| (idx, Self::run_item(policy, item, &f)))
                .collect();
            indexed.sort_by_key(|(idx, _)| *idx);
            indexed.into_iter().map(|(_, r)| r).collect()
        })
    }

    /// wasm32 / sync fallback: run the batch inline, sequentially, with
    /// identical observable semantics — the same per-batch accounting
    /// (there is no scheduler pool, but the accounting is pool-free and
    /// still bumps once per non-empty batch), the same per-item panic
    /// isolation, and one result per input in order.
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn run_batch<T, R, F>(
        &self,
        items: &[T],
        policy: &BatchPolicy<'_, T, R>,
        f: F,
    ) -> Vec<R>
    where
        F: Fn(&T) -> R,
    {
        use std::panic::{catch_unwind, AssertUnwindSafe};
        if items.is_empty() {
            return Vec::new();
        }
        // Same per-batch tracing span as the native path.
        let _span =
            tracing::debug_span!("host_batch", label = policy.label, items = items.len()).entered();
        // Same once-per-non-empty-batch accounting as the native path
        // (pool-free; the nested guard prevents double-counting).
        if !IN_HOST_BATCH.with(Cell::get) {
            if let Some(scheduler) = policy.scheduler {
                scheduler.account_batch_submission();
            }
        }
        items
            .iter()
            .map(|item| {
                match catch_unwind(AssertUnwindSafe(|| {
                    let _guard = InHostBatchGuard::enter();
                    f(item)
                })) {
                    Ok(r) => r,
                    Err(payload) => (policy.on_item_panic)(BatchItemPanic { payload, item }),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(not(target_arch = "wasm32"))]
    fn pool() -> Arc<HostCpuPool> {
        HostCpuPool::new(4)
    }

    /// A panic-free policy whose `on_item_panic` resurfaces the panic as
    /// a hard test failure — used by the order/empty/single tests that
    /// never panic an item, so an accidental panic does not silently
    /// pass through a converter.
    #[cfg(not(target_arch = "wasm32"))]
    fn never_panic_policy<T, R>() -> BatchPolicy<'static, T, R> {
        BatchPolicy {
            scheduler: None,
            label: "test",
            on_item_panic: &|p: BatchItemPanic<'_, T>| -> R {
                panic!("unexpected item panic: {}", p.message())
            },
        }
    }

    /// Empty input fans out nothing and returns an empty vector.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_empty_returns_empty() {
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        let policy = never_panic_policy::<u32, u32>();
        let out = coord.run_batch::<u32, u32, _>(&[], &policy, |x| *x);
        assert!(out.is_empty(), "empty input must produce empty output");
    }

    /// Results are positionally aligned with inputs even though the
    /// parallel map may complete out of order. A non-order-preserving
    /// implementation would scramble these.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_preserves_input_order() {
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        let policy = never_panic_policy::<usize, usize>();
        let items: Vec<usize> = (0..64).collect();
        // Reverse the per-item work so earlier items finish LAST under a
        // naive scheduler — order preservation must still hold.
        let out = coord.run_batch(&items, &policy, |i| {
            // Larger sleep for smaller indices so completion order is
            // (roughly) reversed relative to input order.
            std::thread::sleep(std::time::Duration::from_micros(((64 - *i) * 50) as u64));
            *i * 10
        });
        let expected: Vec<usize> = (0..64).map(|i| i * 10).collect();
        assert_eq!(
            out, expected,
            "run_batch must return one result per input IN INPUT ORDER",
        );
    }

    /// A single cold item runs ON the coordinator pool (skipping the
    /// parallel fan-out machinery) — it performs exactly one install and
    /// runs with the in-batch marker set, so the `External` worker owns
    /// the wait uniformly with multi-item batches. The result is still
    /// the mapped value. The calling thread is left un-marked after
    /// return.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_single_item_runs_on_pool_with_marker() {
        let pool = pool();
        // Per-instance install counter isolates this assertion from any
        // other coordinator the suite runs concurrently.
        let installs = Arc::new(AtomicUsize::new(0));
        let coord = HostBatchCoordinator::new_with_install_counter(&pool, Arc::clone(&installs));
        let policy = never_panic_policy::<u32, u32>();

        // The calling thread must not be marked in-batch before/after.
        assert!(
            !in_host_batch(),
            "calling thread must not be in-batch before run_batch",
        );

        let observed_in_batch = std::sync::atomic::AtomicBool::new(false);
        let out = coord.run_batch(std::slice::from_ref(&7u32), &policy, |x| {
            observed_in_batch.store(in_host_batch(), Ordering::Relaxed);
            *x + 1
        });

        assert_eq!(out, vec![8], "single item must return its mapped value");
        assert_eq!(
            installs.load(Ordering::Relaxed),
            1,
            "a single cold item must run on the coordinator pool (exactly one install), \
             not on the calling thread; got {} installs",
            installs.load(Ordering::Relaxed),
        );
        assert!(
            observed_in_batch.load(Ordering::Relaxed),
            "the single item runs on a coordinator worker with the in-batch marker set",
        );
        assert!(
            !in_host_batch(),
            "calling thread must not remain in-batch after run_batch returns",
        );
    }

    /// Non-reentrant host-batch guard (discriminating).
    ///
    /// An outer `run_batch` whose every item closure ITSELF calls
    /// `run_batch` (a nested multi-item fan-out) must:
    ///
    /// 1. complete — never deadlock or stack a second outer wait on the
    ///    coordinator pool;
    /// 2. return correct, input-ordered results at BOTH levels;
    /// 3. collapse every nested fan-out INLINE — performing exactly ONE
    ///    coordinator-pool `install` (the outer one), not `1 + nested`.
    ///
    /// Discrimination: with the guard removed, each nested `run_batch`
    /// would take the cold path and issue its own `self.pool.install`,
    /// so the install-count delta would be `1 + (#outer items)` instead
    /// of `1`. The pool here is deliberately sized SMALLER than the
    /// outer fan-out width so a guard-less implementation also risks a
    /// hard starvation hang (nested installs contending for the few
    /// workers the outer batch already occupies) — either way this test
    /// fails without the guard, and passes with it.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn nested_run_batch_collapses_inline_without_reinstall() {
        // Coordinator pool narrower than the outer batch width so that a
        // guard-less nested `install` would have to contend for workers
        // the outer fan-out already holds.
        let pool = HostCpuPool::new(2);
        // Per-instance install counter: isolates this assertion from any
        // other coordinator the suite runs concurrently (the coordinator
        // is shared batch infrastructure, so a process-global counter
        // would race with compile / meta-batch tests).
        let installs = Arc::new(AtomicUsize::new(0));
        let coord = HostBatchCoordinator::new_with_install_counter(&pool, Arc::clone(&installs));
        let policy = never_panic_policy::<usize, usize>();

        let outer: Vec<usize> = (0..8).collect();

        // Observed-once: every nested closure must see `in_host_batch()`
        // true (it is running underneath the outer fan-out). Stored as
        // an AtomicBool that starts true and is AND-folded so any single
        // false observation is permanent.
        let all_nested_saw_in_batch = std::sync::atomic::AtomicBool::new(true);

        let outer_out = coord.run_batch(&outer, &policy, |o| {
            // Each outer item fans out a nested batch of 4 sub-items.
            let nested_items: Vec<usize> = (0..4).collect();
            let nested_out = coord.run_batch(&nested_items, &policy, |n| {
                if !in_host_batch() {
                    all_nested_saw_in_batch.store(false, Ordering::Relaxed);
                }
                // Nested result is a deterministic function of (outer,
                // nested) index so we can verify ordering precisely.
                *o * 100 + *n
            });
            // Assert the nested batch preserved input order locally.
            let expected_nested: Vec<usize> = (0..4).map(|n| *o * 100 + n).collect();
            assert_eq!(
                nested_out, expected_nested,
                "nested run_batch must preserve input order (outer item {o})",
            );
            // Fold the nested results into a single value for the outer
            // result so the outer ordering is also checked.
            nested_out.into_iter().sum::<usize>()
        });

        let observed_installs = installs.load(Ordering::Relaxed);

        // (3) Exactly ONE install for the whole nested tree — the guard
        // collapsed all 8 nested fan-outs inline.
        assert_eq!(
            observed_installs, 1,
            "non-reentrant guard must collapse nested run_batch inline: expected exactly 1 \
             coordinator-pool install (the outer fan-out), got {observed_installs}. A guard-less \
             implementation re-installs once per nested call.",
        );

        // (1)+(2) The outer batch completed with correct, ordered
        // results (sum of each item's nested 0..4 results).
        let expected_outer: Vec<usize> = (0..8)
            .map(|o| (0..4).map(|n| o * 100 + n).sum::<usize>())
            .collect();
        assert_eq!(
            outer_out, expected_outer,
            "outer run_batch must return correct input-ordered results over the nested fan-out",
        );

        // Every nested closure observed the in-batch marker (it ran
        // underneath the outer fan-out, inline).
        assert!(
            all_nested_saw_in_batch.load(Ordering::Relaxed),
            "every nested closure must run with the in-batch marker set (inline under the \
             outer fan-out)",
        );
    }

    /// The coordinator performs the per-batch scheduler submission
    /// accounting exactly ONCE for a non-empty batch and NOT AT ALL for
    /// an empty batch — the centralized accounting that used to live at
    /// the meta-batch call sites. Discriminating: a coordinator that
    /// forgot the accounting (or ran it per item) would record `0` /
    /// `items.len()` instead of `1`; a coordinator that accounted on the
    /// empty path would record `1` instead of `0`.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_accounts_submission_once_per_nonempty_batch() {
        use verter_scheduler::scheduler::{Scheduler, SchedulerConfig};
        use verter_scheduler::source_loader::MemorySourceLoader;

        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        let scheduler = Scheduler::new(
            SchedulerConfig::default(),
            Arc::new(MemorySourceLoader::new()),
        );

        // Shared panic converter for every accounting-test policy (no
        // item panics here, so a panic is a hard test failure).
        let on_item_panic = |p: BatchItemPanic<'_, usize>| -> usize {
            panic!("unexpected item panic: {}", p.message())
        };
        let make_policy = || BatchPolicy::<usize, usize> {
            scheduler: Some(scheduler.as_ref()),
            label: "test-accounting",
            on_item_panic: &on_item_panic,
        };

        let before = scheduler.counters().submit_count.load(Ordering::Relaxed);

        // Empty batch: NO accounting bump.
        let empty_policy = make_policy();
        let _ = coord.run_batch::<usize, usize, _>(&[], &empty_policy, |x| *x);
        let after_empty = scheduler.counters().submit_count.load(Ordering::Relaxed);
        assert_eq!(
            after_empty, before,
            "an empty batch must NOT account a scheduler submission",
        );

        // Non-empty multi-item batch: exactly ONE bump for the whole
        // batch, regardless of item count.
        let multi: Vec<usize> = (0..16).collect();
        let multi_policy = make_policy();
        let _ = coord.run_batch(&multi, &multi_policy, |x| *x * 2);
        let after_multi = scheduler.counters().submit_count.load(Ordering::Relaxed);
        assert_eq!(
            after_multi,
            before + 1,
            "a non-empty batch must account EXACTLY ONE scheduler submission for the whole \
             batch (got {} bumps for 16 items)",
            after_multi - before,
        );

        // Single-item batch: also exactly one bump (the single-item fast
        // path must not skip accounting).
        let single_policy = make_policy();
        let _ = coord.run_batch(std::slice::from_ref(&99usize), &single_policy, |x| *x);
        let after_single = scheduler.counters().submit_count.load(Ordering::Relaxed);
        assert_eq!(
            after_single,
            before + 2,
            "a single-item batch must also account exactly one scheduler submission",
        );
    }

    /// A panic in ONE item is isolated by the coordinator's generic
    /// per-item boundary: the panicking slot is converted to a domain
    /// result through `policy.on_item_panic`, sibling items still run
    /// and return their real values, and input→output ordering is
    /// preserved. Discriminating: without the catch boundary the panic
    /// would unwind out of the rayon fan-out and poison the whole batch
    /// (the assertion on the sibling results would never run); a
    /// boundary that dropped order would misattribute the panic slot.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_isolates_single_item_panic() {
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);

        // Convert a panicked item into a sentinel that encodes the
        // failing item value, so we can assert the panic landed in the
        // RIGHT slot (ordering preserved) and carried the right message.
        let policy = BatchPolicy::<usize, String> {
            scheduler: None,
            label: "test-panic-isolation",
            on_item_panic: &|p: BatchItemPanic<'_, usize>| -> String {
                format!("PANIC[item={}]: {}", p.item, p.message())
            },
        };

        let items: Vec<usize> = (0..6).collect();
        let out = coord.run_batch(&items, &policy, |i| {
            if *i == 3 {
                panic!("synthetic panic in item 3");
            }
            format!("ok({i})")
        });

        assert_eq!(out.len(), 6, "every input slot must produce a result");
        // Sibling items ran and returned their real values.
        for (i, slot) in out.iter().enumerate() {
            if i == 3 {
                assert!(
                    slot.starts_with("PANIC[item=3]: "),
                    "the panic must land in slot 3 (ordering preserved): {slot}",
                );
                assert!(
                    slot.contains("synthetic panic in item 3"),
                    "the converter must receive the original panic message: {slot}",
                );
            } else {
                assert_eq!(
                    slot,
                    &format!("ok({i})"),
                    "sibling item {i} must be unaffected by the panic in item 3",
                );
            }
        }

        // The calling thread must not be left marked in-batch after a
        // batch that contained a panic (RAII restore on unwind).
        assert!(
            !in_host_batch(),
            "a panicking item must not leak the in-batch marker",
        );
    }

    /// DETERMINISTIC unwind-restore discrimination.
    ///
    /// The multi-item `run_batch_isolates_single_item_panic` above
    /// asserts the no-leak invariant on the CALLING thread — but the
    /// per-item [`InHostBatchGuard`] is set on whichever POOL WORKER runs
    /// the item, and the calling thread is never marked at the top level.
    /// So that assertion catches a leaked-`IN_HOST_BATCH`-on-unwind
    /// regression only probabilistically (when the calling thread happens
    /// to be the worker that ran the panicking item). This test removes
    /// the probability:
    ///
    /// - it runs a SINGLE-item batch whose one item panics, so the
    ///   panicking item runs deterministically on a coordinator-pool
    ///   worker (the `items.len() == 1` path installs the item ON the
    ///   pool — the calling test thread is not a member of the pool, so
    ///   `install` blocks it and runs the item on a worker);
    /// - it probes the worker's `IN_HOST_BATCH` flag INSIDE
    ///   `on_item_panic`, which the coordinator invokes ON THAT SAME
    ///   WORKER *after* `catch_unwind` has already unwound past the
    ///   guard's drop. With the guard's `Drop` restore intact the worker
    ///   observes `false` (the value restored on unwind); a guard whose
    ///   unwind-restore were removed would leave the worker marked `true`.
    ///
    /// Discrimination (stated for the gate): deleting the `impl Drop for
    /// InHostBatchGuard` restore — or constructing the guard OUTSIDE the
    /// `catch_unwind` closure so the restore happens after the catch
    /// rather than during the unwind — makes
    /// `worker_in_batch_after_unwind` observe `true`, and the
    /// `assert!(!worker_in_batch_after_unwind...)` below FAILS. With the
    /// restore in place it observes `false` and the test passes. The
    /// probe runs on the worker (asserted distinct from the calling
    /// thread), so the outcome does not depend on which thread `install`
    /// happens to schedule onto.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn single_item_panic_restores_in_batch_marker_on_worker() {
        use std::sync::atomic::AtomicBool;
        use std::thread::ThreadId;

        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);

        let calling_thread = std::thread::current().id();

        // Worker observations captured during the panicking item's run
        // and its unwind. `Mutex<Option<ThreadId>>` so we can prove the
        // probe ran on a pool worker (not the caller) and that the
        // item-body and the panic-converter ran on the SAME worker.
        let worker_in_batch_during_item = AtomicBool::new(false);
        let worker_in_batch_after_unwind = AtomicBool::new(false);
        let item_body_thread: std::sync::Mutex<Option<ThreadId>> = std::sync::Mutex::new(None);
        let converter_thread: std::sync::Mutex<Option<ThreadId>> = std::sync::Mutex::new(None);

        // The panic converter runs on the worker thread AFTER the unwind
        // has dropped the per-item guard, so probing `in_host_batch()`
        // here observes the RESTORED value. It also records its thread id
        // and resurfaces the failing item value so we keep the existing
        // attribution guarantee.
        let on_item_panic = |p: BatchItemPanic<'_, u32>| -> String {
            *converter_thread.lock().unwrap() = Some(std::thread::current().id());
            worker_in_batch_after_unwind.store(in_host_batch(), Ordering::SeqCst);
            format!("PANIC[item={}]: {}", p.item, p.message())
        };
        let policy = BatchPolicy::<u32, String> {
            scheduler: None,
            label: "test-single-panic-restore",
            on_item_panic: &on_item_panic,
        };

        assert!(
            !in_host_batch(),
            "calling thread must not be in-batch before run_batch",
        );

        let out = coord.run_batch(std::slice::from_ref(&3u32), &policy, |_i| -> String {
            // Inside the item body the worker IS marked in-batch (the
            // guard is live here). Record it + the worker thread id, then
            // panic so the unwind exercises the guard's drop-restore. The
            // diverging `panic!` (`!`) satisfies the `String` return.
            *item_body_thread.lock().unwrap() = Some(std::thread::current().id());
            worker_in_batch_during_item.store(in_host_batch(), Ordering::SeqCst);
            panic!("synthetic single-item panic for unwind-restore test")
        });

        // The slot converted to the panic sentinel (attribution intact).
        assert_eq!(out.len(), 1, "single-item batch must produce one slot");
        assert!(
            out[0].starts_with("PANIC[item=3]: ")
                && out[0].contains("synthetic single-item panic for unwind-restore test"),
            "the single panicking item must convert via on_item_panic with the right message: {}",
            out[0],
        );

        let body_thread = item_body_thread.lock().unwrap().expect("item body ran");
        let conv_thread = converter_thread.lock().unwrap().expect("converter ran");

        // Determinism evidence: the panicking item ran on a coordinator
        // POOL WORKER (not the calling thread), and the panic converter
        // ran on that SAME worker — so the post-unwind probe below is a
        // worker-thread observation, not a calling-thread one.
        assert_ne!(
            body_thread, calling_thread,
            "the single item must run on a coordinator-pool worker, not the calling thread \
             (otherwise the probe is not deterministic)",
        );
        assert_eq!(
            body_thread, conv_thread,
            "the panic converter must run on the SAME worker that ran the panicking item, so the \
             post-unwind marker probe observes that worker's restored state",
        );

        // Sanity: the worker WAS marked in-batch while the item body ran
        // (the guard was live). If this were false the next assertion
        // would be vacuous.
        assert!(
            worker_in_batch_during_item.load(Ordering::SeqCst),
            "the item body must run with the in-batch marker set on its worker",
        );

        // THE DISCRIMINATING ASSERTION: after the unwind, the worker's
        // in-batch marker is restored to its prior (false) value. With
        // the guard's drop-on-unwind restore removed, the worker would
        // still read `true` here and this fails.
        assert!(
            !worker_in_batch_after_unwind.load(Ordering::SeqCst),
            "the per-item guard must RESTORE the worker's in-batch marker on panic unwind \
             (observed on the worker thread, after the catch): a leaked `true` here means the \
             InHostBatchGuard drop-restore regressed",
        );

        // And the calling thread is, as ever, never left marked.
        assert!(
            !in_host_batch(),
            "the calling thread must not be left marked in-batch after a panicking batch",
        );
    }
}
