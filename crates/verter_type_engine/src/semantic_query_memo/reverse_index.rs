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
use super::RegisteredFacts;
use crate::instant::Instant;
use crate::semantic_query::DepSignature;

/// One immutable carrier projection, reused by every member of a publication
/// batch and every populated slot. The facts and their canonical union travel
/// together so registration cannot accidentally pair different carriers.
pub(super) struct ReverseIndexInputs<'a> {
    registered_facts: RegisteredFacts,
    canonicals: RegisteredCanonicals<'a>,
}

impl<'a> ReverseIndexInputs<'a> {
    /// The canonicals each populated slot registers under.
    pub(super) fn canonical_count(&self) -> usize {
        self.canonicals.ordered.len()
    }

    pub(super) fn new(
        read_set_signature: &'a verter_session_query::facts::fact_cache::ReadSetSignature,
        dispatch_dep_signature: &'a DepSignature,
    ) -> Self {
        Self {
            registered_facts: Arc::clone(&read_set_signature.facts),
            canonicals: RegisteredCanonicals::of(read_set_signature, dispatch_dep_signature),
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
        for crate::semantic_query::DepEntry(canonical, _) in dispatch_dep_signature.iter() {
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

    fn is_empty(&self) -> bool {
        self.ordered.is_empty()
    }
}

/// One canonical's shard: its registrations, keyed by `(admission_seq, slot)`.
type ShardMap = FxHashMap<(u64, ModeSlot), (Arc<FamilyKey>, RegisteredFacts)>;

/// Bytes one registration record's bucket holds in a shard's table: the
/// record and its control byte.
const RECORD_BUCKET_BYTES: usize =
    std::mem::size_of::<((u64, ModeSlot), (Arc<FamilyKey>, RegisteredFacts))>() + 1;

/// Bytes one shard's bucket holds in the outer table: the canonical's
/// handle, the shard's lock and table header, and the control byte.
const SHARD_BUCKET_BYTES: usize = std::mem::size_of::<(Arc<str>, Mutex<ShardMap>)>() + 1;

/// Granule the index's own storage is charged in.
const INDEX_STORAGE_GRANULE: usize = 4 * 1024;

/// The per-canonical reverse index, and the charge of the storage it holds
/// beside its records.
///
/// Every registration record is charged by the candidate that registered it
/// ([`registration_bytes`]). Everything else the index holds is charged
/// here, pinned while it is held: each shard's copy of its canonical's
/// text, every shard table's unused buckets (capacity beyond its live
/// records), and the outer table's buckets. A shard table gives its backing
/// back as its records drain, and a shard is dropped with its last record.
///
/// Every mutation runs under the family memo's `entries` lock (see the
/// module docs), so the storage figures settle without racing each other.
pub(super) struct CanonicalToEntries {
    map: dashmap::DashMap<Arc<str>, Mutex<ShardMap>>,
    storage: Mutex<IndexStorage>,
}

#[derive(Default)]
struct IndexStorage {
    pin: Option<verter_session_query::retention::granular::GranularPin>,
    account: Option<Arc<verter_session_query::retention::SemanticRetentionAccount>>,
    /// The outer table's bucket bytes the pin currently counts.
    outer_bytes: usize,
}

impl IndexStorage {
    fn adjust(&mut self, added: usize, freed: usize) {
        if added == freed {
            return;
        }
        let account = self.account.get_or_insert_with(
            verter_session_query::retention::SemanticRetentionAccount::process_local,
        );
        let pin = self.pin.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                account,
                INDEX_STORAGE_GRANULE,
            )
        });
        if added > freed {
            pin.add(added - freed);
        } else {
            pin.sub(freed - added);
        }
    }
}

/// The unused bucket bytes a shard table holds beyond its live records.
fn shard_slack_bytes(map: &ShardMap) -> usize {
    (map.capacity() - map.len()) * RECORD_BUCKET_BYTES
}

/// Give back the backing a shard table no longer needs.
fn shrink_drained_shard(map: &mut ShardMap) {
    let len = map.len();
    if len == 0 {
        *map = FxHashMap::default();
    } else if len * 4 <= map.capacity() {
        map.shrink_to(len * 2);
    }
}

impl std::ops::Deref for CanonicalToEntries {
    type Target = dashmap::DashMap<Arc<str>, Mutex<ShardMap>>;
    fn deref(&self) -> &Self::Target {
        &self.map
    }
}

impl CanonicalToEntries {
    /// An empty index whose storage is charged to `account`.
    pub(super) fn charged(
        account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
    ) -> Self {
        Self {
            map: dashmap::DashMap::new(),
            storage: Mutex::new(IndexStorage {
                account: Some(Arc::clone(account)),
                ..IndexStorage::default()
            }),
        }
    }

    /// Settle the outer table's bucket charge with its capacity. Called with
    /// no shard guard held.
    fn settle_outer(&self, storage: &mut IndexStorage) {
        let bytes = self.map.capacity() * SHARD_BUCKET_BYTES;
        let before = storage.outer_bytes;
        storage.outer_bytes = bytes;
        storage.adjust(bytes, before);
    }

    /// Remove `canonical`'s whole shard. Its records' charges belong to the
    /// candidates that registered them; the shard's own storage is released
    /// here.
    pub(super) fn remove_shard(&self, canonical: &str) -> Option<(Arc<str>, Mutex<ShardMap>)> {
        let removed = self.map.remove(canonical)?;
        let mut storage = self.storage.lock();
        let freed = {
            let map = removed.1.lock();
            verter_session_query::retention::allocation::arc_block_bytes(removed.0.len())
                + shard_slack_bytes(&map)
        };
        storage.adjust(0, freed);
        self.settle_outer(&mut storage);
        Some(removed)
    }

    /// Remove every shard and give back the outer table's backing.
    pub(super) fn clear_shards(&self) {
        let mut storage = self.storage.lock();
        let mut freed = 0;
        for shard in self.map.iter() {
            freed +=
                verter_session_query::retention::allocation::arc_block_bytes(shard.key().len())
                    + shard_slack_bytes(&shard.value().lock());
        }
        self.map.clear();
        self.map.shrink_to_fit();
        storage.adjust(0, freed);
        self.settle_outer(&mut storage);
    }

    /// Bytes the index's own storage is charged.
    pub(super) fn storage_charge_bytes(&self) -> usize {
        self.storage.lock().pin.as_ref().map_or(
            0,
            verter_session_query::retention::granular::GranularPin::charged,
        )
    }
}

/// Bytes the reverse-index registrations of one candidate keep: one record
/// bucket per `(populated slot × registered canonical)` in the canonical's
/// shard table, and the one `Arc<FamilyKey>` clone of `family` every record
/// of the candidate shares — its block and the boxes the clone copies (its
/// `Arc`s it shares with `family`). A shard's own storage — its canonical's
/// text, its unused buckets — is charged by the index
/// ([`CanonicalToEntries`]).
pub(super) fn registration_bytes(
    family: &FamilyKey,
    registered_canonicals: usize,
    populated_slots: usize,
) -> usize {
    use verter_session_query::retention::allocation::arc_block_bytes;
    if registered_canonicals == 0 || populated_slots == 0 {
        return 0;
    }
    registered_canonicals * populated_slots * RECORD_BUCKET_BYTES
        + arc_block_bytes(std::mem::size_of::<FamilyKey>())
        + super::family_footprint::family_key_clone_heap_bytes(family)
}

/// Register the just-published candidate's reverse-index entries.
///
/// Inserts one `(admission_seq, slot) -> (family, registered_facts)`
/// record per `(populated_slot × canonical)` pair, where canonicals
/// are the UNION of the carrier's `canonical_ids()` and the
/// dispatch-fence canonicals. The two sets overlap in production
/// (the cold build folds the dispatch fence into the carrier); the
/// `seen` dedup collapses them. The dispatch-fence pass covers
/// synthetic / test publishes whose carrier was seeded without the
/// fence merge.
///
/// A carrier whose signature COMPACTED registers under fewer canonicals:
/// its terminal aggregate names none of the files it stands in for. That
/// under-approximation is sound here — this index drives eager eviction,
/// not validity; [`drain_candidate_reverse_index_registrations`] walks the
/// identical projection so the two stay symmetric; and the aggregate
/// rejects at read time on any movement in its domain. The consequence is
/// a later eviction, never a stale serve.
pub(super) fn register_reverse_index(
    canonical_to_entries: &CanonicalToEntries,
    family: &FamilyKey,
    populated_slots: &[ModeSlot],
    inputs: &ReverseIndexInputs<'_>,
    admission_seq: u64,
) {
    if populated_slots.is_empty() || inputs.canonicals.is_empty() {
        return;
    }
    let timing_on = verter_execution::request_context::current_timing_enabled();
    // Every registration of this candidate names the one family.
    let family = Arc::new(family.clone());
    // One shard lookup and lock per canonical takes every populated slot.
    for canonical in inputs.canonicals.iter() {
        register_canonical_slots(
            canonical_to_entries,
            canonical,
            &family,
            populated_slots,
            admission_seq,
            &inputs.registered_facts,
            timing_on,
        );
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
    slot: ModeSlot,
    entry: &MemoEntry,
) {
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
/// A [`register_reverse_index`] inserter holds that shard's lock — read
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
    let mut storage = canonical_to_entries.storage.lock();
    let emptied = match canonical_to_entries.get(canonical) {
        Some(shard) => {
            let mut map = shard.value().lock();
            let before = shard_slack_bytes(&map);
            if map.remove(&(seq, slot)).is_some() {
                shrink_drained_shard(&mut map);
            }
            storage.adjust(shard_slack_bytes(&map), before);
            map.is_empty()
        }
        None => return,
    };
    // Only a shard this removal emptied can be dropped; the drop re-checks
    // emptiness under the shard write lock.
    if emptied
        && canonical_to_entries
            .map
            .remove_if(canonical, |_, mutex| mutex.lock().is_empty())
            .is_some()
    {
        // An empty shard keeps no table: only its canonical's text is left.
        storage.adjust(
            0,
            verter_session_query::retention::allocation::arc_block_bytes(canonical.len()),
        );
        canonical_to_entries.settle_outer(&mut storage);
    }
}

/// Single-shard registration step used by
/// [`register_reverse_index`]. Inserts one
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
    // Each record's bucket is its candidate's charge; the slack the table
    // grows beyond its records is the index's.
    let insert = |map: &mut ShardMap, storage: &mut IndexStorage| {
        let before = shard_slack_bytes(map);
        for populated in populated_slots {
            map.insert(
                (admission_seq, *populated),
                (Arc::clone(family), Arc::clone(registered_facts)),
            );
        }
        storage.adjust(shard_slack_bytes(map), before);
    };
    let mut storage = canonical_to_entries.storage.lock();
    // An existing shard takes the registration under its read lock; only a
    // canonical without one copies its name into a new shard. A shard the
    // prune drops between the two steps is re-created by the second, as the
    // single-step path would.
    if let Some(shard) = canonical_to_entries.get(canonical) {
        let mut map = shard.value().lock();
        record_lock_wait(lock_start);
        insert(&mut map, &mut storage);
        return;
    }
    let mut created = false;
    {
        let shard = canonical_to_entries
            .map
            .entry(Arc::from(canonical))
            .or_insert_with(|| {
                created = true;
                Mutex::new(FxHashMap::default())
            });
        let mut map = shard.value().lock();
        record_lock_wait(lock_start);
        insert(&mut map, &mut storage);
    }
    if created {
        storage.adjust(
            verter_session_query::retention::allocation::arc_block_bytes(canonical.len()),
            0,
        );
        canonical_to_entries.settle_outer(&mut storage);
    }
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
/// union [`register_reverse_index`] iterates (carrier `canonical_ids()`
/// PLUS `dispatch_dep_signature`), so a dispatch-only registration
/// cannot survive the removal. Returns whether at least one registration
/// was pruned. The caller holds the family memo's `entries` `Mutex`.
pub(super) fn drain_family_slots_registrations(
    canonical_to_entries: &CanonicalToEntries,
    slots: &FamilySlots,
) -> bool {
    let mut pruned_any = false;
    for (slot, entry) in slots.iter_populated_slots_all() {
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
