//! Host batch-coordinator primitive.
//!
//! Every host/runtime API that fans a batch of independent items out
//! across the host-owned coordinator pool routes through
//! [`HostBatchCoordinator::run_batch`]. It is the single owner of the
//! outer-coordinator fan-out policy:
//!
//! - **Pool ownership.** The parallel `install` runs on the host's
//!   dedicated coordinator pool ([`verter_scheduler::HostCpuPool`]),
//!   never on the scheduler's stage-execution `cpu_pool`. This is the
//!   load-bearing isolation: a coordinator job may park waiting for
//!   scheduler-owned `Load`/`Parse` work without ever occupying a
//!   worker that the scheduler's driver needs to dispatch that very
//!   work. Routing the outer wait onto the stage pool is exactly the
//!   starvation-deadlock class this primitive exists to prevent.
//! - **Empty / single-item fast path.** Zero items allocate and run
//!   nothing; a single item runs inline on the calling thread (no pool
//!   round-trip for the common interactive-sized batch).
//! - **Deterministic ordering.** Exactly one result per input, in
//!   input order, regardless of completion order.
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
//!   ordering.
//!
//! New invariant (host batch coordination): outer API fan-out may block
//! only on scheduler-owned work; scheduler-owned work must never
//! require host-batch workers; nested host-batch fan-out is collapsed
//! inline by this coordinator.

use std::cell::Cell;

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use verter_scheduler::HostCpuPool;

thread_local! {
    /// Set while the calling thread is executing a host-batch item
    /// closure. `run_batch` installs this marker per item, around each
    /// `f(item)` call on whichever pool worker runs that item, so a
    /// nested `run_batch` reached from inside `f` observes the flag and
    /// collapses to the inline / sequential path instead of issuing a
    /// fresh coordinator-pool `install`.
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

/// Process-wide count of actual coordinator-pool `install` calls.
/// Bumped immediately before each `self.pool.install(...)` so the
/// reentrancy test can assert the non-reentrant guard collapsed nested
/// batches INLINE (zero extra installs) rather than stacking fresh
/// coordinator-pool installs. Test-only.
#[cfg(test)]
static COORDINATOR_INSTALL_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// Read the cumulative coordinator-pool install count. Test-only.
#[cfg(test)]
fn coordinator_install_count() -> usize {
    COORDINATOR_INSTALL_COUNT.load(std::sync::atomic::Ordering::Relaxed)
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

/// Coordinator for host/runtime batch fan-out over the host-owned CPU
/// pool. Borrows the host's coordinator pool; cheap to construct per
/// batch call.
///
/// On wasm32 the type carries no pool handle (there is none) and every
/// batch runs inline.
pub(crate) struct HostBatchCoordinator<'a> {
    #[cfg(not(target_arch = "wasm32"))]
    pool: &'a Arc<HostCpuPool>,
    #[cfg(target_arch = "wasm32")]
    _marker: std::marker::PhantomData<&'a ()>,
}

impl<'a> HostBatchCoordinator<'a> {
    /// Construct a coordinator bound to the host's coordinator pool.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn new(pool: &'a Arc<HostCpuPool>) -> Self {
        Self { pool }
    }

    /// Construct a coordinator on wasm32 (no coordinator pool; all
    /// batches run inline / sequentially).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn new() -> Self {
        Self {
            _marker: std::marker::PhantomData,
        }
    }

    /// Fan `items` out across the host coordinator pool, applying `f` to
    /// each item, and return one result per item in input order.
    ///
    /// Routing rules (single source of truth for host batch fan-out):
    ///
    /// - `items.is_empty()` → no pool work, empty `Vec`.
    /// - exactly one item → run inline on the caller (no pool
    ///   round-trip).
    /// - already inside a host-batch fan-out on this thread → run
    ///   inline / sequentially (non-reentrant guard; never a nested
    ///   coordinator-pool `install`).
    /// - otherwise → `install` on the coordinator pool and fan out with
    ///   rayon, preserving input order via an indexed parallel map.
    ///
    /// `f` must be `Sync + Send` (it runs on multiple pool workers) and
    /// each result `R` must be `Send`. The closure may freely call
    /// scalar host/scheduler operations and may even call back into
    /// `run_batch` (the nested call collapses inline), but it must not
    /// rely on a nested call running in parallel.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn run_batch<T, R, F>(&self, items: &[T], f: F) -> Vec<R>
    where
        T: Sync,
        R: Send,
        F: Fn(&T) -> R + Sync + Send,
    {
        // Empty: nothing to fan out.
        if items.is_empty() {
            return Vec::new();
        }
        // Single item, or a nested host-batch fan-out: run inline /
        // sequentially on the current thread. The nested case is the
        // non-reentrant guard — a fresh `install` here would stack a
        // second outer wait on the same coordinator pool.
        if items.len() == 1 || IN_HOST_BATCH.with(Cell::get) {
            return items.iter().map(&f).collect();
        }

        // Cold outer fan-out: run the parallel map on the coordinator
        // pool. The in-batch marker is set PER ITEM, inside the map
        // closure, NOT once around the whole install: `par_iter`
        // distributes items across every worker in the pool via
        // work-stealing, so a single marker on the install-entry thread
        // would leave stolen items running un-marked — and a nested
        // `run_batch` reached from one of those would wrongly re-install.
        // Scoping the marker to each item's `f` call guarantees every
        // worker running an item is marked for the duration of that
        // item's closure, regardless of which worker stole it.
        #[cfg(test)]
        COORDINATOR_INSTALL_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.pool.install(|| {
            use rayon::prelude::*;
            // Index-paired parallel map keeps results in input order
            // independent of completion order, then strips the index.
            let mut indexed: Vec<(usize, R)> = items
                .par_iter()
                .enumerate()
                .map(|(idx, item)| {
                    let _guard = InHostBatchGuard::enter();
                    (idx, f(item))
                })
                .collect();
            indexed.sort_by_key(|(idx, _)| *idx);
            indexed.into_iter().map(|(_, r)| r).collect()
        })
    }

    /// wasm32 / sync fallback: run the batch inline, sequentially, with
    /// identical observable ordering (one result per input, in order).
    #[cfg(target_arch = "wasm32")]
    pub(crate) fn run_batch<T, R, F>(&self, items: &[T], f: F) -> Vec<R>
    where
        F: Fn(&T) -> R,
    {
        items.iter().map(&f).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Serialises the install-count-sensitive tests against each other
    /// so the process-global `COORDINATOR_INSTALL_COUNT` delta each one
    /// observes is attributable solely to its own `run_batch` calls.
    /// Only the tests that assert on the install count take this lock.
    #[cfg(not(target_arch = "wasm32"))]
    static INSTALL_COUNT_LOCK: parking_lot::Mutex<()> = parking_lot::Mutex::new(());

    #[cfg(not(target_arch = "wasm32"))]
    fn pool() -> Arc<HostCpuPool> {
        HostCpuPool::new(4)
    }

    /// Empty input fans out nothing and returns an empty vector.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_empty_returns_empty() {
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        let out = coord.run_batch::<u32, u32, _>(&[], |x| *x);
        assert!(out.is_empty(), "empty input must produce empty output");
    }

    /// Results are positionally aligned with inputs even though the
    /// parallel map may complete out of order. A non-order-preserving
    /// implementation would scramble these.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_preserves_input_order() {
        let _serial = INSTALL_COUNT_LOCK.lock();
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        let items: Vec<usize> = (0..64).collect();
        // Reverse the per-item work so earlier items finish LAST under a
        // naive scheduler — order preservation must still hold.
        let out = coord.run_batch(&items, |i| {
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

    /// A single item takes the inline fast path (no coordinator-pool
    /// install) and still returns the mapped result.
    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn run_batch_single_item_runs_inline() {
        let pool = pool();
        let coord = HostBatchCoordinator::new(&pool);
        // The calling thread is NOT a host-pool worker, so if the single
        // item ran on the pool the in-batch flag would have been set on
        // a worker, not here. Observe the flag from inside the closure:
        // the inline fast path runs the closure on THIS thread without
        // marking it in-batch.
        let observed_in_batch = std::sync::atomic::AtomicBool::new(true);
        let out = coord.run_batch(std::slice::from_ref(&7u32), |x| {
            observed_in_batch.store(in_host_batch(), std::sync::atomic::Ordering::Relaxed);
            *x + 1
        });
        assert_eq!(out, vec![8]);
        assert!(
            !observed_in_batch.load(std::sync::atomic::Ordering::Relaxed),
            "single-item fast path runs inline and does NOT set the in-batch marker",
        );
    }

    /// **D3 — non-reentrant host-batch guard (discriminating).**
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
    fn run_batch_nested_collapses_inline_and_does_not_reinstall() {
        let _serial = INSTALL_COUNT_LOCK.lock();

        // Coordinator pool narrower than the outer batch width so that a
        // guard-less nested `install` would have to contend for workers
        // the outer fan-out already holds.
        let pool = HostCpuPool::new(2);
        let coord = HostBatchCoordinator::new(&pool);

        let outer: Vec<usize> = (0..8).collect();

        // Observed-once: every nested closure must see `in_host_batch()`
        // true (it is running underneath the outer fan-out). Stored as
        // an AtomicBool that starts true and is AND-folded so any single
        // false observation is permanent.
        let all_nested_saw_in_batch = std::sync::atomic::AtomicBool::new(true);

        let base = coordinator_install_count();

        let outer_out = coord.run_batch(&outer, |o| {
            // Each outer item fans out a nested batch of 4 sub-items.
            let nested_items: Vec<usize> = (0..4).collect();
            let nested_out = coord.run_batch(&nested_items, |n| {
                if !in_host_batch() {
                    all_nested_saw_in_batch.store(false, std::sync::atomic::Ordering::Relaxed);
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

        let installs = coordinator_install_count() - base;

        // (3) Exactly ONE install for the whole nested tree — the guard
        // collapsed all 8 nested fan-outs inline.
        assert_eq!(
            installs, 1,
            "non-reentrant guard must collapse nested run_batch inline: expected exactly 1 \
             coordinator-pool install (the outer fan-out), got {installs}. A guard-less \
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
            all_nested_saw_in_batch.load(std::sync::atomic::Ordering::Relaxed),
            "every nested closure must run with the in-batch marker set (inline under the \
             outer fan-out)",
        );
    }
}
