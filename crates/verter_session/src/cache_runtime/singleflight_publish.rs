//! Publish / removal side of the cooperative-admission lifecycle.
//!
//! [`singleflight`](super::singleflight) owns the winner/joiner state
//! machine; this module owns the two map-mutating tails the winner runs
//! once a cold build is `Cacheable`, plus the warm-hit / joiner-fork
//! removal tail. Splitting them out keeps the state-machine file under
//! the line budget while colocating the publish-side bookkeeping
//! (`removal_cleanup` / `post_publish` symmetry, the per-map-key
//! linearization) in one place.
//!
//! Two publish shapes, selected by the entry point that calls in:
//!
//! - [`publish_entry_insert_then_post_publish`] — the cache-key-is-flight-key
//!   shape. The in-flight table coalesces every caller of one map key onto
//!   ONE cold winner, so there is exactly one publisher per key and the map
//!   slot is always vacant at publish. `map.insert` releases its transient
//!   shard guard BEFORE `post_publish`, so a `post_publish` hook may
//!   re-enter the same map (a budgeted cache's FIFO eviction does exactly
//!   that) without self-deadlocking.
//! - [`publish_entry_linearized_per_map_key`] — the split-identity shape,
//!   where the map key and the flight identity differ so two winners on
//!   different flight lanes can publish under one map key and the second
//!   DISPLACES the first. There the displaced entry's `removal_cleanup` and
//!   the new entry's `post_publish` must run as one per-map-key-atomic
//!   operation, so the publish rides the map's shard write guard across the
//!   whole triple. Its hooks must NOT re-enter the map.

use std::hash::Hash;
use std::sync::Arc;

use dashmap::mapref::entry::Entry as MapEntry;
use dashmap::DashMap;

/// Remove the exact published `Arc<Entry>` the caller validated — a
/// `ptr_eq` guard so a concurrent fresh winner's entry is not evicted —
/// and, on a real removal, run the caller's `removal_cleanup` so the
/// cache's removal-side bookkeeping (live counter, reverse index) stays
/// symmetric with its publish-side bookkeeping.
///
/// Used by both substrate removal sites: the warm-hit path when
/// `validate` rejects a stale entry, and the joiner-fork path when a
/// cross-view follower rejects the winner's entry. A raw `map.remove_if`
/// in either site would skip the cleanup and leave caches with
/// publish-side counters / reverse indexes out of sync after a removal.
///
/// `publish_fence`, when `Some`, is the cache's lifecycle
/// `retention_gate`. The map removal and `removal_cleanup` (which drains
/// the cache's reverse index and retention budget) run under a shared
/// read guard so a concurrent project-generation `clear` — which holds
/// the write guard across its own map+budget clear — cannot interleave
/// its clears with this removal's map/budget mutation. A cache with no
/// retention budget passes `None`.
pub(super) fn remove_published_entry_with_cleanup<K, Entry, RemovalCleanup>(
    map: &DashMap<K, Arc<Entry>>,
    key: &K,
    entry_arc: &Arc<Entry>,
    removal_cleanup: &mut RemovalCleanup,
    publish_fence: Option<&parking_lot::RwLock<()>>,
) where
    K: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    RemovalCleanup: FnMut(&K, &Arc<Entry>),
{
    // Hold the cache's retention gate (shared read) across the whole
    // `remove_if` + `removal_cleanup` so the map removal and the
    // reverse-index / budget / counter cleanup are one lock-domain
    // mutation against a concurrent `clear`.
    let _retention = publish_fence.map(parking_lot::RwLock::read);
    if let Some((removed_key, removed_entry)) =
        map.remove_if(key, |_, existing| Arc::ptr_eq(existing, entry_arc))
    {
        removal_cleanup(&removed_key, &removed_entry);
    }
}

/// Publish `entry_arc` under `map_key` for the **cache-key-is-flight-key**
/// shape, where the in-flight table coalesces every caller of `map_key`
/// onto ONE cold winner.
///
/// Single-publisher-per-key is the load-bearing property: the map slot is
/// always vacant at publish (a concurrent winner cannot exist for the same
/// `map_key`, and the warm/joiner-fork removal tail clears any stale entry
/// before this winner re-enters the cold path), so there is no displaced
/// entry and `removal_cleanup` never fires here. `map.insert` therefore
/// releases its transient shard guard BEFORE `post_publish`, so a
/// `post_publish` hook is free to re-enter the same map — a budgeted cache
/// runs FIFO `evict_budget_victim` → `entries.remove_if` from its
/// `post_publish`, and holding the shard guard across that re-entry would
/// self-deadlock on a same-shard victim. The split-identity shape, which
/// CAN have concurrent publishers per map key, uses
/// [`publish_entry_linearized_per_map_key`] instead. Full rule: the
/// `/type-cache-architecture` skill.
pub(super) fn publish_entry_insert_then_post_publish<MapK, Entry, RemovalCleanup, PostPublish>(
    map: &DashMap<MapK, Arc<Entry>>,
    map_key: &MapK,
    entry_arc: &Arc<Entry>,
    removal_cleanup: &mut RemovalCleanup,
    post_publish: PostPublish,
) where
    MapK: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    RemovalCleanup: FnMut(&MapK, &Arc<Entry>),
    PostPublish: FnOnce(&Arc<Entry>, &MapK),
{
    // `map.insert` takes, mutates, and RELEASES the shard guard, returning
    // any displaced `Arc<Entry>` by value. Under single-publisher-per-key
    // the slot is vacant so `displaced` is always `None`; the cleanup arm
    // is retained only for symmetry with the linearized shape and to keep
    // bookkeeping correct were the invariant ever violated. Critically, the
    // shard guard is no longer held when `post_publish` runs, so a hook
    // that re-enters this `map` (FIFO budget eviction) cannot self-deadlock.
    let displaced = map.insert(map_key.clone(), Arc::clone(entry_arc));
    if let Some(displaced_entry) = displaced {
        removal_cleanup(map_key, &displaced_entry);
    }
    post_publish(entry_arc, map_key);
}

/// Publish `entry_arc` under `map_key`, running the displaced entry's
/// `removal_cleanup` and the new entry's `post_publish` as ONE operation
/// that is atomic per map key against OTHER publishers.
///
/// This shape is for the **by-flight-key** entry point ONLY
/// ([`cooperative_admit_with_post_publish_by_flight_key`]), where `MapK`
/// and `FlightK` are independent so two winners on different flight lanes
/// can publish under the same map key and the second DISPLACES the first.
/// The publish fence is a SHARED read guard that excludes a concurrent
/// `clear` but NOT concurrent publishers, so a bare `map.insert` then a
/// post-hoc displaced cleanup would let the displacing publisher clean up
/// an entry whose own `post_publish` had not yet run — underflowing the
/// live counter. Linearization instead rides the map's per-key shard lock:
/// [`DashMap::entry`] holds the shard write guard across the whole triple
/// (insert/replace, displaced `removal_cleanup`, new `post_publish`), so a
/// displaced entry is observable only to a shard guard acquired after the
/// displaced publisher's `post_publish` completed.
///
/// Because the shard write guard is held across the hooks, the hooks must
/// NOT re-enter this `map` — they would self-deadlock on the guard. The
/// by-flight-key consumers' hooks are lock-free (a `live_counter`
/// `fetch_add`/`sub`) or touch a SEPARATE map (a reverse index), so they
/// are guard-safe. The cache-key-is-flight-key entry point deliberately
/// does NOT use this shape: its hooks DO re-enter the map (budget
/// eviction), and its single-publisher guarantee means it never displaces,
/// so it routes through [`publish_entry_insert_then_post_publish`] where
/// the shard guard is released before the hooks run. Full rule: the
/// `/type-cache-architecture` skill.
///
/// [`cooperative_admit_with_post_publish_by_flight_key`]:
///     super::singleflight::cooperative_admit_with_post_publish_by_flight_key
pub(super) fn publish_entry_linearized_per_map_key<MapK, Entry, RemovalCleanup, PostPublish>(
    map: &DashMap<MapK, Arc<Entry>>,
    map_key: &MapK,
    entry_arc: &Arc<Entry>,
    removal_cleanup: &mut RemovalCleanup,
    post_publish: PostPublish,
) where
    MapK: Eq + Hash + Clone,
    Entry: Send + Sync + 'static,
    RemovalCleanup: FnMut(&MapK, &Arc<Entry>),
    PostPublish: FnOnce(&Arc<Entry>, &MapK),
{
    match map.entry(map_key.clone()) {
        MapEntry::Occupied(mut occupied) => {
            // `insert` returns the displaced `Arc<Entry>` by value while the
            // shard guard is held, so the displaced cleanup and the new
            // `post_publish` both run before any other publisher acquires it.
            let displaced = occupied.insert(Arc::clone(entry_arc));
            removal_cleanup(map_key, &displaced);
            post_publish(entry_arc, map_key);
        }
        MapEntry::Vacant(vacant) => {
            // First publish — nothing displaced. The returned `RefMut`
            // keeps the shard guard alive across `post_publish`.
            let _entry_ref = vacant.insert(Arc::clone(entry_arc));
            post_publish(entry_arc, map_key);
        }
    }
}
