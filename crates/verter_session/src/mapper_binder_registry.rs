//! Host-owned registry for stable mapped-binder ordinal assignment.
//!
//! # Problem
//!
//! `[K in source]` mapped-type binders are lowered through
//! [`crate::project_semantic_dispatch::lower`]'s `TypeExpr::Mapped`
//! arm. Each lowering interns a `SemanticNodeData::TypeParam {
//! decl, param_index, ..., display_name }`. The arena dedupes by
//! `(SemanticNodeData, NodeScopeId)` — so equivalent mappers
//! WOULD share a `SemanticNodeId` if their `param_index` values
//! agree.
//!
//! The legacy ordinal allocator was a per-dispatcher counter
//! (`ProjectSemanticDispatch::next_mapped_binder_ordinal` —
//! deleted as part of this fix). Per-dispatcher meant: the SAME
//! source mapper, lowered through
//! TWO different dispatcher instances, picks up DIFFERENT ordinals
//! depending on whatever other mappers preceded it. Two different
//! ordinals → two different `TypeParam` SemanticNodeIds → two
//! different `MapperKey` cache keys → the
//! `SemanticQueryKey::MappedType` cache MISSES on what should
//! be a HIT.
//!
//! For ChatMessages.vue (Phase G empirical measurement) this
//! produces 258,546 ordinal collisions ≈ 258,611 cold MappedType
//! builds — a 258K-fold cross product over what should be ONE
//! computation per distinct mapper.
//!
//! # Solution
//!
//! Replace the per-dispatcher counter with a host-owned,
//! per-canonical registry keyed by a **mapper structural
//! fingerprint** (`(source_ptr, value_ptr, optional, readonly,
//! name_type_ptr)`). Each call yields the same ordinal for the
//! same fingerprint within the same canonical — across dispatcher
//! instances, across requests, across cache generations.
//!
//! # Stability contract
//!
//! 1. **Same canonical + same fingerprint → same ordinal.** This
//!    makes `TypeParam.param_index` deterministic for a given
//!    mapper, which in turn makes the `parameter_node`
//!    SemanticNodeId, the `MapperKey`, and the
//!    `SemanticQueryKey::MappedType` cache key all stable.
//!
//! 2. **Same canonical + distinct fingerprints → distinct
//!    ordinals.** Two different `[K in ...]` binders in the
//!    same file get different ordinals — preserving the original
//!    "distinct binders get distinct identity tuples" invariant.
//!
//! 3. **Different canonicals are independent.** A mapper in
//!    file A and a mapper in file B never share a registry
//!    slot — the canonical is part of the lookup key.
//!
//! 4. **Cross-generation reset.** The registry is cleared on
//!    [`Self::clear_for_canonical`] when a file's indexed-ready
//!    cache is evicted, so stale `Arc::as_ptr` keys do not
//!    confuse the next generation's fingerprints.
//!
//! # Fingerprint key
//!
//! The fingerprint is the **structural identity** of the source
//! `TypeExpr::Mapped`: the source / value / name-type Arc
//! pointers plus the optional/readonly modifiers. Within one
//! parse generation, two lowerings of the SAME mapper share the
//! SAME Arc pointers — so `Arc::as_ptr` is a stable, cheap
//! discriminator. After invalidation, the pointers are freed +
//! reassigned; the per-canonical clear() resets the slot.

use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::FxHashMap;
use verter_type_expr::{MappedModifier, TypeExpr};

/// Structural fingerprint of a `TypeExpr::Mapped` mapper. Two
/// fingerprints are equal iff their source / value / name-type
/// Arc pointers match AND the optional / readonly modifiers
/// match.
///
/// `Arc::as_ptr` returns a raw pointer that is stable per Arc
/// allocation. The pointer is NOT dereferenced — it is only
/// hashed + compared, so freed Arcs that happen to collide on
/// the same address remain memory-safe (we never deref). The
/// per-canonical [`MapperBinderRegistry::clear_for_canonical`]
/// path resets the registry along with file invalidation, so a
/// pointer reuse cannot cross a content edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MapperFingerprint {
    source_ptr: usize,
    value_ptr: usize,
    name_type_ptr: usize,
    optional: u8,
    readonly: u8,
}

impl MapperFingerprint {
    /// Construct a fingerprint from the source-side `TypeExpr::Mapped`
    /// component Arcs. The caller passes the already-resolved Arcs
    /// (the same ones the lowering will recurse into) so we capture
    /// pointer identity at the SAME granularity the parser/AST cache
    /// preserves.
    pub(crate) fn from_components(
        source: &Arc<TypeExpr>,
        value: &Arc<TypeExpr>,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<&Arc<TypeExpr>>,
    ) -> Self {
        Self {
            source_ptr: Arc::as_ptr(source) as usize,
            value_ptr: Arc::as_ptr(value) as usize,
            name_type_ptr: name_type.map(|nt| Arc::as_ptr(nt) as usize).unwrap_or(0),
            optional: encode_modifier(optional),
            readonly: encode_modifier(readonly),
        }
    }
}

fn encode_modifier(m: MappedModifier) -> u8 {
    match m {
        MappedModifier::None => 0,
        MappedModifier::Add => 1,
        MappedModifier::Remove => 2,
    }
}

/// Host-owned per-canonical mapper-binder registry. The registry
/// hands out STABLE `param_index` ordinals for each
/// `(canonical, display_name, fingerprint)` triple so two
/// lowerings of the SAME source mapper get the SAME ordinal —
/// and therefore the same `TypeParam` SemanticNodeId, the same
/// `MapperKey`, and the same `MappedType` cache key.
///
/// # Storage layout
///
/// `DashMap<Arc<str>, parking_lot::Mutex<PerCanonicalSlot>>`
///
/// - The outer key is the canonical file id (the same string
///   the rest of the host uses to identify files).
/// - The inner slot is small (typically 1-50 mappers per file)
///   so a `Mutex<...>` is sufficient — DashMap shards already
///   give per-canonical parallelism, and within one canonical
///   the linear search through ~50 entries is cheap.
#[derive(Debug, Default)]
pub(crate) struct MapperBinderRegistry {
    per_canonical: DashMap<Arc<str>, parking_lot::Mutex<PerCanonicalSlot>>,
}

/// Per-canonical fingerprint → ordinal table. Distinct mappers
/// within the same canonical get distinct ordinals; the same
/// mapper gets the same ordinal across multiple lowerings.
///
/// The table is per `display_name` because two different mapper
/// names (`[K in ...]` vs `[P in ...]`) intern as different
/// `TypeParam` payloads regardless of `param_index` (the
/// `display_name` field is part of `SemanticNodeData::TypeParam`),
/// so they need their own ordinal sequences.
#[derive(Debug, Default)]
pub(crate) struct PerCanonicalSlot {
    /// `display_name → Vec<MapperFingerprint>` where the
    /// fingerprint's index in the vec is its `param_index`.
    by_display_name: FxHashMap<Arc<str>, Vec<MapperFingerprint>>,
}

impl MapperBinderRegistry {
    /// Construct an empty registry.
    #[must_use]
    pub(crate) fn new() -> Self {
        Self {
            per_canonical: DashMap::new(),
        }
    }

    /// Get or assign a stable `param_index` ordinal for the
    /// given `(canonical, display_name, fingerprint)` triple.
    ///
    /// Lookup is O(n) in the number of distinct mappers with the
    /// same `display_name` within the same canonical — typically
    /// 1-3 entries, so the linear scan is cheap on the hot path.
    /// Within a canonical the slot is guarded by a
    /// `parking_lot::Mutex`; across canonicals the DashMap shards
    /// give parallel access.
    pub(crate) fn ordinal_for(
        &self,
        canonical_id: &Arc<str>,
        display_name: &Arc<str>,
        fingerprint: MapperFingerprint,
    ) -> u16 {
        let slot = self
            .per_canonical
            .entry(Arc::clone(canonical_id))
            .or_default();
        let mut slot = slot.lock();
        let entries = slot
            .by_display_name
            .entry(Arc::clone(display_name))
            .or_default();
        // Linear search for an existing match. Per-canonical
        // tables are small (1-50 mappers / display name) so the
        // scan stays cache-warm.
        if let Some((idx, _)) = entries
            .iter()
            .enumerate()
            .find(|(_, fp)| **fp == fingerprint)
        {
            return idx as u16;
        }
        let new_idx = entries.len();
        entries.push(fingerprint);
        new_idx as u16
    }

    /// Drop the per-canonical entry for `canonical_id` so the next
    /// lowering of any mapper in this file starts with a fresh
    /// `Arc::as_ptr` keyspace. Called by the host on file content
    /// invalidation alongside the indexed-ready cache eviction.
    pub(crate) fn clear_for_canonical(&self, canonical_id: &str) {
        self.per_canonical.remove(canonical_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_fingerprint_returns_same_ordinal() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name: Arc<str> = Arc::from("K");
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let fp = MapperFingerprint::from_components(
            &source,
            &value,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        let a = registry.ordinal_for(&canonical, &name, fp);
        let b = registry.ordinal_for(&canonical, &name, fp);
        assert_eq!(
            a, b,
            "identical fingerprints must collide on the same ordinal"
        );
    }

    #[test]
    fn distinct_fingerprints_within_canonical_get_distinct_ordinals() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name: Arc<str> = Arc::from("K");
        let source_a = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_a = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let source_b = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_b = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let fp_a = MapperFingerprint::from_components(
            &source_a,
            &value_a,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        let fp_b = MapperFingerprint::from_components(
            &source_b,
            &value_b,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        // Different Arc allocations → different pointers → different fps.
        assert_ne!(fp_a, fp_b);
        let a = registry.ordinal_for(&canonical, &name, fp_a);
        let b = registry.ordinal_for(&canonical, &name, fp_b);
        assert_ne!(a, b, "distinct fingerprints must get distinct ordinals");
    }

    #[test]
    fn different_display_names_have_independent_ordinal_sequences() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name_k: Arc<str> = Arc::from("K");
        let name_p: Arc<str> = Arc::from("P");
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let fp = MapperFingerprint::from_components(
            &source,
            &value,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        // First mapper named K → ordinal 0 in the K sequence.
        let k_ord = registry.ordinal_for(&canonical, &name_k, fp);
        // First mapper named P (different display name) → ordinal 0
        // in the INDEPENDENT P sequence. The collision is fine
        // because the `display_name` field on `TypeParam`
        // disambiguates the interned node regardless.
        let p_ord = registry.ordinal_for(&canonical, &name_p, fp);
        assert_eq!(k_ord, 0);
        assert_eq!(p_ord, 0);
    }

    #[test]
    fn distinct_canonicals_are_independent() {
        let registry = MapperBinderRegistry::new();
        let canonical_a: Arc<str> = Arc::from("/a.ts");
        let canonical_b: Arc<str> = Arc::from("/b.ts");
        let name: Arc<str> = Arc::from("K");
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let fp = MapperFingerprint::from_components(
            &source,
            &value,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        let a = registry.ordinal_for(&canonical_a, &name, fp);
        let b = registry.ordinal_for(&canonical_b, &name, fp);
        // Both files independently allocate ordinal 0 for their
        // first K mapper. The canonical_id is part of the
        // `TypeParam.decl` discriminator, so the SemanticNodeIds
        // remain distinct via the decl rather than the
        // param_index.
        assert_eq!(a, 0);
        assert_eq!(b, 0);
    }

    #[test]
    fn clear_for_canonical_resets_the_slot() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name: Arc<str> = Arc::from("K");
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_a = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let value_b = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let fp_a = MapperFingerprint::from_components(
            &source,
            &value_a,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        // First fp → ordinal 0.
        assert_eq!(registry.ordinal_for(&canonical, &name, fp_a), 0);
        registry.clear_for_canonical(&canonical);
        // After clear: the next fp also gets 0 (the slot is
        // empty), not 1 — independent of what came before.
        let fp_b = MapperFingerprint::from_components(
            &source,
            &value_b,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        assert_eq!(registry.ordinal_for(&canonical, &name, fp_b), 0);
    }
}
