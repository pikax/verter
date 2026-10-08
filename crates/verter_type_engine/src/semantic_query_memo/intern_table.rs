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

    /// Hand `out` a handle to every same-kind child this value owns,
    /// leaving the value holding none — even when the value's storage is
    /// shared or weakly observed and cannot be mutated in place. Called
    /// only on a value whose record is being destroyed; a kind with no
    /// same-kind children keeps the default.
    fn take_children(&mut self, out: &mut Vec<Interned<Self>>) {
        let _ = out;
    }
}

/// Declare `$ty` an [`InternDomain`] with a private static index. Extra
/// trait items (a `take_children` override) follow in braces.
macro_rules! intern_domain {
    ($ty:ty) => {
        $crate::semantic_query_memo::intern_table::intern_domain!($ty {});
    };
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
    slots: Mutex<FxHashMap<u64, Bucket<T>>>,
}

type Bucket<T> = SmallVec<[Weak<InternRecord<T>>; 1]>;

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
    digest: u64,
    value: T,
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
            slots: parking_lot::const_mutex(FxHashMap::with_hasher(rustc_hash::FxBuildHasher)),
        }
    }

    /// The handle for `value`'s content: the live record when one exists,
    /// otherwise a fresh record owned by the returned handle.
    fn intern(&self, value: T) -> Interned<T> {
        let digest = digest_of(&value);
        // Upgraded candidates whose content differs. Dropping one may
        // destroy its record, whose destructor takes this lock, so they
        // are released only after the lock is.
        let mut mismatched: SmallVec<[Arc<InternRecord<T>>; 1]> = SmallVec::new();
        let mut slots = self.slots.lock();
        let bucket = slots.entry(digest).or_default();
        let mut found = None;
        for weak in bucket.iter() {
            if let Some(record) = weak.upgrade() {
                if record.value == value {
                    found = Some(record);
                    break;
                }
                mismatched.push(record);
            }
        }
        let record = match found {
            Some(record) => {
                drop(slots);
                drop(mismatched);
                // The caller's duplicate may own child handles of this
                // same kind; it is released with the lock already free.
                drop(value);
                return Interned { record };
            }
            None => Arc::new(InternRecord { digest, value }),
        };
        bucket.push(Arc::downgrade(&record));
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
        if let Some(bucket) = slots.get_mut(&digest) {
            bucket.retain(|weak| weak.strong_count() > 0);
            if bucket.is_empty() {
                slots.remove(&digest);
            } else if bucket.spilled() && bucket.capacity() > bucket.len().saturating_mul(4) {
                bucket.shrink_to_fit();
            }
        }
        let floor = RETAINED_SLOT_FLOOR.max(slots.len());
        if slots.capacity() > floor.saturating_mul(4) {
            slots.shrink_to(floor.saturating_mul(2));
        }
    }

    /// Current occupancy and backing capacity, read under one lock.
    #[must_use]
    pub fn occupancy(&self) -> InternIndexOccupancy {
        let slots = self.slots.lock();
        let mut occupancy = InternIndexOccupancy {
            digests: slots.len(),
            digest_capacity: slots.capacity(),
            ..InternIndexOccupancy::default()
        };
        for bucket in slots.values() {
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
        self.slots.lock().is_empty()
    }

    /// Backing capacity of the digest index, in buckets.
    #[cfg(test)]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.slots.lock().capacity()
    }

    /// Backing capacity of the collision bucket `value`'s digest selects,
    /// in weak entries; `None` when no entry carries that digest.
    #[cfg(test)]
    #[must_use]
    pub fn bucket_capacity(&self, value: &T) -> Option<usize> {
        self.slots
            .lock()
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
        T::index().intern(value)
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

#[cfg(test)]
#[path = "intern_table_tests.rs"]
mod tests;
