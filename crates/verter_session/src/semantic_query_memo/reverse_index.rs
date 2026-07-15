//! Per-canonical reverse index helpers for the family memo.
//!
//! The family memo's three-member consistency cluster — `entries`
//! (the warm map), `memo_budget` (its FIFO ledger), and
//! `canonical_to_entries` (this reverse index) — is mutated under
//! ONE lock domain (the `entries` `Mutex`). The helpers here own the
//! reverse-index portion of that contract: registration on publish,
//! per-candidate drain on displacement / FIFO eviction, and single
//! `(family, slot, seq)` removal during cross-canonical cleanup.
//!
//! **Per-candidate keying.** Reverse-index entries are keyed
//! `(FamilyKey, ModeSlot, admission_seq)` — per-candidate, not
//! per-`(family, slot)`. With multi-candidate slots a single
//! `(family, slot)` may hold up to 4 candidates; each registers
//! independently under its own seq so a cross-canonical cleanup for
//! one evicted candidate strips ONLY that candidate's seq from each
//! canonical it referenced, leaving sibling candidates'
//! registrations intact.
//!
//! Lock contract: every helper here is invoked WHILE the caller
//! holds the family memo's `entries` `Mutex`. The
//! `entries → canonical_to_entries shards` order permits taking a
//! shard mutex while `entries` is held; no path takes a shard mutex
//! then `entries`.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use super::family::{FamilyKey, MemoEntry, ModeSlot};
use super::{CanonicalToEntries, RegisteredFacts};
use crate::instant::Instant;
use crate::semantic_query::DepSignature;

/// Register the just-published candidate's reverse-index entries.
///
/// Inserts one `(family, slot, admission_seq) -> registered_facts`
/// record per `(populated_slot × canonical)` pair, where canonicals
/// are the UNION of the carrier's `canonical_ids()` and the
/// dispatch-fence canonicals. The two sets overlap in production
/// (the cold build folds the dispatch fence into the carrier); the
/// `seen` dedup collapses them. The dispatch-fence pass covers
/// synthetic / test publishes whose carrier was seeded without the
/// fence merge.
pub(super) fn register_reverse_index(
    canonical_to_entries: &CanonicalToEntries,
    family: &FamilyKey,
    populated_slots: &[ModeSlot],
    read_set_signature: &crate::fact_signature_helpers::ReadSetSignature,
    dispatch_dep_signature: &DepSignature,
    admission_seq: u64,
) {
    let timing_on = verter_scheduler::request_context::current_timing_enabled();
    let registered_facts = Arc::clone(&read_set_signature.facts);
    let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
    for populated in populated_slots {
        seen.clear();
        for canonical in read_set_signature.canonical_ids() {
            if !seen.insert(Arc::clone(&canonical)) {
                continue;
            }
            register_single_canonical(
                canonical_to_entries,
                &canonical,
                family,
                *populated,
                admission_seq,
                &registered_facts,
                timing_on,
            );
        }
        for (canonical, _) in dispatch_dep_signature.iter() {
            if !seen.insert(Arc::clone(canonical)) {
                continue;
            }
            register_single_canonical(
                canonical_to_entries,
                canonical,
                family,
                *populated,
                admission_seq,
                &registered_facts,
                timing_on,
            );
        }
    }
}

/// Drain a single candidate's per-candidate reverse-index
/// registrations. Walks the SAME canonical-iteration union
/// [`register_reverse_index`] uses and removes each
/// `(family, slot, candidate.admission_seq)` entry. Surviving
/// sibling candidates' registrations stay intact — each carries its
/// own seq. Caller holds the `entries` lock.
pub(super) fn drain_candidate_reverse_index_registrations(
    canonical_to_entries: &CanonicalToEntries,
    family: &FamilyKey,
    slot: ModeSlot,
    entry: &MemoEntry,
) {
    let key = (family.clone(), slot, entry.admission_seq);
    let mut seen: rustc_hash::FxHashSet<Arc<str>> = rustc_hash::FxHashSet::default();
    for canonical in entry.read_set_signature.canonical_ids() {
        if seen.insert(Arc::clone(&canonical)) {
            prune_reverse_index_registration(canonical_to_entries, &canonical, &key);
        }
    }
    for (canonical, _) in entry.dispatch_dep_signature.iter() {
        if seen.insert(Arc::clone(canonical)) {
            prune_reverse_index_registration(canonical_to_entries, canonical, &key);
        }
    }
}

/// Remove one `(family, slot, seq)` registration from `canonical`'s
/// shard, then drop the outer shard when its inner map is empty.
///
/// **Shard-detach safety.** The inner-map removal releases the
/// per-canonical `Mutex` before the outer drop is attempted; the
/// outer drop is then a single [`dashmap::DashMap::remove_if`] whose
/// emptiness predicate runs while the shard write lock is held.
/// A [`register_reverse_index`] inserter takes that same shard write
/// lock for the whole `entry(canonical).or_insert_with(...)` + inner
/// `insert`, so the two serialise: either the inserter runs first
/// and `remove_if`'s predicate observes the inner map non-empty
/// (drop skipped), or `remove_if` runs first, drops the empty outer
/// entry, and the inserter's later `or_insert_with` re-creates a
/// fresh shard cleanly. A registration can never be stranded in a
/// just-removed outer entry.
pub(super) fn prune_reverse_index_registration(
    canonical_to_entries: &CanonicalToEntries,
    canonical: &Arc<str>,
    registration: &(FamilyKey, ModeSlot, u64),
) {
    if let Some(shard) = canonical_to_entries.get(canonical) {
        shard.value().lock().remove(registration);
    }
    canonical_to_entries.remove_if(canonical, |_, mutex| mutex.lock().is_empty());
}

/// Single-shard registration step used by
/// [`register_reverse_index`]. Inserts one
/// `(family, slot, seq) -> registered_facts` record into the
/// `canonical_to_entries[canonical]` shard, taking the shard write
/// lock for the whole `entry(canonical).or_insert_with(...)` + inner
/// `insert` pair.
#[inline]
fn register_single_canonical(
    canonical_to_entries: &CanonicalToEntries,
    canonical: &Arc<str>,
    family: &FamilyKey,
    populated: ModeSlot,
    admission_seq: u64,
    registered_facts: &RegisteredFacts,
    timing_on: bool,
) {
    let shard = canonical_to_entries
        .entry(Arc::clone(canonical))
        .or_insert_with(|| Mutex::new(FxHashMap::default()));
    let lock_start = if timing_on {
        Some(Instant::now())
    } else {
        None
    };
    let mut map = shard.value().lock();
    let lock_wait = lock_start
        .map(|t| t.elapsed())
        .unwrap_or(std::time::Duration::ZERO);
    crate::host_manage::record_family_map_lock_acquisition(lock_wait);
    map.insert(
        (family.clone(), populated, admission_seq),
        Arc::clone(registered_facts),
    );
}
