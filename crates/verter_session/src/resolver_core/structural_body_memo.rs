//! Bundle-local context-keyed structural-body memo.
//!
//! The SAME syntactic object body, lowered under different
//! `(merge_role, macro-own-body/provenance)` contexts, yields DIFFERENT surface
//! members (different `declaration_origin` / `declared_in_macro_type_arg` /
//! `merge_role`). A context-NEUTRAL cached handle reused across those contexts
//! would therefore serve the WRONG members. This memo partitions the lowered
//! body handle by its lowering context: the key is
//! `(body_slot, provenance, merge_role)`, so the macro-own-body surface and the
//! plain structural surface of the SAME body never collide on one slot.
//!
//! The memo + its registry are a PRIVATE CHILD of one
//! [`PreparedDeclBundle`](crate::resolver_core::prepared_decl::PreparedDeclBundle):
//! one canonical file at one `owner_whole_hash`. That parent bundle is dropped
//! wholesale on any content / import change, so the version rooting lives on the
//! parent (the bundle's `owner_whole_hash`), NEVER per memo entry — the key
//! carries NO content hash, NO whole_hash, NO `SemanticNodeId`, NO `HotTypeRef`,
//! NO body fingerprint, NO symbol text (R6).
//!
//! Built BESIDE the resolution path with REAL map + registry logic, exercised by
//! the discriminating unit test below. It is NOT yet wired into dispatch and NOT
//! yet populated from the producer (that population while structural-lowering
//! real declarations is the later wiring step); hence the honest
//! `#[allow(dead_code)]` on the not-yet-read surface.
//!
//! ## R6 enforcement is structural / compiler-resolved, never a source scan
//!
//! Every keyable type here `#[derive(verter_no_typeexpr::NoTypeExpr)]`s and is
//! pinned by an [`assert_impl_all!`](static_assertions::assert_impl_all): a field
//! that owns a `TypeExpr` (or a raw keyable arena id) makes the bound
//! unsatisfiable, so the BUILD FAILS. But `NoTypeExpr` alone CANNOT reject raw
//! hash bytes — a `[u8; 16]` content hash is a valid `NoTypeExpr` scalar. The
//! residual is closed STRUCTURALLY by the exact-field-set destructure guard
//! ([`structural_body_memo_key_field_set_guard`]) and the exact unit-variant
//! guards over the two axis enums: adding a 4th key field (e.g. a
//! `owner_whole_hash: HashValue`) breaks the exhaustive destructure (a compile
//! error the `NoTypeExpr` assert would let pass), and adding a payload arm to an
//! axis enum (e.g. `MacroTypeArgOwnBody(HashValue)`) breaks its variant guard.

use std::sync::Arc;

use rustc_hash::FxHashMap;
use static_assertions::assert_impl_all;

use crate::semantic_query::{HotTypeRef, LocalScopeId, MemberMergeRole, SurfaceProvenanceContext};

// ---------------------------------------------------------------------------
// The dense bundle-local body slot id
// ---------------------------------------------------------------------------

/// A dense, bundle-local address for "which syntactic body" inside ONE
/// [`PreparedDeclBundle`](crate::resolver_core::prepared_decl::PreparedDeclBundle).
///
/// Minted ONLY by the bundle-owned [`StructuralBodyRegistry`] (no public
/// `from_raw`). Content-free: the self-describing data (symbol name, type/value
/// space, semantic-vs-lookup body, merged-contributor ordinal, intrinsic local
/// scope) lives in the registry [`StructuralBodyDescriptor`], never in this id
/// or in the [`StructuralBodyMemoKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub(crate) struct PreparedStructuralBodySlotId(u32);

#[cfg(test)]
impl PreparedStructuralBodySlotId {
    /// TEST-ONLY constructor for a raw slot index. The production surface mints
    /// ids ONLY through [`StructuralBodyRegistry::register`] (no public
    /// `from_raw`); this `#[cfg(test)]` helper exists solely so a test can build
    /// a known OUT-OF-RANGE index to assert [`StructuralBodyRegistry::descriptor`]
    /// returns `None` past the dense bound.
    pub(super) fn from_raw_for_test(raw: u32) -> Self {
        Self(raw)
    }
}

// ---------------------------------------------------------------------------
// The registry descriptor + the body-kind axis
// ---------------------------------------------------------------------------

/// The space (type vs value) a registered structural body lives in. CONTENT-FREE
/// — it names WHICH symbol table the body was registered from, never the body.
// The closed space taxonomy the future producer-wiring step stamps per body;
// `#[allow(dead_code)]` until that construction wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub(crate) enum StructuralBodySpace {
    /// A type-space declaration body (interface / type-alias / class type side).
    Type,
    /// A value-space declaration body (`const` / function / class value side).
    Value,
}

/// Which structural body of a registered symbol this descriptor names. A symbol
/// can register more than one body (a semantic body, a lookup body, or each
/// ordered contributor of a merged declaration), so the descriptor disambiguates
/// them. CONTENT-FREE — the ordinal selects WHICH contributor, never its members.
// The closed body-kind taxonomy the future producer-wiring step stamps per body;
// `#[allow(dead_code)]` until that construction wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub(crate) enum StructuralBodyKind {
    /// The symbol's primary semantic body (the lowered declaration surface).
    Semantic,
    /// The symbol's `typeof`-relevant lookup body (the value-projection side).
    Lookup,
    /// One ordered contributor of a same-name merged declaration, addressed by
    /// its source-order ordinal (`interface Foo {}` appearing N times merges N
    /// contributors; each is a distinct registered body).
    MergedContributor(u32),
}

/// Self-describing data for one registered syntactic body. The hot
/// [`StructuralBodyMemoKey`] does NOT carry this; it lives here, addressed by the
/// dense [`PreparedStructuralBodySlotId`]. The local scope (when an inner
/// lexical scope owns the body) is intrinsic to the DESCRIPTOR — never lifted
/// into the key (a `LocalScopeId` is the content-free scalar index; the key's
/// axes stay the provenance + merge-role contexts).
// Descriptor fields carry self-describing facts the future producer-wiring step
// records and later reads; `#[allow(dead_code)]` until that read wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(crate) struct StructuralBodyDescriptor {
    /// The declaring symbol's name (the same `Arc<str>` the shallow file state
    /// indexes it under).
    pub symbol_name: Arc<str>,
    /// Type-vs-value space the body was registered from.
    pub space: StructuralBodySpace,
    /// Which body of the symbol this is (semantic / lookup / merged-contributor).
    pub body_kind: StructuralBodyKind,
    /// The inner lexical scope owning the body, when it is not the file
    /// top-level scope (a namespace body, a block, a type-param scope). `None`
    /// for a top-level declaration. Descriptor-side, never key-side.
    pub local_scope: Option<LocalScopeId>,
}

// ---------------------------------------------------------------------------
// The bundle-owned structural-body registry (real mint logic)
// ---------------------------------------------------------------------------

/// The bundle-owned registry that mints dense [`PreparedStructuralBodySlotId`]s
/// and stores each id's [`StructuralBodyDescriptor`].
///
/// One registry per [`PreparedDeclBundle`]; the dense ids are addresses INTO
/// `descriptors` (the id's `u32` is the `Vec` index). The POPULATION FROM ACTUAL
/// DECL BODIES — calling [`register`](Self::register) while structural-lowering
/// real declarations — is the later producer-side wiring step; this type exposes
/// the real mint API the discriminating test exercises directly.
#[derive(Debug, Default)]
pub(crate) struct StructuralBodyRegistry {
    /// Descriptors indexed by the dense slot id's raw `u32` (the id IS the
    /// index). Append-only: a minted id never moves.
    descriptors: Vec<StructuralBodyDescriptor>,
}

#[allow(dead_code)]
impl StructuralBodyRegistry {
    /// An empty registry (no bodies registered yet).
    pub(crate) fn new() -> Self {
        Self {
            descriptors: Vec::new(),
        }
    }

    /// Register one structural body, allocating the next dense
    /// [`PreparedStructuralBodySlotId`] and storing its descriptor. The returned
    /// id is the index of the just-pushed descriptor — append-only, so two
    /// distinct registrations always mint two distinct ids.
    pub(crate) fn register(
        &mut self,
        descriptor: StructuralBodyDescriptor,
    ) -> PreparedStructuralBodySlotId {
        let raw = u32::try_from(self.descriptors.len())
            .expect("structural-body registry overflowed u32 slot space");
        self.descriptors.push(descriptor);
        PreparedStructuralBodySlotId(raw)
    }

    /// The descriptor registered under `id`, or `None` if `id` is out of range
    /// (never minted by this registry). Slot ids are bundle-local: a registry
    /// only resolves the ids it minted, and an id is never legitimately used
    /// against a different registry — a same-index id from another registry is a
    /// non-scenario, not a detected-and-rejected case, so this checks only the
    /// dense bound, not provenance.
    pub(crate) fn descriptor(
        &self,
        id: PreparedStructuralBodySlotId,
    ) -> Option<&StructuralBodyDescriptor> {
        self.descriptors.get(id.0 as usize)
    }

    /// The number of bodies registered so far.
    pub(crate) fn len(&self) -> usize {
        self.descriptors.len()
    }

    /// Whether no body has been registered yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.descriptors.is_empty()
    }
}

// ---------------------------------------------------------------------------
// The context-qualified memo key
// ---------------------------------------------------------------------------

/// The context-qualified structural-body memo key. Bundle-local, R6-clean:
/// carries NO content hash, NO whole_hash, NO `SemanticNodeId`, NO `HotTypeRef`,
/// NO `NodeScopeId`, NO body fingerprint, NO symbol text. Content-version rooting
/// comes from the parent bundle's `owner_whole_hash` (the memo is dropped
/// wholesale on content / import change), never per entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, verter_no_typeexpr::NoTypeExpr)]
pub(super) struct StructuralBodyMemoKey {
    /// Which syntactic body (the dense bundle-local slot id).
    body_slot: PreparedStructuralBodySlotId,
    /// The macro-type-argument own-body provenance context the body is lowered
    /// under (`Structural` vs `MacroTypeArgOwnBody`).
    provenance: SurfaceProvenanceContext,
    /// The surface-merge role the body is lowered under (`Authored` / `OwnBody`
    /// / `Heritage`).
    merge_role: MemberMergeRole,
}

#[allow(dead_code)]
impl StructuralBodyMemoKey {
    /// Construct a context-qualified key from its three axes.
    pub(super) fn new(
        body_slot: PreparedStructuralBodySlotId,
        provenance: SurfaceProvenanceContext,
        merge_role: MemberMergeRole,
    ) -> Self {
        Self {
            body_slot,
            provenance,
            merge_role,
        }
    }
}

// ---------------------------------------------------------------------------
// The memoized cell + the memo shell
// ---------------------------------------------------------------------------

/// A context-qualified structural body cell: the lowered body handle for ONE
/// `(body_slot, provenance, merge_role)` context. `HotTypeRef`-bearing, so it
/// satisfies `NoTypeExpr` (compiler-enforced) — the load-bearing rail that keeps
/// the memo free of any stored `TypeExpr`.
// The body handle is read by the future producer-wiring step (and the
// discriminating test); `#[allow(dead_code)]` until that read wiring lands.
#[allow(dead_code)]
#[derive(Debug, Clone, verter_no_typeexpr::NoTypeExpr)]
pub(crate) struct HotStructuralBodyCell {
    /// The lowered body handle for this context (the interned graph node the
    /// body lowers to under its `(provenance, merge_role)` context).
    pub body: HotTypeRef,
}

#[allow(dead_code)]
impl HotStructuralBodyCell {
    /// A cell holding one lowered body handle.
    pub(crate) fn new(body: HotTypeRef) -> Self {
        Self { body }
    }
}

/// The bundle-local context-keyed structural-body memo: a map from the context
/// key to the memoized cell. NOT populated from the producer (the later wiring
/// step does that); built BESIDE the resolution path. The map lives behind the
/// owning bundle's `Arc` and is immutable-after-build, so a plain `FxHashMap`
/// is the right shape (no shared interior mutability needed dead-code-until-wired).
#[derive(Debug, Default, verter_no_typeexpr::NoTypeExpr)]
pub(crate) struct StructuralBodyMemo {
    /// The context key → memoized cell map. `Arc<HotStructuralBodyCell>` so a
    /// broader read can hand the cell out by clone without copying the handle.
    cells: FxHashMap<StructuralBodyMemoKey, Arc<HotStructuralBodyCell>>,
}

#[allow(dead_code)]
impl StructuralBodyMemo {
    /// An empty memo.
    pub(crate) fn new() -> Self {
        Self {
            cells: FxHashMap::default(),
        }
    }

    /// Insert the cell for `key`, returning the previous cell if one was already
    /// memoized for that exact context. `pub(super)` to match the
    /// `pub(super)` key it accepts (the key is bundle-internal).
    pub(super) fn insert(
        &mut self,
        key: StructuralBodyMemoKey,
        cell: Arc<HotStructuralBodyCell>,
    ) -> Option<Arc<HotStructuralBodyCell>> {
        self.cells.insert(key, cell)
    }

    /// The memoized cell for `key`, or `None` if no cell was inserted for that
    /// exact context. `pub(super)` to match the `pub(super)` key it accepts.
    pub(super) fn get(&self, key: &StructuralBodyMemoKey) -> Option<Arc<HotStructuralBodyCell>> {
        self.cells.get(key).map(Arc::clone)
    }

    /// The number of memoized cells.
    pub(crate) fn len(&self) -> usize {
        self.cells.len()
    }

    /// Whether no cell is memoized yet.
    pub(crate) fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }
}

/// The bundle-local structural-body cache: the slot [`StructuralBodyRegistry`]
/// paired with the context-keyed [`StructuralBodyMemo`]. One per
/// [`PreparedDeclBundle`]; built empty BESIDE the resolution path and dropped
/// wholesale with its parent bundle on content / import change.
// The registry + memo are read by the future producer-wiring step (and the
// discriminating test); `#[allow(dead_code)]` until that read wiring lands.
#[allow(dead_code)]
#[derive(Debug, Default)]
pub(crate) struct PreparedStructuralBodyCache {
    /// The slot registry minting [`PreparedStructuralBodySlotId`]s.
    pub registry: StructuralBodyRegistry,
    /// The context-keyed memo of lowered body cells.
    pub memo: StructuralBodyMemo,
}

#[allow(dead_code)]
impl PreparedStructuralBodyCache {
    /// An empty cache (no bodies registered, no cells memoized).
    pub(crate) fn new() -> Self {
        Self {
            registry: StructuralBodyRegistry::new(),
            memo: StructuralBodyMemo::new(),
        }
    }
}

// ===========================================================================
// R6-clean ENFORCEMENT — structural / compiler guards ONLY (no source scanner).
// ===========================================================================

// The compiler-resolved no-`TypeExpr` / non-keyable-arena-id rail. A field that
// owns a `TypeExpr` (or a raw keyable arena id like `SemanticNodeId`) makes the
// `NoTypeExpr` bound unsatisfiable, so the build FAILS HERE. `Eq`/`Hash`/`Send`/
// `Sync` are required because the key keys a host-owned shared map.
assert_impl_all!(
    StructuralBodyMemoKey: Eq,
    std::hash::Hash,
    verter_no_typeexpr::NoTypeExpr,
    Send,
    Sync
);
assert_impl_all!(
    PreparedStructuralBodySlotId: Copy,
    Eq,
    std::hash::Hash,
    verter_no_typeexpr::NoTypeExpr,
    Send,
    Sync
);
// The cell is `HotTypeRef`-bearing; its `NoTypeExpr` is the rail that keeps the
// memo free of any stored `TypeExpr`. The memo itself derives the marker too.
assert_impl_all!(HotStructuralBodyCell: verter_no_typeexpr::NoTypeExpr, Send, Sync);
assert_impl_all!(StructuralBodyMemo: verter_no_typeexpr::NoTypeExpr, Send, Sync);

// Exact field-set guard: adding a 4th field (e.g. a content/version hash like
// `owner_whole_hash: HashValue`) breaks this exhaustive destructure (a COMPILE
// error), and retyping any field breaks the per-field ascription. This closes
// the `[u8; 16]`-content-hash residual STRUCTURALLY — `NoTypeExpr` alone cannot
// reject raw hash bytes (a `[u8; 16]` is a valid `NoTypeExpr` scalar), so it
// would let a content-hash field through; this guard does not.
#[allow(dead_code)]
fn structural_body_memo_key_field_set_guard(key: StructuralBodyMemoKey) {
    let StructuralBodyMemoKey {
        body_slot,
        provenance,
        merge_role,
    } = key;
    let _: PreparedStructuralBodySlotId = body_slot;
    let _: SurfaceProvenanceContext = provenance;
    let _: MemberMergeRole = merge_role;
}

// Exact shape guard for the dense slot id: it stays a single `u32` newtype. A
// content/version field (or any retype) breaks the single-field destructure.
#[allow(dead_code)]
fn prepared_structural_body_slot_id_shape_guard(slot: PreparedStructuralBodySlotId) {
    let PreparedStructuralBodySlotId(raw) = slot;
    let _: u32 = raw;
}

// Exact UNIT-VARIANT guards for the two axis enums: a future payload arm (e.g. a
// `MacroTypeArgOwnBody(HashValue)` that smuggles a hash in while keeping the
// key's field TYPE unchanged) or a brand-new variant fails to compile here.
#[allow(dead_code)]
fn surface_provenance_context_variant_shape_guard(p: SurfaceProvenanceContext) {
    use crate::semantic_query::SurfaceProvenanceContext::*;
    match p {
        Structural => {}
        MacroTypeArgOwnBody => {}
    }
}

#[allow(dead_code)]
fn member_merge_role_variant_shape_guard(m: MemberMergeRole) {
    use crate::semantic_query::MemberMergeRole::*;
    match m {
        Authored => {}
        OwnBody => {}
        Heritage => {}
    }
}

#[cfg(test)]
#[path = "structural_body_memo_tests.rs"]
mod structural_body_memo_tests;
