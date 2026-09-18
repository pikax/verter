//! Interned handles for large family-key payloads.
//!
//! `RelateMemoKey` (144B) and `ResolveCallKey` intern behind a compact
//! handle so [`super::family::FamilyKey`] stays at the 136B bound with
//! margin instead of embedding those payloads by value. Identity is the
//! intern id (exact equality of the interned value); the handle `Deref`s
//! to the interned payload.
//!
//! Tables hold `Weak` only. Memo eviction dropping the last handle lets
//! the payload `Arc` fall; the next intern (or a sweep) reclaims the
//! hash entry. Lookup identity is unchanged: live handles still `Hash`/
//! `Eq` by intern id and `Deref` without a table walk.

use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;
use std::sync::Weak;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHasher;

use crate::semantic_query::{RelateMemoKey, ResolveCallKey};

/// Auto-sweep after this many inserts so dead buckets do not wait for
/// their hash to be interned again.
const SWEEP_INTERVAL: u64 = 1024;

/// Interned `RelateMemoKey` handle. Copy of the intern id plus a cheap
/// `Arc` so `Deref` needs no table lookup on the read path.
#[derive(Clone)]
pub(super) struct InternedRelateKey {
    id: u32,
    inner: Arc<RelateMemoKey>,
}

/// Interned `ResolveCallKey` handle.
#[derive(Clone)]
pub(super) struct InternedResolveCallKey {
    id: u32,
    inner: Arc<ResolveCallKey>,
}

struct Slot<T> {
    item: Weak<T>,
}

struct Table<T> {
    by_hash: FxHashMap<u64, Vec<u32>>,
    items: Vec<Slot<T>>,
    free: Vec<u32>,
    inserts_since_sweep: u64,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            by_hash: FxHashMap::default(),
            items: Vec::new(),
            free: Vec::new(),
            inserts_since_sweep: 0,
        }
    }
}

fn content_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

fn prune_dead_ids<T>(guard: &mut Table<T>, hash: u64) -> Vec<u32> {
    let Some(ids) = guard.by_hash.remove(&hash) else {
        return Vec::new();
    };
    let mut live = Vec::with_capacity(ids.len());
    for id in ids {
        if guard.items[id as usize].item.strong_count() > 0 {
            live.push(id);
        } else {
            guard.free.push(id);
        }
    }
    if !live.is_empty() {
        guard.by_hash.insert(hash, live.clone());
    }
    live
}

fn intern_in<T: Hash + Eq>(table: &Mutex<Table<T>>, value: T) -> (u32, Arc<T>) {
    let hash = content_hash(&value);
    let mut guard = table.lock();
    for id in prune_dead_ids(&mut guard, hash) {
        if let Some(arc) = guard.items[id as usize].item.upgrade() {
            if *arc == value {
                return (id, arc);
            }
        }
    }
    let inner = Arc::new(value);
    let id = if let Some(id) = guard.free.pop() {
        guard.items[id as usize] = Slot {
            item: Arc::downgrade(&inner),
        };
        id
    } else {
        let id = u32::try_from(guard.items.len()).expect("intern table overflow");
        guard.items.push(Slot {
            item: Arc::downgrade(&inner),
        });
        id
    };
    guard.by_hash.entry(hash).or_default().push(id);
    guard.inserts_since_sweep = guard.inserts_since_sweep.wrapping_add(1);
    if guard.inserts_since_sweep.is_multiple_of(SWEEP_INTERVAL) {
        sweep_table(&mut guard);
    }
    (id, inner)
}

fn sweep_table<T>(guard: &mut Table<T>) {
    let hashes: Vec<u64> = guard.by_hash.keys().copied().collect();
    for hash in hashes {
        let _ = prune_dead_ids(guard, hash);
    }
}

fn relate_table() -> &'static Mutex<Table<RelateMemoKey>> {
    static TABLE: OnceLock<Mutex<Table<RelateMemoKey>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(Table::default()))
}

fn resolve_call_table() -> &'static Mutex<Table<ResolveCallKey>> {
    static TABLE: OnceLock<Mutex<Table<ResolveCallKey>>> = OnceLock::new();
    TABLE.get_or_init(|| Mutex::new(Table::default()))
}

impl InternedRelateKey {
    pub(super) fn intern(key: RelateMemoKey) -> Self {
        let (id, inner) = intern_in(relate_table(), key);
        Self { id, inner }
    }
}

impl InternedResolveCallKey {
    pub(super) fn intern(key: ResolveCallKey) -> Self {
        let (id, inner) = intern_in(resolve_call_table(), key);
        Self { id, inner }
    }
}

impl PartialEq for InternedRelateKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for InternedRelateKey {}
impl Hash for InternedRelateKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl Deref for InternedRelateKey {
    type Target = RelateMemoKey;
    fn deref(&self) -> &RelateMemoKey {
        &self.inner
    }
}
impl std::fmt::Debug for InternedRelateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternedRelateKey")
            .field("id", &self.id)
            .field("inner", self.inner.as_ref())
            .finish()
    }
}

impl PartialEq for InternedResolveCallKey {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
impl Eq for InternedResolveCallKey {}
impl Hash for InternedResolveCallKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}
impl Deref for InternedResolveCallKey {
    type Target = ResolveCallKey;
    fn deref(&self) -> &ResolveCallKey {
        &self.inner
    }
}
impl std::fmt::Debug for InternedResolveCallKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InternedResolveCallKey")
            .field("id", &self.id)
            .field("inner", self.inner.as_ref())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::{RelateMemoKey, RelationContext, SemanticNodeId};

    fn unique_key(tag: u64) -> RelateMemoKey {
        RelateMemoKey::assignable(
            SemanticNodeId(0xF1A1_0000_0000_0000 | tag),
            SemanticNodeId(0xF1A1_0000_0000_0001 | tag << 8),
            RelationContext::default(),
        )
    }

    #[test]
    fn interned_relate_keys_reclaim_when_handles_drop() {
        let key = unique_key(0x51);
        let handle = InternedRelateKey::intern(key.clone());
        let id = handle.id;
        assert!(
            relate_table().lock().items[id as usize].item.strong_count() > 0,
            "a live handle keeps the interned payload"
        );
        drop(handle);
        {
            let mut guard = relate_table().lock();
            sweep_table(&mut guard);
            assert_eq!(
                guard.items[id as usize].item.strong_count(),
                0,
                "memo-eviction dropping the last handle must drop the interned Arc"
            );
        }
        let again = InternedRelateKey::intern(key);
        assert_eq!(again.source, unique_key(0x51).source);
        drop(again);
    }
}
