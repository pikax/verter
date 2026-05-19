//! Map + retention-budget pairs for the `SemanticGraphStore`'s
//! `SemanticNodeId`-keyed side caches.
//!
//! The relation memo and the Vue-macro resolved-named-type identity map
//! are each a `DashMap` plus a [`GlobalRetentionBudget`] that must stay
//! consistent — the budget's FIFO ledger tracks which live map entries
//! exist so it can evict the oldest past the cap. A bulk `clear` that
//! drops the map and the budget as two unsynchronised steps lets a
//! concurrent insert interleave between them — landing a live map entry
//! whose budget admission the clear then erases (or recording an
//! admission for a map entry the clear already dropped). The stranded
//! entry is invisible to FIFO eviction and the cache bound is lost.
//!
//! Each wrapper here owns its map, its budget, AND a lifecycle
//! `retention_gate: RwLock<()>`, and exposes ONLY gated operations:
//! every insert / removal takes the gate read guard across the whole
//! map + budget mutation; `clear` takes the gate write guard across the
//! whole map + budget clear. The invariant — a map and its retention
//! budget are mutated within one lock domain, and `clear` is exclusive
//! against concurrent inserts — is enforced by type structure, so a
//! caller cannot mutate one structure without the other. `DashMap`
//! stays for hot-path per-shard concurrency; the gate is a coarse reset
//! fence, not a hot-path serialiser — concurrent inserts of distinct
//! keys still run in parallel under the shared read guard.

use std::sync::Arc;

use dashmap::DashMap;

use crate::bounded_query_retention::GlobalRetentionBudget;
use crate::semantic_query::{HostResolvedNamedTypeKey, SemanticNodeId};

/// One entry in the relation memo ([`BudgetedRelationMemo`]).
///
/// A relation judgement for a `(source, target)` semantic-node pair is
/// self-version-rooted: `carrier` leads with a self-root `FileWholeHash`
/// for each file-derived input node's originating file, so a content
/// edit to either the source's or the target's file misses the warm
/// relation read. `self_root_canonicals` is the strict self-root
/// canonical set the warm validator checks via
/// [`crate::fact_signature_helpers::ReadSetSignature::validate_with_self_roots`].
#[derive(Clone)]
pub(crate) struct RelationMemoEntry {
    /// The self-version-rooted carrier — built by
    /// `semantic_graph_read_set_signature` from the relation build's
    /// observed self-roots, the traced fact set, and the legacy
    /// `DepSignature` rail.
    pub(crate) carrier: crate::fact_signature_helpers::ReadSetSignature,
    /// The strict self-root canonical set — the file-derived origins of
    /// `source` and `target`.
    pub(crate) self_root_canonicals: Arc<[Arc<str>]>,
    /// The cached tri-state relation result.
    pub(crate) result: crate::semantic_query::RelationResult,
}

/// Relation-engine memo: `(source, target)` semantic-node pairs → a
/// [`RelationMemoEntry`], bounded by a FIFO [`GlobalRetentionBudget`].
///
/// Owns the map, the budget, and the lifecycle `retention_gate`. See
/// the module docs for the map/budget lock-domain invariant.
pub(crate) struct BudgetedRelationMemo {
    memo: DashMap<(SemanticNodeId, SemanticNodeId), RelationMemoEntry>,
    budget: GlobalRetentionBudget<(SemanticNodeId, SemanticNodeId)>,
    /// Lifecycle gate. `insert` takes the read guard across the whole
    /// map + budget mutation; `clear` takes the write guard across the
    /// whole map + budget clear.
    retention_gate: parking_lot::RwLock<()>,
    /// Test-only injection point inside [`Self::clear`], parked between
    /// the map clear and the budget clear with the `retention_gate`
    /// write guard still held. A race test arms it with a barrier and
    /// calls `wait()` twice — see [`Self::test_arm_clear_midpoint_gate`].
    #[cfg(any(test, debug_assertions))]
    clear_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::insert`], parked after
    /// the map insert + budget admission land but before `insert`
    /// returns (with the read guard still held).
    #[cfg(any(test, debug_assertions))]
    insert_post_record_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

impl Default for BudgetedRelationMemo {
    fn default() -> Self {
        Self {
            memo: DashMap::new(),
            budget: GlobalRetentionBudget::default(),
            retention_gate: parking_lot::RwLock::new(()),
            #[cfg(any(test, debug_assertions))]
            clear_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            insert_post_record_gate: parking_lot::Mutex::new(None),
        }
    }
}

impl BudgetedRelationMemo {
    /// Clone the entry stored for `(source, target)` out of its
    /// `DashMap` shard guard. The caller validates the cloned entry
    /// after the shard guard is released so a validator that re-enters
    /// the relation memo cannot deadlock against a held shard guard.
    pub(crate) fn get_cloned(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<RelationMemoEntry> {
        self.memo.get(&(source, target)).map(|e| e.value().clone())
    }

    /// Publish a relation judgement for `(source, target)`.
    ///
    /// Holds the `retention_gate` read guard across the WHOLE map +
    /// budget mutation: the map insert, the new-key budget admission,
    /// and the victim eviction. A concurrent `clear` takes the write
    /// guard, so it cannot interleave its map clear and budget clear
    /// with this insert's two-phase update.
    ///
    /// New-key detection is atomic: `DashMap::insert` returns the prior
    /// value (`Some` on a same-key replace), so the budget admission is
    /// recorded exactly once per distinct key even when two writers
    /// race the same key — a `contains_key`-then-`insert` pair would
    /// let both racing writers observe "absent" and double-record.
    pub(crate) fn insert(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        entry: RelationMemoEntry,
    ) {
        let _retention = self.retention_gate.read();
        let key = (source, target);
        let is_new = self.memo.insert(key, entry).is_none();
        if is_new {
            let seq = crate::bounded_query_retention::next_retention_seq();
            for victim in self.budget.record_admission(seq, key) {
                self.memo.remove(&victim);
            }
        }
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.insert_post_record_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
    }

    /// Drop every relation judgement and its budget ledger.
    ///
    /// Holds the `retention_gate` write guard across BOTH the map clear
    /// and the budget clear, so a concurrent `insert` (read guard)
    /// blocks until this clear completes — no insert can strand a live
    /// map entry whose budget admission this clear erases.
    pub(crate) fn clear(&self) {
        let _retention = self.retention_gate.write();
        self.memo.clear();
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.clear_midpoint_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.budget.clear();
    }

    /// Number of relation memo entries.
    pub(crate) fn len(&self) -> usize {
        self.memo.len()
    }

    /// Test-only accessor for the lifecycle `retention_gate`.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn test_retention_gate(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
    }

    /// Test-only — retention-ledger tracked count.
    #[cfg(test)]
    pub(crate) fn budget_tracked_len(&self) -> usize {
        self.budget.tracked_len()
    }

    /// Test-only driver: arm the [`Self::clear`] injection point. The
    /// next `clear` on this memo calls `barrier.wait()` twice between
    /// the map clear and the budget clear (write guard held).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_clear_midpoint_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.clear_midpoint_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.clear_midpoint_gate,
        }
    }

    /// Test-only driver: arm the [`Self::insert`] injection point. The
    /// next `insert` on this memo calls `barrier.wait()` twice after
    /// its map insert + budget admission land (read guard held).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_insert_post_record_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.insert_post_record_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.insert_post_record_gate,
        }
    }
}

/// Vue-macro resolved-named-type identity map:
/// [`HostResolvedNamedTypeKey`] → [`SemanticNodeId`], bounded by a FIFO
/// [`GlobalRetentionBudget`].
///
/// Owns the map, the budget, and the lifecycle `retention_gate`. See
/// the module docs for the map/budget lock-domain invariant.
pub(crate) struct BudgetedNamedTypeIndex {
    index: DashMap<HostResolvedNamedTypeKey, SemanticNodeId>,
    budget: GlobalRetentionBudget<HostResolvedNamedTypeKey>,
    /// Lifecycle gate. `insert` / `retain_for_canonical` take the read
    /// guard across the whole map + budget mutation; `clear` takes the
    /// write guard across the whole map + budget clear.
    retention_gate: parking_lot::RwLock<()>,
    /// Test-only injection point inside [`Self::clear`] — see
    /// [`Self::test_arm_clear_midpoint_gate`].
    #[cfg(any(test, debug_assertions))]
    clear_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::insert`] — see
    /// [`Self::test_arm_insert_post_record_gate`].
    #[cfg(any(test, debug_assertions))]
    insert_post_record_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

impl Default for BudgetedNamedTypeIndex {
    fn default() -> Self {
        Self {
            index: DashMap::new(),
            budget: GlobalRetentionBudget::default(),
            retention_gate: parking_lot::RwLock::new(()),
            #[cfg(any(test, debug_assertions))]
            clear_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            insert_post_record_gate: parking_lot::Mutex::new(None),
        }
    }
}

impl BudgetedNamedTypeIndex {
    /// Insert the `key → node_id` identity mapping.
    ///
    /// Holds the `retention_gate` read guard across the WHOLE map +
    /// budget mutation: the map insert, the new-key budget admission,
    /// and the victim eviction. A concurrent `clear` takes the write
    /// guard, so it cannot interleave its map clear and budget clear
    /// with this insert's two-phase update.
    ///
    /// New-key detection is atomic: `DashMap::insert` returns the prior
    /// value (`Some` on a same-key replace), so the budget admission is
    /// recorded exactly once per distinct key even when two writers
    /// race the same key.
    pub(crate) fn insert(&self, key: HostResolvedNamedTypeKey, node_id: SemanticNodeId) {
        let _retention = self.retention_gate.read();
        let is_new = self.index.insert(key.clone(), node_id).is_none();
        if is_new {
            let seq = crate::bounded_query_retention::next_retention_seq();
            for victim in self.budget.record_admission(seq, key) {
                self.index.remove(&victim);
            }
        }
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.insert_post_record_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
    }

    /// Look up the [`SemanticNodeId`] for `key`. Refcount-only read —
    /// no gate (a read does not touch the budget).
    pub(crate) fn get(&self, key: &HostResolvedNamedTypeKey) -> Option<SemanticNodeId> {
        self.index.get(key).map(|entry| *entry.value())
    }

    /// Drop every entry whose key's `canonical_id` matches
    /// `canonical_id`; each dropped entry is forgotten from the budget.
    /// Returns the number of entries removed.
    ///
    /// Holds the `retention_gate` read guard across BOTH the map
    /// retention and the per-entry budget `forget`, so a concurrent
    /// `clear` cannot interleave its two clears with this drain.
    pub(crate) fn retain_for_canonical(&self, canonical_id: &str) -> usize {
        let _retention = self.retention_gate.read();
        let mut removed = 0usize;
        let mut forgotten: Vec<HostResolvedNamedTypeKey> = Vec::new();
        self.index.retain(|key, _| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                forgotten.push(key.clone());
                false
            } else {
                true
            }
        });
        for key in &forgotten {
            self.budget.forget(key);
        }
        removed
    }

    /// Drop every entry and its budget ledger.
    ///
    /// Holds the `retention_gate` write guard across BOTH the map clear
    /// and the budget clear, so a concurrent `insert` (read guard)
    /// blocks until this clear completes.
    pub(crate) fn clear(&self) {
        let _retention = self.retention_gate.write();
        self.index.clear();
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.clear_midpoint_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.budget.clear();
    }

    /// Number of resolved-named-type entries.
    pub(crate) fn len(&self) -> usize {
        self.index.len()
    }

    /// Test-only accessor for the lifecycle `retention_gate`.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn test_retention_gate(&self) -> &parking_lot::RwLock<()> {
        &self.retention_gate
    }

    /// Test-only — retention-ledger tracked count.
    #[cfg(test)]
    pub(crate) fn budget_tracked_len(&self) -> usize {
        self.budget.tracked_len()
    }

    /// Test-only driver: arm the [`Self::clear`] injection point. The
    /// next `clear` on this index calls `barrier.wait()` twice between
    /// the map clear and the budget clear (write guard held).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_clear_midpoint_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.clear_midpoint_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.clear_midpoint_gate,
        }
    }

    /// Test-only driver: arm the [`Self::insert`] injection point. The
    /// next `insert` on this index calls `barrier.wait()` twice after
    /// its map insert + budget admission land (read guard held).
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_insert_post_record_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.insert_post_record_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.insert_post_record_gate,
        }
    }
}

/// RAII guard returned by the `test_arm_*` drivers on
/// [`BudgetedRelationMemo`] / [`BudgetedNamedTypeIndex`]. Disarms the
/// per-instance injection point on drop so a later mutation does not
/// park on a stale barrier.
#[cfg(test)]
#[doc(hidden)]
pub(crate) struct BudgetedGateGuard<'a> {
    gate: &'a parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

#[cfg(test)]
impl Drop for BudgetedGateGuard<'_> {
    fn drop(&mut self) {
        *self.gate.lock() = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::RelationResult;
    use std::sync::{Arc as StdArc, Barrier};
    use std::thread;

    fn relation_entry() -> RelationMemoEntry {
        RelationMemoEntry {
            carrier: crate::fact_signature_helpers::ReadSetSignature::empty(),
            self_root_canonicals: StdArc::from(Vec::<StdArc<str>>::new()),
            result: RelationResult::Unknown,
        }
    }

    /// `insert` records a budget admission exactly once per distinct
    /// key. Re-inserting the same key replaces in place and does NOT
    /// grow the ledger — proves the atomic `DashMap::insert`-return
    /// new-key detection. DISCRIMINATES against a `contains_key`-then-
    /// `insert` shape that could double-count under same-key writers.
    #[test]
    fn relation_memo_insert_records_admission_once_per_key() {
        let memo = BudgetedRelationMemo::default();
        let a = SemanticNodeId(1);
        let b = SemanticNodeId(2);
        memo.insert(a, b, relation_entry());
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.budget_tracked_len(), 1, "first insert recorded once");
        // Re-insert the SAME key — replace in place, no new admission.
        memo.insert(a, b, relation_entry());
        assert_eq!(memo.len(), 1, "same key does not grow the map");
        assert_eq!(
            memo.budget_tracked_len(),
            1,
            "re-insert of the same key must NOT record a second admission",
        );
        // A distinct key records a fresh admission.
        memo.insert(a, SemanticNodeId(3), relation_entry());
        assert_eq!(memo.budget_tracked_len(), 2, "distinct key recorded");
    }

    /// MAP / BUDGET DESYNC RACE (relation memo, clear side) — an
    /// in-flight `clear` must hold the `retention_gate` write guard
    /// across BOTH its map clear and budget clear, so a concurrent
    /// `insert` cannot land an entry + admission straddling the two
    /// clears.
    ///
    /// Deterministic. The invalidator is parked, via the `clear`
    /// injection point, BETWEEN the map clear and the budget clear —
    /// with the write guard still held. With `clear` pinned there the
    /// test asserts `retention_gate.try_read()` is `None`: an `insert`
    /// reaching `retention_gate.read()` right now WOULD block.
    ///
    /// DISCRIMINATES. Against an un-gated `clear` (write guard removed)
    /// `try_read()` succeeds (`Some`) and the assertion FAILS — a
    /// concurrent `insert` could interleave between the map clear and
    /// the budget clear, stranding a live entry with no budget record.
    /// With the gate `try_read()` returns `None` and the assertion
    /// PASSES.
    #[test]
    fn relation_memo_inflight_clear_engages_gate_against_insert() {
        let memo = StdArc::new(BudgetedRelationMemo::default());
        memo.insert(SemanticNodeId(1), SemanticNodeId(2), relation_entry());

        let clear_parked = StdArc::new(Barrier::new(2));
        let _guard = memo.test_arm_clear_midpoint_gate(StdArc::clone(&clear_parked));

        let memo_clear = StdArc::clone(&memo);
        let invalidator = thread::spawn(move || memo_clear.clear());

        // `clear` has cleared the map and parked at its midpoint, still
        // holding the `retention_gate` write guard.
        clear_parked.wait();
        assert!(
            memo.test_retention_gate().try_read().is_none(),
            "MAP/BUDGET DESYNC: an in-flight relation-memo `clear` does \
             NOT hold the retention write guard — a concurrent `insert` \
             could land an entry + admission between the map clear and \
             the budget clear. `clear` must hold `retention_gate.write()` \
             across both clears.",
        );
        clear_parked.wait();
        invalidator.join().expect("invalidator thread");
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.budget_tracked_len(), 0, "map and budget consistent");
    }

    /// MAP / BUDGET DESYNC RACE (relation memo, insert side) — an
    /// in-flight `insert` must hold the `retention_gate` read guard
    /// across its whole map-insert + budget-admission, so a concurrent
    /// `clear` cannot interleave its two clears.
    ///
    /// Deterministic. The `insert` is parked, via the `insert`
    /// injection point, AFTER its map insert + budget admission land
    /// but before it returns — read guard still held. The test asserts
    /// `retention_gate.try_write()` is `None`: a `clear` reaching
    /// `retention_gate.write()` right now WOULD block.
    ///
    /// DISCRIMINATES. Against an un-gated `insert` (read guard removed)
    /// `try_write()` succeeds (`Some`) and the assertion FAILS. With
    /// the gate `try_write()` returns `None` and the assertion PASSES.
    #[test]
    fn relation_memo_inflight_insert_engages_gate_against_clear() {
        let memo = StdArc::new(BudgetedRelationMemo::default());

        let insert_parked = StdArc::new(Barrier::new(2));
        let guard = memo.test_arm_insert_post_record_gate(StdArc::clone(&insert_parked));

        let memo_insert = StdArc::clone(&memo);
        let inserter = thread::spawn(move || {
            memo_insert.insert(SemanticNodeId(5), SemanticNodeId(6), relation_entry());
        });

        // `insert` has landed its map insert + budget admission and
        // parked, still holding the `retention_gate` read guard.
        insert_parked.wait();
        assert!(
            memo.test_retention_gate().try_write().is_none(),
            "MAP/BUDGET DESYNC: an in-flight relation-memo `insert` does \
             NOT hold the retention gate — a concurrent `clear` could \
             interleave its map/budget clears with the insert's two-phase \
             update. `insert` must hold `retention_gate.read()` across \
             the whole map+budget mutation.",
        );
        insert_parked.wait();
        inserter.join().expect("inserter thread");
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.budget_tracked_len(), 1);

        drop(guard);
        // Reset fence, not a permanent block.
        memo.clear();
        assert_eq!(memo.len(), 0);
        assert_eq!(memo.budget_tracked_len(), 0);
    }

    /// MAP / BUDGET DESYNC RACE (named-type index, clear side) —
    /// mirror of the relation-memo clear-side test for the second
    /// `SemanticGraphStore` pair.
    ///
    /// DISCRIMINATES identically: an un-gated `clear` leaves
    /// `try_read()` returning `Some` (assertion FAILS); the gated
    /// `clear` holds the write guard so `try_read()` is `None`
    /// (assertion PASSES).
    /// Build a distinct [`HostResolvedNamedTypeKey`] for index tests.
    fn named_type_key(canonical: &str, name: &str) -> super::HostResolvedNamedTypeKey {
        use verter_compiler::utils::oxc::vue::resolve_type::cache_keys::ResolvedNamedTypeCacheKey;
        super::HostResolvedNamedTypeKey {
            canonical_id: StdArc::from(canonical),
            whole_hash: [0u8; 16],
            inner: ResolvedNamedTypeCacheKey {
                name: name.as_bytes().to_vec().into_boxed_slice(),
                surface: None,
                base_offset: 0,
                companion_cache_key: StdArc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
                type_param_bindings: StdArc::from(Vec::new().into_boxed_slice()),
            },
        }
    }

    /// `insert` records a budget admission exactly once per distinct
    /// key — the named-type-index counterpart of the relation-memo
    /// atomic-new-key test. DISCRIMINATES against a `contains_key`-then-
    /// `insert` shape.
    #[test]
    fn named_type_index_insert_records_admission_once_per_key() {
        let index = BudgetedNamedTypeIndex::default();
        let key = named_type_key("/w/a.vue", "Props");
        index.insert(key.clone(), SemanticNodeId(1));
        assert_eq!(index.budget_tracked_len(), 1, "first insert recorded once");
        index.insert(key, SemanticNodeId(2));
        assert_eq!(
            index.budget_tracked_len(),
            1,
            "re-insert of the same key must NOT record a second admission",
        );
        index.insert(named_type_key("/w/b.vue", "Emits"), SemanticNodeId(3));
        assert_eq!(index.budget_tracked_len(), 2, "distinct key recorded");
    }

    /// MAP / BUDGET DESYNC RACE (named-type index, clear side) —
    /// mirror of the relation-memo clear-side test for the second
    /// `SemanticGraphStore` pair.
    ///
    /// DISCRIMINATES identically: an un-gated `clear` leaves
    /// `try_read()` returning `Some` (assertion FAILS); the gated
    /// `clear` holds the write guard so `try_read()` is `None`
    /// (assertion PASSES).
    #[test]
    fn named_type_index_inflight_clear_engages_gate_against_insert() {
        let index = StdArc::new(BudgetedNamedTypeIndex::default());
        index.insert(named_type_key("/w/a.vue", "Props"), SemanticNodeId(1));

        let clear_parked = StdArc::new(Barrier::new(2));
        let _guard = index.test_arm_clear_midpoint_gate(StdArc::clone(&clear_parked));

        let index_clear = StdArc::clone(&index);
        let invalidator = thread::spawn(move || index_clear.clear());

        clear_parked.wait();
        assert!(
            index.test_retention_gate().try_read().is_none(),
            "MAP/BUDGET DESYNC: an in-flight named-type-index `clear` \
             does NOT hold the retention write guard — a concurrent \
             `insert` could land an entry + admission between the map \
             clear and the budget clear.",
        );
        clear_parked.wait();
        invalidator.join().expect("invalidator thread");
        assert_eq!(index.len(), 0);
        assert_eq!(index.budget_tracked_len(), 0);
    }

    /// MAP / BUDGET DESYNC RACE (named-type index, insert side) —
    /// mirror of the relation-memo insert-side test.
    ///
    /// DISCRIMINATES: an un-gated `insert` leaves `try_write()`
    /// returning `Some` (assertion FAILS); the gated `insert` holds the
    /// read guard so `try_write()` is `None` (assertion PASSES).
    #[test]
    fn named_type_index_inflight_insert_engages_gate_against_clear() {
        let index = StdArc::new(BudgetedNamedTypeIndex::default());

        let insert_parked = StdArc::new(Barrier::new(2));
        let guard = index.test_arm_insert_post_record_gate(StdArc::clone(&insert_parked));

        let index_insert = StdArc::clone(&index);
        let key = named_type_key("/w/c.vue", "Model");
        let inserter = thread::spawn(move || index_insert.insert(key, SemanticNodeId(9)));

        insert_parked.wait();
        assert!(
            index.test_retention_gate().try_write().is_none(),
            "MAP/BUDGET DESYNC: an in-flight named-type-index `insert` \
             does NOT hold the retention gate — a concurrent `clear` \
             could interleave its map/budget clears with the insert's \
             two-phase update.",
        );
        insert_parked.wait();
        inserter.join().expect("inserter thread");
        assert_eq!(index.len(), 1);
        assert_eq!(index.budget_tracked_len(), 1);

        drop(guard);
        index.clear();
        assert_eq!(index.len(), 0);
        assert_eq!(index.budget_tracked_len(), 0);
    }
}
