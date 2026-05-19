//! Test-only injection-point arming drivers for [`SemanticGraphStore`].
//!
//! `SemanticGraphStore` carries a set of `cfg(test, debug_assertions)`
//! barrier injection points inside `invalidate_all`, the warm-publish
//! path, the cold-winner path, and the resolved-named-type insert path.
//! A race test arms one of these points with a [`std::sync::Barrier`],
//! drives a concurrent operation into the resulting window, and asserts
//! a lock-domain / ordering invariant. The arming drivers and the RAII
//! disarm guard live here, split out of the `mod.rs` production module
//! so test scaffolding does not count against that module's size
//! budget.
//!
//! Each driver returns a [`TestInvalidateAllGateGuard`] (or, for the
//! resolved-named-type gate, a `BudgetedGateGuard`) that disarms the
//! per-store injection point on drop, so a later operation on the same
//! store cannot park on a stale barrier. Every gate is per-store, never
//! a process-global, so a test arming one cannot perturb a concurrent
//! unrelated test running on its own store.

use std::sync::Arc;

use super::SemanticGraphStore;

#[cfg(any(test, debug_assertions))]
impl SemanticGraphStore {
    /// Test-only driver: arm the [`Self::invalidate_all`] post-`entries`-
    /// clear injection point with `barrier`. The next `invalidate_all` on
    /// **this store** calls `barrier.wait()` right after releasing the
    /// `entries` lock that performed the in-flight abort + memo clear, so
    /// a test holding the other `barrier` party can deterministically
    /// drive a cold winner's `warm_publish_one` against that tail.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped, so
    /// it cannot park a concurrent unrelated test's `invalidate_all`.
    #[doc(hidden)]
    #[must_use]
    pub fn test_invalidate_all_post_entries_clear_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.invalidate_all_post_entries_clear_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.invalidate_all_post_entries_clear_gate,
        }
    }

    /// Test-only driver: arm the [`Self::invalidate_all`] pre-`memo_budget`-
    /// clear injection point with `barrier`. The next `invalidate_all` on
    /// **this store** calls `barrier.wait()` TWICE right before the
    /// `memo_budget` clear and — in final-state code — with the `entries`
    /// lock that performed `entries.clear()` still held, so a test can
    /// assert (via `entries.try_lock()`) that the `entries` + `memo_budget`
    /// clears run in one lock domain.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_invalidate_all_pre_memo_budget_clear_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.invalidate_all_pre_memo_budget_clear_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.invalidate_all_pre_memo_budget_clear_gate,
        }
    }

    /// Test-only driver: arm the warm-slot-publish post-`memo_budget`-
    /// record injection point with `barrier`. The next publish on **this
    /// store** that records a fresh family admission calls `barrier.wait()`
    /// TWICE right after the `memo_budget` admission lands and — in
    /// final-state code — with the `entries` lock still held, so a test
    /// can assert (via `entries.try_lock()`) that the `entries` publish
    /// and the `memo_budget` admission run in one lock domain.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_publish_post_memo_budget_record_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.publish_post_memo_budget_record_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.publish_post_memo_budget_record_gate,
        }
    }

    /// Test-only driver: arm the [`Self::execute_cooperative`] cold-winner
    /// pre-prefix-backfill injection point with `barrier`. The next
    /// cold-winner publish on **this store** calls `barrier.wait()` TWICE
    /// after `warm_publish_one` published the parent and before the
    /// prefix-backfill loop runs, so a test can run `invalidate_all` in
    /// that window and assert the winner's backfills are skipped.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_cold_winner_pre_backfill_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.cold_winner_pre_backfill_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.cold_winner_pre_backfill_gate,
        }
    }

    /// Test-only driver: arm the [`Self::invalidate_all`]
    /// pre-`canonical_to_entries`-clear injection point with `barrier`.
    /// The next `invalidate_all` on **this store** calls `barrier.wait()`
    /// TWICE right before the `canonical_to_entries` reverse-index clear
    /// and — in final-state code — with the `entries` lock that performed
    /// `entries.clear()` still held, so a test can assert (via
    /// `entries.try_lock()`) that the reverse-index clear runs in the same
    /// `entries` lock domain as the `entries` + `memo_budget` clears.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_invalidate_all_pre_reverse_index_clear_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.invalidate_all_pre_reverse_index_clear_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.invalidate_all_pre_reverse_index_clear_gate,
        }
    }

    /// Test-only driver: arm the [`Self::record_family_admission_locked`]
    /// post-reverse-index-prune injection point with `barrier`. The next
    /// publish on **this store** that records a fresh family admission
    /// AND FIFO-evicts at least one budget victim calls `barrier.wait()`
    /// TWICE right after the evicted victims' `canonical_to_entries`
    /// reverse-index registrations are pruned and — in final-state code —
    /// with the `entries` lock still held, so a test can assert (via
    /// `entries.try_lock()`) that the victim's reverse-index pruning runs
    /// in the same `entries` lock domain as the victim's `entries`
    /// removal.
    ///
    /// The returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_publish_post_reverse_index_prune_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.publish_post_reverse_index_prune_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.publish_post_reverse_index_prune_gate,
        }
    }

    /// Test-only driver: arm the [`Self::invalidate_all`] in-flight-abort
    /// injection point. The next `invalidate_all` on **this store** calls
    /// `barrier.wait()` TWICE while iterating the collected entry handles
    /// and locking each `state`, with the `inflight` table lock NOT held,
    /// so a test can assert (via `inflight.try_lock()`) the
    /// collect-then-release lock order. The returned guard disarms the
    /// gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_invalidate_all_inflight_abort_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.invalidate_all_inflight_abort_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.invalidate_all_inflight_abort_gate,
        }
    }

    /// Test-only driver: arm the [`Self::invalidate_canonical`]
    /// in-flight-abort injection point. The next `invalidate_canonical` on
    /// **this store** calls `barrier.wait()` TWICE while iterating the
    /// collected entry handles and locking each `state`, with the
    /// `inflight` table lock NOT held, so a test can assert (via
    /// `inflight.try_lock()`) that `invalidate_canonical` honours the same
    /// collect-then-release lock order as [`Self::invalidate_all`]. The
    /// returned guard disarms the gate on drop. Per-store scoped.
    #[doc(hidden)]
    #[must_use]
    pub fn test_invalidate_canonical_inflight_abort_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> TestInvalidateAllGateGuard<'_> {
        *self.invalidate_canonical_inflight_abort_gate.lock() = Some(barrier);
        TestInvalidateAllGateGuard {
            gate: &self.invalidate_canonical_inflight_abort_gate,
        }
    }
}

/// RAII guard returned by the per-store test injection-point arming
/// drivers on [`SemanticGraphStore`]. Clears the per-store gate it
/// borrows on drop so a later operation on the same store does not park
/// on a stale barrier.
#[cfg(any(test, debug_assertions))]
#[doc(hidden)]
pub struct TestInvalidateAllGateGuard<'a> {
    pub(super) gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(any(test, debug_assertions))]
impl Drop for TestInvalidateAllGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}
