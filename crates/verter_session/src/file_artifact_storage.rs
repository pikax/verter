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
pub(super) struct TableStorage {
    state: parking_lot::Mutex<(GranularPin, Box<[usize]>)>,
}

impl Default for TableStorage {
    fn default() -> Self {
        Self {
            state: parking_lot::Mutex::new((
                GranularPin::new(PinContext::default().account(), 1024),
                Box::new([]),
            )),
        }
    }
}

impl TableStorage {
    pub fn initialize<K: Hash + Eq, V>(&self, map: &DashMap<K, V>) {
        let mut state = self.state.lock();
        let capacities = vec![0; map.shards().len()].into_boxed_slice();
        state.0.add(
            heap_block_bytes(std::mem::size_of_val(map.shards()))
                + heap_block_bytes(std::mem::size_of_val(&*capacities)),
        );
        state.1 = capacities;
    }

    pub fn settle<K: Hash + Eq, V>(&self, map: &DashMap<K, V>, key: &K) {
        let shard = map.determine_map(key);
        // Release the shard before taking accounting's lock. A slower
        // settler may observe an older capacity, so only advance its maximum.
        let bytes = hash_table_bytes(
            map.shards()[shard].read().capacity(),
            std::mem::size_of::<(K, V)>(),
        );
        let mut state = self.state.lock();
        let old = state.1[shard];
        if bytes > old {
            state.0.add(bytes - old);
            state.1[shard] = bytes;
        }
    }
}
