//! Concurrent, self-charged maps owned by one file's artifacts.
//! Shards allocate on first write; each entry carries its payload pin.
//! Keys hash deterministically, so equal contents occupy equal shards and
//! two maps holding the same entries charge the same bucket storage.
//! An entry's pin lives beside its value: a shared handle readers clone out
//! of the map must own its storage (`SelfPinned`, or an embedded pin), so
//! its charge outlives the entry with the reader.
use dashmap::DashMap;
use std::borrow::Borrow;
use std::hash::Hash;
use std::ops::{Deref, DerefMut};
use std::sync::OnceLock;
use verter_retention::allocation::heap_block_bytes;
use verter_retention::granular::GranularPin;
use verter_retention::heap_size::{hash_table_bytes, HeapMeter, HeapSize};
use verter_retention::pinned_heap::{PinContext, PinnedHeap};

const PER_FILE_MAP_SHARDS: usize = 4;
type Table<K, V> = DashMap<K, Value<V>, rustc_hash::FxBuildHasher>;
struct Value<V> {
    value: V,
    _key_pin: PinnedHeap,
    value_pin: PinnedHeap,
}

pub struct PerFileMap<K, V> {
    map: OnceLock<Table<K, V>>,
    backing: parking_lot::Mutex<(GranularPin, [usize; PER_FILE_MAP_SHARDS])>,
}
impl<K, V> Default for PerFileMap<K, V> {
    fn default() -> Self {
        Self {
            map: OnceLock::new(),
            backing: parking_lot::Mutex::new((
                GranularPin::new(PinContext::default().account(), 1024),
                [0; PER_FILE_MAP_SHARDS],
            )),
        }
    }
}
impl<K: Eq + Hash + Clone + HeapSize, V: Clone + HeapSize> Clone for PerFileMap<K, V> {
    fn clone(&self) -> Self {
        let copy = Self::default();
        if let Some(map) = self.map.get() {
            for item in map {
                copy.insert(item.key().clone(), item.value().value.clone());
            }
        }
        copy
    }
}
impl<K: Eq + Hash + std::fmt::Debug, V: std::fmt::Debug> std::fmt::Debug for PerFileMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut out = f.debug_map();
        if let Some(map) = self.map.get() {
            for item in map {
                out.entry(item.key(), &item.value().value);
            }
        }
        out.finish()
    }
}
impl<K, V> HeapSize for PerFileMap<K, V> {
    fn heap_size<'t>(&'t self, _meter: &mut HeapMeter<'_, 't>) -> usize {
        0
    }
}
impl<K: Eq + Hash, V> PerFileMap<K, V> {
    fn written(&self) -> &Table<K, V> {
        self.map.get_or_init(|| {
            let map = DashMap::with_hasher_and_shard_amount(
                rustc_hash::FxBuildHasher,
                PER_FILE_MAP_SHARDS,
            );
            self.backing
                .lock()
                .0
                .add(heap_block_bytes(std::mem::size_of_val(map.shards())));
            map
        })
    }
    fn settle_shard(&self, shard: usize) {
        let Some(map) = self.map.get() else {
            return;
        };
        let bytes = hash_table_bytes(
            map.shards()[shard].read().capacity(),
            std::mem::size_of::<(K, Value<V>)>(),
        );
        // Never hold accounting's lock while waiting for a shard. Removals
        // retain bucket storage, so an older racing observation cannot shrink it.
        let mut backing = self.backing.lock();
        let old = backing.1[shard];
        if bytes > old {
            backing.0.add(bytes - old);
            backing.1[shard] = bytes;
        }
    }
    pub fn get<Q>(&self, key: &Q) -> Option<MapRef<'_, K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get()?.get(key).map(MapRef)
    }
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get().is_some_and(|map| map.contains_key(key))
    }
    pub fn remove_if<Q>(&self, key: &Q, remove: impl FnOnce(&K, &V) -> bool) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        let map = self.map.get()?;
        let shard = map.determine_map(key);
        let (key, value) = map.remove_if(key, |k, v| remove(k, &v.value))?;
        self.settle_shard(shard);
        Some((key, value.value))
    }
    pub fn len(&self) -> usize {
        self.map.get().map_or(0, Table::len)
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
impl<K: Eq + Hash + HeapSize, V: HeapSize> PerFileMap<K, V> {
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        let map = self.written();
        let shard = map.determine_map(&key);
        let value_pin = PinnedHeap::measure(&value, 0, &PinContext::default());
        let old = match map.entry(key) {
            dashmap::mapref::entry::Entry::Occupied(mut entry) => {
                let held = entry.get_mut();
                let old = std::mem::replace(&mut held.value, value);
                held.value_pin = value_pin;
                Some(old)
            }
            dashmap::mapref::entry::Entry::Vacant(entry) => {
                let key_pin = PinnedHeap::measure(entry.key(), 0, &PinContext::default());
                entry.insert(Value {
                    value,
                    _key_pin: key_pin,
                    value_pin,
                });
                None
            }
        };
        self.settle_shard(shard);
        old
    }
    pub fn entry(&self, key: K) -> MapEntry<'_, K, V> {
        let map = self.written();
        let shard = map.determine_map(&key);
        MapEntry {
            entry: map.entry(key),
            owner: self,
            shard,
        }
    }
}

pub struct MapRef<'a, K, V>(dashmap::mapref::one::Ref<'a, K, Value<V>>);
impl<K: Eq + Hash, V> Deref for MapRef<'_, K, V> {
    type Target = V;
    fn deref(&self) -> &V {
        &self.0.value().value
    }
}
impl<K: Eq + Hash, V> MapRef<'_, K, V> {
    pub fn value(&self) -> &V {
        self
    }
}

pub struct MapEntry<'a, K: Eq + Hash + HeapSize, V: HeapSize> {
    entry: dashmap::mapref::entry::Entry<'a, K, Value<V>>,
    owner: &'a PerFileMap<K, V>,
    shard: usize,
}
impl<'a, K: Eq + Hash + HeapSize, V: HeapSize> MapEntry<'a, K, V> {
    pub fn or_insert(self, value: V) -> MapRefMut<'a, K, V> {
        self.or_insert_with(|| value)
    }
    pub fn or_default(self) -> MapRefMut<'a, K, V>
    where
        V: Default,
    {
        self.or_insert_with(V::default)
    }
    pub fn or_insert_with(self, build: impl FnOnce() -> V) -> MapRefMut<'a, K, V> {
        use dashmap::mapref::entry::Entry;
        let inner = match self.entry {
            Entry::Occupied(entry) => entry.into_ref(),
            Entry::Vacant(entry) => {
                let value = build();
                let key_pin = PinnedHeap::measure(entry.key(), 0, &PinContext::default());
                let value_pin = PinnedHeap::measure(&value, 0, &PinContext::default());
                entry.insert(Value {
                    value,
                    _key_pin: key_pin,
                    value_pin,
                })
            }
        };
        MapRefMut {
            inner: Some(inner),
            owner: self.owner,
            shard: self.shard,
        }
    }
}
pub struct MapRefMut<'a, K: Eq + Hash + HeapSize, V: HeapSize> {
    inner: Option<dashmap::mapref::one::RefMut<'a, K, Value<V>>>,
    owner: &'a PerFileMap<K, V>,
    shard: usize,
}
impl<K: Eq + Hash + HeapSize, V: HeapSize> Deref for MapRefMut<'_, K, V> {
    type Target = V;
    fn deref(&self) -> &V {
        &self.inner.as_ref().unwrap().value().value
    }
}
impl<K: Eq + Hash + HeapSize, V: HeapSize + Copy> DerefMut for MapRefMut<'_, K, V> {
    fn deref_mut(&mut self) -> &mut V {
        &mut self.inner.as_mut().unwrap().value_mut().value
    }
}
impl<K: Eq + Hash + HeapSize, V: HeapSize> Drop for MapRefMut<'_, K, V> {
    fn drop(&mut self) {
        drop(self.inner.take());
        self.owner.settle_shard(self.shard);
    }
}
pub fn per_file_map<K: Eq + Hash, V>() -> PerFileMap<K, V> {
    PerFileMap::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacing_a_value_keeps_the_actual_stored_keys_capacity_charged() {
        let context = PinContext::new(verter_retention::SemanticRetentionAccount::new(
            verter_retention::RetentionLimits::defaults(),
        ));
        let _scope = context.enter_for_test();
        let account = context.account();
        let baseline = account.snapshot().pinned_bytes;
        let map = per_file_map();
        let mut key = String::with_capacity(1024 * 1024);
        key.push_str("same-key");
        map.insert(key, 1u32);
        assert_eq!(map.insert(String::from("same-key"), 2), Some(1));
        assert!(account.snapshot().pinned_bytes >= baseline + 1024 * 1024);
        assert_eq!(map.get("same-key").map(|value| *value), Some(2));
        drop(map);
        assert_eq!(account.snapshot().pinned_bytes, baseline);
    }

    #[test]
    fn a_map_allocates_its_shards_on_first_insertion_only() {
        let map: PerFileMap<u32, u32> = per_file_map();
        assert!(map.get(&1).is_none());
        assert!(!map.contains_key(&1));
        assert!(map.remove_if(&1, |_, _| true).is_none());
        assert_eq!(map.len(), 0);
        assert!(map.map.get().is_none(), "reads never allocate the shards");
        map.insert(1, 10);
        *map.entry(2).or_insert(0) += 20;
        assert_eq!(map.get(&1).map(|value| *value), Some(10));
        assert_eq!(map.get(&2).map(|value| *value), Some(20));
        assert!(map.remove_if(&1, |_, value| *value == 10).is_some());
        assert_eq!(map.len(), 1);
    }
}
