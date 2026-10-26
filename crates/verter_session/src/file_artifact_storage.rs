//! Storage accounting for versioned artifact entries. Payload pins and table
//! backing follow their actual owners; no scan of the store's payloads is
//! needed on publication or retirement.

use dashmap::{
    mapref::{entry::Entry, one::RefMut},
    DashMap,
};
use smallvec::{Array, SmallVec};
use std::hash::Hash;
use std::ops::Deref;
use verter_retention::heap_size::{hash_table_bytes, HeapSize};
use verter_retention::pinned_heap::{PinContext, PinnedHeap};
use verter_retention::{allocation::heap_block_bytes, granular::GranularPin};

pub(super) struct PinnedChain<A: Array> {
    values: SmallVec<A>,
    pins: SmallVec<[Option<PinnedHeap>; 2]>,
    _key: PinnedHeap,
    backing: GranularPin,
}

impl<A: Array> PinnedChain<A> {
    fn new(key: &impl HeapSize) -> Self {
        let context = PinContext::default();
        Self {
            values: SmallVec::new(),
            pins: SmallVec::new(),
            _key: PinnedHeap::measure(key, 0, &context),
            backing: GranularPin::new(context.account(), 1024),
        }
    }
    fn settle(&mut self) {
        let values = if self.values.spilled() {
            heap_block_bytes(self.values.capacity() * std::mem::size_of::<A::Item>())
        } else {
            0
        };
        let pins = if self.pins.spilled() {
            heap_block_bytes(self.pins.capacity() * std::mem::size_of::<Option<PinnedHeap>>())
        } else {
            0
        };
        let bytes = values + pins;
        let old = self.backing.used();
        if bytes > old {
            self.backing.add(bytes - old);
        } else {
            self.backing.sub(old - bytes);
        }
    }
    pub(super) fn push(&mut self, value: A::Item)
    where
        A::Item: HeapSize,
    {
        self.pins
            .push(Some(PinnedHeap::measure(&value, 0, &Default::default())));
        self.values.push(value);
        self.settle();
    }
    pub(super) fn retain(&mut self, mut keep: impl FnMut(&A::Item) -> bool) {
        let mut index = 0;
        self.values.retain(|value| {
            let retained = keep(value);
            if !retained {
                self.pins[index] = None;
            }
            index += 1;
            retained
        });
        self.pins.retain(|pin| pin.is_some());
        self.settle();
    }
    pub(super) fn iter_mut(&mut self) -> std::slice::IterMut<'_, A::Item> {
        self.values.iter_mut()
    }
}
impl<A: Array> Deref for PinnedChain<A> {
    type Target = SmallVec<A>;
    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

pub(super) fn chain_entry<K: Hash + Eq + HeapSize, A: Array>(
    entry: Entry<'_, K, PinnedChain<A>>,
) -> RefMut<'_, K, PinnedChain<A>> {
    match entry {
        Entry::Occupied(entry) => entry.into_ref(),
        Entry::Vacant(entry) => {
            let chain = PinnedChain::new(entry.key());
            entry.insert(chain)
        }
    }
}

/// DashMap removals retain bucket capacity. Only a growing shard needs
/// settlement, after its entry guard drops. Never scan other shards: a
/// publication callback can legitimately hold one of their write guards.
///
/// The table's fixed shell (its shard array and this record's capacity
/// slice) stays allocated for the table's whole life, empty or not, so it
/// stays charged from [`TableStorage::initialize`] until the table drops;
/// bucket storage is charged on top of it.
pub(super) struct TableStorage {
    state: parking_lot::Mutex<TableCharge>,
}

struct TableCharge {
    backing: GranularPin,
    capacities: Box<[usize]>,
}

impl TableCharge {
    fn set_shard(&mut self, shard: usize, bytes: usize) {
        let old = std::mem::replace(&mut self.capacities[shard], bytes);
        if bytes > old {
            self.backing.add(bytes - old);
        } else {
            self.backing.sub(old - bytes);
        }
    }
}

impl Default for TableStorage {
    fn default() -> Self {
        Self {
            state: parking_lot::Mutex::new(TableCharge {
                backing: GranularPin::new(PinContext::default().account(), 1024),
                capacities: Box::new([]),
            }),
        }
    }
}

impl TableStorage {
    pub fn initialize<K: Hash + Eq, V>(&self, map: &DashMap<K, V>) {
        let mut state = self.state.lock();
        let capacities = vec![0; map.shards().len()].into_boxed_slice();
        state.backing.add(
            heap_block_bytes(std::mem::size_of_val(map.shards()))
                + heap_block_bytes(std::mem::size_of_val(&*capacities)),
        );
        state.capacities = capacities;
    }

    pub fn settle<K: Hash + Eq, V>(&self, map: &DashMap<K, V>, key: &K) {
        self.settle_shard(map, map.determine_map(key));
    }

    fn settle_shard<K: Hash + Eq, V>(&self, map: &DashMap<K, V>, shard: usize) {
        // Release the shard before taking accounting's lock. A slower
        // settler may observe an older capacity, so only advance its maximum.
        let bytes = shard_bytes(map, shard);
        let mut state = self.state.lock();
        if bytes > state.capacities[shard] {
            state.set_shard(shard, bytes);
        }
    }

    /// Give back the bucket capacity a drained table no longer needs and
    /// lower every shard's charge to what it still holds. Called only at a
    /// release boundary, never with an entry guard of `map` held.
    pub fn reclaim<K: Hash + Eq, V>(&self, map: &DashMap<K, V>) {
        map.shrink_to_fit();
        for shard in 0..map.shards().len() {
            let bytes = shard_bytes(map, shard);
            {
                let mut state = self.state.lock();
                if bytes < state.capacities[shard] {
                    state.set_shard(shard, bytes);
                }
                state.backing.release_slack();
            }
            // A growth settled between the read above and the lowering is
            // re-advanced here; a later growth settles itself.
            self.settle_shard(map, shard);
        }
    }
}

fn shard_bytes<K: Hash + Eq, V>(map: &DashMap<K, V>, shard: usize) -> usize {
    hash_table_bytes(
        map.shards()[shard].read().capacity(),
        std::mem::size_of::<(K, V)>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use verter_retention::{RetentionLimits, SemanticRetentionAccount};

    /// A table's shard array and capacity record are allocated for its whole
    /// life, so they stay charged while it is empty — fresh, or drained and
    /// reclaimed — and only its drop gives them back.
    #[test]
    fn an_empty_tables_allocated_shell_stays_charged_until_it_drops() {
        let context = PinContext::new(SemanticRetentionAccount::new(RetentionLimits::defaults()));
        let _scope = context.enter_for_test();
        let pinned = || context.account().snapshot().pinned_bytes;
        let baseline = pinned();

        let map: DashMap<u64, [u64; 8]> = DashMap::new();
        let storage = TableStorage::default();
        storage.initialize(&map);
        let shell = heap_block_bytes(std::mem::size_of_val(map.shards()))
            + heap_block_bytes(map.shards().len() * std::mem::size_of::<usize>());
        let fresh = pinned() - baseline;
        assert!(
            fresh >= shell,
            "a fresh empty table charges its {shell}-byte shell, not {fresh}"
        );

        for key in 0..256 {
            map.insert(key, [key; 8]);
            storage.settle(&map, &key);
        }
        let populated = pinned() - baseline;
        let buckets: usize = (0..map.shards().len())
            .map(|shard| shard_bytes(&map, shard))
            .sum();
        assert!(
            populated >= shell + buckets,
            "a populated table charges its shell and {buckets} bucket bytes, not {populated}"
        );

        map.clear();
        storage.reclaim(&map);
        assert_eq!(
            pinned() - baseline,
            fresh,
            "a drained, reclaimed table charges exactly what it did fresh"
        );

        drop(storage);
        assert_eq!(
            pinned(),
            baseline,
            "dropping the table gives its shell back"
        );
    }
}
