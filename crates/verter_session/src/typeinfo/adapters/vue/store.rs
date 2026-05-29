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
    AnalyzedEmitField, AnalyzedMacroKind, AnalyzedPropField, AnalyzedSlotField, Hash16,
};

use crate::fact_signature_helpers::ReadSetSignature;
use crate::resolver_core::StoreView;
use crate::typeinfo::types::TypeInfoQueryLevel;

/// Cache key for one `.vue` macro surface's normalized DTOs.
///
/// Content-addressed (carries `whole_hash`) so a content edit changes the key
/// and forces a cold rebuild. The [`TypeInfoQueryLevel`] is folded in as a
/// QUERY-IDENTITY tag (NOT an env-hash dimension) so the same macro resolved at
/// PublicType vs FullMetadata occupies distinct slots. The macro KIND is part
/// of the key so two requests that name the same `(canonical, whole_hash,
/// macro_index, level)` slot but disagree on the macro kind (a caller bug, or
/// a snapshot whose macro at that index is a different kind than the caller
/// believed) cannot read or poison each other's entry — the kind is derived
/// from the authoritative `IndexedReady` snapshot at insert time.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VueMacroDtoKey {
    /// Canonical id of the `.vue` SFC.
    pub canonical: Arc<str>,
    /// The SFC's content identity (`IndexedReady::whole_hash`).
    pub whole_hash: Hash16,
    /// Stable index of the macro in the SFC's analysis snapshot.
    pub macro_index: usize,
    /// The macro kind the cached DTO bundle was normalized for, derived from
    /// the `IndexedReady` snapshot's `macros[macro_index].kind` (NOT a
    /// caller-supplied hint). Part of the key so a kind mismatch occupies a
    /// distinct slot rather than reading / poisoning the sibling kind's entry.
    pub macro_kind: AnalyzedMacroKind,
    /// Query level — query identity, not an env hash.
    pub level_tag: u8,
}

impl VueMacroDtoKey {
    /// Construct a key for `macro_index` in `canonical` at content `whole_hash`,
    /// macro `macro_kind`, and query `level`.
    #[must_use]
    pub fn new(
        canonical: Arc<str>,
        whole_hash: Hash16,
        macro_index: usize,
        macro_kind: AnalyzedMacroKind,
        level: TypeInfoQueryLevel,
    ) -> Self {
        Self {
            canonical,
            whole_hash,
            macro_index,
            macro_kind,
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

/// A cached [`VueMacroDtos`] bundle plus the cross-file dependency facts the
/// resolution observed.
///
/// The DTO bundle is materialised by resolving the macro surface through the
/// shared typeinfo path, which reads CROSS-FILE carrier types (imported
/// interfaces / aliases). The SFC's own `whole_hash` keys the slot but does NOT
/// change when a CARRIER file is edited, so a content-addressed key alone would
/// serve stale DTOs after a dependency edit. This entry carries the
/// path-precise [`ReadSetSignature`] observed under an installed fact tracer
/// plus the project generation it was validated at; warm reads revalidate BOTH
/// gates against the live [`StoreView`] (the same rail
/// [`crate::component_meta_result_db::ComponentMetaResultDb`] uses) so a carrier
/// edit invalidates the entry lazily through recorded facts.
#[derive(Debug, Clone)]
pub struct VueMacroDtosEntry {
    /// The normalized DTO bundle (immutable, refcounted so warm reads hand out
    /// the SAME `Arc` rather than re-cloning).
    pub dtos: Arc<VueMacroDtos>,
    /// Path-precise fact signature observed while resolving the surface.
    pub read_set_signature: ReadSetSignature,
    /// Project generation the entry was validated at. A generation reset bumps
    /// no file content, so this gate rejects an entry whose facts still
    /// validate but whose project shape was reset.
    pub validated_at_generation: u64,
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
    entries: DashMap<VueMacroDtoKey, Arc<VueMacroDtosEntry>>,
}

impl VueShallowMetadataStore {
    /// Construct an empty store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            entries: DashMap::new(),
        }
    }

    /// Return the cached entry for `key` IFF it still validates under the live
    /// `view` and project `generation`.
    ///
    /// The content-addressed key (`whole_hash`) covers the SFC's own content;
    /// the [`ReadSetSignature`] gate covers CROSS-FILE carrier edits (an
    /// imported type the resolution read). Both must pass — a carrier edit that
    /// bumps a dependency fact, or a project-generation reset, invalidates the
    /// entry lazily without touching the SFC's own hash. An entry with an empty
    /// signature (a macro whose surface read no cross-file facts) trivially
    /// validates the fact rail.
    #[must_use]
    pub fn get_with_view<V: StoreView + ?Sized>(
        &self,
        key: &VueMacroDtoKey,
        view: &V,
        generation: u64,
    ) -> Option<Arc<VueMacroDtosEntry>> {
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
    /// the canonical `Arc` for the freshly-inserted value.
    ///
    /// The cold path that calls this runs ONLY after a `get_with_view` miss
    /// (the slot was empty, or its existing entry failed fact / generation
    /// validation). A carrier edit does NOT bump the project generation — it
    /// bumps a per-canonical fact — so a stale carrier-dep entry and its fresh
    /// replacement share the SAME `validated_at_generation`. A same-generation
    /// keep would therefore PIN the stale value (every read would reject it via
    /// the fact rail, recompute, then keep the stale entry on insert — and the
    /// caller would receive the kept STALE entry, not its fresh compute). So
    /// the insert unconditionally overwrites. Concurrent cold races compute the
    /// SAME fresh value against the SAME content + carrier facts, so last-writer
    /// -wins is value-equivalent to first-writer-wins.
    pub fn insert(&self, key: VueMacroDtoKey, entry: VueMacroDtosEntry) -> Arc<VueMacroDtosEntry> {
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
