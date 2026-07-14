//! Map + retention-budget pairing for the `SemanticGraphStore`'s
//! `SemanticNodeId`-keyed relation memo.
//!
//! The relation memo is a `DashMap` plus a [`GlobalRetentionBudget`]
//! that must stay
//! consistent — the budget's FIFO ledger tracks which live map entries
//! exist so it can evict the oldest past the cap. A bulk `clear` that
//! drops the map and the budget as two unsynchronised steps lets a
//! concurrent insert interleave between them — landing a live map entry
//! whose budget admission the clear then erases (or recording an
//! admission for a map entry the clear already dropped). The stranded
//! entry is invisible to FIFO eviction and the cache bound is lost.
//!
//! The wrapper here owns its map, its budget, AND a lifecycle
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
use crate::semantic_query::RelateMemoKey;

/// One entry in the relation memo ([`BudgetedRelationMemo`]).
///
/// A relation judgement for a [`RelateMemoKey`] (the full relation identity,
/// not the bare `(source, target)` pair) is
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
    /// observed self-roots and the traced fact set.
    pub(crate) carrier: crate::fact_signature_helpers::ReadSetSignature,
    /// The strict self-root canonical set — the file-derived origins of
    /// `source` and `target`.
    pub(crate) self_root_canonicals: Arc<[Arc<str>]>,
    /// The cached tri-state relation result.
    pub(crate) result: crate::semantic_query::RelationResult,
    /// Project generation this judgement was computed under,
    /// snapshotted by the relation engine before `decide_relation`
    /// dispatched any work. The `carrier` validates only file-content
    /// whole-hashes; a `ProjectGeneration` reset (tsconfig /
    /// path-alias / SDK / workspace-folder change) bumps no file
    /// content, so without this stamp a `clear_relation_memo` racing
    /// `relate_nodes` could land a stale-by-project-generation
    /// judgement whose carrier still validates on file-content terms.
    /// Every read-side gate ([`super::SemanticGraphStore::get_relation`])
    /// rejects the entry when `validated_at_generation` differs from
    /// the live
    /// [`crate::project_type_store::ProjectTypeStore::current_project_generation`].
    pub(crate) validated_at_generation: u64,
    /// FIFO retention-ledger admission identity — the unique sequence
    /// number this entry's key was first recorded under. A budget FIFO
    /// victim carries its admission seq; the victim removal is scoped to
    /// it (`remove_if` on `admission_seq`) so a concurrent same-key
    /// re-`insert`, which carries a distinct seq, survives. Allocated on
    /// the vacant-slot path and carried forward unchanged across a
    /// same-key replace (the ledger record is never removed on a
    /// replace, so the stored seq must stay paired with it).
    admission_seq: u64,
}

/// Relation-engine memo: full-identity [`RelateMemoKey`]s → a
/// [`RelationMemoEntry`], bounded by a FIFO [`GlobalRetentionBudget`].
///
/// Keyed by the FULL relation identity (source / target / relation kind /
/// policy / source freshness / inference context / env), NOT the bare
/// `(source, target)` pair — two judgements over the same nodes that differ in
/// any identity axis occupy distinct slots.
///
/// Owns the map, the budget, and the lifecycle `retention_gate`. See
/// the module docs for the map/budget lock-domain invariant.
pub(crate) struct BudgetedRelationMemo {
    memo: DashMap<RelateMemoKey, RelationMemoEntry>,
    budget: GlobalRetentionBudget<RelateMemoKey>,
    /// Lifecycle gate. `insert` takes the read guard across the whole
    /// map + budget mutation; `clear` takes the write guard across the
    /// whole map + budget clear.
    retention_gate: parking_lot::RwLock<()>,
    /// Write-side consistency-domain lock. `insert` holds it across the
    /// `DashMap::entry` new-vs-replace decision, the `record_admission`,
    /// and the returned-victim `remove_if` — so the map mutation and the
    /// budget mutation are one structurally-serialised write step. The
    /// substrate is sound TODAY without it (`DashMap::entry` makes
    /// same-key new-vs-replace atomic and victim removals are
    /// identity-scoped by `admission_seq`), but this lock makes the
    /// single-write-domain invariant structural, so a future edit
    /// cannot silently split the map and budget mutations into
    /// separately-raced critical sections. Reads (`get_cloned`) do NOT
    /// take it — `DashMap` read concurrency is preserved. Lock order:
    /// `retention_gate (read) → admission_lock → DashMap shard → budget
    /// Mutex`.
    admission_lock: parking_lot::Mutex<()>,
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
            admission_lock: parking_lot::Mutex::new(()),
            #[cfg(any(test, debug_assertions))]
            clear_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            insert_post_record_gate: parking_lot::Mutex::new(None),
        }
    }
}

impl BudgetedRelationMemo {
    /// Clone the entry stored for `key` out of its
    /// `DashMap` shard guard. The caller validates the cloned entry
    /// after the shard guard is released so a validator that re-enters
    /// the relation memo cannot deadlock against a held shard guard.
    pub(crate) fn get_cloned(&self, key: &RelateMemoKey) -> Option<RelationMemoEntry> {
        self.memo.get(key).map(|e| e.value().clone())
    }

    /// Publish a relation judgement for `key`.
    ///
    /// Holds the `retention_gate` read guard across the WHOLE map +
    /// budget mutation: the map insert, the new-key budget admission,
    /// and the victim eviction. A concurrent `clear` takes the write
    /// guard, so it cannot interleave its map clear and budget clear
    /// with this insert's two-phase update.
    ///
    /// New-key detection is atomic: the slot is taken through the
    /// `DashMap::entry` API, so the "vacant vs occupied" decision AND
    /// the slot write happen under one held shard write guard. The
    /// budget admission is therefore recorded exactly once per distinct
    /// key even when two writers race the same key — a
    /// `contains_key`-then-`insert` pair would let both racing writers
    /// observe "absent" and double-record.
    ///
    /// The `admission_lock` is held across the `DashMap::entry`
    /// decision, the `record_admission`, and the returned-victim
    /// `remove_if`, so the map mutation and the budget mutation are one
    /// structurally-serialised write step (see the field docs).
    ///
    /// On a same-key replace the entry keeps the PRIOR admission's
    /// `admission_seq` — the FIFO ledger still tracks the original
    /// admission, so the stored seq must stay paired with the ledger
    /// record it identifies. A fresh seq on a replace would desync the
    /// entry from its ledger record and leak it past a budget removal.
    pub(crate) fn insert(
        &self,
        key: RelateMemoKey,
        carrier: crate::fact_signature_helpers::ReadSetSignature,
        self_root_canonicals: Arc<[Arc<str>]>,
        result: crate::semantic_query::RelationResult,
        validated_at_generation: u64,
    ) {
        use dashmap::mapref::entry::Entry;

        let _retention = self.retention_gate.read();
        // Single write-side consistency domain — held across the
        // `DashMap::entry` decision, the `record_admission`, and the
        // victim `remove_if`.
        let _admission = self.admission_lock.lock();
        // Decide new-vs-replace and write the slot atomically under the
        // shard write guard the `entry` API holds. `admitted_seq` is
        // `Some(seq)` only when this call freshly admitted the key — on
        // a same-key replace the prior seq is carried forward (the
        // ledger record was never removed) and no admission is recorded.
        // `RelateMemoKey` is not `Copy`; clone it for the `entry` slot so
        // the original stays owned for the `record_admission` below (only
        // reached on the vacant path).
        let admitted_seq = match self.memo.entry(key.clone()) {
            Entry::Occupied(mut occ) => {
                let admission_seq = occ.get().admission_seq;
                occ.insert(RelationMemoEntry {
                    carrier,
                    self_root_canonicals,
                    result,
                    validated_at_generation,
                    admission_seq,
                });
                None
            }
            Entry::Vacant(vac) => {
                let admission_seq = crate::bounded_query_retention::next_retention_seq();
                vac.insert(RelationMemoEntry {
                    carrier,
                    self_root_canonicals,
                    result,
                    validated_at_generation,
                    admission_seq,
                });
                Some(admission_seq)
            }
        };
        if let Some(seq) = admitted_seq {
            // New key: record the admission and FIFO-evict the oldest
            // entries past the cap. The victim removal is identity-scoped
            // — `remove_if` drops the `victim_key` entry ONLY when its
            // stored `admission_seq` still equals `victim_seq`. A
            // concurrent `insert` re-admitting the same key carries a
            // fresh seq, so a bare-key removal would evict that fresh
            // entry and strand its ledger record.
            for (victim_seq, victim_key) in self.budget.record_admission(seq, key) {
                self.memo
                    .remove_if(&victim_key, |_, entry| entry.admission_seq == victim_seq);
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

    /// Test-only accessor for the write-side `admission_lock`. An
    /// engagement test parks an `insert` mid-flight (via the
    /// `insert_post_record_gate`, which sits inside the `admission_lock`
    /// span) and uses `try_lock()` on this to assert the in-flight
    /// `insert` holds the lock across its map + budget mutation.
    #[cfg(test)]
    #[doc(hidden)]
    pub(crate) fn test_admission_lock(&self) -> &parking_lot::Mutex<()> {
        &self.admission_lock
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

/// RAII guard returned by the `test_arm_*` drivers on
/// [`BudgetedRelationMemo`]. Disarms the
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
    use crate::semantic_query::{RelateMemoKey, RelationContext, RelationResult, SemanticNodeId};
    use std::sync::{Arc as StdArc, Barrier};
    use std::thread;

    /// Publish one relation judgement for the assignability identity of
    /// `(source, target)` with empty carrier / self-roots and an `Unknown`
    /// result — the minimal entry the budget/desync tests admit. Distinct
    /// `(source, target)` pairs map to distinct [`RelateMemoKey`]s.
    fn insert_relation(
        memo: &BudgetedRelationMemo,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) {
        memo.insert(
            RelateMemoKey::assignable(source, target, RelationContext::default()),
            crate::fact_signature_helpers::ReadSetSignature::empty(),
            StdArc::from(Vec::<StdArc<str>>::new()),
            RelationResult::Unknown,
            0,
        );
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
        insert_relation(&memo, a, b);
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.budget_tracked_len(), 1, "first insert recorded once");
        // Re-insert the SAME key — replace in place, no new admission.
        insert_relation(&memo, a, b);
        assert_eq!(memo.len(), 1, "same key does not grow the map");
        assert_eq!(
            memo.budget_tracked_len(),
            1,
            "re-insert of the same key must NOT record a second admission",
        );
        // A distinct key records a fresh admission.
        insert_relation(&memo, a, SemanticNodeId(3));
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
        insert_relation(&memo, SemanticNodeId(1), SemanticNodeId(2));

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
            insert_relation(&memo_insert, SemanticNodeId(5), SemanticNodeId(6));
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

    /// WRITE-SIDE CONSISTENCY DOMAIN (relation memo) — an in-flight
    /// `insert` holds the `admission_lock` across its `DashMap::entry`
    /// decision, its `record_admission`, and its victim `remove_if`, so
    /// the map mutation and the budget mutation are one
    /// structurally-serialised write step.
    ///
    /// Deterministic. The `insert` is parked, via the
    /// `insert_post_record_gate` injection point, AFTER its map insert +
    /// budget admission land but before it returns — a point INSIDE the
    /// `admission_lock` span. The test asserts
    /// `test_admission_lock().try_lock()` is `None`.
    ///
    /// DISCRIMINATES on the hardening. With the `admission_lock` held
    /// across the write-side map + budget mutation, `try_lock()` returns
    /// `None` and the assertion PASSES. Were the `admission_lock`
    /// removed there would be no write-side lock to engage — the map
    /// `DashMap::entry` and the budget `record_admission` would be two
    /// separately-raceable critical sections, the exact structural gap
    /// this hardening closes; `try_lock()` would then succeed (`Some`)
    /// and the assertion FAILS.
    #[test]
    fn relation_memo_inflight_insert_engages_admission_lock() {
        let memo = StdArc::new(BudgetedRelationMemo::default());

        let insert_parked = StdArc::new(Barrier::new(2));
        let guard = memo.test_arm_insert_post_record_gate(StdArc::clone(&insert_parked));

        let memo_insert = StdArc::clone(&memo);
        let inserter = thread::spawn(move || {
            insert_relation(&memo_insert, SemanticNodeId(11), SemanticNodeId(12));
        });

        // `insert` has landed its map insert + budget admission and
        // parked, still inside its `admission_lock` span.
        insert_parked.wait();
        assert!(
            memo.test_admission_lock().try_lock().is_none(),
            "WRITE-SIDE CONSISTENCY DOMAIN: an in-flight relation-memo \
             `insert` must hold the `admission_lock` across its \
             `DashMap::entry` decision, its `record_admission`, and its \
             victim `remove_if` — the map mutation and the budget \
             mutation must be one structurally-serialised write step so \
             a future edit cannot split them into separately-raced \
             critical sections.",
        );
        insert_parked.wait();
        inserter.join().expect("inserter thread");
        assert_eq!(memo.len(), 1);
        assert_eq!(memo.budget_tracked_len(), 1);

        drop(guard);
        // The `admission_lock` is free again once `insert` returned.
        assert!(
            memo.test_admission_lock().try_lock().is_some(),
            "the admission_lock is released when `insert` completes",
        );
    }
}
