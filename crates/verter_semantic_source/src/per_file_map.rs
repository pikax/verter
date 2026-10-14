//! Concurrent maps owned by one file's artifacts.
//!
//! `DashMap::default()` sizes its shard table to the machine (four shards
//! per hardware thread, each padded to a cache line), so an empty map costs
//! kilobytes. A project opens one set of these maps per file; machine-wide
//! sharding would make an empty file's memo cost more than its content.
//! Per-file maps see the demands of one file, so a few shards keep them
//! uncontended. Most of a file's maps are never written — a file of
//! augmentations has no value declarations, a script has no augmentation
//! blocks — so the shard table is allocated on a map's first insertion,
//! and a never-written map costs one pointer.

use std::borrow::Borrow;
use std::hash::Hash;
use std::sync::OnceLock;

use dashmap::mapref::entry::Entry;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;

/// Shards of a map owned by one file's artifacts.
const PER_FILE_MAP_SHARDS: usize = 4;

/// A concurrent map owned by one file's artifacts, allocated on its first
/// insertion.
pub struct PerFileMap<K, V> {
    map: OnceLock<DashMap<K, V>>,
}

impl<K, V> Default for PerFileMap<K, V> {
    fn default() -> Self {
        Self {
            map: OnceLock::new(),
        }
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Clone for PerFileMap<K, V> {
    fn clone(&self) -> Self {
        let map = OnceLock::new();
        if let Some(written) = self.map.get() {
            let _ = map.set(written.clone());
        }
        Self { map }
    }
}

impl<K: Eq + Hash + std::fmt::Debug, V: std::fmt::Debug> std::fmt::Debug for PerFileMap<K, V> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.map.get() {
            Some(map) => map.fmt(f),
            None => f.write_str("{}"),
        }
    }
}

impl<K: Eq + Hash, V> PerFileMap<K, V> {
    fn written(&self) -> &DashMap<K, V> {
        self.map
            .get_or_init(|| DashMap::with_shard_amount(PER_FILE_MAP_SHARDS))
    }

    /// The value of `key`, if any.
    pub fn get<Q>(&self, key: &Q) -> Option<Ref<'_, K, V>>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get()?.get(key)
    }

    /// Whether `key` has a value.
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get().is_some_and(|map| map.contains_key(key))
    }

    /// Insert `value` at `key`, returning the value it replaced.
    pub fn insert(&self, key: K, value: V) -> Option<V> {
        self.written().insert(key, value)
    }

    /// The entry of `key`.
    pub fn entry(&self, key: K) -> Entry<'_, K, V> {
        self.written().entry(key)
    }

    /// Remove `key`'s value when `remove` accepts it.
    pub fn remove_if<Q>(&self, key: &Q, remove: impl FnOnce(&K, &V) -> bool) -> Option<(K, V)>
    where
        K: Borrow<Q>,
        Q: Hash + Eq + ?Sized,
    {
        self.map.get()?.remove_if(key, remove)
    }

    /// How many keys have a value.
    pub fn len(&self) -> usize {
        self.map.get().map_or(0, DashMap::len)
    }

    /// Whether no key has a value.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// An empty concurrent map sized for one file's artifacts.
pub fn per_file_map<K: Eq + Hash, V>() -> PerFileMap<K, V> {
    PerFileMap::default()
}

#[cfg(test)]
mod tests {
    use super::*;

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
