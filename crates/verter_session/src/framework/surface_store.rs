#![deny(missing_docs)]
//! The framework-neutral surface DTO store.
//!
//! Every framework adapter materializes its component surfaces (props / emits /
//! slots / options / expose / model) ONCE per `(canonical, content)` per the
//! Shallow File Processing Core Invariant. The host owns the cache so the
//! materialization is shared across requests rather than recomputed per query.
//!
//! [`FrameworkSurfaceStore<K, B>`] is the generic, content-addressed store: its
//! [`FullKey<K>`] carries the four framework-neutral identity columns
//! (`kind`, `query_level`, `canonical`, `owner_whole_hash`) plus the adapter's
//! typed key remainder `K`. The Vue adapter's remainder is
//! [`VueSurfaceKey`](crate::typeinfo::framework_surface::VueSurfaceKey).
//!
//! Cache discipline matches the retired Vue store exactly:
//! - warm read = STRICT same-generation gate (`validated_at_generation ==
//!   live generation`) AND `ReadSetSignature.facts` validation against the
//!   caller's live view;
//! - publication only via [`crate::cache_runtime::SignatureAdmission::Cacheable`];
//! - NO env dims, NO digest, NO version column — a normalizer change is a
//!   registry reset that clears the store.
//!
//! The store is erased behind [`ErasedFrameworkSurfaceStore`] on the
//! registration row; the owning adapter's executor delegate reaches its typed
//! [`FrameworkSurfaceStore<K, B>`] through ONE downcast at store acquisition
//! (not per entry), exactly the public-hidden downcast doctrine the carriers
//! use.

use std::any::Any;
use std::collections::VecDeque;
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use verter_protocol::typeinfo::graph::FrameworkSurfaceKind;
use verter_semantic::analysis::types::Hash16;

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::StoreView;
use crate::typeinfo::types::TypeInfoQueryLevel;

/// Marker + `Any`-bridge for a typed framework-surface DTO bundle.
///
/// Implemented by each adapter's concrete bundle type (the Vue adapter's
/// neutral [`MacroSurfaceDtos`](crate::typeinfo::framework_surface::MacroSurfaceDtos)).
/// Typed retrieval is keyed per-adapter, so a downcast never crosses an adapter
/// boundary in practice; the `Any` bridge exists for the one
/// store-acquisition downcast.
pub trait FrameworkSurfaceDtoBundle: Send + Sync + 'static {
    /// Upcast to `&dyn Any` for the typed store-acquisition downcast.
    fn as_any(&self) -> &dyn Any;
}

/// The framework-neutral cache key.
///
/// The four columns here are common to every adapter; `K` is the adapter's
/// typed key remainder. Content-addressed via `owner_whole_hash` (an edit to
/// the owner SFC's content changes the key and forces a cold rebuild). The
/// [`TypeInfoQueryLevel`] is a QUERY-IDENTITY column (not an env-hash
/// dimension). NO env dims and NO version column enter this key.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FullKey<K: Clone + PartialEq + Eq + std::hash::Hash> {
    /// The framework-surface kind this slot caches.
    pub kind: FrameworkSurfaceKind,
    /// The query level — query identity, not an env hash.
    pub query_level: TypeInfoQueryLevel,
    /// Canonical id of the owner component file.
    pub canonical: Arc<str>,
    /// The owner file's content identity (`IndexedReady::whole_hash`).
    pub owner_whole_hash: Hash16,
    /// The adapter's typed key remainder.
    pub adapter_key: K,
}

/// A cached DTO bundle plus the validation rails the resolution observed.
#[derive(Debug, Clone)]
pub struct StoredSurfaceDto<B> {
    /// The fully-owned, immutable normalizer output.
    pub dto_bundle: Arc<B>,
    /// Path-precise fact signature observed while resolving the surface
    /// (covers cross-file carrier edits).
    pub read_set_signature: ReadSetSignature,
    /// Project generation the entry was validated at.
    pub validated_at_generation: u64,
}

/// Type-erased view over a [`FrameworkSurfaceStore`] for the registration row.
///
/// The registration carries `Arc<dyn ErasedFrameworkSurfaceStore>`; the owning
/// adapter's executor delegate downcasts ONCE (at store acquisition) to its
/// typed `FrameworkSurfaceStore<K, B>`.
pub trait ErasedFrameworkSurfaceStore: Send + Sync {
    /// Upcast to `&dyn Any` for the one store-acquisition downcast.
    fn as_any(&self) -> &dyn Any;
    /// Number of cached surface entries (retention observability).
    fn entry_count(&self) -> usize;
}

/// How many distinct CONTENT versions of one owner file the store keeps.
///
/// Entries are content-addressed by the owner's `whole_hash`, so every edited
/// version of a component mints a fresh slot per surface kind, macro and query
/// level — and before this window nothing ever removed one. A session typing in
/// a component retained one complete normalized props/emits/slots surface per
/// keystroke for its whole life (a 140-prop component: ~180 KiB each, ~250 KiB
/// of heap per open/edit/close cycle in the WSP6 churn lane).
///
/// The versions worth keeping are the ones an editor comes back to: the
/// on-disk content (reloaded after every close), the buffer being edited, and
/// an overlay session's view beside the base one. Recency is refreshed on
/// every warm read, so a version that keeps being asked for stays; an older
/// version is recomputed on demand, which the content-addressed key makes
/// exact. The window is per owner file, so editing one component never evicts
/// another's surfaces.
const CONTENT_VERSIONS_PER_OWNER: usize = 3;

/// Recency of the content versions each owner file currently holds, oldest
/// first, with the exact keys published for each version so eviction removes
/// precisely that version's entries.
struct OwnerVersions<K: Clone + PartialEq + Eq + std::hash::Hash> {
    versions: VecDeque<(Hash16, Vec<FullKey<K>>)>,
}

impl<K: Clone + PartialEq + Eq + std::hash::Hash> Default for OwnerVersions<K> {
    fn default() -> Self {
        Self {
            versions: VecDeque::new(),
        }
    }
}

/// The generic, content-addressed framework-surface DTO store.
///
/// `DashMap`-backed so concurrent cold requests for distinct keys do not
/// serialize. Admission is content-addressed and fact-validated; there is NO
/// in-flight singleflight collapse today — the call sites read via
/// [`Self::get_with_view`] then publish via [`Self::insert`], so two concurrent
/// cold callers for the SAME key both materialize and the last writer wins (the
/// values are content-equivalent, so this is safe; see [`Self::insert`]). True
/// singleflight is a follow-up for when this store is consolidated onto
/// `ProjectTypeStore`. Hands out immutable `Arc` values.
///
/// PROVISIONAL: this store lives on the framework registry row, OUTSIDE the
/// single `ProjectTypeStore`. It is fact-validated (content-addressed admission
/// gated at the call sites), so it is correct today, but it is a temporary
/// off-`ProjectTypeStore` cache still to be consolidated onto `ProjectTypeStore`
/// (which also adds true in-flight singleflight).
#[derive(Debug)]
pub struct FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash,
{
    entries: DashMap<FullKey<K>, Arc<StoredSurfaceDto<B>>>,
    /// Per owner file: the content versions it holds, oldest first (see
    /// [`CONTENT_VERSIONS_PER_OWNER`]). Taken AFTER any `entries` shard
    /// reference is released, never while one is held.
    owners: parking_lot::Mutex<FxHashMap<Arc<str>, OwnerVersions<K>>>,
}

impl<K: Clone + PartialEq + Eq + std::hash::Hash> std::fmt::Debug for OwnerVersions<K> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OwnerVersions")
            .field("versions", &self.versions.len())
            .finish()
    }
}

impl<K, B> Default for FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash,
{
    fn default() -> Self {
        Self {
            entries: DashMap::new(),
            owners: parking_lot::Mutex::new(FxHashMap::default()),
        }
    }
}

impl<K, B> FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash,
{
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the cached entry for `key` IFF it still validates under the live
    /// `view` and project `generation`.
    ///
    /// Both gates must pass: the strict same-generation gate AND the
    /// `ReadSetSignature.facts` validation against the caller's live view. A
    /// carrier edit that bumps a dependency fact, or a project-generation
    /// reset, invalidates the entry lazily.
    #[must_use]
    pub fn get_with_view<V: StoreView + ?Sized>(
        &self,
        key: &FullKey<K>,
        view: &V,
        generation: u64,
    ) -> Option<Arc<StoredSurfaceDto<B>>> {
        let candidate = Arc::clone(self.entries.get(key)?.value());
        if candidate.validated_at_generation != generation {
            return None;
        }
        if !view.validates_fact_signature(&candidate.read_set_signature.facts) {
            return None;
        }
        self.touch(&key.canonical, key.owner_whole_hash);
        Some(candidate)
    }

    /// Mark `(canonical, whole_hash)` as the owner's most recently used
    /// version, so a version that keeps being read is not the one evicted.
    fn touch(&self, canonical: &Arc<str>, whole_hash: Hash16) {
        let mut owners = self.owners.lock();
        if let Some(owner) = owners.get_mut(canonical) {
            if let Some(position) = owner
                .versions
                .iter()
                .position(|(hash, _)| *hash == whole_hash)
            {
                if let Some(version) = owner.versions.remove(position) {
                    owner.versions.push_back(version);
                }
            }
        }
    }

    /// Record `key` under its owner's version list (making that version the
    /// most recent) and return the keys of any version pushed out of the
    /// window, for the caller to remove from `entries`.
    fn record_version(&self, key: &FullKey<K>) -> Vec<FullKey<K>> {
        let mut owners = self.owners.lock();
        let owner = owners.entry(Arc::clone(&key.canonical)).or_default();
        let version = match owner
            .versions
            .iter()
            .position(|(hash, _)| *hash == key.owner_whole_hash)
        {
            Some(position) => owner.versions.remove(position).unwrap_or_default(),
            None => (key.owner_whole_hash, Vec::new()),
        };
        let (hash, mut keys) = version;
        if !keys.contains(key) {
            keys.push(key.clone());
        }
        owner.versions.push_back((hash, keys));
        let mut evicted = Vec::new();
        while owner.versions.len() > CONTENT_VERSIONS_PER_OWNER {
            if let Some((_, keys)) = owner.versions.pop_front() {
                evicted.extend(keys);
            }
        }
        evicted
    }

    /// Memoize `entry` under `key`, REPLACING any existing entry, and return
    /// the canonical `Arc`.
    ///
    /// The cold path that calls this runs ONLY after a `get_with_view` miss.
    /// A carrier edit bumps a per-canonical fact (not the project generation),
    /// so a stale carrier-dep entry and its fresh replacement share the same
    /// generation; an unconditional overwrite is therefore required (a
    /// same-generation keep would pin the stale value). Concurrent cold races
    /// compute the same fresh value, so last-writer-wins is value-equivalent.
    pub fn insert(&self, key: FullKey<K>, entry: StoredSurfaceDto<B>) -> Arc<StoredSurfaceDto<B>> {
        let arc = Arc::new(entry);
        let evicted = self.record_version(&key);
        self.entries.insert(key, Arc::clone(&arc));
        for stale in evicted {
            self.entries.remove(&stale);
        }
        arc
    }

    /// Number of cached entries (retention observability and the
    /// cache-identity discriminating tests).
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<K, B> ErasedFrameworkSurfaceStore for FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash + Send + Sync + 'static,
    B: Send + Sync + 'static,
{
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq, Hash)]
    struct FixtureKey {
        macro_index: usize,
    }

    /// Whole-struct destructure pin: every column of `FullKey` is named, so a
    /// future field addition forces this test to acknowledge it (cache-key
    /// structurality guard).
    #[test]
    fn full_key_is_structural_whole_destructure() {
        let key = FullKey {
            kind: FrameworkSurfaceKind::Props,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from("/a.vue"),
            owner_whole_hash: [0u8; 16],
            adapter_key: FixtureKey { macro_index: 0 },
        };
        let FullKey {
            kind,
            query_level,
            canonical,
            owner_whole_hash,
            adapter_key,
        } = &key;
        assert_eq!(*kind, FrameworkSurfaceKind::Props);
        assert_eq!(*query_level, TypeInfoQueryLevel::FullMetadata);
        assert_eq!(canonical.as_ref(), "/a.vue");
        assert_eq!(*owner_whole_hash, [0u8; 16]);
        assert_eq!(adapter_key.macro_index, 0);
    }

    /// The Svelte adapter remainder: a `FullKey` carrying the
    /// `SvelteSurfaceKey { source }` remainder destructures whole-struct, and
    /// two distinct source families never alias. Pins the Svelte adapter's one
    /// key column (the program's flagship non-Vue vertical).
    #[test]
    fn full_key_with_svelte_remainder_is_structural() {
        use crate::typeinfo::framework_surface::{SvelteSurfaceKey, SvelteSurfaceSource};
        let key = FullKey {
            kind: FrameworkSurfaceKind::Slots,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from("/App.svelte"),
            owner_whole_hash: [0u8; 16],
            adapter_key: SvelteSurfaceKey {
                source: SvelteSurfaceSource::SnippetProps,
            },
        };
        let FullKey {
            kind,
            query_level,
            canonical,
            owner_whole_hash,
            adapter_key,
        } = &key;
        assert_eq!(*kind, FrameworkSurfaceKind::Slots);
        assert_eq!(*query_level, TypeInfoQueryLevel::FullMetadata);
        assert_eq!(canonical.as_ref(), "/App.svelte");
        assert_eq!(*owner_whole_hash, [0u8; 16]);
        // The whole-struct destructure of the Svelte remainder — a new source
        // family column would force this to acknowledge it.
        let SvelteSurfaceKey { source } = adapter_key;
        assert_eq!(*source, SvelteSurfaceSource::SnippetProps);
        // The two SLOTS source families never alias under the same kind/owner.
        let legacy = FullKey {
            adapter_key: SvelteSurfaceKey {
                source: SvelteSurfaceSource::LegacySlotInventory,
            },
            ..key.clone()
        };
        assert_ne!(key, legacy);
    }

    #[test]
    fn distinct_columns_never_alias() {
        let base = FullKey {
            kind: FrameworkSurfaceKind::Props,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from("/a.vue"),
            owner_whole_hash: [1u8; 16],
            adapter_key: FixtureKey { macro_index: 0 },
        };
        // macro_index 0 vs 1 never alias.
        let other_index = FullKey {
            adapter_key: FixtureKey { macro_index: 1 },
            ..base.clone()
        };
        assert_ne!(base, other_index);
        // PublicType vs FullMetadata distinct slots.
        let other_level = FullKey {
            query_level: TypeInfoQueryLevel::PublicType,
            ..base.clone()
        };
        assert_ne!(base, other_level);
        // Distinct kind never aliases.
        let other_kind = FullKey {
            kind: FrameworkSurfaceKind::Emits,
            ..base.clone()
        };
        assert_ne!(base, other_kind);
    }

    #[test]
    fn warm_read_requires_same_generation() {
        let store: FrameworkSurfaceStore<FixtureKey, u32> = FrameworkSurfaceStore::new();
        let key = FullKey {
            kind: FrameworkSurfaceKind::Props,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from("/a.vue"),
            owner_whole_hash: [2u8; 16],
            adapter_key: FixtureKey { macro_index: 0 },
        };
        store.insert(
            key.clone(),
            StoredSurfaceDto {
                dto_bundle: Arc::new(7u32),
                read_set_signature: ReadSetSignature::empty(),
                validated_at_generation: 5,
            },
        );

        let live_view = crate::resolver_core::PermissiveStoreView;
        // Same generation + empty facts ⇒ warm hit.
        assert!(store.get_with_view(&key, &live_view, 5).is_some());
        // Generation bump ⇒ miss (strict same-generation gate).
        assert!(store.get_with_view(&key, &live_view, 6).is_none());
    }

    /// A `StoreView` that REJECTS every non-empty fact signature — used to
    /// discriminate the fact-rail gate (a view that rejects a tracked
    /// cross-file fact must miss the warm entry even at the right generation).
    fn version_key(canonical: &str, version: u8, macro_index: usize) -> FullKey<FixtureKey> {
        FullKey {
            kind: FrameworkSurfaceKind::Props,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from(canonical),
            owner_whole_hash: [version; 16],
            adapter_key: FixtureKey { macro_index },
        }
    }

    fn publish(store: &FrameworkSurfaceStore<FixtureKey, u32>, key: &FullKey<FixtureKey>) {
        store.insert(
            key.clone(),
            StoredSurfaceDto {
                dto_bundle: Arc::new(1u32),
                read_set_signature: ReadSetSignature::empty(),
                validated_at_generation: 0,
            },
        );
    }

    /// An editing session keeps a bounded window of each owner file's content
    /// versions instead of one surface per keystroke for the life of the host.
    ///
    /// Discriminating: before the window, entries were only ever inserted, so
    /// eight edited versions of one component left eight props surfaces (and
    /// eight emits surfaces) resident — the WSP6 churn lane's ~250 KiB of heap
    /// per open/edit/close cycle. Here the owner keeps its newest
    /// `CONTENT_VERSIONS_PER_OWNER` versions, every surface of an evicted
    /// version goes with it, and another owner's entries are untouched.
    #[test]
    fn an_edit_loop_keeps_a_bounded_window_of_content_versions_per_owner() {
        let store: FrameworkSurfaceStore<FixtureKey, u32> = FrameworkSurfaceStore::new();
        let neighbour = version_key("/b.vue", 200, 0);
        publish(&store, &neighbour);
        for version in 1..=8u8 {
            // Two surfaces per version (two macros), as a component with both
            // defineProps and defineEmits publishes.
            publish(&store, &version_key("/a.vue", version, 0));
            publish(&store, &version_key("/a.vue", version, 1));
            let owned = usize::from(version).min(CONTENT_VERSIONS_PER_OWNER);
            assert_eq!(
                store.len(),
                1 + 2 * owned,
                "after version {version}: the owner holds at most its newest \
                 {CONTENT_VERSIONS_PER_OWNER} versions, never one per edit"
            );
        }
        let live_view = crate::resolver_core::PermissiveStoreView;
        assert!(
            store.get_with_view(&neighbour, &live_view, 0).is_some(),
            "editing one owner never evicts another owner's surfaces"
        );
        assert!(store
            .get_with_view(&version_key("/a.vue", 8, 1), &live_view, 0)
            .is_some());
        assert!(store
            .get_with_view(&version_key("/a.vue", 5, 0), &live_view, 0)
            .is_none());
    }

    /// A version that keeps being READ stays in the window while newer
    /// versions come and go — the on-disk content an editor reloads after every
    /// close is exactly this shape.
    ///
    /// Discriminating: with insertion-order eviction only, the disk version
    /// (published first and afterwards only read) is the oldest entry and is
    /// evicted as soon as the window fills, so every reopen recomputes it.
    #[test]
    fn a_version_that_keeps_being_read_survives_newer_edits() {
        let store: FrameworkSurfaceStore<FixtureKey, u32> = FrameworkSurfaceStore::new();
        let live_view = crate::resolver_core::PermissiveStoreView;
        let disk = version_key("/a.vue", 1, 0);
        publish(&store, &disk);
        for edit in 2..=10u8 {
            assert!(
                store.get_with_view(&disk, &live_view, 0).is_some(),
                "the on-disk version is still warm before edit {edit}"
            );
            publish(&store, &version_key("/a.vue", edit, 0));
        }
        assert!(store.get_with_view(&disk, &live_view, 0).is_some());
        assert_eq!(store.len(), CONTENT_VERSIONS_PER_OWNER);
    }

    struct RejectingStoreView;
    impl crate::resolver_core::StoreView for RejectingStoreView {
        fn compat_token(&self) -> crate::resolver_core::StoreViewCompatToken {
            crate::resolver_core::StoreViewCompatToken {
                epoch: 0,
                session: None,
                validity_fingerprint: 0,
            }
        }
        fn validates(&self, _fact: &crate::resolver_core::FactVersionRef) -> bool {
            false
        }
    }

    #[test]
    fn warm_read_rejects_when_fact_rail_fails() {
        let store: FrameworkSurfaceStore<FixtureKey, u32> = FrameworkSurfaceStore::new();
        let key = FullKey {
            kind: FrameworkSurfaceKind::Props,
            query_level: TypeInfoQueryLevel::FullMetadata,
            canonical: Arc::from("/a.vue"),
            owner_whole_hash: [3u8; 16],
            adapter_key: FixtureKey { macro_index: 0 },
        };
        // A NON-EMPTY cross-file fact signature: the entry observed a carrier
        // dependency's whole hash. The fact rail must be consulted on warm read.
        let cross_file_fact = crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: "/Carrier.ts".to_string(),
            hash: [9u8; 16],
        };
        store.insert(
            key.clone(),
            StoredSurfaceDto {
                dto_bundle: Arc::new(11u32),
                read_set_signature: ReadSetSignature::new(Arc::from(
                    vec![cross_file_fact].into_boxed_slice(),
                )),
                validated_at_generation: 5,
            },
        );

        // Right generation but a view that rejects the tracked fact ⇒ MISS.
        // (If the fact-rail gate were deleted, this would WRONGLY warm-hit.)
        assert!(store.get_with_view(&key, &RejectingStoreView, 5).is_none());
        // The permissive view accepts the same tracked fact ⇒ warm hit, proving
        // the miss above is the fact rail and not the generation gate.
        let permissive = crate::resolver_core::PermissiveStoreView;
        assert!(store.get_with_view(&key, &permissive, 5).is_some());
    }
}
