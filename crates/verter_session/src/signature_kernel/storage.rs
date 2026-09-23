//! Private append-only interner adapter over `boxcar::Vec`.
//!
//! Sixteen dedup shards are a benchmark parameter. Hash is computed outside
//! shard locks; equality decides collisions. Records are fully initialized
//! before a handle is published. `boxcar` `count()` is not a published-handle
//! range: holes from cancellation or panic are never returned as values.

use std::hash::{Hash, Hasher};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

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

    fn insert(&mut self, hash: u64, index: u32) {
        self.by_hash.entry(hash).or_default().push(index);
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
}

impl<T> AppendInterner<T> {
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
        }
    }

    #[must_use]
    pub fn epoch(&self) -> GraphEpoch {
        self.epoch
    }

    #[must_use]
    pub fn shard_lock_acquires(&self) -> u64 {
        self.shard_lock_acquires.load(Ordering::Relaxed)
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
    /// Slots claimed in this epoch (retention observability).
    #[must_use]
    pub fn len(&self) -> usize {
        usize::try_from(self.claimed_slots.load(Ordering::Relaxed)).unwrap_or(usize::MAX)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn intern(&self, value: T, cancelled: Option<&AtomicBool>) -> Result<u64, InternError>
    where
        T: Eq + Hash,
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
        let index = self.slots.push(value);
        let index_u32 = match u32::try_from(index) {
            Ok(i) if i <= self.max_index => i,
            _ => return Err(InternError::Overflow),
        };
        {
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
            shard.insert(hash, index_u32);
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
        T: Eq + Hash,
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
