//! Per-canonical reverse index helpers for the family memo.
//!
//! The family memo's three-member consistency cluster — `entries`
//! (the warm map), `memo_budget` (its FIFO ledger), and
//! `canonical_to_entries` (this reverse index) — is mutated under
//! ONE lock domain (the `entries` `Mutex`). The helpers here own the
//! reverse-index portion of that contract: registration of the published
//! candidates not yet indexed, per-candidate drain on displacement / FIFO
//! eviction, and single `(family, slot, seq)` removal during
//! cross-canonical cleanup.
//!
//! **Registration on read.** Publishing a candidate leaves it unindexed
//! and notes its family and admission sequence in [`PendingRegistrations`];
//! the next read of the index registers exactly those candidates, under
//! the same `entries` lock hold as that read
//! ([`SemanticGraphStore::index_unregistered_candidates_locked`](super::SemanticGraphStore)),
//! so its work follows the publishes since the previous read, not the
//! memo's size. An index read therefore always sees every resident
//! candidate, and a candidate displaced or evicted before any read is
//! dropped with nothing to prune.
//!
//! **Per-candidate keying.** Reverse-index entries identify
//! `(FamilyKey, ModeSlot, admission_seq)` — per-candidate, not
//! per-`(family, slot)`. The store hands out each `admission_seq` once,
//! so a shard keys an entry by `(admission_seq, ModeSlot)` and keeps its
//! family beside the registered rail. With multi-candidate slots a single
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
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

use super::family::{FamilyKey, FamilySlots, MemoEntry, ModeSlot};
use super::{CanonicalToEntries, RegisteredFacts};
use crate::instant::Instant;
use crate::semantic_query::DepSignature;

/// The candidates published since the reverse index was last read, so the
/// read registers exactly those instead of walking every resident family.
/// Locked only while the family memo's `entries` lock is held
/// (`entries → pending`), so it never contends.
#[derive(Default)]
pub(super) struct PendingRegistrations {
    /// Each publish's family and admission sequence, in publish order. An
    /// entry whose candidate was displaced or evicted before the read
    /// finds nothing to register.
    published: Vec<(FamilyKey, u64)>,
    /// The list grew to the memo's resident family count (repeat publishes
    /// into resident families, or candidates evicted unread) and was
    /// dropped: the next read registers every unindexed resident candidate
    /// in one walk. The list therefore never outgrows the memo it indexes,
    /// and the walk costs no more per publish than the list it replaces.
    overflowed: bool,
}

/// What a reverse-index read registers.
pub(super) enum PendingWork {
    /// Nothing was published since the previous read.
    None,
    /// These publishes' candidates.
    Published(Vec<(FamilyKey, u64)>),
    /// Every unindexed resident candidate.
    All,
}

impl PendingRegistrations {
    /// Note one published candidate; `resident_families` is the memo's
    /// family count after the publish.
    pub(super) fn note(
        &mut self,
        family: &FamilyKey,
        admission_seq: u64,
        resident_families: usize,
    ) {
        if self.overflowed {
            return;
        }
        if self.published.len() >= resident_families {
            self.published = Vec::new();
            self.overflowed = true;
            return;
        }
        self.published.push((family.clone(), admission_seq));
    }

    /// Take the registration work a read owes, leaving none pending.
    pub(super) fn take(&mut self) -> PendingWork {
        let pending = std::mem::take(self);
        if pending.overflowed {
            PendingWork::All
        } else if pending.published.is_empty() {
            PendingWork::None
        } else {
            PendingWork::Published(pending.published)
        }
    }
}

/// The canonicals one candidate registers under, borrowed from its own
/// carriers: the UNION of the carrier's canonicals
/// ([`ReadSetSignature::for_each_canonical_id`](verter_session_query::facts::fact_cache::ReadSetSignature::for_each_canonical_id))
/// and the dispatch-fence canonicals, each once, in first-observation order.
///
/// Registration and every drain derive the set through this one projection,
/// so they stay symmetric by construction; nothing is copied to compute it.
struct RegisteredCanonicals<'a> {
    ordered: SmallVec<[&'a str; 16]>,
    /// Membership index, built once the set is too wide for a linear scan.
    wide: Option<FxHashSet<&'a str>>,
}

impl<'a> RegisteredCanonicals<'a> {
    /// Up to this many canonicals, membership is a linear scan.
    const LINEAR_SCAN_MAX: usize = 32;

    fn of(
        read_set_signature: &'a verter_session_query::facts::fact_cache::ReadSetSignature,
        dispatch_dep_signature: &'a DepSignature,
    ) -> Self {
        let mut canonicals = Self {
            ordered: SmallVec::new(),
            wide: None,
        };
        read_set_signature.for_each_canonical_id(|canonical| canonicals.insert(canonical));
        for (canonical, _) in dispatch_dep_signature.iter() {
            canonicals.insert(canonical);
        }
        canonicals
    }

    fn insert(&mut self, canonical: &'a str) {
        let fresh = match &mut self.wide {
            Some(index) => index.insert(canonical),
            None => !self.ordered.contains(&canonical),
        };
        if !fresh {
            return;
        }
        self.ordered.push(canonical);
        if self.wide.is_none() && self.ordered.len() > Self::LINEAR_SCAN_MAX {
            self.wide = Some(self.ordered.iter().copied().collect());
        }
    }

    fn iter(&self) -> impl Iterator<Item = &'a str> + '_ {
        self.ordered.iter().copied()
    }
}

/// Register one candidate's reverse-index entries: one
/// `(admission_seq, slot) -> (family, registered_facts)` record under each
/// canonical the candidate registers under — the UNION of its carrier's
/// `canonical_ids()` and its dispatch-fence canonicals, each once. The two
/// sets overlap in production (the cold build folds the dispatch fence into
/// the carrier); the dispatch-fence pass covers synthetic / test publishes
/// whose carrier was seeded without the fence merge.
///
/// A carrier whose signature COMPACTED registers under fewer canonicals:
/// its terminal aggregate names none of the files it stands in for. That
/// under-approximation is sound here — this index drives eager eviction,
/// not validity; [`drain_candidate_reverse_index_registrations`] walks the
/// identical projection so the two stay symmetric; and the aggregate
/// rejects at read time on any movement in its domain. The consequence is
/// a later eviction, never a stale serve.
pub(super) fn register_candidate(
    canonical_to_entries: &CanonicalToEntries,
    family: &Arc<FamilyKey>,
    slot: ModeSlot,
    entry: &MemoEntry,
    timing_on: bool,
) {
    let canonicals =
        RegisteredCanonicals::of(&entry.read_set_signature, &entry.dispatch_dep_signature);
    let registered_facts: RegisteredFacts = Arc::clone(&entry.read_set_signature.facts);
    // One shard lookup and lock per canonical.
    for canonical in canonicals.iter() {
        register_canonical_slots(
            canonical_to_entries,
            canonical,
            family,
            &[slot],
            entry.admission_seq,
            &registered_facts,
            timing_on,
        );
    }
}

/// Drain a single candidate's per-candidate reverse-index
/// registrations. Walks the SAME canonical-iteration union
/// [`register_candidate`] uses and removes each
/// `(family, slot, candidate.admission_seq)` entry. Surviving
/// sibling candidates' registrations stay intact — each carries its
/// own seq. Caller holds the `entries` lock.
pub(super) fn drain_candidate_reverse_index_registrations(
    canonical_to_entries: &CanonicalToEntries,
    slot: ModeSlot,
    entry: &MemoEntry,
) {
    if !entry.reverse_indexed {
        return;
    }
    let registration = (slot, entry.admission_seq);
    let canonicals =
        RegisteredCanonicals::of(&entry.read_set_signature, &entry.dispatch_dep_signature);
    for canonical in canonicals.iter() {
        prune_reverse_index_registration(canonical_to_entries, canonical, registration);
    }
}

/// Drain one evicted candidate's registrations from every canonical it
/// registered under except `skip` — the canonical whose own shard the
/// caller already cleared. Caller holds the `entries` lock.
pub(super) fn drain_registrations_except(
    canonical_to_entries: &CanonicalToEntries,
    read_set_signature: &verter_session_query::facts::fact_cache::ReadSetSignature,
    dispatch_dep_signature: &DepSignature,
    registration: (ModeSlot, u64),
    skip: &str,
) {
    let canonicals = RegisteredCanonicals::of(read_set_signature, dispatch_dep_signature);
    for canonical in canonicals.iter().filter(|canonical| *canonical != skip) {
        prune_reverse_index_registration(canonical_to_entries, canonical, registration);
    }
}

/// Remove one `(family, slot, seq)` registration from `canonical`'s
/// shard, then drop the outer shard when its inner map is empty.
///
/// **Shard-detach safety.** The inner-map removal releases the
/// per-canonical `Mutex` before the outer drop is attempted; the
/// outer drop is then a single [`dashmap::DashMap::remove_if`] whose
/// emptiness predicate runs while the shard write lock is held.
/// A [`register_candidate`] inserter holds that shard's lock — read
/// for an existing shard, write for `entry(canonical).or_insert_with(...)`
/// — across its inner `insert`, so the two serialise: either the inserter
/// runs first
/// and `remove_if`'s predicate observes the inner map non-empty
/// (drop skipped), or `remove_if` runs first, drops the empty outer
/// entry, and the inserter's later `or_insert_with` re-creates a
/// fresh shard cleanly. A registration can never be stranded in a
/// just-removed outer entry.
///
/// The outer drop is attempted only when this removal left the inner map
/// empty: a shard that still holds another registration is looked up once.
pub(super) fn prune_reverse_index_registration(
    canonical_to_entries: &CanonicalToEntries,
    canonical: &str,
    (slot, seq): (ModeSlot, u64),
) {
    let emptied = match canonical_to_entries.get(canonical) {
        Some(shard) => {
            let mut map = shard.value().lock();
            map.remove(&(seq, slot));
            map.is_empty()
        }
        None => return,
    };
    // Only a shard this removal emptied can be dropped; the drop re-checks
    // emptiness under the shard write lock.
    if emptied {
        canonical_to_entries.remove_if(canonical, |_, mutex| mutex.lock().is_empty());
    }
}

/// Single-shard registration step used by
/// [`register_candidate`]. Inserts one
/// `(seq, slot) -> (family, registered_facts)` record per populated slot
/// into the `canonical_to_entries[canonical]` shard, holding the shard lock
/// (read for an existing shard, write to create one) across the inner
/// inserts.
#[inline]
fn register_canonical_slots(
    canonical_to_entries: &CanonicalToEntries,
    canonical: &str,
    family: &Arc<FamilyKey>,
    populated_slots: &[ModeSlot],
    admission_seq: u64,
    registered_facts: &RegisteredFacts,
    timing_on: bool,
) {
    let lock_start = if timing_on {
        Some(Instant::now())
    } else {
        None
    };
    let insert = |map: &mut FxHashMap<_, _>| {
        for populated in populated_slots {
            map.insert(
                (admission_seq, *populated),
                (Arc::clone(family), Arc::clone(registered_facts)),
            );
        }
    };
    // An existing shard takes the registration under its read lock; only a
    // canonical without one copies its name into a new shard. A shard the
    // prune drops between the two steps is re-created by the second, as the
    // single-step path would.
    if let Some(shard) = canonical_to_entries.get(canonical) {
        let mut map = shard.value().lock();
        record_lock_wait(lock_start);
        insert(&mut map);
        return;
    }
    let shard = canonical_to_entries
        .entry(Arc::from(canonical))
        .or_insert_with(|| Mutex::new(FxHashMap::default()));
    let mut map = shard.value().lock();
    record_lock_wait(lock_start);
    insert(&mut map);
}

fn record_lock_wait(lock_start: Option<Instant>) {
    let lock_wait = lock_start
        .map(|t| t.elapsed())
        .unwrap_or(std::time::Duration::ZERO);
    crate::request_observers::record_family_map_lock_acquisition(lock_wait);
}

/// Prune every reverse-index registration a REMOVED family's slots hold.
///
/// Walks every candidate in every slot — a multi-view slot must drain
/// registrations for each candidate, not just the first — over the SAME
/// union [`register_candidate`] iterates (carrier `canonical_ids()`
/// PLUS `dispatch_dep_signature`), so a dispatch-only registration
/// cannot survive the removal. Returns whether at least one registration
/// was pruned. The caller holds the family memo's `entries` `Mutex`.
pub(super) fn drain_family_slots_registrations(
    canonical_to_entries: &CanonicalToEntries,
    slots: &FamilySlots,
) -> bool {
    let mut pruned_any = false;
    for (slot, entry) in slots.iter_populated_slots_all() {
        if !entry.reverse_indexed {
            continue;
        }
        let canonicals =
            RegisteredCanonicals::of(&entry.read_set_signature, &entry.dispatch_dep_signature);
        for canonical in canonicals.iter() {
            prune_reverse_index_registration(
                canonical_to_entries,
                canonical,
                (slot, entry.admission_seq),
            );
            pruned_any = true;
        }
    }
    pruned_any
}
