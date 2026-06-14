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
use std::sync::Arc;

use dashmap::DashMap;
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
}

/// The generic, content-addressed framework-surface DTO store.
///
/// `DashMap`-backed so concurrent cold requests for distinct keys do not
/// serialize; concurrent cold requests for the SAME key collapse onto the
/// first writer's value via the normal `entry` API at the call site. Hands out
/// immutable `Arc` values.
#[derive(Debug)]
pub struct FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash,
{
    entries: DashMap<FullKey<K>, Arc<StoredSurfaceDto<B>>>,
}

impl<K, B> Default for FrameworkSurfaceStore<K, B>
where
    K: Clone + PartialEq + Eq + std::hash::Hash,
{
    fn default() -> Self {
        Self {
            entries: DashMap::new(),
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
        Some(candidate)
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
        self.entries.insert(key, Arc::clone(&arc));
        arc
    }

    /// Number of cached entries. Test-only — exercised by the cache-identity
    /// discriminating tests.
    #[cfg(test)]
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store has no entries. Test-only.
    #[cfg(test)]
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

    /// The Svelte adapter remainder (D-bc): a `FullKey` carrying the
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
