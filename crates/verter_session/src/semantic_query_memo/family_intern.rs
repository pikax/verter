//! Interned handles for large family-key payloads.
//!
//! `RelateMemoKey` (144B) and `ResolveCallKey` intern behind a compact
//! handle so [`super::family::FamilyKey`] stays at the 136B bound with
//! margin instead of embedding those payloads by value. Identity is the
//! intern id (exact equality of the interned value); the handle `Deref`s
//! to the interned payload.

use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::sync::OnceLock;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;
use rustc_hash::FxHasher;

use crate::semantic_query::{RelateMemoKey, ResolveCallKey};

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

struct Table<T> {
    by_hash: FxHashMap<u64, Vec<u32>>,
    items: Vec<Arc<T>>,
}

impl<T> Default for Table<T> {
    fn default() -> Self {
        Self {
            by_hash: FxHashMap::default(),
            items: Vec::new(),
        }
    }
}

fn content_hash<T: Hash>(value: &T) -> u64 {
    let mut hasher = FxHasher::default();
    value.hash(&mut hasher);
    hasher.finish()
}

fn intern_in<T: Hash + Eq>(table: &Mutex<Table<T>>, value: T) -> (u32, Arc<T>) {
    let hash = content_hash(&value);
    let mut guard = table.lock();
    if let Some(ids) = guard.by_hash.get(&hash) {
        for &id in ids {
            if *guard.items[id as usize] == value {
                return (id, Arc::clone(&guard.items[id as usize]));
            }
        }
    }
    let id = u32::try_from(guard.items.len()).expect("intern table overflow");
    let inner = Arc::new(value);
    guard.items.push(Arc::clone(&inner));
    guard.by_hash.entry(hash).or_default().push(id);
    (id, inner)
}

impl InternedRelateKey {
    pub(super) fn intern(key: RelateMemoKey) -> Self {
        static TABLE: OnceLock<Mutex<Table<RelateMemoKey>>> = OnceLock::new();
        let table = TABLE.get_or_init(|| Mutex::new(Table::default()));
        let (id, inner) = intern_in(table, key);
        Self { id, inner }
    }
}

impl InternedResolveCallKey {
    pub(super) fn intern(key: ResolveCallKey) -> Self {
        static TABLE: OnceLock<Mutex<Table<ResolveCallKey>>> = OnceLock::new();
        let table = TABLE.get_or_init(|| Mutex::new(Table::default()));
        let (id, inner) = intern_in(table, key);
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
