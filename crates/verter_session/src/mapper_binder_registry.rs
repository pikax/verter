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
//! source mapper, lowered through TWO different dispatcher
//! instances, picks up DIFFERENT ordinals depending on whatever
//! other mappers preceded it. Two different ordinals → two
//! different `TypeParam` SemanticNodeIds → two different
//! `MapperKey` cache keys → the `SemanticQueryKey::MappedType`
//! cache MISSES on what should be a HIT.
//!
//! For ChatMessages.vue (empirical measurement) this
//! produces 258,546 ordinal collisions ≈ 258,611 cold MappedType
//! builds — a 258K-fold cross product over what should be ONE
//! computation per distinct mapper.
//!
//! # Solution
//!
//! Replace the per-dispatcher counter with a host-owned,
//! per-canonical registry keyed by a **mapper structural
//! fingerprint** (`u64` content-hash over the mapper's source /
//! value / name-type subtrees plus modifiers). Each call yields
//! the same ordinal for the same fingerprint within the same
//! canonical — across dispatcher instances, across requests,
//! across cache generations, AND across value-cloned bundles
//! whose `TypeExpr` subtrees share STRUCTURE but live in
//! distinct `Arc` allocations.
//!
//! # Stability contract
//!
//! 1. **Same canonical + structurally-equivalent mapper inputs →
//!    same ordinal.** The fingerprint is content-addressed — two
//!    mappers with identical `TypeExpr` structure produce the same
//!    `u64` fingerprint regardless of which `Arc` allocation
//!    carries them. This makes `TypeParam.param_index`
//!    deterministic for a given mapper SHAPE, which in turn makes
//!    the `parameter_node` SemanticNodeId, the `MapperKey`, and
//!    the `SemanticQueryKey::MappedType` cache key all stable
//!    across value-cloned bundles (R7 cross-owner reusable
//!    identity, R16 semantic fingerprint).
//!
//! 2. **Same canonical + structurally-distinct mappers → distinct
//!    ordinals.** Two different `[K in ...]` binders in the same
//!    file get different ordinals — preserving the original
//!    "distinct binders get distinct identity tuples" invariant.
//!
//! 3. **Different canonicals are independent.** A mapper in
//!    file A and a mapper in file B never share a registry
//!    slot — the canonical is part of the lookup key.
//!
//! 4. **Cross-generation reset.** The registry is cleared on
//!    [`Self::clear_for_canonical`] when a file's indexed-ready
//!    cache is evicted, so stale fingerprints from a prior
//!    content generation do not confuse the next one.
//!
//! # Fingerprint key — content-addressed, stack-safe
//!
//! The fingerprint is a `u64` produced by walking the mapper's
//! `source`, `value`, and optional `name_type` `TypeExpr` subtrees
//! with an iterative worklist (NOT recursion — R27 stack-safety).
//! Each node contributes its discriminator tag plus its leaf data
//! to the hash state. Two structurally-equivalent subtrees produce
//! the same `u64` regardless of `Arc` allocation identity, so
//! value-cloned `PreparedTypeDecl.body` bundles dedupe correctly.
//!
//! The walker is bounded only by the tree's structural size; it
//! never allocates per-node frames on the Rust call stack and so
//! tolerates the deeply-nested `Array<Array<...>>` chains that
//! real-world TS produces. The architectural rule: high budgets
//! are fine, but the stack itself must never be the budget.

use std::hash::{Hash, Hasher};
use std::sync::Arc;

use dashmap::DashMap;
use rustc_hash::{FxHashMap, FxHasher};
use verter_type_expr::{
    MappedModifier, ObjectMember, PrimitiveName, RecursiveConditionalBranch,
    SyntheticCarrierSurfaceKind, TypeExpr,
};

/// Structural fingerprint of a `TypeExpr::Mapped` mapper. The
/// `u64` is the content hash of the mapper's `source`, `value`,
/// optional `name_type`, and `(optional, readonly)` modifiers —
/// computed by a stack-safe worklist walker, so two
/// structurally-equivalent mappers produce the same fingerprint
/// regardless of which `Arc` allocation holds the subtree.
///
/// This intentionally REPLACES the prior pointer-identity
/// fingerprint (`Arc::as_ptr(source) as usize`), which was stable
/// only when the SAME `Arc` was reused. `PreparedTypeDecl.body:
/// TypeExpr` is value-cloned per bundle, so the pointer primitive
/// produced distinct fingerprints for structurally-identical
/// mappers from different load paths — empirically witnessed as
/// 258,505 `mapped_binder_ordinal_collision` ≈ 258,608
/// `mapped_type_cold` on ChatMessages.vue (99.96% of cold builds
/// were pointer-aliased duplicates of the same logical mapper).
///
/// (R16 semantic fingerprint, R27 stack-safe, R7
/// cross-owner reusable identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct MapperFingerprint(u64);

impl MapperFingerprint {
    /// Construct a content-addressed fingerprint from the
    /// source-side `TypeExpr::Mapped` components. Walks the
    /// `source`, `value`, and optional `name_type` `TypeExpr`
    /// subtrees structurally (iterative worklist, R27
    /// stack-safe) and folds the discriminator + leaf data of
    /// every visited node into the hash state. The resulting
    /// `u64` is identical for any two mappers whose structural
    /// content matches — regardless of `Arc` allocation identity.
    pub(crate) fn from_components(
        source: &Arc<TypeExpr>,
        value: &Arc<TypeExpr>,
        optional: MappedModifier,
        readonly: MappedModifier,
        name_type: Option<&Arc<TypeExpr>>,
    ) -> Self {
        let mut hasher = FxHasher::default();
        // Domain separator so a `MapperFingerprint` over (source,
        // value, ...) cannot accidentally collide with another
        // structural hash that hashes the same subtrees in the
        // same order.
        b"MapperFingerprint::v2".hash(&mut hasher);
        // Modifier bytes are leaf-cheap; hash them first so a
        // mapper that differs ONLY in `optional` / `readonly`
        // changes the hash even if the source/value/name_type
        // subtrees are identical.
        encode_modifier(optional).hash(&mut hasher);
        encode_modifier(readonly).hash(&mut hasher);
        // `name_type` presence is itself part of the structural
        // identity — `Some` vs `None` flips the discriminator.
        if let Some(nt) = name_type {
            1u8.hash(&mut hasher);
            hash_type_expr_structurally(nt, &mut hasher);
        } else {
            0u8.hash(&mut hasher);
        }
        // Component-separator tags between source / value so
        // permuting `(source, value)` cannot land on the same
        // fingerprint as the swapped pair.
        b"|source|".hash(&mut hasher);
        hash_type_expr_structurally(source, &mut hasher);
        b"|value|".hash(&mut hasher);
        hash_type_expr_structurally(value, &mut hasher);
        Self(hasher.finish())
    }

    /// Internal accessor for the raw `u64` content hash. Exposed
    /// at `pub(crate)` so the `test_only` module wrapper in
    /// `lib.rs` can surface it to integration tests without
    /// leaking the wrapper internals to production code. Not
    /// used on any production code path.
    #[doc(hidden)]
    pub(crate) fn raw(self) -> u64 {
        self.0
    }
}

fn encode_modifier(m: MappedModifier) -> u8 {
    match m {
        MappedModifier::None => 0,
        MappedModifier::Add => 1,
        MappedModifier::Remove => 2,
    }
}

/// Stack-safe structural hash of a `TypeExpr` subtree.
///
/// Walks the tree iteratively with a manually-managed worklist
/// (`Vec<&TypeExpr>`), folding each node's discriminator tag plus
/// its leaf data into `hasher`. Child subtrees are pushed onto
/// the worklist for later processing rather than recursing into
/// them on the Rust call stack — so the function tolerates trees
/// of arbitrary depth (deeply-nested `Array<Array<...>>` chains,
/// long `extends ? : extends ? : ...` chains, deeply-quasi'd
/// template literals, etc.) without ever risking stack overflow.
///
/// The hash is deterministic: a fixed visit order (each variant
/// arm hashes its leaf data, then pushes its children in a fixed
/// order onto the worklist) guarantees that two structurally-equal
/// `TypeExpr` trees produce the same `u64`.
fn hash_type_expr_structurally<H: Hasher>(root: &TypeExpr, hasher: &mut H) {
    // Worklist of references into the live `TypeExpr` graph.
    // We push every node's children here so the loop visits the
    // whole subtree without recursing. Borrow-checker note: all
    // refs are into `root`'s subtree which outlives the loop.
    let mut worklist: Vec<&TypeExpr> = Vec::with_capacity(16);
    worklist.push(root);

    while let Some(node) = worklist.pop() {
        // Discriminator tag — one fixed byte per variant. The tag
        // must change whenever a variant's shape changes; we use
        // explicit constants rather than `mem::discriminant` so
        // the wire format is stable across `TypeExpr` reorderings.
        match node {
            TypeExpr::Primitive(name) => {
                0u8.hash(hasher);
                encode_primitive(*name).hash(hasher);
            }
            TypeExpr::Literal(lit) => {
                1u8.hash(hasher);
                // `LiteralValue` already has a manual `Hash` impl
                // that handles the f64-NaN edge — reuse it.
                lit.hash(hasher);
            }
            TypeExpr::Union(items) => {
                2u8.hash(hasher);
                (items.len() as u64).hash(hasher);
                // Push in iteration order so the leaf nodes are
                // visited in reverse order — the visit order
                // itself is fixed for a given tree, which is all
                // we need for determinism.
                for item in items.iter() {
                    worklist.push(item);
                }
            }
            TypeExpr::Intersection(items) => {
                3u8.hash(hasher);
                (items.len() as u64).hash(hasher);
                for item in items.iter() {
                    worklist.push(item);
                }
            }
            TypeExpr::Array { element, readonly } => {
                4u8.hash(hasher);
                (*readonly as u8).hash(hasher);
                worklist.push(element);
            }
            TypeExpr::Tuple { elements, readonly } => {
                5u8.hash(hasher);
                (*readonly as u8).hash(hasher);
                (elements.len() as u64).hash(hasher);
                for el in elements.iter() {
                    // TupleElement carries (label, ty, optional, rest)
                    el.label.hash(hasher);
                    (el.optional as u8).hash(hasher);
                    (el.rest as u8).hash(hasher);
                    worklist.push(&el.ty);
                }
            }
            TypeExpr::Object(obj) => {
                6u8.hash(hasher);
                (obj.properties.len() as u64).hash(hasher);
                for member in obj.properties.iter() {
                    hash_object_member(member, hasher, &mut worklist);
                }
            }
            TypeExpr::Function(func) => {
                7u8.hash(hasher);
                hash_function_expr(func, hasher, &mut worklist);
            }
            // A constructor type carries the same `FunctionExpr` payload as a
            // function type but is a DISTINCT type, so it hashes with a distinct
            // discriminant (`22`, the next free tag — matching the frozen
            // `TypeExpr` discriminant scheme) before the shared function body, so
            // `new () => X` never collides with `() => X` in this hash.
            TypeExpr::ConstructorType(func) => {
                22u8.hash(hasher);
                hash_function_expr(func, hasher, &mut worklist);
            }
            TypeExpr::Ref {
                name,
                type_arguments,
            } => {
                8u8.hash(hasher);
                name.hash(hasher);
                (type_arguments.len() as u64).hash(hasher);
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
            }
            TypeExpr::TypeParameter(tp) => {
                9u8.hash(hasher);
                tp.name.hash(hasher);
                // TypeParam.constraint / default may carry
                // Arc<TypeExpr> children — visit them too.
                if let Some(c) = tp.constraint.as_ref() {
                    1u8.hash(hasher);
                    worklist.push(c);
                } else {
                    0u8.hash(hasher);
                }
                if let Some(d) = tp.default.as_ref() {
                    1u8.hash(hasher);
                    worklist.push(d);
                } else {
                    0u8.hash(hasher);
                }
            }
            TypeExpr::KeyOf(inner) => {
                10u8.hash(hasher);
                worklist.push(inner);
            }
            TypeExpr::TypeOf(value_ref) => {
                11u8.hash(hasher);
                // ValueRef derives Hash via the type itself.
                value_ref.hash(hasher);
            }
            TypeExpr::IndexedAccess { object, index } => {
                12u8.hash(hasher);
                worklist.push(object);
                worklist.push(index);
            }
            TypeExpr::Conditional {
                check,
                extends,
                true_type,
                false_type,
            } => {
                13u8.hash(hasher);
                worklist.push(check);
                worklist.push(extends);
                worklist.push(true_type);
                worklist.push(false_type);
            }
            TypeExpr::Mapped {
                parameter,
                source,
                value,
                optional,
                readonly,
                name_type,
            } => {
                14u8.hash(hasher);
                parameter.hash(hasher);
                encode_modifier(*optional).hash(hasher);
                encode_modifier(*readonly).hash(hasher);
                worklist.push(source);
                worklist.push(value);
                if let Some(nt) = name_type.as_ref() {
                    1u8.hash(hasher);
                    worklist.push(nt);
                } else {
                    0u8.hash(hasher);
                }
            }
            TypeExpr::TemplateLiteral {
                quasis,
                expressions,
            } => {
                15u8.hash(hasher);
                (quasis.len() as u64).hash(hasher);
                for q in quasis {
                    q.hash(hasher);
                }
                (expressions.len() as u64).hash(hasher);
                for e in expressions.iter() {
                    worklist.push(e);
                }
            }
            TypeExpr::Infer { name } => {
                16u8.hash(hasher);
                name.hash(hasher);
            }
            TypeExpr::Rest(inner) => {
                17u8.hash(hasher);
                worklist.push(inner);
            }
            TypeExpr::Parenthesized(inner) => {
                18u8.hash(hasher);
                worklist.push(inner);
            }
            TypeExpr::RecursiveRef {
                name,
                type_arguments,
                conditional_context,
            } => {
                19u8.hash(hasher);
                name.hash(hasher);
                (type_arguments.len() as u64).hash(hasher);
                for arg in type_arguments.iter() {
                    worklist.push(arg);
                }
                (conditional_context.len() as u64).hash(hasher);
                for frame in conditional_context.iter() {
                    hash_recursive_conditional_frame(frame, hasher, &mut worklist);
                }
            }
            TypeExpr::SyntheticSlotBinding(carrier) => {
                20u8.hash(hasher);
                carrier.scope_canonical_id.hash(hasher);
                encode_synthetic_surface(carrier.surface_kind).hash(hasher);
                carrier.slot_name.hash(hasher);
                carrier.binding_name.hash(hasher);
                carrier.value_node.hash(hasher);
            }
            TypeExpr::Unknown { raw } => {
                21u8.hash(hasher);
                raw.hash(hasher);
            }
        }
    }
}

fn hash_object_member<'a, H: Hasher>(
    member: &'a ObjectMember,
    hasher: &mut H,
    worklist: &mut Vec<&'a TypeExpr>,
) {
    match member {
        ObjectMember::Property(p) => {
            0u8.hash(hasher);
            p.name.hash(hasher);
            (p.optional as u8).hash(hasher);
            (p.readonly as u8).hash(hasher);
            hash_member_visibility(p.visibility, hasher);
            worklist.push(&p.ty);
        }
        ObjectMember::IndexSignature(s) => {
            1u8.hash(hasher);
            s.key_name.hash(hasher);
            (s.readonly as u8).hash(hasher);
            worklist.push(&s.key_type);
            worklist.push(&s.value_type);
        }
        ObjectMember::CallSignature(f) => {
            2u8.hash(hasher);
            hash_function_expr(f, hasher, worklist);
        }
        ObjectMember::ConstructSignature(f) => {
            3u8.hash(hasher);
            hash_function_expr(f, hasher, worklist);
        }
        ObjectMember::Method(m) => {
            4u8.hash(hasher);
            m.name.hash(hasher);
            (m.optional as u8).hash(hasher);
            hash_member_visibility(m.visibility, hasher);
            hash_function_expr(&m.function, hasher, worklist);
        }
    }
}

/// Fold a member-visibility marker into the mapper content-hash, emitting bytes
/// ONLY for a non-public member. A class's externally-visible member set
/// (`keyof T`) depends on accessibility, so a mapped source differing only in a
/// member's visibility is a genuinely different mapper and must not share an
/// ordinal. `Public` emits NOTHING, so an all-public mapper hash is unchanged
/// from before visibility existed (zero ordinal churn) — the SAME
/// marker-only-for-non-public scheme as the `TypeExpr` `Hash` + facts hasher.
fn hash_member_visibility<H: Hasher>(
    visibility: verter_type_expr::MemberVisibility,
    hasher: &mut H,
) {
    use verter_type_expr::MemberVisibility;
    match visibility {
        MemberVisibility::Public => {}
        MemberVisibility::Protected => {
            0x65u8.hash(hasher);
            1u8.hash(hasher);
        }
        MemberVisibility::Private => {
            0x65u8.hash(hasher);
            2u8.hash(hasher);
        }
    }
}

fn hash_function_expr<'a, H: Hasher>(
    func: &'a verter_type_expr::FunctionExpr,
    hasher: &mut H,
    worklist: &mut Vec<&'a TypeExpr>,
) {
    (func.parameters.len() as u64).hash(hasher);
    for p in func.parameters.iter() {
        p.name.hash(hasher);
        (p.optional as u8).hash(hasher);
        (p.rest as u8).hash(hasher);
        worklist.push(&p.ty);
    }
    (func.type_parameters.len() as u64).hash(hasher);
    for tp in func.type_parameters.iter() {
        tp.name.hash(hasher);
        if let Some(c) = tp.constraint.as_ref() {
            1u8.hash(hasher);
            worklist.push(c);
        } else {
            0u8.hash(hasher);
        }
        if let Some(d) = tp.default.as_ref() {
            1u8.hash(hasher);
            worklist.push(d);
        } else {
            0u8.hash(hasher);
        }
    }
    if let Some(ret) = func.return_type.as_ref() {
        1u8.hash(hasher);
        worklist.push(ret);
    } else {
        0u8.hash(hasher);
    }
}

fn hash_recursive_conditional_frame<'a, H: Hasher>(
    frame: &'a verter_type_expr::RecursiveConditionalFrame,
    hasher: &mut H,
    worklist: &mut Vec<&'a TypeExpr>,
) {
    // RecursiveConditionalFrame stores: { branch, decided, check,
    // extends }. Walk both subtrees and tag the branch + decided.
    encode_recursive_branch(frame.branch).hash(hasher);
    (frame.decided as u8).hash(hasher);
    worklist.push(&frame.check);
    worklist.push(&frame.extends);
}

fn encode_primitive(p: PrimitiveName) -> u8 {
    match p {
        PrimitiveName::String => 1,
        PrimitiveName::Number => 2,
        PrimitiveName::Boolean => 3,
        PrimitiveName::Symbol => 4,
        PrimitiveName::BigInt => 5,
        PrimitiveName::Any => 6,
        PrimitiveName::Unknown => 7,
        PrimitiveName::Void => 8,
        PrimitiveName::Never => 9,
        PrimitiveName::Null => 10,
        PrimitiveName::Undefined => 11,
        PrimitiveName::Object => 12,
    }
}

fn encode_synthetic_surface(s: SyntheticCarrierSurfaceKind) -> u8 {
    match s {
        SyntheticCarrierSurfaceKind::SlotBinding => 0,
        SyntheticCarrierSurfaceKind::Binding => 1,
    }
}

fn encode_recursive_branch(b: RecursiveConditionalBranch) -> u8 {
    match b {
        RecursiveConditionalBranch::True => 0,
        RecursiveConditionalBranch::False => 1,
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
    /// fingerprint keyspace. Called by the host on file content
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

    /// Distinct fingerprints — driven by STRUCTURALLY different
    /// `TypeExpr` content — must get distinct ordinals.
    ///
    /// Under the content-addressed fingerprint, two value-cloned
    /// subtrees with the SAME structure but distinct `Arc`
    /// allocations produce the SAME ordinal (covered by
    /// `fingerprint_content_addressed_across_value_cloned_arcs`
    /// in `tests/mapper_fingerprint_content_addressed.rs`).
    /// Distinct ordinals therefore require STRUCTURALLY
    /// distinct inputs — different source, different value,
    /// different modifiers, or different name-type rename.
    #[test]
    fn distinct_fingerprints_within_canonical_get_distinct_ordinals() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name: Arc<str> = Arc::from("K");
        // Two STRUCTURALLY-DISTINCT mappers: different value types.
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_a = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let value_b = Arc::new(TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Boolean,
        ));
        let fp_a = MapperFingerprint::from_components(
            &source,
            &value_a,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        let fp_b = MapperFingerprint::from_components(
            &source,
            &value_b,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        // Different value types → structurally different mappers
        // → different fingerprints.
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

    /// `clear_for_canonical` resets the slot so the next
    /// lowering starts at ordinal 0.
    ///
    /// Under the content-addressed fingerprint, two
    /// STRUCTURALLY-equivalent `(source, value, modifiers)`
    /// triples produce the SAME fingerprint regardless of which
    /// `Arc` allocation carries them. So this test asserts the
    /// reset behavior with a fresh STRUCTURALLY-distinct fp_b
    /// after the clear — any structurally-identical fp would
    /// still be assigned ordinal 0 (which is also what we want
    /// post-clear, since the slot is empty).
    #[test]
    fn clear_for_canonical_resets_the_slot() {
        let registry = MapperBinderRegistry::new();
        let canonical: Arc<str> = Arc::from("/file.ts");
        let name: Arc<str> = Arc::from("K");
        let source = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_a = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let value_b = Arc::new(TypeExpr::Primitive(
            verter_type_expr::PrimitiveName::Boolean,
        ));
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

    /// Discriminator: two FRESH `Arc<TypeExpr>` trees with
    /// identical mapped content produce the SAME fingerprint.
    /// This locks in the content-addressed primitive.
    #[test]
    fn fingerprint_content_addressed_across_fresh_arcs() {
        // Same structural content, distinct Arc allocations.
        let source_1 = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_1 = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        let source_2 = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::String));
        let value_2 = Arc::new(TypeExpr::Primitive(verter_type_expr::PrimitiveName::Number));
        assert_ne!(
            Arc::as_ptr(&source_1) as usize,
            Arc::as_ptr(&source_2) as usize,
            "test premise: the two Arcs must NOT share an allocation"
        );
        let fp_1 = MapperFingerprint::from_components(
            &source_1,
            &value_1,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        let fp_2 = MapperFingerprint::from_components(
            &source_2,
            &value_2,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        assert_eq!(
            fp_1, fp_2,
            "structurally-identical mappers must share a fingerprint regardless of Arc identity"
        );
    }

    /// A mapper `source` object differing ONLY in a member's visibility is a
    /// genuinely different mapper (a class's `keyof T` set depends on
    /// accessibility), so it must get a DISTINCT fingerprint — otherwise two
    /// structurally-different sources would collide on one binder ordinal. An
    /// all-public source's fingerprint is unchanged (marker-only-for-non-public).
    ///
    /// Discriminating: against the tree where `hash_object_member` omits
    /// visibility, the public / protected / private sources collide and the
    /// `assert_ne!`s FAIL.
    #[test]
    fn mapper_fingerprint_discriminates_member_visibility() {
        use verter_type_expr::{
            MemberSpans, MemberVisibility, ObjectExpr, ObjectMember, ObjectProperty, PrimitiveName,
        };

        let source_with = |vis: MemberVisibility| {
            Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
                properties: vec![ObjectMember::Property(ObjectProperty::with_visibility(
                    "x".to_string(),
                    TypeExpr::Primitive(PrimitiveName::Number),
                    false,
                    false,
                    vis,
                    MemberSpans::default(),
                ))],
            })))
        };
        let value = Arc::new(TypeExpr::Primitive(PrimitiveName::String));

        let fp = |vis| {
            MapperFingerprint::from_components(
                &source_with(vis),
                &value,
                MappedModifier::None,
                MappedModifier::None,
                None,
            )
        };

        let pub_fp = fp(MemberVisibility::Public);
        let prot_fp = fp(MemberVisibility::Protected);
        let priv_fp = fp(MemberVisibility::Private);

        assert_ne!(
            pub_fp, prot_fp,
            "a public vs protected member source must fingerprint distinctly",
        );
        assert_ne!(
            pub_fp, priv_fp,
            "a public vs private member source must fingerprint distinctly",
        );
        assert_ne!(
            prot_fp, priv_fp,
            "a protected vs private member source must fingerprint distinctly",
        );

        // An all-public source built via the explicit-Public constructor must
        // fingerprint identically to one built via `synthetic` (both Public) —
        // the marker is only-for-non-public.
        let via_synthetic = Arc::new(TypeExpr::Object(Arc::new(ObjectExpr {
            properties: vec![ObjectMember::Property(ObjectProperty::synthetic_public(
                "x".to_string(),
                TypeExpr::Primitive(PrimitiveName::Number),
                false,
                false,
            ))],
        })));
        let synthetic_fp = MapperFingerprint::from_components(
            &via_synthetic,
            &value,
            MappedModifier::None,
            MappedModifier::None,
            None,
        );
        assert_eq!(
            pub_fp, synthetic_fp,
            "an all-public source's fingerprint must not depend on how Public was constructed",
        );
    }
}
