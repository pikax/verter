//! Owned identity handles over a weak deduplication index.
//!
//! Semantic identity records — intersection recipes, semantic contexts,
//! order domains and the large family-key payloads — are owned by the
//! handles that name them, never by a table. An [`Interned<T>`] is one
//! `Arc` pointer: holding it keeps its record alive, and the last handle
//! dropping frees the record. The [`WeakInternTable`] each identity kind
//! declares through [`InternDomain`] only DEDUPLICATES: it maps a content
//! digest to `Weak` references, owns no record, and forgets an entry the
//! moment its record is destroyed, so after every owner drains both the
//! records and the index's backing capacity return to baseline.
//!
//! Identity is exact equality of the interned value. Two handles are equal
//! when they share a record, or when their digests match AND their values
//! compare equal. The digest accelerates lookup and rejects most
//! inequalities; it never substitutes for the value check, so a digest
//! collision cannot alias two identities. No raw ordinal is ever handed
//! out, so there is no slot number another index could also mint.
//!
//! **Retained children.** A record owns exactly what its value owns. A
//! value that embeds other handles (an ordered-steps recipe whose
//! subgroups are themselves recipes) keeps those children alive for as
//! long as the parent record lives; the index never holds a child alive.
//! A kind whose values nest handles of the SAME kind declares
//! [`InternDomain::take_children`], and a destroyed record then reclaims
//! its whole released subtree iteratively, so the depth of a nesting chain
//! never becomes the depth of the destructor's stack.
//!
//! **Storage charge.** A record is a self-charged allocation: its block
//! and its value's own heap are pinned against the process retention
//! account from the moment the record exists until its last handle drops,
//! so a holder of a handle pays only the handle's inline bytes. Each index
//! charges its own backing — its digest table and spilled collision
//! buckets — by capacity, settled as entries come and go.
//!
//! **Occupancy.** [`WeakInternTable::occupancy`] reads an index's records,
//! digests and both backing capacities under its lock, and
//! [`IdentityIndexSnapshot`] collects every engine index for the host's
//! retention snapshot. Both are always compiled; neither keeps a counter.
//!
//! **Lock discipline.** A record's destructor locks its index to forget
//! itself. Nothing that can destroy a record of the same kind — an
//! upgraded candidate that turned out not to match, or the caller's
//! duplicate value with its own child handles — is dropped while that
//! index's lock is held.

use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::{Arc, Weak};

use parking_lot::Mutex;
use rustc_hash::{FxHashMap, FxHasher};
use smallvec::SmallVec;

/// Capacity a drained index may keep without shrinking.
const RETAINED_SLOT_FLOOR: usize = 16;

/// An identity kind with its own weak deduplication index.
pub trait InternDomain: Hash + Eq + Send + Sync + Sized + 'static {
    /// The kind's index. One per kind, owning no record.
    fn index() -> &'static WeakInternTable<Self>;

    /// The heap this value owns beyond its inline bytes, charged with its
    /// record. A same-kind child handle the value embeds is its own
    /// self-charged record; only the handle's own attachments count here.
    ///
    /// Owned handles the value reaches are offered to `aliases`: the record
    /// pins them, each charged by its own cell.
    fn value_heap_bytes(&self, aliases: &mut verter_session_query::retention::allocation::AliasSet) -> usize;

    /// Hand `out` a handle to every same-kind child this value owns,
    /// leaving the value holding none — even when the value's storage is
    /// shared or weakly observed and cannot be mutated in place. Called
    /// only on a value whose record is being destroyed; a kind with no
    /// same-kind children keeps the default.
    fn take_children(&mut self, out: &mut Vec<Interned<Self>>) {
        let _ = out;
    }
}

/// Declare `$ty` an [`InternDomain`] with a private static index. The
/// trait items follow in braces: `value_heap_bytes`, and optionally a
/// `take_children` override.
macro_rules! intern_domain {
    ($ty:ty { $($items:tt)* }) => {
        impl $crate::semantic_query_memo::intern_table::InternDomain for $ty {
            fn index() -> &'static $crate::semantic_query_memo::intern_table::WeakInternTable<Self>
            {
                static INDEX: $crate::semantic_query_memo::intern_table::WeakInternTable<$ty> =
                    $crate::semantic_query_memo::intern_table::WeakInternTable::new();
                &INDEX
            }
            $($items)*
        }
    };
}
pub(crate) use intern_domain;

/// A weak, non-owning deduplication index over [`Interned`] records.
pub struct WeakInternTable<T: InternDomain> {
    slots: Mutex<IndexTable<T>>,
}

type Bucket<T> = SmallVec<[Weak<InternRecord<T>>; 1]>;

/// An index's digest table and the pinned charge of its backing.
struct IndexTable<T: InternDomain> {
    map: FxHashMap<u64, Bucket<T>>,
    backing: Option<verter_session_query::retention::granular::GranularPin>,
}

/// Granule an index's backing is charged in.
const INDEX_BACKING_GRANULE: usize = 1024;

impl<T: InternDomain> IndexTable<T> {
    /// Bytes one digest bucket holds in the table: its key, its inline
    /// entry and its control byte.
    const TABLE_BUCKET_BYTES: usize = std::mem::size_of::<(u64, Bucket<T>)>() + 1;

    /// Bytes a collision bucket's spilled entries hold.
    fn spilled_bytes(bucket: &Bucket<T>) -> usize {
        if bucket.spilled() {
            verter_session_query::retention::allocation::heap_block_bytes(
                bucket.capacity() * std::mem::size_of::<Weak<InternRecord<T>>>(),
            )
        } else {
            0
        }
    }

    fn table_bytes(&self) -> usize {
        self.map.capacity() * Self::TABLE_BUCKET_BYTES
    }

    /// Settle the backing charge after the backing changed from `before`
    /// to `after` bytes.
    fn settle(&mut self, after: usize, before: usize) {
        if after == before {
            return;
        }
        let backing = self.backing.get_or_insert_with(|| {
            verter_session_query::retention::granular::GranularPin::new(
                &verter_session_query::retention::SemanticRetentionAccount::process_local(),
                INDEX_BACKING_GRANULE,
            )
        });
        if after > before {
            backing.add(after - before);
        } else {
            backing.sub(before - after);
        }
    }
}

/// What one identity index holds right now. Every figure is read from the
/// index itself under its lock; nothing is a counter or a history.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct InternIndexOccupancy {
    /// Indexed records: live, or destroyed and not yet forgotten.
    pub records: usize,
    /// Distinct content digests the index maps.
    pub digests: usize,
    /// Backing capacity of the digest map, in buckets.
    pub digest_capacity: usize,
    /// Heap-backed capacity of collision buckets that outgrew their single
    /// inline entry, in weak entries.
    pub collision_entry_capacity: usize,
}

/// Occupancy of every identity index the engine declares, the
/// production lifetime count of these process-wide tables.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct IdentityIndexSnapshot {
    pub intersection_recipes: InternIndexOccupancy,
    pub semantic_contexts: InternIndexOccupancy,
    pub order_domains: InternIndexOccupancy,
    pub relate_keys: InternIndexOccupancy,
    pub resolve_call_keys: InternIndexOccupancy,
}

impl IdentityIndexSnapshot {
    /// Read every index, each under its own lock.
    #[must_use]
    pub fn capture() -> Self {
        use crate::semantic_query::{
            semantic_context::{OrderDomainKey, SemanticContext},
            IntersectionRecipe, RelateMemoKey, ResolveCallKey,
        };
        Self {
            intersection_recipes: IntersectionRecipe::index().occupancy(),
            semantic_contexts: SemanticContext::index().occupancy(),
            order_domains: OrderDomainKey::index().occupancy(),
            relate_keys: RelateMemoKey::index().occupancy(),
            resolve_call_keys: ResolveCallKey::index().occupancy(),
        }
    }

    fn indexes(&self) -> [&InternIndexOccupancy; 5] {
        [
            &self.intersection_recipes,
            &self.semantic_contexts,
            &self.order_domains,
            &self.relate_keys,
            &self.resolve_call_keys,
        ]
    }

    /// Indexed records across every index.
    #[must_use]
    pub fn records(&self) -> usize {
        self.indexes().iter().map(|index| index.records).sum()
    }

    /// Backing capacity across every index: digest-map buckets plus
    /// spilled collision entries.
    #[must_use]
    pub fn backing_slots(&self) -> usize {
        self.indexes()
            .iter()
            .map(|index| index.digest_capacity + index.collision_entry_capacity)
            .sum()
    }
}

/// One interned record: the value plus its content digest. Freed with
/// its last owning handle; its destructor forgets the index entry.
struct InternRecord<T: InternDomain> {
    arena: Option<usize>,
    digest: u64,
    value: T,
    /// The record's block and its value's heap, pinned for exactly as long
    /// as the record lives.
    _charge: verter_session_query::retention::RetentionCharge,
}

/// An owning handle to one interned value. See the module docs.
pub struct Interned<T: InternDomain> {
    record: Arc<InternRecord<T>>,
}

fn digest_of<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

impl<T: InternDomain> WeakInternTable<T> {
    /// An empty index. `const` so a domain's index can live in a `static`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: parking_lot::const_mutex(IndexTable {
                map: FxHashMap::with_hasher(rustc_hash::FxBuildHasher),
                backing: None,
            }),
        }
    }

    /// The handle for `value`'s content: the live record when one exists,
    /// otherwise a fresh record owned by the returned handle.
    fn intern(&self, value: T, arena: Option<usize>) -> Interned<T> {
        let digest = digest_of(&value);
        // Upgraded candidates whose content differs. Dropping one may
        // destroy its record, whose destructor takes this lock, so they
        // are released only after the lock is.
        let mut mismatched: SmallVec<[Arc<InternRecord<T>>; 1]> = SmallVec::new();
        let mut slots = self.slots.lock();
        let table_before = slots.table_bytes();
        let bucket = slots.map.entry(digest).or_default();
        let spilled_before = IndexTable::<T>::spilled_bytes(bucket);
        let mut found = None;
        for weak in bucket.iter() {
            if let Some(record) = weak.upgrade() {
                if record.arena == arena && record.value == value {
                    found = Some(record);
                    break;
                }
                mismatched.push(record);
            }
        }
        let record = match found {
            Some(record) => {
                let spilled_after = IndexTable::<T>::spilled_bytes(bucket);
                let table_after = slots.table_bytes();
                slots.settle(table_after + spilled_after, table_before + spilled_before);
                drop(slots);
                drop(mismatched);
                // The caller's duplicate may own child handles of this
                // same kind; it is released with the lock already free.
                drop(value);
                return Interned { record };
            }
            None => {
                let mut aliases = verter_session_query::retention::allocation::AliasSet::new();
                let bytes = verter_session_query::retention::allocation::arc_block_bytes(
                    std::mem::size_of::<InternRecord<T>>(),
                ) + value.value_heap_bytes(&mut aliases);
                aliases.take_handles().pin(verter_session_query::retention::owned::ChargeCtx::new(
                    &verter_session_query::retention::SemanticRetentionAccount::process_local(),
                ));
                Arc::new(InternRecord {
                    arena,
                    digest,
                    value,
                    _charge:
                        verter_session_query::retention::SemanticRetentionAccount::process_local()
                            .pin(bytes),
                })
            }
        };
        bucket.push(Arc::downgrade(&record));
        let spilled_after = IndexTable::<T>::spilled_bytes(bucket);
        let table_after = slots.table_bytes();
        slots.settle(table_after + spilled_after, table_before + spilled_before);
        drop(slots);
        drop(mismatched);
        Interned { record }
    }

    /// The live handle for `value`'s content, without minting one.
    #[cfg(test)]
    #[must_use]
    pub fn get(&self, value: &T) -> Option<Interned<T>> {
        let digest = digest_of(value);
        let mut candidates: SmallVec<[Arc<InternRecord<T>>; 1]> = {
            let slots = self.slots.lock();
            slots
                .map
                .get(&digest)
                .map(|bucket| bucket.iter().filter_map(Weak::upgrade).collect())
                .unwrap_or_default()
        };
        let index = candidates
            .iter()
            .position(|record| record.value == *value)?;
        Some(Interned {
            record: candidates.swap_remove(index),
        })
    }

    /// Forget the entries of destroyed records under `digest` — the
    /// caller's own and any other already-dead peer — releasing a
    /// surviving bucket's and the index's backing capacity once each has
    /// drained well below it.
    fn forget(&self, digest: u64) {
        let mut slots = self.slots.lock();
        let table_before = slots.table_bytes();
        let mut spilled_before = 0;
        let mut spilled_after = 0;
        if let Some(bucket) = slots.map.get_mut(&digest) {
            spilled_before = IndexTable::<T>::spilled_bytes(bucket);
            bucket.retain(|weak| weak.strong_count() > 0);
            if bucket.is_empty() {
                slots.map.remove(&digest);
            } else {
                if bucket.spilled() && bucket.capacity() > bucket.len().saturating_mul(4) {
                    bucket.shrink_to_fit();
                }
                spilled_after = IndexTable::<T>::spilled_bytes(bucket);
            }
        }
        let len = slots.map.len();
        if len == 0 {
            slots.map = FxHashMap::with_hasher(rustc_hash::FxBuildHasher);
        } else {
            let floor = RETAINED_SLOT_FLOOR.max(len);
            if slots.map.capacity() > floor.saturating_mul(4) {
                slots.map.shrink_to(floor.saturating_mul(2));
            }
        }
        let table_after = slots.table_bytes();
        slots.settle(table_after + spilled_after, table_before + spilled_before);
    }

    /// Current occupancy and backing capacity, read under one lock.
    #[must_use]
    pub fn occupancy(&self) -> InternIndexOccupancy {
        let slots = self.slots.lock();
        let mut occupancy = InternIndexOccupancy {
            digests: slots.map.len(),
            digest_capacity: slots.map.capacity(),
            ..InternIndexOccupancy::default()
        };
        for bucket in slots.map.values() {
            occupancy.records += bucket.len();
            if bucket.spilled() {
                occupancy.collision_entry_capacity += bucket.capacity();
            }
        }
        occupancy
    }

    /// Indexed records (live, or destroyed and not yet forgotten).
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.occupancy().records
    }

    #[cfg(test)]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.lock().map.is_empty()
    }

    /// Backing capacity of the digest index, in buckets.
    #[cfg(test)]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.lock().map.capacity()
    }

    /// Bytes the index's backing is charged.
    #[cfg(test)]
    #[must_use]
    pub fn backing_charge_bytes(&self) -> usize {
        self.slots.lock().backing.as_ref().map_or(
            0,
            verter_session_query::retention::granular::GranularPin::charged,
        )
    }

    /// Backing capacity of the collision bucket `value`'s digest selects,
    /// in weak entries; `None` when no entry carries that digest.
    #[cfg(test)]
    #[must_use]
    pub fn bucket_capacity(&self, value: &T) -> Option<usize> {
        self.slots
            .lock()
            .map
            .get(&digest_of(value))
            .map(SmallVec::capacity)
    }
}

impl<T: InternDomain> Default for WeakInternTable<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: InternDomain> Drop for InternRecord<T> {
    fn drop(&mut self) {
        T::index().forget(self.digest);
        // Reclaim the released subtree with an explicit worklist: a child
        // this record held last is unwrapped and emptied here, so its own
        // destructor finds nothing left to release and never recurses.
        let mut released = Vec::new();
        self.value.take_children(&mut released);
        while let Some(child) = released.pop() {
            if let Some(mut record) = Arc::into_inner(child.record) {
                record.value.take_children(&mut released);
            }
        }
    }
}

impl<T: InternDomain> Interned<T> {
    /// The owning handle for `value`'s content in its kind's index.
    #[must_use]
    pub fn new(value: T) -> Self {
        T::index().intern(value, None)
    }

    /// Separate node-bearing records by their issuing arena so a shared
    /// content record cannot import another arena's nested root attachments.
    /// Equality and hashing still compare only semantic content.
    pub(crate) fn new_scoped(value: T) -> Self {
        T::index().intern(
            value,
            super::node_roots::NodeRootScope::current_arena_identity(),
        )
    }

    /// The interned value.
    #[must_use]
    pub fn value(&self) -> &T {
        &self.record.value
    }

    /// The record's content digest.
    #[must_use]
    pub fn digest(&self) -> u64 {
        self.record.digest
    }

    /// Whether both handles name the same record.
    #[must_use]
    pub fn same_record(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.record, &other.record)
    }
}

impl<T: InternDomain> Clone for Interned<T> {
    fn clone(&self) -> Self {
        Self {
            record: Arc::clone(&self.record),
        }
    }
}

impl<T: InternDomain> PartialEq for Interned<T> {
    fn eq(&self, other: &Self) -> bool {
        self.same_record(other)
            || (self.record.digest == other.record.digest
                && self.record.value == other.record.value)
    }
}

impl<T: InternDomain> Eq for Interned<T> {}

impl<T: InternDomain> Hash for Interned<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        state.write_u64(self.record.digest);
    }
}

impl<T: InternDomain> Deref for Interned<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.record.value
    }
}

impl<T: InternDomain + fmt::Debug> fmt::Debug for Interned<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.record.value.fmt(f)
    }
}

impl<T: InternDomain> Interned<T> {
    /// Bytes the record this handle names is charged.
    #[cfg(test)]
    #[must_use]
    pub fn record_charge_bytes(&self) -> usize {
        self.record._charge.bytes()
    }
}

#[cfg(test)]
#[path = "intern_table_tests.rs"]
mod tests;
