//! The family-memo GLOBAL retention step.
//!
//! `FamilySlots::publish` bounds candidates WITHIN one family; this
//! module bounds the number of FAMILIES the memo retains at all. A
//! family that newly enters `entries` records one ledger admission
//! against the shared `GlobalRetentionBudget`, and once the ledger
//! exceeds its cap the oldest families are evicted from `entries` and
//! their `canonical_to_entries` registrations pruned — every step under
//! the caller's `entries` hold, so the three-member consistency cluster
//! (`entries`, `memo_budget`, `canonical_to_entries`) moves as one.
//!
//! Two entries over one shared victim-eviction tail: the single-family
//! form every ordinary publish uses, and the exempting GROUP form the
//! batched SCC member publish uses so a component's own admissions can
//! never select the component's own root or siblings as their FIFO
//! victims.

use super::*;

impl SemanticGraphStore {
    /// Record a newly-admitted family against the memo retention
    /// budget and FIFO-evict the oldest families once the family count
    /// exceeds the budget cap. Evicting a still-valid family only forces
    /// a recompute; it never yields an incorrect result.
    ///
    /// Called exactly once per family that newly enters the `entries`
    /// memo (a re-publish into a different slot of an already-present
    /// family does not re-record).
    ///
    /// **Family-memo consistency-cluster fence.** The caller passes the
    /// `entries` guard it is already holding — the `memo_budget`
    /// admission is recorded, the FIFO victims are removed from
    /// `entries`, AND each evicted victim's `canonical_to_entries`
    /// reverse-index registrations are pruned, ALL WITHIN that same lock
    /// hold. The family memo is a three-member consistency cluster
    /// (`entries`, `memo_budget`, `canonical_to_entries`); a concurrent
    /// [`Self::invalidate_all`] clears all three under the SAME `entries`
    /// lock, so the publish's `(entries slot, memo_budget record,
    /// reverse-index register)` triple is atomic against the reset.
    ///
    /// Pruning the victim's reverse index here — rather than deferring it
    /// past the `entries`-lock drop — closes the race where a fresh
    /// same-`(family, slot)` re-publish registers into
    /// `canonical_to_entries` between the victim's `entries` removal and
    /// a deferred key-only reverse-index cleanup, which would delete the
    /// fresh registration and leave the live re-published memo slot
    /// invisible to `invalidate_canonical`. With the prune inside the
    /// `entries` lock — and the publish-side `register_reverse_index`
    /// also under that lock — no concurrent publish can register a fresh
    /// `(family, slot)` in the gap, so there is no gap. The `entries →
    /// canonical_to_entries shards` lock order PERMITS taking a shard
    /// mutex while `entries` is held (`entries` is outermost), and no
    /// path takes a `canonical_to_entries` shard mutex then `entries`, so
    /// this nesting is sound.
    ///
    /// The injection point armed by
    /// [`Self::test_publish_post_memo_budget_record_gate`] fires after
    /// the admission lands; the one armed by
    /// [`Self::test_publish_post_reverse_index_prune_gate`] fires after
    /// the victims' reverse-index registrations are pruned — both with
    /// the `entries` lock still held.
    pub(super) fn record_family_admission_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        family: &FamilyKey,
    ) {
        let seq = crate::bounded_query_retention::next_retention_seq();
        let victims = self.memo_budget.record_admission(seq, family.clone());
        self.evict_family_victims_locked(entries, victims);
    }

    /// Record a GROUP of newly-admitted families as ONE retention-budget
    /// step, exempting every family the group needs to stay resident from
    /// victim selection.
    ///
    /// This is the entry the batched SCC member publish uses. A component
    /// is only coherent whole: its members are warm-readable only while
    /// the root candidate they were fenced on is still resident, and a
    /// member published beside an evicted sibling is the same torn
    /// component the root-witness fence forbids. Recording the whole
    /// group in one call — with the root and every member family exempt —
    /// removes the per-member admission that could otherwise select the
    /// group's own root (or an earlier member of the same group) as its
    /// FIFO victim. See
    /// [`crate::bounded_query_retention::GlobalRetentionBudget::record_admissions_exempt`]
    /// for the substrate contract, including the caller's obligation to
    /// refuse a group larger than the budget cap.
    ///
    /// Everything else — the consistency-cluster fence, the victim
    /// `entries` removal, the reverse-index prune, and both injection
    /// points — is the shared tail both forms run
    /// ([`Self::evict_family_victims_locked`]).
    pub(super) fn record_family_admissions_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        families: &[FamilyKey],
        exempt: &dyn Fn(&FamilyKey) -> bool,
    ) {
        if families.is_empty() {
            return;
        }
        let victims = self.memo_budget.record_admissions_exempt(
            families.iter().map(|family| {
                (
                    crate::bounded_query_retention::next_retention_seq(),
                    family.clone(),
                )
            }),
            exempt,
        );
        self.evict_family_victims_locked(entries, victims);
    }

    /// Land the `entries` removal and reverse-index prune for a budget
    /// step's FIFO victims, under the caller's still-held `entries` lock.
    /// The shared tail of both admission entries.
    fn evict_family_victims_locked(
        &self,
        entries: &mut FxHashMap<FamilyKey, FamilySlots>,
        victims: Vec<(u64, FamilyKey)>,
    ) {
        // Test-only injection point — parked after the `memo_budget`
        // admission lands and with the `entries` lock still held, so a
        // race test can assert the admission is recorded in the `entries`
        // lock domain. `None` (the production default) is a no-op.
        #[cfg(any(test, feature = "test-support"))]
        {
            let gate = self.publish_post_memo_budget_record_gate.lock().clone();
            if let Some(barrier) = gate {
                barrier.wait();
                barrier.wait();
            }
        }
        // `record_admissions_exempt` hands back `(seq, FamilyKey)`
        // victims. The removal here is by `FamilyKey` alone, NOT by
        // admission seq — sound because this whole method runs under the
        // caller's exclusive `entries` lock (`&mut FxHashMap<FamilyKey,
        // FamilySlots>`), the same lock domain every family admission is
        // recorded under and that `invalidate_all` clears `entries` +
        // `memo_budget` under. With the exclusive lock held no concurrent
        // writer can re-admit a FIFO victim's `FamilyKey` between the
        // budget step and this drain, so a key-based removal cannot evict
        // a fresh same-key re-admission. The `GlobalRetentionBudget`
        // victim-identity contract permits a key-based removal for
        // exactly this exclusive-lock-serialised case. No victim can name
        // an exempt family, so a group publish never removes a family its
        // own coherence depends on.
        // Tracks whether at least one victim reverse-index registration
        // was pruned, so the post-prune injection point fires only when a
        // FIFO eviction actually happened. `cfg`-gated — absent from
        // release builds, where the gate block is also compiled out.
        #[cfg(any(test, feature = "test-support"))]
        let mut pruned_any = false;
        for (_victim_seq, victim) in victims {
            // Remove the victim from `entries` (keeping map and budget
            // in lockstep), then prune its reverse-index registrations
            // under the same lock. The prune walks the SAME union
            // `register_reverse_index` iterates — carrier
            // `canonical_ids()` PLUS `dispatch_dep_signature` — so a
            // dispatch-only registration (notably `<project>` from
            // `project_generation_signature()`) cannot survive FIFO
            // eviction. `prune_reverse_index_registration` is
            // idempotent, so a canonical in both rails is pruned twice
            // harmlessly. Each candidate's registrations are keyed
            // PER-CANDIDATE on `(family, slot, admission_seq)` so a
            // multi-view slot's cleanup strips only this candidate's
            // seq — surviving sibling candidates in the same slot
            // keep their own seq registrations.
            if let Some(slots) = entries.remove(&victim) {
                let drained = reverse_index::drain_family_slots_registrations(
                    &self.canonical_to_entries,
                    &victim,
                    &slots,
                );
                #[cfg(any(test, feature = "test-support"))]
                {
                    pruned_any |= drained;
                }
                #[cfg(not(any(test, feature = "test-support")))]
                let _ = drained;
            }
        }
        // Test-only injection point — parked after the FIFO victims'
        // `canonical_to_entries` reverse-index registrations have been
        // pruned and with the `entries` lock still held, so a race test
        // can assert the reverse-index prune runs in the `entries` lock
        // domain. Fires only when at least one victim registration was
        // pruned (so a publish that evicts nothing does not park).
        // `None` (the production default) is a no-op.
        #[cfg(any(test, feature = "test-support"))]
        {
            if pruned_any {
                let gate = self.publish_post_reverse_index_prune_gate.lock().clone();
                if let Some(barrier) = gate {
                    barrier.wait();
                    barrier.wait();
                }
            }
        }
    }
}
