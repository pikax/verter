//! Residency of request-overlay resolution state: who holds it, what it
//! costs, and when it goes.
//!
//! Two structures outlive the request that fills them: the Engine's overlay
//! lane (overlay resolution answers) and its overlay value table (the
//! versions of overlay values with no exact encoding). Both are an
//! [`AuthorityHeld`] map. Every entry is HELD by the overlay authorities
//! that produced or reused it, and it lives exactly as long as one of them
//! still holds it:
//!
//! - an [`OverlayAuthority`] is a live overlay domain — a session whose
//!   overlay outlives one request — or, for a request overlay with no
//!   session, the request's own snapshot;
//! - when the last handle to an authority drops (its session closed, or
//!   its request's snapshot superseded by the next request's), the
//!   authority releases every entry it holds, and an entry no other
//!   authority holds goes with it;
//! - every entry's bytes are charged to the host's aggregate retention
//!   account through [`ResolutionRetentionAccount`]; a refused charge
//!   leaves the entry out (the answer is served, not retained);
//! - independently of authority, each map is bounded: a per-key item cap
//!   and a key cap, oldest key evicted first. Evicting a live entry only
//!   forces a recompute.
use verter_session_query::retention::resolution_charge::{
    ResolutionRetentionAccount, ResolutionRetentionCharge,
};

use std::collections::VecDeque;
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::{Mutex, RwLock};
use rustc_hash::{FxHashMap, FxHashSet};
use smallvec::SmallVec;

// ─────────────────────────────────────────────────────────────────────────
// The retention-account seam
// ─────────────────────────────────────────────────────────────────────────

/// The Engine's installed account, if any. A workspace with no host behind
/// it has no account: its entries are bounded but not charged.
#[derive(Default)]
pub(crate) struct RetentionHook(RwLock<Option<Arc<dyn ResolutionRetentionAccount>>>);

/// A reservation the installed account refused.
pub(crate) struct RetentionRefused;

impl RetentionHook {
    pub(crate) fn install(&self, account: Arc<dyn ResolutionRetentionAccount>) {
        *self.0.write() = Some(account);
    }

    pub(crate) fn reserve(
        &self,
        bytes: usize,
    ) -> Result<Option<ResolutionRetentionCharge>, RetentionRefused> {
        match self.0.read().as_ref() {
            None => Ok(None),
            Some(account) => account
                .reserve_retained(bytes)
                .map(Some)
                .ok_or(RetentionRefused),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Overlay authority
// ─────────────────────────────────────────────────────────────────────────

/// Something that holds entries on an authority's behalf and gives them up
/// when the authority goes.
pub(crate) trait AuthorityHolder: Send + Sync {
    fn release_authority(&self, authority: u64);
}

static NEXT_OVERLAY_AUTHORITY: AtomicU64 = AtomicU64::new(1);

/// The identity a live overlay domain holds its resident resolution state
/// under. Cloning shares the one authority; when the last clone drops, the
/// authority releases everything it holds.
///
/// A session holds one for its whole life and hands a clone to each
/// request's snapshot, so its overlay answers stay warm across requests
/// and go when the session closes (after its last in-flight request). A
/// request overlay with no session gets a fresh one, so its answers go
/// when the request's snapshot is superseded.
#[derive(Clone)]
pub struct OverlayAuthority(Arc<AuthorityInner>);

struct AuthorityInner {
    id: u64,
    holders: Mutex<Vec<Weak<dyn AuthorityHolder>>>,
}

impl OverlayAuthority {
    /// Mint a process-unique authority.
    #[must_use]
    pub fn new() -> Self {
        Self(Arc::new(AuthorityInner {
            id: NEXT_OVERLAY_AUTHORITY.fetch_add(1, Ordering::Relaxed),
            holders: Mutex::new(Vec::new()),
        }))
    }

    /// The authority's process-unique id.
    #[must_use]
    pub fn id(&self) -> u64 {
        self.0.id
    }

    /// Record that `holder` holds entries for this authority, so the
    /// authority's release reaches it. Idempotent per holder.
    pub(crate) fn held_by(&self, holder: Weak<dyn AuthorityHolder>) {
        let mut holders = self.0.holders.lock();
        if !holders.iter().any(|known| Weak::ptr_eq(known, &holder)) {
            holders.retain(|known| known.strong_count() > 0);
            holders.push(holder);
        }
    }
}

impl Default for OverlayAuthority {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for OverlayAuthority {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("OverlayAuthority").field(&self.0.id).finish()
    }
}

impl Drop for AuthorityInner {
    fn drop(&mut self) {
        for holder in std::mem::take(self.holders.get_mut()) {
            if let Some(holder) = holder.upgrade() {
                holder.release_authority(self.id);
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Authority-held map
// ─────────────────────────────────────────────────────────────────────────

/// A bounded map whose items live while an overlay authority holds them.
///
/// Each key holds up to `items_per_key` items, oldest dropped first; the
/// map holds up to `key_cap` keys, oldest evicted first. Each item carries
/// its holders and its retention charge. All state moves under one lock.
pub(crate) struct AuthorityHeld<K, T> {
    state: RwLock<HeldState<K, T>>,
    retention: Arc<RetentionHook>,
    items_per_key: usize,
    key_cap: usize,
    next_seq: AtomicU64,
}

struct HeldState<K, T> {
    slots: FxHashMap<K, HeldSlot<T>>,
    /// Keys in admission order, each with the generation of the slot it
    /// admitted. A released slot leaves its entry behind; eviction skips
    /// it, and the queue is compacted once stale entries dominate.
    order: VecDeque<(K, u64)>,
    by_authority: FxHashMap<u64, FxHashSet<K>>,
    next_generation: u64,
}

struct HeldSlot<T> {
    generation: u64,
    items: SmallVec<[HeldItem<T>; 4]>,
}

struct HeldItem<T> {
    seq: u64,
    value: T,
    holders: SmallVec<[u64; 2]>,
    _charge: Option<ResolutionRetentionCharge>,
}

impl<K, T> AuthorityHeld<K, T>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    pub(crate) fn new(retention: Arc<RetentionHook>, items_per_key: usize, key_cap: usize) -> Self {
        Self {
            state: RwLock::new(HeldState {
                slots: FxHashMap::default(),
                order: VecDeque::new(),
                by_authority: FxHashMap::default(),
                next_generation: 0,
            }),
            retention,
            items_per_key,
            key_cap,
            next_seq: AtomicU64::new(1),
        }
    }

    /// The items `key` holds, oldest first, each with its admission
    /// sequence number.
    pub(crate) fn items(&self, key: &K) -> SmallVec<[(u64, T); 4]> {
        self.state
            .read()
            .slots
            .get(key)
            .map(|slot| {
                slot.items
                    .iter()
                    .map(|item| (item.seq, item.value.clone()))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Retain `value` under `key` for `authority`, charging `bytes`.
    /// Returns its sequence number, or `None` when the retention account
    /// refused it (the caller serves the value without retaining it).
    pub(crate) fn insert(
        self: &Arc<Self>,
        key: K,
        value: T,
        bytes: usize,
        authority: &OverlayAuthority,
    ) -> Option<u64> {
        let charge = self.retention.reserve(bytes).ok()?;
        let seq = {
            let mut state = self.state.write();
            self.insert_locked(&mut state, key, value, charge, authority)
        };
        self.register(authority);
        Some(seq)
    }

    fn insert_locked(
        &self,
        state: &mut HeldState<K, T>,
        key: K,
        value: T,
        charge: Option<ResolutionRetentionCharge>,
        authority: &OverlayAuthority,
    ) -> u64 {
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        if !state.slots.contains_key(&key) {
            let generation = state.next_generation;
            state.next_generation += 1;
            state.slots.insert(
                key.clone(),
                HeldSlot {
                    generation,
                    items: SmallVec::new(),
                },
            );
            state.order.push_back((key.clone(), generation));
        }
        let slot = state
            .slots
            .get_mut(&key)
            .expect("the slot was just ensured");
        while slot.items.len() >= self.items_per_key {
            let dropped = slot.items.remove(0);
            Self::forget_holders(&mut state.by_authority, &key, &slot.items, &dropped);
        }
        slot.items.push(HeldItem {
            seq,
            value,
            holders: SmallVec::from_elem(authority.id(), 1),
            _charge: charge,
        });
        state
            .by_authority
            .entry(authority.id())
            .or_default()
            .insert(key);
        self.enforce_key_cap(state);
        seq
    }

    /// `authority` reuses item `seq` under `key`: it holds the item from
    /// here on, so the item outlives whichever authority produced it.
    pub(crate) fn adopt(self: &Arc<Self>, key: &K, seq: u64, authority: &OverlayAuthority) {
        let id = authority.id();
        {
            let state = self.state.upgradable_read();
            let held = state.slots.get(key).is_some_and(|slot| {
                slot.items
                    .iter()
                    .any(|item| item.seq == seq && item.holders.contains(&id))
            });
            if held {
                return;
            }
            let mut state = parking_lot::RwLockUpgradableReadGuard::upgrade(state);
            let state = &mut *state;
            let Some(item) = state
                .slots
                .get_mut(key)
                .and_then(|slot| slot.items.iter_mut().find(|item| item.seq == seq))
            else {
                return;
            };
            if item.holders.contains(&id) {
                return;
            }
            item.holders.push(id);
            state
                .by_authority
                .entry(id)
                .or_default()
                .insert(key.clone());
        }
        self.register(authority);
    }

    /// The value `key` holds, adopted by `authority`; or `make()` retained
    /// under `key` for `authority`. A value the retention account refuses
    /// is returned without being retained.
    pub(crate) fn get_or_insert(
        self: &Arc<Self>,
        key: K,
        bytes: usize,
        authority: &OverlayAuthority,
        make: impl FnOnce() -> T,
    ) -> T {
        // One write section decides "reuse or retain", so two concurrent
        // compositions of one value agree on it.
        let value = {
            let mut state = self.state.write();
            let state = &mut *state;
            let id = authority.id();
            let existing = state
                .slots
                .get_mut(&key)
                .and_then(|slot| slot.items.first_mut());
            match existing {
                Some(item) => {
                    if !item.holders.contains(&id) {
                        item.holders.push(id);
                        state
                            .by_authority
                            .entry(id)
                            .or_default()
                            .insert(key.clone());
                    }
                    item.value.clone()
                }
                None => {
                    let value = make();
                    match self.retention.reserve(bytes) {
                        Ok(charge) => {
                            self.insert_locked(state, key, value.clone(), charge, authority);
                        }
                        Err(RetentionRefused) => return value,
                    }
                    value
                }
            }
        };
        self.register(authority);
        value
    }

    /// Keys currently held.
    pub(crate) fn len(&self) -> usize {
        self.state.read().slots.len()
    }

    /// Entries in the key eviction queue, stale ones included.
    #[cfg(test)]
    pub(crate) fn queue_len(&self) -> usize {
        self.state.read().order.len()
    }

    /// Items currently held, across every key.
    #[cfg(test)]
    pub(crate) fn item_count(&self) -> usize {
        self.state
            .read()
            .slots
            .values()
            .map(|slot| slot.items.len())
            .sum()
    }

    fn register(self: &Arc<Self>, authority: &OverlayAuthority) {
        let holder: Arc<dyn AuthorityHolder> = self.clone();
        authority.held_by(Arc::downgrade(&holder));
    }

    /// `dropped` left `key`'s slot: every holder that no longer holds any
    /// remaining item there stops listing the key.
    fn forget_holders(
        by_authority: &mut FxHashMap<u64, FxHashSet<K>>,
        key: &K,
        remaining: &[HeldItem<T>],
        dropped: &HeldItem<T>,
    ) {
        for holder in &dropped.holders {
            if remaining.iter().any(|item| item.holders.contains(holder)) {
                continue;
            }
            if let Some(keys) = by_authority.get_mut(holder) {
                keys.remove(key);
                if keys.is_empty() {
                    by_authority.remove(holder);
                }
            }
        }
    }

    fn enforce_key_cap(&self, state: &mut HeldState<K, T>) {
        while state.slots.len() > self.key_cap {
            let Some((key, generation)) = state.order.pop_front() else {
                break;
            };
            let current = state
                .slots
                .get(&key)
                .is_some_and(|slot| slot.generation == generation);
            if current {
                let slot = state.slots.remove(&key).expect("checked present");
                for item in &slot.items {
                    Self::forget_holders(&mut state.by_authority, &key, &[], item);
                }
            }
        }
        Self::compact_order(state);
    }

    /// Drop the queue's stale entries once they outnumber the live ones,
    /// so the queue stays proportional to the map.
    fn compact_order(state: &mut HeldState<K, T>) {
        if state.order.len() > 2 * state.slots.len() + 64 {
            let slots = &state.slots;
            state.order.retain(|(key, generation)| {
                slots
                    .get(key)
                    .is_some_and(|slot| slot.generation == *generation)
            });
        }
    }
}

impl<K, T> std::fmt::Debug for AuthorityHeld<K, T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.state.read();
        f.debug_struct("AuthorityHeld")
            .field("keys", &state.slots.len())
            .field("authorities", &state.by_authority.len())
            .finish_non_exhaustive()
    }
}

impl<K, T> AuthorityHolder for AuthorityHeld<K, T>
where
    K: Hash + Eq + Clone + Send + Sync + 'static,
    T: Clone + Send + Sync + 'static,
{
    fn release_authority(&self, authority: u64) {
        let mut state = self.state.write();
        let state = &mut *state;
        let Some(keys) = state.by_authority.remove(&authority) else {
            return;
        };
        for key in keys {
            let Some(slot) = state.slots.get_mut(&key) else {
                continue;
            };
            slot.items.retain(|item| {
                item.holders.retain(|holder| *holder != authority);
                !item.holders.is_empty()
            });
            if slot.items.is_empty() {
                state.slots.remove(&key);
            }
        }
        Self::compact_order(state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn held(items_per_key: usize, key_cap: usize) -> Arc<AuthorityHeld<u32, u32>> {
        Arc::new(AuthorityHeld::new(
            Arc::new(RetentionHook::default()),
            items_per_key,
            key_cap,
        ))
    }

    #[test]
    fn a_key_keeps_its_newest_items_and_the_map_its_newest_keys() {
        let map = held(2, 3);
        let authority = OverlayAuthority::new();
        for value in 0..3 {
            map.insert(7, value, 1, &authority);
        }
        let values: Vec<u32> = map.items(&7).into_iter().map(|(_, value)| value).collect();
        assert_eq!(values, [1, 2], "the oldest item leaves first");

        for key in 0..5 {
            map.insert(key, key, 1, &authority);
        }
        assert_eq!(map.len(), 3, "the oldest keys leave first");
        assert!(map.items(&7).is_empty() && map.items(&0).is_empty());
        drop(authority);
        assert_eq!(map.len(), 0, "and the authority releases what remains");
    }

    #[test]
    fn an_item_lives_while_any_authority_holding_it_lives() {
        let map = held(4, 16);
        let first = OverlayAuthority::new();
        let second = OverlayAuthority::new();
        let seq = map.insert(1, 10, 1, &first).expect("no account refuses");
        map.adopt(&1, seq, &second);
        drop(first);
        assert_eq!(map.items(&1).len(), 1);
        drop(second);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn get_or_insert_reuses_the_held_value() {
        let map = held(1, 16);
        let first = OverlayAuthority::new();
        let second = OverlayAuthority::new();
        assert_eq!(map.get_or_insert(3, 1, &first, || 30), 30);
        assert_eq!(map.get_or_insert(3, 1, &second, || 31), 30);
        drop(first);
        assert_eq!(map.get_or_insert(3, 1, &second, || 32), 30);
        drop(second);
        assert_eq!(map.len(), 0);
    }

    #[test]
    fn released_keys_do_not_pile_up_in_the_eviction_queue() {
        let map = held(1, 16);
        for key in 0..1000 {
            let authority = OverlayAuthority::new();
            map.insert(key, key, 1, &authority);
        }
        assert_eq!(map.len(), 0);
        assert!(map.queue_len() <= 64, "queue: {}", map.queue_len());
    }
}
