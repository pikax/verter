//! Bounded query-identity retention substrate.
//!
//! Query-identity caches store entries whose effective identity carries
//! self-version state (an owner whole-hash, a `DeclIdentity` embedding a
//! file whole-hash, a content-derived `SemanticNodeId`). Each distinct
//! content edit of an owner appends a fresh entry, so without a routine
//! reclamation path those caches grow monotonically with the edit count
//! in a long-lived session.
//!
//! This module is the single shared substrate that bounds that whole
//! class. It exposes two cooperating pieces tuned per cache:
//!
//! - [`GlobalRetentionBudget`] — a process-cheap, insertion-ordered
//!   (FIFO) total-size budget. A cache records each admitted entry's key
//!   and the budget returns the keys to evict once the recorded count
//!   exceeds the cache's configured cap. Caches whose backing map is
//!   owned by the cooperative-admission primitive
//!   ([`crate::component_meta_caches::MaterializeStructureDb`],
//!   [`crate::component_meta_caches::RefCycleResultDb`], the
//!   [`crate::semantic_query_memo::SemanticGraphStore`] memo + node
//!   arena) embed a `GlobalRetentionBudget` and drive eviction from
//!   their write-side `post_publish` hook.
//!
//! - [`BoundedCandidateMap`] — a query-identity slot map whose outer key
//!   is **content-free**; each slot holds a bounded candidate list and a
//!   distinct concurrent version is a candidate inside the slot. A fifth
//!   candidate in a four-deep slot evicts the slot's oldest candidate; a
//!   `GlobalRetentionBudget` additionally caps the total candidate count
//!   across all slots. [`crate::component_meta_result_db::ComponentMetaResultDb`]
//!   is built on this.
//!
//! ## Eviction policy
//!
//! Eviction is **stale-first when cheaply detectable, then FIFO**.
//! Insertion sequence numbers (a shared monotonic counter) provide the
//! FIFO order; the substrate never does read-time LRU bookkeeping, so a
//! warm read is a shared borrow with no atomic write. Evicting a *valid*
//! entry is permitted — it only forces a recompute, never an incorrect
//! result. Cleanup runs write-side (on insert).
//!
//! ## Concurrency
//!
//! A reader clones the candidate `Arc` out of the slot before
//! validating it, so a concurrent removal never invalidates an in-flight
//! reader's borrow. Removal is keyed by candidate identity (the
//! insertion sequence number, unique per admitted candidate), so a
//! concurrent re-admission under the same discriminant is never mistaken
//! for the candidate being evicted.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use dashmap::DashMap;

/// Process-wide monotonic allocator for candidate / entry insertion
/// sequence numbers. The sequence number is the FIFO eviction order and
/// doubles as a per-admission identity that survives a same-key
/// re-admission.
static RETENTION_SEQ: AtomicU64 = AtomicU64::new(1);

/// Allocate the next insertion sequence number. Strictly monotonic and
/// never reused for the lifetime of the process.
#[must_use]
pub fn next_retention_seq() -> u64 {
    RETENTION_SEQ.fetch_add(1, Ordering::Relaxed)
}

// ===========================================================================
// GlobalRetentionBudget — shared FIFO total-size budget
// ===========================================================================

/// Insertion-ordered total-size budget shared by every bounded
/// query-identity cache.
///
/// A cache calls [`Self::record_admission`] for each entry it admits,
/// passing the entry's map key and its insertion sequence number. The
/// budget keeps a FIFO ledger of admitted `(seq, key)` pairs; when the
/// ledger exceeds `cap` it returns the keys of the oldest entries so the
/// caller can evict them from its own map (running whatever reverse-index
/// / counter cleanup the cache needs).
///
/// The ledger holds keys only — never payloads — so it is cheap. A cache
/// that removes an entry through its own invalidation path calls
/// [`Self::forget`] so the ledger does not later hand back a key whose
/// entry is already gone.
pub struct GlobalRetentionBudget<K> {
    /// FIFO ledger of admitted entries, oldest at the front.
    ledger: parking_lot::Mutex<VecDeque<(u64, K)>>,
    /// Maximum number of live admitted entries retained. Exceeding it on
    /// an admission returns the oldest keys for eviction.
    cap: usize,
}

impl<K> GlobalRetentionBudget<K>
where
    K: Clone + PartialEq,
{
    /// Construct a budget with the given total cap. A `cap` of `0` is
    /// clamped to `1` so a cache always retains at least its newest
    /// entry.
    #[must_use]
    pub fn new(cap: usize) -> Self {
        Self {
            ledger: parking_lot::Mutex::new(VecDeque::new()),
            cap: cap.max(1),
        }
    }

    /// The configured total cap.
    #[must_use]
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// Record one freshly-admitted entry. Returns the keys of the oldest
    /// entries that must now be evicted to bring the ledger back within
    /// `cap` (empty when the cache is still within budget).
    ///
    /// The returned keys are removed from the ledger immediately, so a
    /// caller that evicts them keeps the ledger consistent with its map.
    #[must_use]
    pub fn record_admission(&self, seq: u64, key: K) -> Vec<K> {
        let mut ledger = self.ledger.lock();
        ledger.push_back((seq, key));
        let mut evict = Vec::new();
        while ledger.len() > self.cap {
            if let Some((_, victim)) = ledger.pop_front() {
                evict.push(victim);
            }
        }
        evict
    }

    /// Drop every ledger entry for `key`. Called when a cache removes an
    /// entry through its own invalidation path (per-canonical drain,
    /// project-generation reset, stale-read reaping) so the ledger never
    /// later returns a key whose entry the cache already removed.
    pub fn forget(&self, key: &K) {
        let mut ledger = self.ledger.lock();
        ledger.retain(|(_, k)| k != key);
    }

    /// Drop the single ledger entry identified by `seq`. Used when an
    /// individual candidate (not a whole slot) is evicted — the sequence
    /// number is unique per admission, so this removes exactly that
    /// candidate's ledger record and never a re-admission under the same
    /// key.
    pub fn forget_seq(&self, seq: u64) {
        let mut ledger = self.ledger.lock();
        ledger.retain(|(s, _)| *s != seq);
    }

    /// Clear the whole ledger. Called on a project-generation reset that
    /// drops every cache entry at once.
    pub fn clear(&self) {
        self.ledger.lock().clear();
    }

    /// Number of entries currently tracked. Test-only diagnostics.
    #[cfg(test)]
    #[must_use]
    pub fn tracked_len(&self) -> usize {
        self.ledger.lock().len()
    }
}

/// Default total cap for a `GlobalRetentionBudget` constructed via
/// [`Default`]. Sized for a query memo (`SemanticGraphStore`); caches
/// that want a different cap construct the budget with an explicit
/// [`GlobalRetentionBudget::new`].
pub const DEFAULT_BUDGET_CAP: usize = 4096;

impl<K> Default for GlobalRetentionBudget<K>
where
    K: Clone + PartialEq,
{
    fn default() -> Self {
        Self::new(DEFAULT_BUDGET_CAP)
    }
}

// ===========================================================================
// BoundedCandidateMap — content-free slot key, bounded candidate list
// ===========================================================================

/// Default per-slot candidate cap. Concurrent overlay variants of the
/// same query identity coexist as candidates inside one slot; four
/// covers the `{current, previous, two concurrent overlays}` working
/// set. Per the multi-candidate cache model (architecture rule R20).
pub const DEFAULT_CANDIDATE_CAP: usize = 4;

/// One stored candidate inside a [`BoundedCandidateMap`] slot.
///
/// `discriminant` is the self-version state the slot key intentionally
/// omits (e.g. an owner whole-hash). `seq` is the FIFO insertion order
/// and the candidate's removal identity. `value` is the payload the
/// caller stores — typically a payload `Arc` plus its read-set / fact
/// signature.
pub struct RetentionCandidate<D, V> {
    /// Self-version discriminant carried by the candidate, not the slot
    /// key. Two candidates in one slot differ by this value.
    pub discriminant: D,
    /// Monotonic insertion sequence — FIFO order and removal identity.
    pub seq: u64,
    /// Caller payload.
    pub value: V,
}

/// A query-identity slot — a bounded, insertion-ordered list of
/// candidates. Held behind an `Arc` in the outer map; the candidate
/// vector itself is behind a `Mutex` so admissions and stale reaping
/// serialise per slot while the outer map stays lock-free per shard.
pub struct CandidateSlot<D, V> {
    candidates: parking_lot::Mutex<Vec<Arc<RetentionCandidate<D, V>>>>,
}

impl<D, V> CandidateSlot<D, V> {
    fn new() -> Self {
        Self {
            candidates: parking_lot::Mutex::new(Vec::new()),
        }
    }

    /// Snapshot the slot's candidates. The returned `Arc`s are owned by
    /// the caller, so a concurrent removal cannot invalidate them — a
    /// reader validates the snapshot without holding the slot lock.
    #[cfg(test)]
    #[must_use]
    pub fn snapshot(&self) -> Vec<Arc<RetentionCandidate<D, V>>> {
        self.candidates.lock().clone()
    }
}

/// Content-free query-identity slot map with bounded per-slot candidate
/// lists and a shared global total-size budget.
///
/// The outer key `K` is content-free (it carries query / owner / options
/// / env identity but no content version). Concurrent versions of the
/// same query identity are candidates inside one slot, capped at
/// `per_slot_cap`. A `GlobalRetentionBudget` caps the total candidate
/// count across all slots; both caps evict oldest-first.
pub struct BoundedCandidateMap<K, D, V> {
    slots: DashMap<K, Arc<CandidateSlot<D, V>>>,
    budget: GlobalRetentionBudget<(K, u64)>,
    per_slot_cap: usize,
}

impl<K, D, V> BoundedCandidateMap<K, D, V>
where
    K: Eq + std::hash::Hash + Clone,
    D: PartialEq + Clone,
{
    /// Construct with an explicit per-slot candidate cap and global
    /// total-candidate cap. Both are clamped to at least `1`.
    #[must_use]
    pub fn with_caps(per_slot_cap: usize, global_cap: usize) -> Self {
        Self {
            slots: DashMap::new(),
            budget: GlobalRetentionBudget::new(global_cap),
            per_slot_cap: per_slot_cap.max(1),
        }
    }

    /// The per-slot candidate cap.
    #[must_use]
    pub fn per_slot_cap(&self) -> usize {
        self.per_slot_cap
    }

    /// The global total-candidate cap.
    #[must_use]
    pub fn global_cap(&self) -> usize {
        self.budget.cap()
    }

    /// Total live candidate count across every slot. This is the cache's
    /// authoritative occupancy — the number the bound-proof asserts on.
    #[must_use]
    pub fn live_count(&self) -> usize {
        self.slots
            .iter()
            .map(|slot| slot.value().candidates.lock().len())
            .sum()
    }

    /// Snapshot the candidates of slot `key`. Empty when the slot is
    /// absent. The returned `Arc`s outlive any concurrent removal.
    /// Test-only enumeration accessor.
    #[cfg(test)]
    #[must_use]
    pub fn slot_candidates(&self, key: &K) -> Vec<Arc<RetentionCandidate<D, V>>> {
        match self.slots.get(key) {
            Some(slot) => slot.value().snapshot(),
            None => Vec::new(),
        }
    }

    /// Look up the candidate in slot `key` whose discriminant matches
    /// `discriminant`. Returns an owned `Arc` so the caller can validate
    /// it after the slot lock is released.
    #[must_use]
    pub fn get_candidate(
        &self,
        key: &K,
        discriminant: &D,
    ) -> Option<Arc<RetentionCandidate<D, V>>> {
        let slot = self.slots.get(key)?;
        let found = slot
            .value()
            .candidates
            .lock()
            .iter()
            .find(|c| &c.discriminant == discriminant)
            .cloned();
        found
    }

    /// Admit a candidate into slot `key`.
    ///
    /// A candidate already present under the same `discriminant` is
    /// replaced in place (a re-publish of the same version refreshes the
    /// payload without growing the slot). Otherwise the candidate is
    /// appended; if the slot then exceeds `per_slot_cap` its oldest
    /// candidate is evicted. The global budget is consulted last and may
    /// evict an oldest candidate from a *different* slot.
    ///
    /// Returns the count of candidates evicted (per-slot + global) so the
    /// caller can keep an external live counter consistent.
    ///
    /// **Slot-detach safety.** The candidate push happens while the
    /// `DashMap` shard write guard for `key` (the `RefMut` returned by
    /// `entry().or_insert_with()`) is still held. An empty-slot reaper
    /// ([`Self::remove_candidate_by_seq`]'s `remove_if`) acquires the
    /// same shard write lock to test-and-detach a slot, so it can never
    /// interleave between "slot observed empty" and "this admit pushed
    /// its candidate" — the slot the admitter populates is always still
    /// attached when the shard guard is released, and the published
    /// candidate is always reachable by later reads / `live_count`.
    pub fn admit(&self, key: K, discriminant: D, value: V) -> usize {
        let seq = next_retention_seq();

        let mut evicted = 0usize;
        let mut forget_seqs: Vec<u64> = Vec::new();
        {
            // Hold the shard write guard for `key` across the candidate
            // push: a concurrent reaper's `remove_if`-empty needs this
            // same guard, so it cannot detach the slot mid-admit.
            let slot_ref = self
                .slots
                .entry(key.clone())
                .or_insert_with(|| Arc::new(CandidateSlot::new()));
            let mut candidates = slot_ref.candidates.lock();
            if let Some(existing) = candidates
                .iter_mut()
                .find(|c| c.discriminant == discriminant)
            {
                // Same-version re-publish: replace in place. The ledger
                // still tracks the prior admission's seq — drop it and
                // record the fresh one so FIFO order reflects the latest
                // write.
                forget_seqs.push(existing.seq);
                *existing = Arc::new(RetentionCandidate {
                    discriminant,
                    seq,
                    value,
                });
            } else {
                candidates.push(Arc::new(RetentionCandidate {
                    discriminant,
                    seq,
                    value,
                }));
                // Per-slot cap: evict oldest-by-seq until within cap.
                while candidates.len() > self.per_slot_cap {
                    // Oldest = smallest seq.
                    if let Some((idx, _)) = candidates.iter().enumerate().min_by_key(|(_, c)| c.seq)
                    {
                        let removed = candidates.remove(idx);
                        forget_seqs.push(removed.seq);
                        evicted += 1;
                    } else {
                        break;
                    }
                }
            }
            // Release the candidate lock then the shard guard before the
            // global-budget step: that step can re-enter `self.slots`
            // for a victim on this same shard, which would deadlock if
            // the shard guard were still held.
            drop(candidates);
            drop(slot_ref);
        }
        for s in forget_seqs {
            self.budget.forget_seq(s);
        }

        // Global budget: record the fresh admission, evict oldest across
        // all slots if the total exceeds the global cap.
        //
        // Eviction is two-phase by design — the per-slot push above runs
        // under the slot lock, this global trim runs after. Between the
        // two phases a concurrent `live_count` may transiently observe
        // one candidate over the global cap (this admit's push landed,
        // its over-budget victim not yet removed). That is a momentary
        // overcount of a *bounded* quantity (at most one admit's worth
        // per concurrent admitter), not unbounded growth — the budget
        // ledger is authoritative and the victim is removed before
        // `admit` returns. A future reader should not treat the window
        // as a bug.
        let over_budget = self.budget.record_admission(seq, (key, seq));
        for (victim_key, victim_seq) in over_budget {
            if self.remove_candidate_by_seq(&victim_key, victim_seq) {
                evicted += 1;
            }
        }
        evicted
    }

    /// Remove the single candidate identified by `(key, seq)`. Returns
    /// `true` when a candidate was removed. An empty slot is dropped.
    fn remove_candidate_by_seq(&self, key: &K, seq: u64) -> bool {
        let Some(slot) = self.slots.get(key) else {
            return false;
        };
        let removed = {
            let mut candidates = slot.value().candidates.lock();
            if let Some(idx) = candidates.iter().position(|c| c.seq == seq) {
                candidates.remove(idx);
                true
            } else {
                false
            }
        };
        drop(slot);
        if removed {
            // Drop the slot if it is still empty. `remove_if` holds the
            // shard write lock while it runs the emptiness predicate and
            // detaches the slot; [`Self::admit`] holds that same shard
            // write guard across its candidate push. The two therefore
            // serialise: a `remove_if` that observes the slot empty has
            // exclusive shard access, so no in-flight admit can be
            // mid-push into this slot — and a `remove_if` racing an
            // admit either runs first (predicate sees the slot, which
            // the admit then repopulates and re-checks is irrelevant —
            // the slot was non-empty so detach is skipped) or runs after
            // (predicate sees the admit's candidate, detach skipped). A
            // freshly published candidate is never stranded in a
            // detached slot.
            self.slots
                .remove_if(key, |_, slot| slot.candidates.lock().is_empty());
        }
        removed
    }

    /// Remove a candidate by its identity (`seq`) from slot `key`,
    /// running the budget cleanup. Used by callers that proactively reap
    /// a candidate they found stale on read. Returns `true` when a
    /// candidate was removed.
    pub fn evict_candidate(&self, key: &K, seq: u64) -> bool {
        let removed = self.remove_candidate_by_seq(key, seq);
        if removed {
            self.budget.forget_seq(seq);
        }
        removed
    }

    /// Drop every candidate in slot `key` (all versions). Returns the
    /// number removed. Test-only single-slot drain — production
    /// per-owner invalidation goes through [`Self::retain_slots`].
    #[cfg(test)]
    pub fn evict_slot(&self, key: &K) -> usize {
        let Some((_, slot)) = self.slots.remove(key) else {
            return 0;
        };
        let drained = std::mem::take(&mut *slot.candidates.lock());
        for c in &drained {
            self.budget.forget_seq(c.seq);
        }
        drained.len()
    }

    /// Drop every slot and every candidate. Returns the number of
    /// candidates removed. Used on a project-generation reset.
    pub fn clear(&self) -> usize {
        let mut removed = 0usize;
        for slot in self.slots.iter() {
            removed += slot.value().candidates.lock().len();
        }
        self.slots.clear();
        self.budget.clear();
        removed
    }

    /// Retain only the slots whose key satisfies `keep`; every candidate
    /// of a dropped slot is forgotten from the budget. Returns the
    /// number of candidates removed.
    pub fn retain_slots<F>(&self, mut keep: F) -> usize
    where
        F: FnMut(&K) -> bool,
    {
        let mut removed = 0usize;
        self.slots.retain(|key, slot| {
            if keep(key) {
                true
            } else {
                let candidates = slot.candidates.lock();
                for c in candidates.iter() {
                    self.budget.forget_seq(c.seq);
                }
                removed += candidates.len();
                false
            }
        });
        removed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `GlobalRetentionBudget` returns the oldest keys once the ledger
    /// exceeds the cap, in FIFO order. DISCRIMINATES: an unbounded
    /// ledger would return an empty eviction list forever.
    #[test]
    fn budget_evicts_oldest_first_past_cap() {
        let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(3);
        assert!(budget.record_admission(1, 10).is_empty(), "1st within cap");
        assert!(budget.record_admission(2, 11).is_empty(), "2nd within cap");
        assert!(budget.record_admission(3, 12).is_empty(), "3rd within cap");
        // 4th admission overflows — the oldest (key 10) is evicted.
        assert_eq!(
            budget.record_admission(4, 13),
            vec![10],
            "4th admission must evict the oldest key (FIFO)",
        );
        // 5th — key 11 is now oldest.
        assert_eq!(
            budget.record_admission(5, 14),
            vec![11],
            "5th admission must evict the next-oldest key",
        );
        assert_eq!(budget.tracked_len(), 3, "ledger stays bounded at cap");
    }

    /// `forget` drops a key so a later overflow does not return it.
    #[test]
    fn budget_forget_drops_key_from_ledger() {
        let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(2);
        let _ = budget.record_admission(1, 100);
        let _ = budget.record_admission(2, 101);
        budget.forget(&100);
        assert_eq!(budget.tracked_len(), 1, "forget removed key 100");
        // Next admission does NOT overflow (only one tracked entry left).
        assert!(
            budget.record_admission(3, 102).is_empty(),
            "after forget the ledger is within cap again",
        );
    }

    /// A zero cap is clamped to one — the cache always keeps its newest
    /// entry.
    #[test]
    fn budget_zero_cap_clamps_to_one() {
        let budget: GlobalRetentionBudget<u32> = GlobalRetentionBudget::new(0);
        assert_eq!(budget.cap(), 1);
        assert!(budget.record_admission(1, 1).is_empty());
        assert_eq!(
            budget.record_admission(2, 2),
            vec![1],
            "second admission evicts the first under a cap of 1",
        );
    }

    /// `BoundedCandidateMap` keeps at most `per_slot_cap` candidates in
    /// one slot, evicting the oldest. DISCRIMINATES: an unbounded slot
    /// would retain all five.
    #[test]
    fn candidate_map_bounds_one_slot_at_per_slot_cap() {
        let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(3, 100);
        for v in 0u8..5 {
            map.admit("owner", v, u32::from(v));
        }
        let slot = map.slot_candidates(&"owner");
        assert_eq!(
            slot.len(),
            3,
            "one slot must retain at most per_slot_cap (3) candidates",
        );
        // Oldest two discriminants (0, 1) evicted; newest three remain.
        let mut discs: Vec<u8> = slot.iter().map(|c| c.discriminant).collect();
        discs.sort_unstable();
        assert_eq!(discs, vec![2, 3, 4], "oldest candidates evicted first");
        assert_eq!(map.live_count(), 3);
    }

    /// Re-admitting the same discriminant replaces the candidate in
    /// place — the slot does not grow.
    #[test]
    fn candidate_map_same_discriminant_replaces_in_place() {
        let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
        map.admit("owner", 7, 1);
        map.admit("owner", 7, 2);
        map.admit("owner", 7, 3);
        let slot = map.slot_candidates(&"owner");
        assert_eq!(
            slot.len(),
            1,
            "same-discriminant re-admit must not grow the slot"
        );
        assert_eq!(slot[0].value, 3, "re-admit refreshes the payload");
    }

    /// The global budget caps the total candidate count across distinct
    /// slots — the per-slot cap alone would not bound a workload that
    /// edits many DISTINCT owners once each.
    #[test]
    fn candidate_map_global_budget_bounds_across_slots() {
        let map: BoundedCandidateMap<u32, u8, u32> = BoundedCandidateMap::with_caps(4, 5);
        // 12 distinct slots, one candidate each — per-slot cap never
        // triggers, only the global cap of 5 does.
        for owner in 0u32..12 {
            map.admit(owner, 0, owner);
        }
        assert!(
            map.live_count() <= 5,
            "global budget must bound total candidates across slots — got {}",
            map.live_count(),
        );
    }

    /// A reader's cloned candidate `Arc` survives a concurrent eviction
    /// of that candidate — removal does not invalidate an in-flight
    /// reader.
    #[test]
    fn candidate_map_reader_arc_survives_eviction() {
        let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
        map.admit("owner", 1, 42);
        let held = map.get_candidate(&"owner", &1).expect("candidate present");
        let seq = held.seq;
        // Evict the exact candidate the reader is holding.
        assert!(map.evict_candidate(&"owner", seq), "candidate evicted");
        assert!(
            map.get_candidate(&"owner", &1).is_none(),
            "evicted candidate is gone from the map",
        );
        // The reader's clone is still valid and unchanged.
        assert_eq!(held.value, 42, "in-flight reader's Arc still resolves");
    }

    /// SLOT-DETACH REGRESSION — a candidate admitted into a slot that
    /// was just emptied (and therefore eligible for opportunistic
    /// detach) MUST remain reachable by `get_candidate` and counted by
    /// `live_count`.
    ///
    /// Sequence: admit one candidate; remove it — the slot is now empty
    /// and `remove_candidate_by_seq`'s `remove_if` detaches it from the
    /// outer map. Then admit a fresh candidate under the SAME key:
    /// `admit` re-creates the slot via `entry().or_insert_with()`. The
    /// fresh candidate must be visible. A pre-fix `admit` that cloned the
    /// slot `Arc` and dropped the shard guard before pushing could push
    /// into a slot a concurrent reaper had detached; this test pins the
    /// post-detach re-admit path so a regression to that shape would
    /// leave the re-admitted candidate invisible (`live_count() == 0`).
    #[test]
    fn candidate_map_readmit_into_emptied_slot_stays_visible() {
        let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
        // Admit, then evict — the slot is emptied and detached.
        map.admit("owner", 1, 100);
        let first = map.get_candidate(&"owner", &1).expect("first admitted");
        assert!(
            map.evict_candidate(&"owner", first.seq),
            "first candidate evicted — slot now empty + detached",
        );
        assert_eq!(map.live_count(), 0, "slot emptied");
        assert!(map.slot_candidates(&"owner").is_empty());

        // Re-admit under the same key. The fresh candidate MUST be
        // reachable — `admit` re-attaches the slot under its shard guard.
        map.admit("owner", 2, 200);
        let readmitted = map
            .get_candidate(&"owner", &2)
            .expect("re-admitted candidate must be reachable after slot detach");
        assert_eq!(readmitted.value, 200, "re-admitted payload visible");
        assert_eq!(
            map.live_count(),
            1,
            "re-admitted candidate must be counted by live_count — a \
             candidate pushed into a detached slot would read as 0",
        );
        assert_eq!(
            map.slot_candidates(&"owner").len(),
            1,
            "the re-attached slot holds exactly the re-admitted candidate",
        );
    }

    /// SLOT-DETACH RACE under real thread contention — a reaper emptying
    /// a slot must never strand a concurrent admitter's candidate. Many
    /// rounds of `{admit / evict-then-readmit}` race on one key; after
    /// every round the key's live candidate must be reachable. Drives
    /// the shard-guard serialisation between `admit`'s push and
    /// `remove_if`'s detach.
    #[test]
    fn candidate_map_concurrent_admit_vs_detach_never_strands() {
        use std::sync::Arc as StdArc;
        use std::thread;

        let map: StdArc<BoundedCandidateMap<u32, u8, u32>> =
            StdArc::new(BoundedCandidateMap::with_caps(4, 4096));
        // Seed every key so the first reaper round has something to
        // evict (and thus a slot to attempt to detach).
        for key in 0u32..8 {
            map.admit(key, 0, key);
        }

        let rounds = 400usize;
        let reaper = {
            let map = StdArc::clone(&map);
            thread::spawn(move || {
                for r in 0..rounds {
                    let key = (r % 8) as u32;
                    // Evict whatever candidate currently holds disc 0,
                    // emptying + detaching the slot, then re-admit it.
                    if let Some(c) = map.get_candidate(&key, &0) {
                        map.evict_candidate(&key, c.seq);
                    }
                    map.admit(key, 0, key);
                }
            })
        };
        let admitter = {
            let map = StdArc::clone(&map);
            thread::spawn(move || {
                for r in 0..rounds {
                    let key = (r % 8) as u32;
                    // Admit a second discriminant concurrently with the
                    // reaper churning disc 0 on the same slot.
                    map.admit(key, 1, key + 1000);
                }
            })
        };
        reaper.join().expect("reaper thread");
        admitter.join().expect("admitter thread");

        // Every key must still resolve disc 0 — the reaper's final
        // re-admit cannot have been stranded in a detached slot.
        for key in 0u32..8 {
            assert!(
                map.get_candidate(&key, &0).is_some(),
                "key {key}: disc-0 candidate stranded by a slot-detach race",
            );
        }
        // Total live count must reflect reachable candidates only — a
        // stranded candidate would be recorded in the budget but absent
        // from `live_count`, so `live_count` would not exceed the number
        // of reachable candidates. Assert at least the 8 disc-0 entries
        // are reachable and counted.
        assert!(
            map.live_count() >= 8,
            "live_count must count every reachable candidate — observed {}",
            map.live_count(),
        );
    }

    /// `evict_slot` drops every version in a slot and forgets them all
    /// from the budget.
    #[test]
    fn candidate_map_evict_slot_drops_all_versions() {
        let map: BoundedCandidateMap<&str, u8, u32> = BoundedCandidateMap::with_caps(4, 100);
        map.admit("a", 0, 1);
        map.admit("a", 1, 2);
        map.admit("b", 0, 3);
        assert_eq!(map.evict_slot(&"a"), 2, "both versions of slot a removed");
        assert_eq!(map.live_count(), 1, "only slot b survives");
        assert!(map.slot_candidates(&"a").is_empty());
    }
}
