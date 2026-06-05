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
use crate::semantic_query::{HostResolvedNamedTypeKey, RelateMemoKey, SemanticNodeId};

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

/// One entry in the resolved-named-type identity map
/// ([`BudgetedNamedTypeIndex`]).
///
/// `node_id` is the interned [`SemanticNodeId`] the key resolves to.
/// `admission_seq` is the FIFO retention-ledger admission identity — the
/// unique sequence number this entry was recorded under when its key
/// first entered the map. Every budget removal is scoped to that exact
/// seq (`forget_seq`), so a per-canonical drain that races a concurrent
/// `insert` re-admitting the same key forgets only the stale entry's
/// ledger record and never the fresh re-admission's (which carries a
/// different seq). Carrying the seq on the entry is the established
/// substrate pattern — see [`crate::component_meta_caches`]'s
/// `MaterializeStructureEntry` / `RefCycleEntry`.
#[derive(Clone, Copy)]
pub(crate) struct NamedTypeIndexEntry {
    /// The interned semantic node id the key resolves to.
    pub(crate) node_id: SemanticNodeId,
    /// FIFO retention-ledger admission identity — unique per admission,
    /// survives a same-key re-admission.
    admission_seq: u64,
}

/// Vue-macro resolved-named-type identity map:
/// [`HostResolvedNamedTypeKey`] → [`NamedTypeIndexEntry`], bounded by a
/// FIFO [`GlobalRetentionBudget`].
///
/// Owns the map, the budget, the lifecycle `retention_gate`, AND the
/// resolved-named-type reset epoch (`generation`). See the module docs
/// for the map/budget lock-domain invariant.
///
/// **Reset-epoch / insert atomicity.** A macro-resolution build aborted
/// by a project-generation reset keeps running until its next abort
/// check, so it can perform a straggler `insert` after the reset. The
/// reset epoch fences that straggler out, but the fence only holds if
/// the epoch check and the map insert are atomic against the epoch bump
/// and the map clear. This wrapper makes them atomic by serialising all
/// four under the one `retention_gate`:
///
/// - [`Self::clear_and_bump_generation`] takes `retention_gate.write()`
///   and, under that ONE guard, clears the map, clears the budget, AND
///   bumps `generation` — bump + clear are one critical section.
/// - [`Self::insert_if_generation_matches`] takes `retention_gate.read()`
///   and, under ONE continuously-held read guard, reads the live epoch,
///   compares it to the caller's frozen snapshot, and — only on a match
///   — performs the map insert. The read guard is never released between
///   the check and the insert.
///
/// Because `retention_gate` is an `RwLock` (read and write mutually
/// exclude), a straggler insert (read guard held across check+insert) is
/// fully ordered against a concurrent clear+bump (write guard held): it
/// runs entirely before the write section — it inserts, then the clear
/// empties the map, no survival — or entirely after it — its in-guard
/// epoch read sees the bumped epoch and the insert is rejected. There is
/// no interleaving in which a stale entry survives.
pub(crate) struct BudgetedNamedTypeIndex {
    index: DashMap<HostResolvedNamedTypeKey, NamedTypeIndexEntry>,
    budget: GlobalRetentionBudget<HostResolvedNamedTypeKey>,
    /// Monotonic resolved-named-type reset epoch. Bumped ONLY by
    /// [`Self::clear_and_bump_generation`], inside the SAME
    /// `retention_gate.write()` critical section that clears the map and
    /// the budget. Read by [`Self::generation`] and — co-located with
    /// the insert under one `retention_gate.read()` guard — by
    /// [`Self::insert_if_generation_matches`]. Owning the counter inside
    /// this wrapper makes the "every bump is under the gate write guard,
    /// every fence read is under the gate read guard" invariant
    /// structural: a caller cannot bump or fence-check the epoch outside
    /// the gate.
    generation: std::sync::atomic::AtomicU64,
    /// Lifecycle gate. `insert` / `insert_if_generation_matches` /
    /// `retain_for_canonical` take the read guard across the whole map +
    /// budget mutation; `clear` / `clear_and_bump_generation` take the
    /// write guard across the whole map + budget clear (+ epoch bump).
    retention_gate: parking_lot::RwLock<()>,
    /// Write-side consistency-domain lock. `insert` holds it across the
    /// `DashMap::entry` new-vs-replace decision, the `record_admission`,
    /// and the returned-victim `remove_if`; `retain_for_canonical` holds
    /// it across the `index` `retain` removal AND the per-entry
    /// `forget_seq` loop — so every write-side path mutates the map and
    /// the budget as one structurally-serialised step. The substrate is
    /// sound TODAY without it (`DashMap::entry` makes same-key
    /// new-vs-replace atomic and every victim / drained removal is
    /// identity-scoped by `admission_seq`), but this lock makes the
    /// single-write-domain invariant structural, so a future edit
    /// cannot silently split the map and budget mutations into
    /// separately-raced critical sections. Reads (`get`) do NOT take it
    /// — `DashMap` read concurrency is preserved. Lock order:
    /// `retention_gate (read) → admission_lock → DashMap shard → budget
    /// Mutex`.
    admission_lock: parking_lot::Mutex<()>,
    /// Test-only injection point inside [`Self::clear`] — see
    /// [`Self::test_arm_clear_midpoint_gate`].
    #[cfg(any(test, debug_assertions))]
    clear_midpoint_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::insert`] — see
    /// [`Self::test_arm_insert_post_record_gate`].
    #[cfg(any(test, debug_assertions))]
    insert_post_record_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside [`Self::retain_for_canonical`],
    /// parked AFTER the stale `index` entries are removed but BEFORE
    /// their budget records are forgotten — with the `retention_gate`
    /// read guard still held. A race test arms it to drive a concurrent
    /// `insert` (re-admitting a just-removed key) into that gap and
    /// assert the per-entry `forget_seq` does not clobber the fresh
    /// admission. See [`Self::test_arm_retain_pre_forget_gate`].
    #[cfg(any(test, debug_assertions))]
    retain_pre_forget_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
    /// Test-only injection point inside
    /// [`Self::insert_if_generation_matches`], parked AFTER the in-guard
    /// epoch read + epoch comparison but BEFORE the map insert — with
    /// the `retention_gate` read guard STILL held. A race test arms it
    /// to park a straggler insert in that window and run a concurrent
    /// [`Self::clear_and_bump_generation`]; the test then proves the
    /// `RwLock` mutual-exclusion fully orders the two, so no stale entry
    /// survives. See [`Self::test_arm_insert_post_epoch_check_gate`].
    #[cfg(any(test, debug_assertions))]
    insert_post_epoch_check_gate: parking_lot::Mutex<Option<Arc<std::sync::Barrier>>>,
}

impl Default for BudgetedNamedTypeIndex {
    fn default() -> Self {
        Self {
            index: DashMap::new(),
            budget: GlobalRetentionBudget::default(),
            generation: std::sync::atomic::AtomicU64::new(0),
            retention_gate: parking_lot::RwLock::new(()),
            admission_lock: parking_lot::Mutex::new(()),
            #[cfg(any(test, debug_assertions))]
            clear_midpoint_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            insert_post_record_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            retain_pre_forget_gate: parking_lot::Mutex::new(None),
            #[cfg(any(test, debug_assertions))]
            insert_post_epoch_check_gate: parking_lot::Mutex::new(None),
        }
    }
}

impl BudgetedNamedTypeIndex {
    /// Reset-epoch-fenced insert: record `key → node_id` ONLY when the
    /// caller's frozen epoch snapshot still equals the live reset epoch.
    ///
    /// Returns `true` when the entry was inserted, `false` when the
    /// snapshot is stale (nothing is recorded).
    ///
    /// ## Atomic against [`Self::clear_and_bump_generation`] — no window
    ///
    /// This method holds ONE `retention_gate.read()` guard across the
    /// epoch read, the epoch comparison, AND the map insert — it never
    /// releases the guard between the check and the insert.
    /// `clear_and_bump_generation` holds `retention_gate.write()` across
    /// its map clear, budget clear, and epoch bump. `RwLock` read and
    /// write guards mutually exclude, so a straggler insert and a
    /// concurrent clear+bump cannot interleave:
    ///
    /// - **Straggler acquires the read guard before clear+bump acquires
    ///   the write guard.** The straggler reads the still-old epoch
    ///   under its guard, matches, and inserts. clear+bump then blocks
    ///   until the read guard drops, takes the write guard, and clears
    ///   the map — the just-inserted entry is dropped. No survival.
    /// - **clear+bump acquires the write guard before the straggler
    ///   acquires the read guard.** clear+bump clears the map and bumps
    ///   the epoch, then releases the write guard. The straggler then
    ///   takes the read guard, reads the BUMPED epoch, the snapshot no
    ///   longer matches, and the insert is rejected — nothing recorded.
    ///
    /// There is no third case: a reader and a writer can never hold the
    /// gate simultaneously, so the straggler's check+insert runs wholly
    /// before or wholly after the clear+bump. In every interleaving the
    /// stale entry is either cleared or never inserted.
    pub(crate) fn insert_if_generation_matches(
        &self,
        key: HostResolvedNamedTypeKey,
        node_id: SemanticNodeId,
        observed_generation: u64,
    ) -> bool {
        // ONE continuously-held read guard across the epoch check AND
        // the map insert. A concurrent `clear_and_bump_generation`
        // (write guard) is fully ordered against this whole span.
        let _retention = self.retention_gate.read();
        // In-guard epoch read — `Acquire` pairs with the `Release` bump
        // in `clear_and_bump_generation`.
        if self.generation.load(std::sync::atomic::Ordering::Acquire) != observed_generation {
            // Snapshot superseded — the bump landed (and the map was
            // cleared) before this guard was acquired. Reject; record
            // nothing.
            return false;
        }
        // Test-only injection point: parked AFTER the in-guard epoch
        // check but BEFORE the map insert, with the `retention_gate`
        // read guard STILL held. A race test arms it to prove the gate
        // orders a parked straggler against a concurrent clear+bump.
        // `None` (the production default) is a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.insert_post_epoch_check_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        self.insert_under_read_guard(key, node_id);
        true
    }

    /// Map + budget write step, run by [`Self::insert_if_generation_matches`]
    /// with the `retention_gate` read guard ALREADY held by the caller.
    ///
    /// Takes the `admission_lock` across the `DashMap::entry`
    /// new-vs-replace decision, the `record_admission`, and the
    /// returned-victim `remove_if`, so the map mutation and the budget
    /// mutation are one structurally-serialised write step.
    ///
    /// New-key detection is atomic: the slot is taken through the
    /// `DashMap::entry` API, so the "vacant vs occupied" decision AND
    /// the slot write happen under one held shard write guard.
    ///
    /// On a same-key replace the entry keeps the PRIOR admission's
    /// `admission_seq` — the FIFO ledger still tracks the original
    /// admission, so the stored seq must stay equal to the ledger record
    /// it identifies.
    fn insert_under_read_guard(&self, key: HostResolvedNamedTypeKey, node_id: SemanticNodeId) {
        use dashmap::mapref::entry::Entry;

        // Single write-side consistency domain — held across the
        // `DashMap::entry` decision, the `record_admission`, and the
        // victim `remove_if`.
        let _admission = self.admission_lock.lock();
        // Decide new-vs-replace and write the slot atomically under the
        // shard write guard the `entry` API holds. `admitted_seq` is
        // `Some(seq)` only when this call freshly admitted the key — on
        // a same-key replace the prior seq is carried forward (the
        // ledger record was never removed) and no admission is recorded.
        let admitted_seq = match self.index.entry(key.clone()) {
            Entry::Occupied(mut occ) => {
                // Same-key replace: keep the prior entry's seq so the
                // stored entry stays paired with its live ledger record.
                let admission_seq = occ.get().admission_seq;
                occ.insert(NamedTypeIndexEntry {
                    node_id,
                    admission_seq,
                });
                None
            }
            Entry::Vacant(vac) => {
                let admission_seq = crate::bounded_query_retention::next_retention_seq();
                vac.insert(NamedTypeIndexEntry {
                    node_id,
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
            // entry and strand its ledger record; scoping to the seq
            // leaves the fresh re-admission intact.
            for (victim_seq, victim_key) in self.budget.record_admission(seq, key) {
                self.index
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

    /// Current resolved-named-type reset epoch. A macro-resolution build
    /// snapshots this when its adapter is constructed and threads the
    /// snapshot back into [`Self::insert_if_generation_matches`].
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(std::sync::atomic::Ordering::Acquire)
    }

    /// Look up the [`SemanticNodeId`] for `key`. Refcount-only read —
    /// no gate (a read does not touch the budget).
    pub(crate) fn get(&self, key: &HostResolvedNamedTypeKey) -> Option<SemanticNodeId> {
        self.index.get(key).map(|entry| entry.value().node_id)
    }

    /// Drop every entry whose key's `canonical_id` matches
    /// `canonical_id`; each dropped entry is forgotten from the budget.
    /// Returns the number of entries removed.
    ///
    /// Holds the `retention_gate` read guard across BOTH the map
    /// retention and the per-entry budget removal, so a concurrent
    /// `clear` cannot interleave its two clears with this drain.
    ///
    /// **Removal-identity invariant.** Each dropped entry is forgotten
    /// from the budget by its own `admission_seq` (`forget_seq`), NOT by
    /// a key-wide `forget`. Scoping the removal to each stale entry's
    /// captured `admission_seq` leaves a concurrently re-admitted fresh
    /// entry's distinct ledger record intact. Mirrors the `forget_seq`
    /// identity scoping in [`crate::component_meta_caches`]'s
    /// per-canonical drains.
    ///
    /// **Single write-side consistency domain.** The `admission_lock` is
    /// held across BOTH the `index` `retain` removal AND the per-entry
    /// `forget_seq` loop, so this drain mutates the map and the budget
    /// as one structurally-serialised step — an `insert` (which also
    /// takes `admission_lock`) cannot interleave a re-admission of a
    /// just-removed key between the `index` removal and the budget
    /// removal. The per-entry `forget_seq` identity scoping above is the
    /// removal-identity defence; the `admission_lock` makes the
    /// single-write-domain invariant structural.
    pub(crate) fn retain_for_canonical(&self, canonical_id: &str) -> usize {
        let _retention = self.retention_gate.read();
        // Single write-side consistency domain — held across the `index`
        // `retain` removal AND the per-entry `forget_seq` loop.
        let _admission = self.admission_lock.lock();
        let mut removed = 0usize;
        // Capture each removed entry's OWN admission seq so the budget
        // removal can be scoped to that exact ledger record.
        let mut forgotten_seqs: Vec<u64> = Vec::new();
        self.index.retain(|key, entry| {
            if key.canonical_id.as_ref() == canonical_id {
                removed += 1;
                forgotten_seqs.push(entry.admission_seq);
                false
            } else {
                true
            }
        });
        // Test-only injection point — parked AFTER the stale `index`
        // entries are removed but BEFORE their ledger records are
        // forgotten, with the `retention_gate` read guard still held. A
        // race test arms it so a concurrent `insert` re-admits one of
        // the just-removed keys into this gap; the per-entry `forget_seq`
        // below must then forget only the STALE seq and leave the fresh
        // re-admission's record counted. `None` (production default) is
        // a no-op.
        #[cfg(any(test, debug_assertions))]
        {
            let gate = self.retain_pre_forget_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        for seq in forgotten_seqs {
            self.budget.forget_seq(seq);
        }
        removed
    }

    /// Drop every entry and its budget ledger.
    ///
    /// Holds the `retention_gate` write guard across BOTH the map clear
    /// and the budget clear, so a concurrent `insert` (read guard)
    /// blocks until this clear completes.
    ///
    /// Does NOT bump the reset epoch — used by per-canonical eviction
    /// paths that drop the whole map without superseding a project
    /// generation. A project-generation reset uses
    /// [`Self::clear_and_bump_generation`] instead.
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

    /// Project-generation reset: drop every entry, drop the budget
    /// ledger, AND advance the resolved-named-type reset epoch — all
    /// under ONE `retention_gate.write()` critical section.
    ///
    /// Bumping the epoch inside the same write section that clears the
    /// map is what makes the reset-epoch fence airtight. A straggler
    /// [`Self::insert_if_generation_matches`] holds `retention_gate.read()`
    /// across its epoch check + map insert; `RwLock` read/write mutual
    /// exclusion then orders the straggler wholly before this write
    /// section (its entry is cleared) or wholly after it (its in-guard
    /// epoch read sees the bumped epoch and the insert is rejected).
    /// There is no interleaving in which a stale entry survives — see
    /// `insert_if_generation_matches`'s no-window argument.
    ///
    /// `Release` on the bump pairs with the `Acquire` load in
    /// `insert_if_generation_matches` / [`Self::generation`].
    pub(crate) fn clear_and_bump_generation(&self) {
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
        // Advance the reset epoch under the SAME write guard as the
        // clears above — bump + clear are one atomic critical section.
        self.generation
            .fetch_add(1, std::sync::atomic::Ordering::Release);
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

    /// Test-only accessor for the write-side `admission_lock`. An
    /// engagement test parks an `insert` / `retain_for_canonical`
    /// mid-flight (via an injection point inside the `admission_lock`
    /// span) and uses `try_lock()` on this to assert the in-flight write
    /// holds the lock across its map + budget mutation.
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

    /// Test-only driver: arm the [`Self::retain_for_canonical`]
    /// injection point. The next `retain_for_canonical` on this index
    /// calls `barrier.wait()` twice AFTER it removes the stale `index`
    /// entries but BEFORE it forgets their budget records (read guard
    /// held). A race test uses this to interleave a concurrent `insert`
    /// that re-admits a just-removed key and prove the per-entry
    /// `forget_seq` does not clobber the fresh admission.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_retain_pre_forget_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.retain_pre_forget_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.retain_pre_forget_gate,
        }
    }

    /// Test-only driver: arm the [`Self::insert_if_generation_matches`]
    /// injection point. The next `insert_if_generation_matches` on this
    /// index calls `barrier.wait()` twice AFTER its in-guard epoch check
    /// passes but BEFORE its map insert (read guard held). A race test
    /// uses this to park a straggler insert between its epoch check and
    /// its map insert and prove a concurrent `clear_and_bump_generation`
    /// is fully ordered against the parked straggler.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) fn test_arm_insert_post_epoch_check_gate(
        &self,
        barrier: Arc<std::sync::Barrier>,
    ) -> BudgetedGateGuard<'_> {
        *self.insert_post_epoch_check_gate.lock() = Some(barrier);
        BudgetedGateGuard {
            gate: &self.insert_post_epoch_check_gate,
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
    use crate::semantic_query::{RelateMemoKey, RelationContext, RelationResult};
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
        resolve_env_hash: Default::default(),
        type_env_hash: Default::default(),
        lib_env_hash: Default::default(),
        project_identity: 0,
            inner: ResolvedNamedTypeCacheKey {
                name: name.as_bytes().to_vec().into_boxed_slice(),
                surface: None,
                base_offset: 0,
                from_root_body: true,
                companion_cache_key: StdArc::from(Vec::<Box<[u8]>>::new().into_boxed_slice()),
                type_param_bindings: StdArc::from(Vec::new().into_boxed_slice()),
            },
        }
    }

    /// `insert_if_generation_matches` records a budget admission exactly
    /// once per distinct key — the named-type-index counterpart of the
    /// relation-memo atomic-new-key test. DISCRIMINATES against a
    /// `contains_key`-then-`insert` shape.
    #[test]
    fn named_type_index_insert_records_admission_once_per_key() {
        let index = BudgetedNamedTypeIndex::default();
        let g = index.generation();
        let key = named_type_key("/w/a.vue", "Props");
        assert!(index.insert_if_generation_matches(key.clone(), SemanticNodeId(1), g));
        assert_eq!(index.budget_tracked_len(), 1, "first insert recorded once");
        assert!(index.insert_if_generation_matches(key, SemanticNodeId(2), g));
        assert_eq!(
            index.budget_tracked_len(),
            1,
            "re-insert of the same key must NOT record a second admission",
        );
        assert!(index.insert_if_generation_matches(
            named_type_key("/w/b.vue", "Emits"),
            SemanticNodeId(3),
            g,
        ));
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
        let g = index.generation();
        index.insert_if_generation_matches(
            named_type_key("/w/a.vue", "Props"),
            SemanticNodeId(1),
            g,
        );

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
        let g = index.generation();
        let inserter = thread::spawn(move || {
            index_insert.insert_if_generation_matches(key, SemanticNodeId(9), g)
        });

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

    /// WRITE-SIDE CONSISTENCY DOMAIN (named-type index) —
    /// `retain_for_canonical` and a concurrent `insert` of the same key
    /// must NOT interleave: the `admission_lock` makes the map removal +
    /// budget removal one structurally-serialised write step, so an
    /// `insert` (which also takes `admission_lock`) cannot re-admit a
    /// just-removed key between the `index` removal and the budget
    /// `forget_seq`.
    ///
    /// Deterministic. `retain_for_canonical("/w/a.vue")` removes the
    /// stale entry for `("/w/a.vue", "Props")` from `index`, then parks
    /// at its pre-`forget` injection point — `index` entry gone, budget
    /// record not yet forgotten, `retention_gate` read guard AND
    /// `admission_lock` both still held. A concurrent `insert` of the
    /// SAME key is started on a separate thread; it blocks on
    /// `admission_lock` until the retain pass releases it.
    ///
    /// DISCRIMINATES on the write-side lock domain. With the retain pass
    /// parked inside its `admission_lock` span,
    /// `test_admission_lock().try_lock()` returns `None` — the lock is
    /// engaged across the removal + `forget_seq`. Were the hardening
    /// removed there would be no `admission_lock` to engage and the
    /// concurrent `insert` could interleave the gap. After the retain
    /// pass is released the two operations serialise and the substrate
    /// ends consistent: the retain pass forgot exactly the stale entry's
    /// seq (`forget_seq`, identity-scoped), the `insert` then re-admitted
    /// the key, and the ledger holds exactly one record for the one live
    /// entry.
    ///
    /// `budget_tracked_len()` is exactly the count
    /// `GlobalRetentionBudget::record_admission` compares against the
    /// cap, so `budget_tracked_len() == len()` is a fully FIFO-tracked
    /// map.
    #[test]
    fn named_type_index_retain_for_canonical_serialises_concurrent_insert() {
        let index = StdArc::new(BudgetedNamedTypeIndex::default());
        let key = named_type_key("/w/a.vue", "Props");
        let g = index.generation();

        // Seed the stale entry the retain pass will remove.
        index.insert_if_generation_matches(key.clone(), SemanticNodeId(1), g);
        assert_eq!(index.len(), 1);
        assert_eq!(index.budget_tracked_len(), 1, "stale entry admitted");

        // Park `retain_for_canonical` AFTER it removed the stale `index`
        // entry but BEFORE it forgets the stale budget record — inside
        // its `admission_lock` span.
        let retain_parked = StdArc::new(Barrier::new(2));
        let guard = index.test_arm_retain_pre_forget_gate(StdArc::clone(&retain_parked));

        let index_retain = StdArc::clone(&index);
        let retainer = thread::spawn(move || index_retain.retain_for_canonical("/w/a.vue"));

        // The retain pass has removed the stale entry from `index` and
        // parked at its pre-`forget` midpoint.
        retain_parked.wait();
        assert_eq!(
            index.len(),
            0,
            "retain pass removed the stale entry from the map",
        );

        // THE DISCRIMINATOR — with the retain pass parked inside its
        // `admission_lock` span, the write-side lock is engaged.
        assert!(
            index.test_admission_lock().try_lock().is_none(),
            "WRITE-SIDE CONSISTENCY DOMAIN: `retain_for_canonical` must \
             hold the `admission_lock` across BOTH its `index` removal \
             and its per-entry `forget_seq` loop — so a concurrent \
             `insert` cannot interleave a re-admission of a just-removed \
             key between the two. The map and budget mutations are one \
             structurally-serialised write step.",
        );

        // Disarm the injection point so the spawned `insert` does not
        // park, then start it on a separate thread — it blocks on
        // `admission_lock` until the retain pass releases it.
        drop(guard);
        let index_insert = StdArc::clone(&index);
        let key_insert = key.clone();
        let inserter = thread::spawn(move || {
            index_insert.insert_if_generation_matches(key_insert, SemanticNodeId(2), g)
        });

        // Release the retain pass — it runs its budget `forget_seq`,
        // drops `admission_lock`; the blocked `insert` then proceeds.
        retain_parked.wait();
        let removed = retainer.join().expect("retainer thread");
        assert_eq!(removed, 1, "retain pass removed exactly the stale entry");
        inserter.join().expect("inserter thread");

        // The two operations serialised; the substrate ends consistent.
        assert_eq!(
            index.len(),
            1,
            "the `insert` re-admitted the key after the retain pass",
        );
        assert_eq!(
            index.budget_tracked_len(),
            1,
            "the ledger holds exactly one record for the one live entry \
             — the retain pass forgot exactly the stale seq (forget_seq, \
             identity-scoped) and the `insert` re-admitted under a fresh \
             seq, so map and budget stay consistent",
        );
        assert_eq!(
            index.get(&key),
            Some(SemanticNodeId(2)),
            "the surviving entry is the re-admitted one",
        );
    }

    /// RESET-EPOCH FENCE ATOMICITY (named-type index) — a straggler
    /// `insert_if_generation_matches` must hold the `retention_gate` read
    /// guard CONTINUOUSLY across its in-guard epoch check AND its map
    /// insert, so the check+insert is one critical section atomic against
    /// a concurrent `clear_and_bump_generation`'s write-guarded map clear
    /// + epoch bump.
    ///
    /// A macro-resolution build aborted by a project-generation reset can
    /// straggle and perform a late `insert_if_generation_matches`. The
    /// epoch fence rejects it ONLY if the epoch read and the map insert
    /// are atomic against the bump+clear; a non-atomic check-then-insert
    /// (read guard released between the check and the insert) lets the
    /// straggler pass the check, lose the race to `clear_and_bump_generation`,
    /// and then land its stale entry AFTER the clear — the entry survives
    /// the reset.
    ///
    /// Deterministic. The straggler is parked, via the
    /// `insert_post_epoch_check_gate` injection point, AFTER its in-guard
    /// epoch check passes but BEFORE its map insert.
    ///
    /// DISCRIMINATES on the atomicity. With the parked straggler still
    /// holding ONE continuously-held `retention_gate.read()` guard across
    /// check+insert, `test_retention_gate().try_write()` returns `None` —
    /// a `clear_and_bump_generation` reaching `retention_gate.write()`
    /// right now WOULD block. Were the check and the insert two
    /// separately-guarded sections (read guard released between them),
    /// the parked straggler would hold NO guard, `try_write()` would
    /// succeed (`Some`), and the assertion FAILS — the exact non-atomic
    /// shape the fence forbids. The test then runs a real
    /// `clear_and_bump_generation` concurrently and proves the straggler's
    /// entry does NOT survive the reset and the next stale-snapshot
    /// insert is rejected.
    #[test]
    fn named_type_index_insert_epoch_check_atomic_against_clear_bump() {
        let index = StdArc::new(BudgetedNamedTypeIndex::default());
        // The straggler's frozen epoch snapshot — the live (pre-reset)
        // epoch, exactly what a macro-resolution build records.
        let straggler_epoch = index.generation();
        let key = named_type_key("/w/aborted.vue", "Props");

        // Park `insert_if_generation_matches` AFTER its in-guard epoch
        // check passes but BEFORE its map insert — `retention_gate` read
        // guard still held.
        let straggler_parked = StdArc::new(Barrier::new(2));
        let gate_guard =
            index.test_arm_insert_post_epoch_check_gate(StdArc::clone(&straggler_parked));

        let index_straggler = StdArc::clone(&index);
        let key_straggler = key.clone();
        let straggler = thread::spawn(move || {
            index_straggler.insert_if_generation_matches(
                key_straggler,
                SemanticNodeId(1),
                straggler_epoch,
            )
        });

        // The straggler has passed its in-guard epoch check and parked,
        // still holding the `retention_gate` read guard.
        straggler_parked.wait();

        // THE DISCRIMINATOR — the parked straggler holds ONE
        // continuously-held read guard across its epoch check AND its map
        // insert, so a concurrent `clear_and_bump_generation`'s
        // `retention_gate.write()` would block right now.
        assert!(
            index.test_retention_gate().try_write().is_none(),
            "RESET-EPOCH FENCE NON-ATOMIC: a straggler parked between its \
             in-guard epoch check and its map insert does NOT hold the \
             `retention_gate` read guard — the check and the insert are \
             two separately-guarded sections, so a concurrent \
             `clear_and_bump_generation` can interleave its map clear + \
             epoch bump between them and the straggler's stale entry \
             survives the reset. `insert_if_generation_matches` must hold \
             ONE continuously-held `retention_gate.read()` guard across \
             the epoch check AND the map insert.",
        );

        // Run a real project-generation reset (`clear_and_bump_generation`)
        // on a second thread. It must take `retention_gate.write()` — it
        // blocks behind the straggler's still-held read guard.
        let index_reset = StdArc::clone(&index);
        let resetter = thread::spawn(move || index_reset.clear_and_bump_generation());

        // Release the straggler: it completes its map insert under the
        // read guard it still holds, then drops the guard. Only THEN can
        // `clear_and_bump_generation` take the write guard and clear the
        // map — dropping the just-inserted entry.
        straggler_parked.wait();

        let straggler_inserted = straggler.join().expect("straggler thread");
        resetter.join().expect("resetter thread");
        // Disarm the injection point — the straggler has finished; the
        // stale-snapshot insert below rejects at the epoch check before
        // the gate, but disarm anyway so nothing can park.
        drop(gate_guard);

        // The straggler's `insert_if_generation_matches` ran under the
        // still-old epoch (its in-guard read saw the pre-bump value, so
        // it inserted), but `clear_and_bump_generation`'s subsequent
        // write-guarded clear dropped that entry. The stale entry does
        // NOT survive the reset.
        assert!(
            straggler_inserted,
            "the straggler inserted under the still-old epoch (in-guard \
             read saw the pre-bump value)",
        );
        assert_eq!(
            index.len(),
            0,
            "RESET-EPOCH FENCE: the straggler's stale entry must NOT \
             survive the project-generation reset — `clear_and_bump_generation`'s \
             write-guarded map clear, fully ordered after the straggler's \
             read-guarded insert, drops it.",
        );
        assert_eq!(
            index.budget_tracked_len(),
            0,
            "map and budget both cleared by the reset",
        );
        assert_ne!(
            index.generation(),
            straggler_epoch,
            "`clear_and_bump_generation` advanced the reset epoch",
        );

        // The fence now rejects an insert carrying the superseded
        // snapshot: the in-guard epoch read sees the bumped epoch.
        let stale_rejected = index.insert_if_generation_matches(
            named_type_key("/w/aborted2.vue", "Emits"),
            SemanticNodeId(2),
            straggler_epoch,
        );
        assert!(
            !stale_rejected,
            "an insert carrying the pre-reset epoch snapshot must be \
             rejected after `clear_and_bump_generation` advanced the epoch",
        );
        assert_eq!(index.len(), 0, "the rejected insert recorded nothing");

        // A current-generation insert (fresh snapshot) is still accepted.
        let fresh = index.insert_if_generation_matches(
            named_type_key("/w/fresh.vue", "Model"),
            SemanticNodeId(3),
            index.generation(),
        );
        assert!(
            fresh,
            "a current-generation insert (fresh epoch snapshot) is \
             accepted — the fence rejects only superseded snapshots",
        );
        assert_eq!(index.len(), 1, "the fresh insert landed");
    }
}
