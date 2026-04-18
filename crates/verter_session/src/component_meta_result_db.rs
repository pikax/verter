//! Final component-meta result cache (Phase 3 of the project-global overhaul).
//!
//! [`ComponentMetaResultDb`] is the authoritative final-payload cache for
//! component-meta results. Identical repeated requests on an unchanged
//! owner return from this cache with near-zero resolver work; concurrent
//! cold requests for the same owner/query coalesce onto one build.
//!
//! ## Contract
//!
//! - Key: [`ComponentMetaResultKey`] =
//!   `(owner_canonical, owner_whole_hash, query_kind, options_fingerprint)`.
//! - Value: immutable `Arc` payloads — the native component-meta result
//!   and any strictly projected derivatives.
//! - **Transitive dependency validation on lookup**: every warm entry
//!   stores the exact [`DepSignature`] it observed at build time; lookups
//!   revalidate that signature against the live host.
//! - **`options_fingerprint` is a stable `Hash16`** produced from a
//!   manually-stable serialization of output-affecting fields only —
//!   never request ids, trace flags, or caller metadata.
//! - Cancelled, budget-exceeded, or partial results are **not** promoted
//!   into the cache. They must surface as `QueryError` variants to the
//!   caller.

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::Hash16;

use crate::semantic_query::DepSignature;

/// Output-affecting query shape. Expanded as new public component-meta
/// query kinds land — every distinct output shape becomes a variant so
/// the cache does not collapse them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ComponentMetaQueryKind {
    /// The existing `getComponentMeta` payload (native schema).
    Native,
    /// The compat projection layer exposed to `vue-component-meta`
    /// interoperability callers. Kept as a distinct variant because the
    /// projection differs in shape, not just metadata.
    Compat,
}

/// Stable fingerprint over output-affecting options. Constructed by the
/// caller from an explicitly versioned serialization; the type alias
/// points at the workspace-wide [`Hash16`] so downstream tooling does not
/// invent a parallel hash.
pub type ComponentMetaOptionsFingerprint = Hash16;

/// Cache key for the final component-meta result.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ComponentMetaResultKey {
    pub owner_canonical: Arc<str>,
    pub owner_whole_hash: Hash16,
    pub query_kind: ComponentMetaQueryKind,
    pub options_fingerprint: ComponentMetaOptionsFingerprint,
}

/// Cache entry — the payload plus the exact dep signature observed during
/// the build.
pub struct ComponentMetaResultEntry<P> {
    pub payload: Arc<P>,
    pub dep_signature: DepSignature,
}

impl<P> Clone for ComponentMetaResultEntry<P> {
    fn clone(&self) -> Self {
        Self {
            payload: self.payload.clone(),
            dep_signature: self.dep_signature.clone(),
        }
    }
}

/// Host-owned final result cache. Generic over the payload type so native
/// and compat projections can share the same backing without double-caching
/// semantic meaning.
pub struct ComponentMetaResultDb<P> {
    entries: DashMap<ComponentMetaResultKey, ComponentMetaResultEntry<P>>,
    /// Soft size cap — when exceeded, bounded cleanup slices run on the
    /// top-level query exit path (plan § A1.8). Cleanup in Phase 3 is
    /// `stale-first, LRU next`; precise policy lives with the dispatcher.
    capacity: usize,
}

impl<P> ComponentMetaResultDb<P> {
    /// Default capacity — editor sessions with many owner/options
    /// combinations cap here before triggering bounded stale cleanup. Tuned
    /// later against live profiling; 512 matches the order-of-magnitude
    /// called out in the plan's memory budget.
    pub const DEFAULT_CAPACITY: usize = 512;

    pub fn new() -> Self {
        Self::with_capacity(Self::DEFAULT_CAPACITY)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: DashMap::new(),
            capacity,
        }
    }

    /// Cache size target. Exceeding it triggers bounded stale cleanup.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Strict lookup — returns the cached entry when present. The caller
    /// is responsible for revalidating the dep signature before publishing
    /// the result; this split keeps the cache decoupled from the live host.
    pub fn get(&self, key: &ComponentMetaResultKey) -> Option<ComponentMetaResultEntry<P>> {
        self.entries.get(key).map(|v| v.clone())
    }

    /// Insert a final result entry. Cancelled, budget-exceeded, or partial
    /// results must **not** be passed here — callers are responsible for
    /// filtering. The cache does not inspect the payload.
    pub fn insert(&self, key: ComponentMetaResultKey, entry: ComponentMetaResultEntry<P>) {
        self.entries.insert(key, entry);
    }

    /// Remove a key outright.
    pub fn remove(&self, key: &ComponentMetaResultKey) -> Option<ComponentMetaResultEntry<P>> {
        self.entries.remove(key).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Iterate through keys present in the cache. Primarily used by the
    /// bounded cleanup helper below.
    pub fn keys(&self) -> Vec<ComponentMetaResultKey> {
        self.entries
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }
}

impl<P> Default for ComponentMetaResultDb<P> {
    fn default() -> Self {
        Self::new()
    }
}

impl<P> std::fmt::Debug for ComponentMetaResultDb<P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ComponentMetaResultDb")
            .field("entries", &self.len())
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::DepVersion;

    #[test]
    fn insert_and_get_roundtrip() {
        #[derive(Clone, PartialEq, Eq, Debug)]
        struct MockPayload(u32);
        let db: ComponentMetaResultDb<MockPayload> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/Accordion.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [9u8; 16],
        };
        let entry = ComponentMetaResultEntry {
            payload: Arc::new(MockPayload(42)),
            dep_signature: Arc::from(
                vec![(
                    Arc::<str>::from("/w/Accordion.vue"),
                    DepVersion::WholeHash([1u8; 16]),
                )]
                .into_boxed_slice(),
            ),
        };
        db.insert(key.clone(), entry);
        let hit = db.get(&key).unwrap();
        assert_eq!(*hit.payload, MockPayload(42));
        assert_eq!(hit.dep_signature.len(), 1);
    }

    #[test]
    fn distinct_query_kinds_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let native_key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [0u8; 16],
        };
        let compat_key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Compat,
            options_fingerprint: [0u8; 16],
        };
        db.insert(
            native_key.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.get(&native_key).is_some());
        assert!(db.get(&compat_key).is_none());
    }

    #[test]
    fn distinct_options_fingerprints_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k1 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [1u8; 16],
        };
        let k2 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [2u8; 16],
        };
        db.insert(
            k1.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.get(&k1).is_some());
        assert!(db.get(&k2).is_none());
    }

    #[test]
    fn distinct_owner_hashes_do_not_alias() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let k_v1 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [9u8; 16],
        };
        let k_v2 = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [2u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [9u8; 16],
        };
        db.insert(
            k_v1.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(1u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.get(&k_v1).is_some());
        assert!(db.get(&k_v2).is_none());
    }

    #[test]
    fn remove_clears_entry() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        let key = ComponentMetaResultKey {
            owner_canonical: Arc::from("/w/o.vue"),
            owner_whole_hash: [1u8; 16],
            query_kind: ComponentMetaQueryKind::Native,
            options_fingerprint: [0u8; 16],
        };
        db.insert(
            key.clone(),
            ComponentMetaResultEntry {
                payload: Arc::new(5u32),
                dep_signature: Arc::from(Vec::new().into_boxed_slice()),
            },
        );
        assert!(db.remove(&key).is_some());
        assert!(db.get(&key).is_none());
    }

    #[test]
    fn capacity_default_matches_plan() {
        let db: ComponentMetaResultDb<u32> = ComponentMetaResultDb::new();
        assert_eq!(
            db.capacity(),
            ComponentMetaResultDb::<u32>::DEFAULT_CAPACITY
        );
        assert_eq!(ComponentMetaResultDb::<u32>::DEFAULT_CAPACITY, 512);
    }
}
