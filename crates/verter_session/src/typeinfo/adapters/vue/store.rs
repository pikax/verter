#![deny(missing_docs)]
//! Host-owned cache of a `.vue` SFC's extracted shallow macro metadata.
//!
//! A `.vue`'s public surface and per-macro normalized component-meta DTOs are
//! materialized ONCE per `(canonical, content)` and stored here, honoring the
//! Shallow File Processing Core Invariant (process a canonical file's surface
//! once per content hash through one shared host path) and the
//! host-owned-cache rule (no request-local `RefCell` / per-request map — the
//! cache lives on `VerterHost`).
//!
//! **What is cached: the normalized DTO bundle, NOT the graph surface.** The
//! cached value is the fully-owned, immutable normalizer OUTPUT — a
//! [`VueMacroDtos`] carrying the `AnalyzedPropField` / `AnalyzedEmitField` /
//! `AnalyzedSlotField` vectors (each built from owned `TypeExpr` + scope +
//! `String`). It deliberately does NOT cache the transient
//! [`super::surface::VueMacroSurface`], whose `SemanticNodeId`s are graph-
//! generation-scoped and therefore unsafe to retain across a generation flip.
//! The DTO bundle is content-addressed: the cache key carries the `.vue`'s
//! `whole_hash`, so a content edit yields a fresh key and a cold rebuild — a
//! stale entry can never be returned for changed content (content-addressed
//! artifact cache discipline, R-series).
//!
//! The macro-shape producers consult this store for a `.vue`'s normalized
//! macro DTOs; the host owns it so the materialization is shared across
//! requests rather than recomputed per query.

use std::sync::Arc;

use dashmap::DashMap;
use verter_semantic::analysis::types::{
    AnalyzedEmitField, AnalyzedPropField, AnalyzedSlotField, Hash16,
};

use crate::typeinfo::types::TypeInfoQueryLevel;

/// Cache key for one `.vue` macro surface's normalized DTOs.
///
/// Content-addressed (carries `whole_hash`) so a content edit changes the key
/// and forces a cold rebuild. The [`TypeInfoQueryLevel`] is folded in as a
/// QUERY-IDENTITY tag (NOT an env-hash dimension) so the same macro resolved at
/// PublicType vs FullMetadata occupies distinct slots.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VueMacroDtoKey {
    /// Canonical id of the `.vue` SFC.
    pub canonical: Arc<str>,
    /// The SFC's content identity (`IndexedReady::whole_hash`).
    pub whole_hash: Hash16,
    /// Stable index of the macro in the SFC's analysis snapshot.
    pub macro_index: usize,
    /// Query level — query identity, not an env hash.
    pub level_tag: u8,
}

impl VueMacroDtoKey {
    /// Construct a key for `macro_index` in `canonical` at content `whole_hash`
    /// and query `level`.
    #[must_use]
    pub fn new(
        canonical: Arc<str>,
        whole_hash: Hash16,
        macro_index: usize,
        level: TypeInfoQueryLevel,
    ) -> Self {
        Self {
            canonical,
            whole_hash,
            macro_index,
            level_tag: level.cache_tag(),
        }
    }
}

/// The normalized component-meta DTOs extracted from ONE `.vue` macro surface.
///
/// Exactly one of `props` / `emits` / `slots` is populated for a given macro
/// kind (a `DefineProps` / `WithDefaults` / `DefineModel` macro contributes
/// `props`; `DefineEmits` contributes `emits`; `DefineSlots` contributes
/// `slots`). All fields are fully owned + immutable — safe for a host-owned
/// `Send + Sync` cache and stable across graph-generation flips.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct VueMacroDtos {
    /// Prop fields (`DefineProps` / `WithDefaults` / `DefineModel`).
    pub props: Vec<AnalyzedPropField>,
    /// Emit fields (`DefineEmits`).
    pub emits: Vec<AnalyzedEmitField>,
    /// Slot fields (`DefineSlots`).
    pub slots: Vec<AnalyzedSlotField>,
}

/// Host-owned cache of `.vue` macro-surface normalized DTOs.
///
/// `DashMap`-backed (native) so concurrent cold requests for distinct
/// `.vue` files do not serialize on a single lock; concurrent cold requests
/// for the SAME key collapse onto the first writer's value via the normal
/// `entry` API at the call site. Stores immutable `Arc<VueMacroDtos>` values
/// so a reader holds a cheap refcount, never a borrow into the map.
#[derive(Debug, Default)]
pub struct VueShallowMetadataStore {
    entries: DashMap<VueMacroDtoKey, Arc<VueMacroDtos>>,
}

impl VueShallowMetadataStore {
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Return the cached DTO bundle for `key`, if present. Cloning the
    /// `Arc` is O(1) and releases the map shard immediately.
    #[must_use]
    pub fn get(&self, key: &VueMacroDtoKey) -> Option<Arc<VueMacroDtos>> {
        self.entries.get(key).map(|entry| Arc::clone(entry.value()))
    }

    /// Memoize `value` under `key` and return the canonical `Arc` for it.
    ///
    /// First-writer-wins: if a concurrent cold request already published a
    /// value for `key`, the existing `Arc` is returned and `value` is dropped
    /// (the cache stays immutable-after-publish). The content-addressed key
    /// guarantees both writers computed against the same content.
    pub fn get_or_insert(&self, key: VueMacroDtoKey, value: VueMacroDtos) -> Arc<VueMacroDtos> {
        Arc::clone(
            self.entries
                .entry(key)
                .or_insert_with(|| Arc::new(value))
                .value(),
        )
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
