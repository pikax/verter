//! Family memo — mode-erased keys + per-mode slots.
//!
//! `FamilyKey` is the mode-erased identity for one [`SemanticQueryKey`]
//! family; per-mode results live in distinct slots inside `FamilySlots`.
//! `family_and_slot` projects a key onto its `(family, slot)` pair, and
//! `backfill_targets` describes the slot-fan-out used by the
//! broader-satisfies-narrower backfill rule.

use std::sync::Arc;

use crate::fact_signature_helpers::ReadSetSignature;
use crate::semantic_query::{
    DeclIdentity, DepSignature, HostResolvedNamedTypeKey, IndexKey, MapperKey, PathSegment,
    ProjectionMode, ProjectionReductionContext, QueryResult, ReductionDemand, ResolveDeclKey,
    SemanticNodeId, SemanticQueryKey, ValueRootKey,
};

#[derive(Clone)]
pub(super) struct MemoEntry {
    pub(super) result: QueryResult<SemanticNodeId>,
    /// Carrier holding the path-precise R28 fact signature for this
    /// entry — the sole cache-validity rail. Warm-hit reads validate
    /// the carrier against the live store view — validating every
    /// [`Self::self_root_canonicals`] entry's `FileWholeHash`
    /// *strictly* — BEFORE bubbling the carrier's observations (see
    /// [`MemoEntry::validate`]). The reverse index registers under
    /// every canonical yielded by `read_set_signature.canonical_ids()`
    /// so `invalidate_canonical` drains every fact dependency.
    pub(super) read_set_signature: ReadSetSignature,
    /// The cold build's dispatch-return signature — the
    /// `QueryBuildOutput.dep_signature` the build produced. NOT a
    /// cache-validity rail (the carrier above is the sole validity
    /// oracle); it is the transitive-dependency payload a warm hit
    /// returns on `CacheRead.dep_signature` so the component-meta
    /// dispatch accumulator folds the warm sub-query's deps into the
    /// owner's `fact_versions`. Validity is decided exclusively by
    /// `read_set_signature` / `self_root_canonicals`.
    pub(super) dispatch_dep_signature: DepSignature,
    /// The entry's **self-root canonicals** — the keyed canonical(s) the
    /// cold build's value depends on for its own identity (its keyed
    /// canonical for `ResolveDecl` / `TypeOf` / `Instantiate` /
    /// `ResolveMacroPayload`, or the file-derived origin of every input
    /// node for the node kinds keyed by interned `SemanticNodeId`s).
    ///
    /// A warm read validates each listed canonical's self-root
    /// `FileWholeHash` *strictly* via
    /// [`crate::fact_signature_helpers::validate_fact_signature_with_self_roots`]:
    /// a same-canonical content edit — or a self-root canonical the live
    /// store view no longer tracks — rejects the entry. The
    /// `FileWholeHash` fact for any *non-listed* canonical (a cross-file
    /// dependency loaded after the view snapshot) keeps the lazy
    /// `validates` "untracked → optimistically accept" rule.
    ///
    /// Empty for entries published outside an observable cold-compute
    /// pass (synthetic / test fixtures); validation then degrades to the
    /// plain `read_set_signature.validate(ctx)` rails with no strict
    /// self-root check.
    pub(super) self_root_canonicals: Arc<[Arc<str>]>,
    /// Walker diagnostics observed during the cold build that produced
    /// this entry. Replayed on warm hits via `CacheRead.walker_diagnostics`.
    /// Empty for non-walker queries.
    pub(super) walker_diagnostics: Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
}

impl MemoEntry {
    /// Validate the entry's carrier against the live store view,
    /// validating every [`Self::self_root_canonicals`] entry's
    /// self-root `FileWholeHash` *strictly*.
    ///
    /// Returns `true` only when the path-precise fact rail validates
    /// (self-roots strict, cross-file dependency facts lazy). An
    /// overflow carrier always fails (it must never warm-hit). An
    /// empty carrier with no self-roots validates vacuously.
    ///
    /// This is the strict warm-read validation entry point: a
    /// same-canonical content edit on any self-root canonical, or a
    /// self-root canonical the live store view no longer tracks, fails
    /// validation and the warm read recomputes.
    pub(super) fn validate(&self, ctx: &dyn crate::resolver_core::ResolverContext) -> bool {
        self.read_set_signature
            .validate_with_self_roots(ctx, &self.self_root_canonicals)
    }
}

/// Mode-erased identity for one [`SemanticQueryKey`] family.
///
/// Two semantic queries that mean the same thing apart from `mode` produce
/// the same [`FamilyKey`]; their per-mode results live in distinct slots
/// inside [`FamilySlots`]. Variants without a `mode` field (everything
/// except [`SemanticQueryKey::ProjectMember`] /
/// [`SemanticQueryKey::IndexedAccess`] / [`SemanticQueryKey::ProjectPath`])
/// use only the `single` slot, exactly mirroring the pre-B1b behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum FamilyKey {
    ResolveDecl(ResolveDeclKey),
    Instantiate {
        base: DeclIdentity,
        args: Arc<[SemanticNodeId]>,
        /// Surface-provenance dimension (codex BINDING design). A
        /// macro-type-argument own-body instantiation and a plain
        /// structural instantiation of the SAME decl + args produce
        /// distinct surfaces (`declared_in_macro_type_arg` differs), so
        /// they MUST NOT collide on one family slot. The mode still maps
        /// to `ModeSlot`; this keeps the provenance variants apart at
        /// the family-identity level so per-mode backfill stays within
        /// one provenance family.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
    },
    ProjectMember {
        base: SemanticNodeId,
        member: Arc<str>,
    },
    IndexedAccess {
        base: SemanticNodeId,
        index: IndexKey,
    },
    KeyOf {
        base: SemanticNodeId,
    },
    MappedType {
        source: SemanticNodeId,
        mapper: MapperKey,
    },
    Conditional {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    },
    TypeOf {
        value_root: ValueRootKey,
    },
    NormalizeUnion {
        members: Arc<[SemanticNodeId]>,
    },
    NormalizeIntersection {
        members: Arc<[SemanticNodeId]>,
    },
    ProjectPath {
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        /// Surface-provenance dimension (codex BINDING design). A
        /// macro-type-argument own-body path projection (the empty-path
        /// macro-payload surface read) and a plain structural path
        /// projection of the SAME `(base, path)` produce distinct
        /// surfaces (`declared_in_macro_type_arg` differs on the
        /// expanded DeclPlaceholder's own-body members), so they MUST
        /// NOT collide on one family slot.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
    },
    /// Included for completeness so `family_and_slot` is total, but
    /// [`SemanticQueryKey::ResolvedNamedType`] bypasses the family memo at
    /// admission and never lands in the warm map.
    ResolvedNamedType {
        key: Arc<HostResolvedNamedTypeKey>,
    },
    /// Binding amendment — `ResolveMacroPayload`. Mode-erased
    /// for the family memo; the per-mode result lives in the matching
    /// `FamilySlots` slot.
    ResolveMacroPayload {
        owner: DeclIdentity,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: Arc<[SemanticNodeId]>,
    },
}

/// Per-family slot selector. For non-mode variants only `Single` is used;
/// for mode-bearing variants one of `Identity` / `Navigate` / `Shallow` /
/// `Expanded` is selected from the key's `ProjectionMode`.
///
/// Codex-hybrid spec: the `Instantiate` / `KeyOf` /
/// `MappedType` families carry a [`ProjectionReductionContext`] in
/// their key, not just a `ProjectionMode`. Their slots are picked from
/// the `TransitShallow` / `TransitNavigate` / `TransitIdentity` /
/// `TransitExpanded` set whenever the context's `demand` is
/// `StructuralTransit`, keeping transit results from colliding with
/// `Published` results on the same node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ModeSlot {
    Single,
    Identity,
    Navigate,
    Shallow,
    Expanded,
    /// Skeleton mode. Distinct semantics from
    /// Identity/Navigate/Shallow/Expanded (preserves open generics as
    /// TypeParam shells); does NOT backfill or get backfilled by other
    /// modes.
    Skeleton,
    /// `StructuralTransit` variants of the four publication modes —
    /// Codex-hybrid spec. Distinct from the
    /// publication slots; do NOT backfill the publication slots and
    /// are not backfilled by them.
    TransitIdentity,
    TransitNavigate,
    TransitShallow,
    TransitExpanded,
}

/// Per-family per-slot warm storage. Each slot independently holds an
/// optional [`MemoEntry`]. Backfill on completion fills empty narrower
/// slots from a successful broader compute (see [`FamilySlots::publish`]).
#[derive(Default, Clone)]
pub(super) struct FamilySlots {
    single: Option<MemoEntry>,
    identity: Option<MemoEntry>,
    navigate: Option<MemoEntry>,
    shallow: Option<MemoEntry>,
    expanded: Option<MemoEntry>,
    /// Skeleton mode slot. Independent from
    /// Navigate/Expanded; does NOT participate in backfill.
    skeleton: Option<MemoEntry>,
    /// `StructuralTransit` slot mirrors of the four publication slots —
    /// Codex-hybrid spec. Independent from the
    /// publication slots; backfill within the transit family follows
    /// the same `Expanded → Shallow → Navigate → Identity` hierarchy.
    transit_identity: Option<MemoEntry>,
    transit_navigate: Option<MemoEntry>,
    transit_shallow: Option<MemoEntry>,
    transit_expanded: Option<MemoEntry>,
}

impl FamilySlots {
    pub(super) fn slot(&self, slot: ModeSlot) -> Option<&MemoEntry> {
        match slot {
            ModeSlot::Single => self.single.as_ref(),
            ModeSlot::Identity => self.identity.as_ref(),
            ModeSlot::Navigate => self.navigate.as_ref(),
            ModeSlot::Shallow => self.shallow.as_ref(),
            ModeSlot::Expanded => self.expanded.as_ref(),
            ModeSlot::Skeleton => self.skeleton.as_ref(),
            ModeSlot::TransitIdentity => self.transit_identity.as_ref(),
            ModeSlot::TransitNavigate => self.transit_navigate.as_ref(),
            ModeSlot::TransitShallow => self.transit_shallow.as_ref(),
            ModeSlot::TransitExpanded => self.transit_expanded.as_ref(),
        }
    }

    pub(super) fn slot_mut(&mut self, slot: ModeSlot) -> &mut Option<MemoEntry> {
        match slot {
            ModeSlot::Single => &mut self.single,
            ModeSlot::Identity => &mut self.identity,
            ModeSlot::Navigate => &mut self.navigate,
            ModeSlot::Shallow => &mut self.shallow,
            ModeSlot::Expanded => &mut self.expanded,
            ModeSlot::Skeleton => &mut self.skeleton,
            ModeSlot::TransitIdentity => &mut self.transit_identity,
            ModeSlot::TransitNavigate => &mut self.transit_navigate,
            ModeSlot::TransitShallow => &mut self.transit_shallow,
            ModeSlot::TransitExpanded => &mut self.transit_expanded,
        }
    }

    /// Publish `entry` to `slot` and backfill every narrower slot whose
    /// cell is empty. The narrower slots store the same `Arc`-shared
    /// [`MemoEntry`] (same result + same dep-signature) — this is the
    /// conservative "broader satisfies narrower" rule from; a
    /// dep-signature tightening pass against the actual narrower read-set
    /// is permitted follow-up work tracked in §1.4.
    ///
    /// Returns the list of slots that this publish actually populated
    /// (the primary slot + any previously-empty narrower slots that were
    /// backfilled). — the caller registers a
    /// reverse-index entry per populated slot in the per-canonical
    /// `canonical_to_entries` index. Capped at 6 (single + identity +
    /// navigate + shallow + expanded + skeleton), so a stack `SmallVec`
    /// keeps allocation off the hot path.
    pub(super) fn publish(
        &mut self,
        slot: ModeSlot,
        entry: MemoEntry,
    ) -> smallvec::SmallVec<[ModeSlot; 6]> {
        let mut populated = smallvec::SmallVec::<[ModeSlot; 6]>::new();
        *self.slot_mut(slot) = Some(entry.clone());
        populated.push(slot);
        for narrower in backfill_targets(slot) {
            let cell = self.slot_mut(*narrower);
            if cell.is_none() {
                *cell = Some(entry.clone());
                populated.push(*narrower);
            }
        }
        populated
    }

    pub(super) fn populated_count(&self) -> usize {
        let slots = [
            &self.single,
            &self.identity,
            &self.navigate,
            &self.shallow,
            &self.expanded,
            &self.skeleton,
            &self.transit_identity,
            &self.transit_navigate,
            &self.transit_shallow,
            &self.transit_expanded,
        ];
        slots.iter().filter(|s| s.is_some()).count()
    }

    /// Audit-only iterator that yields one `(slot_label, &MemoEntry)`
    /// pair per populated slot. Used by
    /// [`super::SemanticGraphStore::audit_eager_key_dump`] to flatten
    /// family state into per-slot rows for the Tier 0 Step 0.2 corpus
    /// snapshot.
    pub(super) fn iter_populated_slots(&self) -> Vec<(&'static str, &MemoEntry)> {
        let mut out: Vec<(&'static str, &MemoEntry)> = Vec::new();
        if let Some(e) = &self.single {
            out.push(("single", e));
        }
        if let Some(e) = &self.identity {
            out.push(("identity", e));
        }
        if let Some(e) = &self.navigate {
            out.push(("navigate", e));
        }
        if let Some(e) = &self.shallow {
            out.push(("shallow", e));
        }
        if let Some(e) = &self.expanded {
            out.push(("expanded", e));
        }
        if let Some(e) = &self.skeleton {
            out.push(("skeleton", e));
        }
        if let Some(e) = &self.transit_identity {
            out.push(("transit_identity", e));
        }
        if let Some(e) = &self.transit_navigate {
            out.push(("transit_navigate", e));
        }
        if let Some(e) = &self.transit_shallow {
            out.push(("transit_shallow", e));
        }
        if let Some(e) = &self.transit_expanded {
            out.push(("transit_expanded", e));
        }
        out
    }
}

/// One row in [`super::SemanticGraphStore::audit_eager_key_dump`] — used
/// by the Tier 0 Step 0.2 corpus snapshot to record interned keys +
/// cached payload hashes + dep-signatures for offline analysis. Not on
/// any hot path.
#[derive(Debug, Clone)]
pub struct AuditEagerKeyRow {
    pub key_repr: String,
    pub result_hash: String,
    pub dep_signature: String,
}

/// Slot fan-out for backfill. `Expanded` satisfies `Shallow` / `Navigate` /
/// `Identity`; `Shallow` satisfies `Navigate` / `Identity`; `Navigate`
/// satisfies `Identity`. `Identity` and `Single` backfill nothing.
/// `Skeleton` is independent of the Identity/Navigate/Shallow/Expanded
/// hierarchy (different semantics: preserves open generics) — it backfills
/// nothing AND nothing backfills it.
///
/// Codex-hybrid spec: the `Transit*` slots mirror the
/// publication-slot fan-out within the transit family. Cross-family
/// backfill (Transit → publication or publication → Transit) is NOT
/// admitted — a publication-context result and a transit-context
/// result have different reduction semantics and must not share a
/// cache cell.
pub(super) fn backfill_targets(slot: ModeSlot) -> &'static [ModeSlot] {
    match slot {
        ModeSlot::Single => &[],
        ModeSlot::Identity => &[],
        ModeSlot::Navigate => &[ModeSlot::Identity],
        ModeSlot::Shallow => &[ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Expanded => &[ModeSlot::Shallow, ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Skeleton => &[],
        ModeSlot::TransitIdentity => &[],
        ModeSlot::TransitNavigate => &[ModeSlot::TransitIdentity],
        ModeSlot::TransitShallow => &[ModeSlot::TransitNavigate, ModeSlot::TransitIdentity],
        ModeSlot::TransitExpanded => &[
            ModeSlot::TransitShallow,
            ModeSlot::TransitNavigate,
            ModeSlot::TransitIdentity,
        ],
    }
}

pub(super) fn mode_to_slot(mode: ProjectionMode) -> ModeSlot {
    match mode {
        ProjectionMode::Identity => ModeSlot::Identity,
        ProjectionMode::Navigate => ModeSlot::Navigate,
        ProjectionMode::Shallow => ModeSlot::Shallow,
        ProjectionMode::Expanded => ModeSlot::Expanded,
        ProjectionMode::Skeleton => ModeSlot::Skeleton,
    }
}

/// Map a [`ProjectionReductionContext`] to the matching [`ModeSlot`].
/// Publication contexts use the standard
/// Identity/Navigate/Shallow/Expanded/Skeleton slots; transit contexts
/// use the `Transit*` mirrors.
pub(super) fn context_to_slot(ctx: ProjectionReductionContext) -> ModeSlot {
    match ctx.demand {
        ReductionDemand::Published => mode_to_slot(ctx.mode),
        ReductionDemand::StructuralTransit => match ctx.mode {
            ProjectionMode::Identity => ModeSlot::TransitIdentity,
            ProjectionMode::Navigate => ModeSlot::TransitNavigate,
            ProjectionMode::Shallow => ModeSlot::TransitShallow,
            ProjectionMode::Expanded => ModeSlot::TransitExpanded,
            // Skeleton has its own slot — distinct semantics (open-
            // generic preservation) that the codex-hybrid leaves
            // outside the reduction-demand axis.
            ProjectionMode::Skeleton => ModeSlot::Skeleton,
        },
    }
}

/// Project a [`SemanticQueryKey`] onto its `(family, slot)` pair. For
/// mode-bearing variants the mode is stripped into the slot; for everything
/// else the slot is `Single`.
pub(super) fn family_and_slot(key: &SemanticQueryKey) -> (FamilyKey, ModeSlot) {
    match key {
        SemanticQueryKey::ResolveDecl(decl) => {
            (FamilyKey::ResolveDecl(decl.clone()), ModeSlot::Single)
        }
        SemanticQueryKey::Instantiate {
            base,
            args,
            context,
        } => (
            FamilyKey::Instantiate {
                base: base.clone(),
                args: Arc::clone(args),
                provenance: context.provenance,
            },
            context_to_slot(*context),
        ),
        SemanticQueryKey::ProjectMember { base, member, mode } => (
            FamilyKey::ProjectMember {
                base: *base,
                member: Arc::clone(member),
            },
            mode_to_slot(*mode),
        ),
        SemanticQueryKey::IndexedAccess { base, index, mode } => (
            FamilyKey::IndexedAccess {
                base: *base,
                index: index.clone(),
            },
            mode_to_slot(*mode),
        ),
        SemanticQueryKey::KeyOf { base, context } => {
            (FamilyKey::KeyOf { base: *base }, context_to_slot(*context))
        }
        SemanticQueryKey::MappedType {
            source,
            mapper,
            context,
        } => (
            FamilyKey::MappedType {
                source: *source,
                mapper: mapper.clone(),
            },
            context_to_slot(*context),
        ),
        SemanticQueryKey::Conditional {
            check,
            extends,
            true_branch,
            false_branch,
            distributive,
        } => (
            FamilyKey::Conditional {
                check: *check,
                extends: *extends,
                true_branch: *true_branch,
                false_branch: *false_branch,
                distributive: *distributive,
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::TypeOf { value_root } => (
            FamilyKey::TypeOf {
                value_root: value_root.clone(),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::NormalizeUnion { members } => (
            FamilyKey::NormalizeUnion {
                members: Arc::clone(members),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::NormalizeIntersection { members } => (
            FamilyKey::NormalizeIntersection {
                members: Arc::clone(members),
            },
            ModeSlot::Single,
        ),
        SemanticQueryKey::ProjectPath {
            base,
            path,
            context,
        } => (
            FamilyKey::ProjectPath {
                base: *base,
                path: Arc::clone(path),
                provenance: context.provenance,
            },
            context_to_slot(*context),
        ),
        SemanticQueryKey::ResolvedNamedType { key } => (
            FamilyKey::ResolvedNamedType {
                key: Arc::clone(key),
            },
            ModeSlot::Single,
        ),
        // `Relate` bypasses the family memo entirely — it stores its
        // tri-state result in the dedicated `relation_memo` DashMap.
        // `family_and_slot` returning a placeholder is safe because
        // `execute_cooperative` admission short-circuits `Relate`
        // before this function is consulted.
        SemanticQueryKey::Relate { source, target } => (
            FamilyKey::IndexedAccess {
                base: *source,
                index: crate::semantic_query::IndexKey::TypeNode(*target),
            },
            ModeSlot::Single,
        ),
        // Binding amendment — `ResolveMacroPayload`. The
        // mode is stripped into the slot per the standard mode-bearing
        // pattern; the family identity is the (owner, macro_index,
        // macro_kind, type_args) tuple.
        SemanticQueryKey::ResolveMacroPayload {
            owner,
            macro_index,
            macro_kind,
            type_args,
            mode,
        } => (
            FamilyKey::ResolveMacroPayload {
                owner: owner.clone(),
                macro_index: *macro_index,
                macro_kind: *macro_kind,
                type_args: Arc::clone(type_args),
            },
            mode_to_slot(*mode),
        ),
    }
}

/// Returns true iff any [`crate::resolver_core::FactVersionRef`] in
/// `facts` carries `canonical_id` as its referenced canonical. Used by
/// `invalidate_canonical` to discriminate entries whose path-precise
/// fact signature references `canonical_id`. The reverse index
/// registers under every canonical the fact rail names, so this
/// predicate is the fact-rail membership check the drain falls back to.
pub(super) fn carrier_facts_reference_canonical(
    facts: &[crate::resolver_core::FactVersionRef],
    canonical_id: &str,
) -> bool {
    facts.iter().any(|fact| match fact {
        crate::resolver_core::FactVersionRef::FileWholeHash {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        crate::resolver_core::FactVersionRef::DerivedFactHash {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        crate::resolver_core::FactVersionRef::Parse(p) => p.canonical_id.as_str() == canonical_id,
        crate::resolver_core::FactVersionRef::ResolveImports(r) => {
            r.canonical_id.as_str() == canonical_id
        }
        crate::resolver_core::FactVersionRef::RouteSurface(r) => {
            r.canonical_id.as_str() == canonical_id
        }
        // Not file-scoped — references no canonical.
        crate::resolver_core::FactVersionRef::ProjectGeneration { .. } => false,
    })
}

/// Every [`ModeSlot`] variant as a static slice. Pre-Γ.B
/// `invalidate_canonical` linearly walked every family × every slot
/// here. Post-Γ.B the per-canonical reverse index drives the sweep,
/// but the constant is retained for invalidate-all and diagnostic
/// paths that still need to enumerate all slots.
#[allow(dead_code)]
pub(super) const ALL_MODE_SLOTS: &[ModeSlot] = &[
    ModeSlot::Single,
    ModeSlot::Identity,
    ModeSlot::Navigate,
    ModeSlot::Shallow,
    ModeSlot::Expanded,
    ModeSlot::Skeleton,
    ModeSlot::TransitIdentity,
    ModeSlot::TransitNavigate,
    ModeSlot::TransitShallow,
    ModeSlot::TransitExpanded,
];
