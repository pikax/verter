//! Private append-only interner adapter over `boxcar::Vec`.
//!
//! Sixteen dedup shards are a benchmark parameter. Hash is computed outside
//! shard locks; equality decides collisions. Records are fully initialized
//! before a handle is published. `boxcar` `count()` is not a published-handle
//! range: holes from cancellation or panic are never returned as values.

use std::hash::{Hash, Hasher};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;
use rustc_hash::FxHasher;

use super::records::{pack_handle, GraphEpoch};

/// Dedup shard count (benchmark parameter, not an ABI constant).
pub const DEDUP_SHARDS: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InternError {
    Cancelled,
    Overflow,
    Panicked,
}

/// The heap one interned record owns beyond its slot.
pub trait RecordHeap {
    /// Bytes the record keeps reachable beyond its inline slot.
    fn record_heap_bytes(&self) -> usize {
        0
    }
}

/// The granule an epoch's storage is charged in.
const EPOCH_STORAGE_GRANULE: usize = 64 * 1024;

/// Where boxcar 0.2.14 (the `Cargo.lock` pin) places index `i`, from its
/// `src/buckets.rs`: `SKIPPED_BUCKETS = 5` and
/// `SKIPPED_ENTRIES = 2^SKIPPED_BUCKETS - 1`, so `Index::new(i)` stores
/// `i + SKIPPED_ENTRIES + 1` — index `i` sits at position `i + 32` — and
/// `Index::location` puts bucket `b` over the positions `[2^b, 2^(b+1))`,
/// each bucket a boxed slice of `2^b` `vec::raw::Entry<T>` (the value
/// beside its `AtomicBool` presence flag). A boxcar upgrade must re-check
/// this model against that file.
const BOXCAR_FIRST_POSITION: usize = 32;

/// Boxcar 0.2.14's `Buckets::get_or_alloc` (`src/buckets.rs`) eagerly
/// allocates the NEXT bucket when a write lands on entry
/// `bucket_len - bucket_len / 8` of the current one (`alloc_bucket_after`):
/// the next bucket exists from that write on.
const fn boxcar_eager_entry(bucket_len: usize) -> usize {
    bucket_len - (bucket_len >> 3)
}

/// One epoch's storage charge: every table's slot buckets, the records'
/// own heap and the dedup indexes, pinned against the store's account from
/// the moment they are allocated until the epoch's last holder drops — the
/// epoch's retirement releases them. Append-only tables only grow; the
/// epoch's side maps also shrink.
///
/// Growth that fits the published charge takes no lock; a settlement
/// re-reads the live total after publishing its charge and settles again
/// when growth landed meanwhile, so a shrink computed from an older total
/// never leaves the charge below the live storage.
pub struct EpochStorage {
    account: Arc<OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>>,
    used: AtomicUsize,
    charged: AtomicUsize,
    pin: Mutex<Option<verter_session_query::retention::granular::GranularPin>>,
    /// Test hook run by a settlement right after it reads the live total.
    #[cfg(test)]
    settle_hook: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

impl EpochStorage {
    /// Storage charged to the account `account` binds (the process-local
    /// one when the store never binds one).
    #[must_use]
    pub fn new(
        account: Arc<OnceLock<Arc<verter_session_query::retention::SemanticRetentionAccount>>>,
    ) -> Arc<Self> {
        Arc::new(Self {
            account,
            used: AtomicUsize::new(0),
            charged: AtomicUsize::new(0),
            pin: Mutex::new(None),
            #[cfg(test)]
            settle_hook: Mutex::new(None),
        })
    }

    /// `bytes` more storage. The account's lock is taken only when the total
    /// crosses the charged granule.
    ///
    /// The total is raised before the published charge is read (both
    /// sequentially consistent), so a growth that skips settlement because
    /// it fits an older, larger charge is seen by the settlement replacing
    /// that charge, which re-reads the total after publishing.
    pub fn add(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let used = self.used.fetch_add(bytes, Ordering::SeqCst) + bytes;
        if used <= self.charged.load(Ordering::SeqCst) {
            return;
        }
        self.settle();
    }

    /// `bytes` of storage freed.
    pub fn sub(&self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        self.used.fetch_sub(bytes, Ordering::SeqCst);
        self.settle();
    }

    fn settle(&self) {
        let mut pin = self.pin.lock();
        let pin = pin.get_or_insert_with(|| {
            let account = self.account.get_or_init(
                verter_session_query::retention::SemanticRetentionAccount::process_local,
            );
            verter_session_query::retention::granular::GranularPin::new(
                account,
                EPOCH_STORAGE_GRANULE,
            )
        });
        loop {
            let used = self.used.load(Ordering::SeqCst);
            #[cfg(test)]
            {
                let hook = self.settle_hook.lock().clone();
                if let Some(hook) = hook {
                    hook();
                }
            }
            if used > pin.used() {
                pin.add(used - pin.used());
            } else {
                pin.sub(pin.used() - used);
            }
            self.charged.store(pin.charged(), Ordering::SeqCst);
            // Growth that read the previous, larger charge skipped its own
            // settlement: cover it now.
            if self.used.load(Ordering::SeqCst) <= pin.charged() {
                return;
            }
        }
    }

    /// Bytes currently charged for the epoch's storage.
    #[must_use]
    pub fn charged(&self) -> usize {
        self.charged.load(Ordering::SeqCst)
    }

    /// The live storage total.
    #[cfg(test)]
    pub(crate) fn used(&self) -> usize {
        self.used.load(Ordering::SeqCst)
    }

    /// Run `hook` in every settlement right after it reads the live total.
    #[cfg(test)]
    pub(crate) fn set_settle_hook(&self, hook: Option<Arc<dyn Fn() + Send + Sync>>) {
        *self.settle_hook.lock() = hook;
    }

    /// Charge the growth of a side map from `before` to `after` buckets of
    /// `entry` bytes each (with each bucket's control byte).
    pub fn map_growth(&self, before: usize, after: usize, entry: usize) {
        if after > before {
            self.add((after - before) * (entry + 1));
        }
    }
}

struct DedupShard {
    by_hash: rustc_hash::FxHashMap<u64, Vec<u32>>,
}

impl DedupShard {
    fn new() -> Self {
        Self {
            by_hash: rustc_hash::FxHashMap::default(),
        }
    }

    fn candidates(&self, hash: u64) -> &[u32] {
        self.by_hash.get(&hash).map(Vec::as_slice).unwrap_or(&[])
    }

    /// Record `index` under `hash`; the storage bytes the index grew by.
    fn insert(&mut self, hash: u64, index: u32) -> usize {
        use verter_session_query::retention::allocation::heap_block_bytes;
        let table_before = self.by_hash.capacity();
        let ids = self.by_hash.entry(hash).or_default();
        let ids_before = ids.capacity();
        ids.push(index);
        let ids_growth = heap_block_bytes(ids.capacity() * std::mem::size_of::<u32>())
            - heap_block_bytes(ids_before * std::mem::size_of::<u32>());
        let table_growth = self.by_hash.capacity().saturating_sub(table_before)
            * (std::mem::size_of::<(u64, Vec<u32>)>() + 1);
        ids_growth + table_growth
    }
}

/// Append-only intern table. `T` is stored by value; reads return `&T`.
pub struct AppendInterner<T> {
    epoch: GraphEpoch,
    shards: [Mutex<DedupShard>; DEDUP_SHARDS],
    slots: boxcar::Vec<T>,
    shard_lock_acquires: AtomicU64,
    /// Inclusive maximum published index. Production uses `u32::MAX`.
    max_index: u32,
    /// Reservations that passed the overflow check. Incremented before
    /// `push` so a racy loser never plants an unpublished boxcar slot.
    claimed_slots: AtomicU64,
    /// The epoch storage charge this table's growth is added to; `None`
    /// for a fixture table outside any store.
    storage: Option<Arc<EpochStorage>>,
    /// Bit `b` is set once boxcar bucket `b` is charged, so a bucket opened
    /// either eagerly or by its first write is charged exactly once.
    charged_buckets: AtomicU64,
}

impl<T> AppendInterner<T> {
    /// A table whose every allocation is charged to `storage`.
    #[must_use]
    pub fn charged(epoch: GraphEpoch, storage: &Arc<EpochStorage>) -> Self {
        let mut table = Self::with_max_index(epoch, u32::MAX);
        table.storage = Some(Arc::clone(storage));
        table
    }

    /// A fixture table charging no account.
    #[cfg(any(test, feature = "test-support"))]
    #[must_use]
    pub fn new(epoch: GraphEpoch) -> Self {
        Self::with_max_index(epoch, u32::MAX)
    }

    #[must_use]
    pub fn with_max_index(epoch: GraphEpoch, max_index: u32) -> Self {
        Self {
            epoch,
            shards: std::array::from_fn(|_| Mutex::new(DedupShard::new())),
            slots: boxcar::Vec::new(),
            shard_lock_acquires: AtomicU64::new(0),
            max_index,
            claimed_slots: AtomicU64::new(0),
            storage: None,
            charged_buckets: AtomicU64::new(0),
        }
    }

    /// The buckets a write at `index` may have opened that this table has
    /// not charged yet: the bucket holding it, and — at the eager point —
    /// the next one (see [`BOXCAR_FIRST_POSITION`] and
    /// [`boxcar_eager_entry`]). Each bucket is `2^b` entries, a record and
    /// its presence flag each.
    pub(super) fn opened_bucket_bytes(&self, index: usize) -> usize {
        let position = index + BOXCAR_FIRST_POSITION;
        let bucket = position.ilog2();
        let bucket_len = 1_usize << bucket;
        let mut bytes = self.charge_bucket(bucket);
        if position - bucket_len == boxcar_eager_entry(bucket_len) {
            bytes += self.charge_bucket(bucket + 1);
        }
        bytes
    }

    /// The bytes of bucket `bucket` the first time it is charged, `0` after.
    fn charge_bucket(&self, bucket: u32) -> usize {
        let bit = 1_u64 << bucket;
        if self.charged_buckets.fetch_or(bit, Ordering::AcqRel) & bit != 0 {
            return 0;
        }
        verter_session_query::retention::allocation::heap_block_bytes(
            (1_usize << bucket) * std::mem::size_of::<(T, AtomicBool)>(),
        )
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.epoch
    }

    #[must_use]
    pub fn shard_lock_acquires(&self) -> u64 {
        self.shard_lock_acquires.load(Ordering::Relaxed)
    }

    /// Records this table holds, including a duplicate a racing publisher
    /// stored before equality chose the other id. It is the table's
    /// retained size, not a published-handle range.
    #[must_use]
    pub fn record_count(&self) -> usize {
        self.slots.count()
    }

    fn shard_index(hash: u64) -> usize {
        (hash as usize) % DEDUP_SHARDS
    }

    fn digest(value: &T) -> u64
    where
        T: Hash,
    {
        let mut hasher = FxHasher::default();
        value.hash(&mut hasher);
        hasher.finish()
    }

    fn lock_shard(&self, idx: usize) -> parking_lot::MutexGuard<'_, DedupShard> {
        self.shard_lock_acquires.fetch_add(1, Ordering::Relaxed);
        self.shards[idx].lock()
    }

    /// Borrow a published slot. Does not take a shard lock.
    #[must_use]
    pub fn get(&self, index: u32) -> Option<&T> {
        self.slots.get(index as usize)
    }

    /// Intern `value`. Fully initializes the record before publishing a handle.
    /// Duplicate publishers race on the shard; equality selects one id. A
    /// cancelled or panicking producer publishes nothing.
    pub fn intern(&self, value: T, cancelled: Option<&AtomicBool>) -> Result<u64, InternError>
    where
        T: Eq + Hash + RecordHeap,
    {
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(InternError::Cancelled);
        }
        let hash = Self::digest(&value);
        let shard_idx = Self::shard_index(hash);
        if let Some(existing) = self.lookup_equal(shard_idx, hash, &value) {
            return Ok(pack_handle(self.epoch, existing));
        }
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(InternError::Cancelled);
        }
        // Reserve before push so overflow never plants an unpublished hole,
        // including when two publishers both observe `count() <= max_index`.
        // Do not decrement on overflow: a fetch_sub would reopen capacity
        // while a winner's push is still in flight.
        let claimed = self.claimed_slots.fetch_add(1, Ordering::Relaxed);
        if claimed > u64::from(self.max_index) {
            if let Some(existing) = self.lookup_equal(shard_idx, hash, &value) {
                return Ok(pack_handle(self.epoch, existing));
            }
            return Err(InternError::Overflow);
        }
        let heap = value.record_heap_bytes();
        let index = self.slots.push(value);
        // The stored record — even one a racing duplicate makes redundant —
        // is retained storage from here on.
        if let Some(storage) = &self.storage {
            storage.add(heap + self.opened_bucket_bytes(index));
        }
        let index_u32 = match u32::try_from(index) {
            Ok(i) if i <= self.max_index => i,
            _ => return Err(InternError::Overflow),
        };
        let index_growth = {
            let mut shard = self.lock_shard(shard_idx);
            if let Some(ids) = shard.by_hash.get(&hash) {
                if let Some(mine) = self.slots.get(index) {
                    if let Some(id) = ids.iter().copied().find(|&id| {
                        self.slots
                            .get(id as usize)
                            .is_some_and(|stored| stored == mine)
                    }) {
                        return Ok(pack_handle(self.epoch, id));
                    }
                }
            }
            shard.insert(hash, index_u32)
        };
        if let Some(storage) = &self.storage {
            storage.add(index_growth);
        }
        Ok(pack_handle(self.epoch, index_u32))
    }

    /// Look up `value` without publishing. Miss is `None`.
    #[must_use]
    pub fn lookup(&self, value: &T) -> Option<u64>
    where
        T: Eq + Hash,
    {
        let hash = Self::digest(value);
        let shard_idx = Self::shard_index(hash);
        self.lookup_equal(shard_idx, hash, value)
            .map(|index| pack_handle(self.epoch, index))
    }

    /// Build then intern. A panic in `build` publishes no handle.
    pub fn intern_with<F>(
        &self,
        cancelled: Option<&AtomicBool>,
        build: F,
    ) -> Result<u64, InternError>
    where
        T: Eq + Hash + RecordHeap,
        F: FnOnce() -> T,
    {
        if cancelled.is_some_and(|c| c.load(Ordering::Acquire)) {
            return Err(InternError::Cancelled);
        }
        match catch_unwind(AssertUnwindSafe(build)) {
            Ok(value) => self.intern(value, cancelled),
            Err(_) => Err(InternError::Panicked),
        }
    }

    fn lookup_equal(&self, shard_idx: usize, hash: u64, value: &T) -> Option<u32>
    where
        T: Eq,
    {
        let shard = self.lock_shard(shard_idx);
        shard.candidates(hash).iter().copied().find(|&id| {
            self.slots
                .get(id as usize)
                .is_some_and(|stored| stored == value)
        })
    }
}

/// Records whose every byte is inline in their slot.
macro_rules! inline_records {
    ($($t:ty),* $(,)?) => {
        $(impl RecordHeap for $t {})*
    };
}

inline_records!(
    u64,
    crate::semantic_query::SemanticNodeId,
    super::records::SignatureInputShape,
    super::records::SignatureTemplate,
    super::records::SignatureDescriptor,
    super::records::SignatureResultRecipe,
    super::records::ParameterSlot,
    super::records::AppliedResult,
    super::provenance::SignatureProvenance,
);

impl RecordHeap for Arc<str> {
    fn record_heap_bytes(&self) -> usize {
        verter_session_query::retention::allocation::arc_block_bytes(self.len())
    }
}

/// A boxed slice's backing.
fn boxed_slice_bytes<T>(slice: &[T]) -> usize {
    verter_session_query::retention::allocation::heap_block_bytes(std::mem::size_of_val(slice))
}

impl RecordHeap for super::records::ParameterLayout {
    fn record_heap_bytes(&self) -> usize {
        boxed_slice_bytes(&self.parameters)
            + self
                .rest
                .as_ref()
                .map_or(0, |rest| boxed_slice_bytes(&rest.tail))
    }
}

impl RecordHeap for super::records::BinderSpace {
    fn record_heap_bytes(&self) -> usize {
        boxed_slice_bytes(&self.binders)
    }
}

impl RecordHeap for super::records::SignatureSet {
    fn record_heap_bytes(&self) -> usize {
        boxed_slice_bytes(&self.candidates)
    }
}

impl RecordHeap for super::provenance::ConstituentSequence {
    fn record_heap_bytes(&self) -> usize {
        boxed_slice_bytes(&self.edges)
    }
}

impl RecordHeap for crate::semantic_query::CanonicalTypeSubstitution {
    /// The substitution's pair list and its root attachment.
    fn record_heap_bytes(&self) -> usize {
        self.retained_heap_bytes()
    }
}

impl RecordHeap for super::substitution::CallSubstitution {
    fn record_heap_bytes(&self) -> usize {
        match self {
            Self::Map { map, .. } => map.record_heap_bytes(),
            _ => 0,
        }
    }
}
