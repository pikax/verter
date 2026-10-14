//! Concurrent maps owned by one file's artifacts.
//!
//! `DashMap::default()` sizes its shard table to the machine (four shards
//! per hardware thread, each padded to a cache line), so an empty map costs
//! kilobytes. A project opens one set of these maps per file; machine-wide
//! sharding would make an empty file's memo cost more than its content.
//! Per-file maps see the demands of one file, so a few shards keep them
//! uncontended.

use std::hash::Hash;

use dashmap::DashMap;

/// Shards of a map owned by one file's artifacts.
const PER_FILE_MAP_SHARDS: usize = 4;

/// An empty concurrent map sized for one file's artifacts.
pub fn per_file_map<K: Eq + Hash, V>() -> DashMap<K, V> {
    DashMap::with_shard_amount(PER_FILE_MAP_SHARDS)
}
