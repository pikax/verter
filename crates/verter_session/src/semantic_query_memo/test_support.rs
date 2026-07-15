//! Test-only observability surface for [`SemanticGraphStore`].
//!
//! The methods and free functions here exist solely so the in-crate
//! `tests.rs` and the integration-test crate under
//! `crates/verter_session/tests/` can deterministically drive and observe
//! the cooperative-admission machinery (in-flight aborts, joiner
//! admission strong-counts, the per-entry condvar pairing, and the
//! per-store cold-abort trigger) WITHOUT loosening the production public
//! API of `SemanticGraphStore`.
//!
//! They are co-located in this sibling module — rather than inline in
//! `mod.rs` — so the hot-path memo logic in the parent stays under the
//! Tier-2 module-size budget. Each item is `#[doc(hidden)]` and reached
//! only through the `for_tests` re-export shim; the methods access the
//! parent type's private fields directly because a descendant module can
//! see an ancestor module's private items.

use super::*;

impl SemanticGraphStore {
    /// Test-only driver: set `aborted = true` on the in-flight entry
    /// for `key`, plant an `Error(Other)` sentinel on `completed` if
    /// absent, notify waiters, and remove the entry from the table.
    /// Mirrors `invalidate_canonical` exactly but bypasses the step 1
    /// warm-slot gate so joiner-retry tests don't have to race a real
    /// invalidation window between publish and inflight retirement.
    ///
    /// Returns `true` when an entry for `key` was aborted, `false` when
    /// the in-flight table did not contain the key.
    ///
    /// `#[doc(hidden)]` and reached only through the `for_tests`
    /// re-export shim (`crate::for_tests::test_trigger_inflight_abort`)
    /// so the integration-test surface in
    /// `crates/verter_session/tests/` can drive joiner retry without
    /// loosening the public API of `SemanticGraphStore`. In-crate
    /// tests reach the same body via the same shim function.
    #[doc(hidden)]
    pub fn test_trigger_inflight_abort_impl(&self, key: &SemanticQueryKey) -> bool {
        let mut table = self.inflight.lock();
        let Some(inflight) = table.remove(key) else {
            return false;
        };
        drop(table);
        {
            let mut state = inflight.state.lock();
            state.aborted = true;
            if state.completed.is_none() {
                state.completed = Some(QueryResult::Error(QueryError::Other(Arc::from(
                    "aborted by test_trigger_inflight_abort",
                ))));
                state.dep_signature = Some(empty_signature());
            }
        }
        inflight.ready.notify_all();
        true
    }

    /// Test-only observability accessor: non-destructively read the
    /// `Arc::strong_count` of the in-flight entry for `key`, or `0` if
    /// the table has no entry.
    ///
    /// Joiner-retry tests use this to deterministically synchronise:
    /// each caller of `execute_cooperative` clones the entry's `Arc`
    /// (step 3: `table.entry(key).or_insert_with(...).clone()`). While
    /// only the cold winner is mid-build, three references are live —
    /// the table entry, the winner's `inflight` local, and the
    /// `InflightPanicGuard`'s clone — so the count is `3`; an admitted
    /// joiner raises it to `4`. Polling this to `> 3` replaces a
    /// wall-clock `sleep` that races the joiner under parallel test
    /// load (test hermeticity) — it never touches the entry's state,
    /// so it cannot perturb the build it observes.
    ///
    /// `#[doc(hidden)]` and reached only through the `for_tests`
    /// re-export shim, mirroring `test_trigger_inflight_abort`.
    #[doc(hidden)]
    #[must_use]
    pub fn test_inflight_strong_count(&self, key: &SemanticQueryKey) -> usize {
        let table = self.inflight.lock();
        table.get(key).map_or(0, Arc::strong_count)
    }

    /// Test-only observability accessor: the number of cooperative
    /// joiners that have reached the point of suspending on a per-entry
    /// `ready` condvar (the increment fires immediately before
    /// `wait_while` in [`SemanticGraphStore::execute_cooperative`]).
    ///
    /// A condvar-pairing test polls this to `>= 1` to observe a joiner
    /// genuinely PARKED on the condvar — strictly stronger than
    /// [`Self::test_inflight_strong_count`], which rises one step earlier
    /// when the joiner merely clones the in-flight `Arc` (before it
    /// reaches the condvar). It never touches the entry's `state`, so it
    /// cannot perturb the build it observes. The accessor and the counter
    /// it reads are both `cfg`-gated to `test` / `debug_assertions` and
    /// are absent from release builds.
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    #[must_use]
    pub fn test_joiner_on_condvar_count(&self) -> usize {
        self.joiner_on_condvar_count
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Test-only probe: `true` when the `inflight` table `Mutex` can be
    /// `try_lock`-acquired right now (no thread is holding it). The
    /// abort-loop lock-order test uses it to assert
    /// [`SemanticGraphStore::invalidate_all`] does not hold the table lock
    /// while locking each collected entry's `state` (the
    /// collect-then-release order).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_inflight_table_is_unlocked(&self) -> bool {
        self.inflight.try_lock().is_some()
    }

    /// Public test driver: set this store's per-store cold-abort
    /// trigger for the duration of the returned guard so the next
    /// `execute_cooperative` cold-build on **this store**
    /// deterministically hits the TOCTOU abort path. Used by
    /// integration tests in `crates/verter_session/tests/` that drive
    /// the counter-helper plumbing.
    ///
    /// The trigger is scoped to the store the guard borrows — a test
    /// forcing an abort affects only its own store, never a
    /// concurrently-running unrelated test's store. The guard restores
    /// the flag to `false` on drop. Tests must hold the guard for the
    /// duration of the `execute_cooperative` call.
    #[doc(hidden)]
    #[must_use]
    pub fn test_force_cold_abort_sweep(&self) -> TestForceColdAbortGuard<'_> {
        self.force_cold_abort_sweep.store(true, Ordering::SeqCst);
        TestForceColdAbortGuard {
            flag: &self.force_cold_abort_sweep,
        }
    }
}

/// Public test driver: build an empty `DepSignature` for tests in the
/// integration-test crate that drive `execute_cooperative` directly.
/// The integration-test surface is not part of the production resolver
/// stack — its only job is to discriminate per-request counter
/// attribution, so an empty signature is sufficient.
#[doc(hidden)]
#[allow(dead_code)]
#[must_use]
pub fn empty_signature_for_tests() -> DepSignature {
    empty_signature()
}

/// Public test driver: trigger an in-flight abort for `key` on `store`.
/// Forwards to [`SemanticGraphStore::test_trigger_inflight_abort_impl`]
/// so integration tests in `crates/verter_session/tests/` and in-crate
/// `tests.rs` drive the same joiner-retry body through one call site.
#[doc(hidden)]
#[allow(dead_code)]
pub fn test_trigger_inflight_abort(store: &SemanticGraphStore, key: &SemanticQueryKey) -> bool {
    store.test_trigger_inflight_abort_impl(key)
}

/// Public test driver: read the in-flight entry's `Arc` strong count
/// for `key` on `store`. Forwards to
/// [`SemanticGraphStore::test_inflight_strong_count`] so joiner-retry
/// tests deterministically poll for joiner admission instead of
/// sleeping. See that method's docs for the strong-count contract
/// (`3` while only the winner is mid-build, `4` once a joiner joins).
#[doc(hidden)]
#[allow(dead_code)]
#[must_use]
pub fn test_inflight_strong_count(store: &SemanticGraphStore, key: &SemanticQueryKey) -> usize {
    store.test_inflight_strong_count(key)
}

/// RAII guard returned by
/// [`SemanticGraphStore::test_force_cold_abort_sweep`]. Borrows the
/// driving store's per-store
/// [`force_cold_abort_sweep`](SemanticGraphStore::force_cold_abort_sweep)
/// flag and restores it to `false` on drop, so a panicking test does
/// not leak the trigger onto a later `execute_cooperative` on the same
/// store. The trigger never reaches another store, so sibling tests
/// running in parallel are unaffected regardless.
#[doc(hidden)]
pub struct TestForceColdAbortGuard<'a> {
    flag: &'a std::sync::atomic::AtomicBool,
}

impl Drop for TestForceColdAbortGuard<'_> {
    fn drop(&mut self) {
        self.flag.store(false, Ordering::SeqCst);
    }
}
