//! Family memo — mode-erased keys + per-mode slots.
//!
//! `FamilyKey` is the mode-erased identity for one [`SemanticQueryKey`]
//! family; per-mode results live in distinct slots inside `FamilySlots`.
//! `family_and_slot` projects a key onto its `(family, slot)` pair, and
//! `backfill_targets` describes the slot-fan-out used by the
//! broader-satisfies-narrower backfill rule.

use std::sync::Arc;

use crate::semantic_query::{
    DeclIdentity, DepSignature, HostResolvedNamedTypeKey, IndexKey, MapperKey, PathSegment,
    ProjectionMode, QueryResult, ResolveDeclKey, SemanticNodeId, SemanticQueryKey, ValueRootKey,
};

#[derive(Clone)]
pub(super) struct MemoEntry {
    pub(super) result: QueryResult<SemanticNodeId>,
    pub(super) dep_signature: DepSignature,
    /// R3/R26/R28 path-precise dep signature sibling to
    /// `dep_signature`. Bubbles into outer fact tracers via
    /// [`crate::fact_signature_helpers::bubble_fact_signature`] so an
    /// active outer cold-compute sees this memo's observation set on
    /// transitive hits. The AND-gate alongside the legacy
    /// `dep_signature`.
    ///
    /// Allowed unread for now: warm-hit consumers continue to validate
    /// via the legacy `dep_signature` AND-gate; subsequent follow-up
    /// work wires the validators and bubble-up paths to read this
    /// substrate.
    #[allow(dead_code)]
    pub(super) fact_dep_signature: Arc<[crate::resolver_core::FactVersionRef]>,
    /// Walker diagnostics observed during the cold build that produced
    /// this entry. Replayed on warm hits via `CacheRead.walker_diagnostics`.
    /// Empty for non-walker queries.
    pub(super) walker_diagnostics: Arc<[crate::project_semantic_dispatch::walk::ShallowDiagnostic]>,
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
pub(super) fn backfill_targets(slot: ModeSlot) -> &'static [ModeSlot] {
    match slot {
        ModeSlot::Single => &[],
        ModeSlot::Identity => &[],
        ModeSlot::Navigate => &[ModeSlot::Identity],
        ModeSlot::Shallow => &[ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Expanded => &[ModeSlot::Shallow, ModeSlot::Navigate, ModeSlot::Identity],
        ModeSlot::Skeleton => &[],
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
            body_mode,
        } => (
            FamilyKey::Instantiate {
                base: base.clone(),
                args: Arc::clone(args),
            },
            mode_to_slot(*body_mode),
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
        SemanticQueryKey::KeyOf { base } => (FamilyKey::KeyOf { base: *base }, ModeSlot::Single),
        SemanticQueryKey::MappedType { source, mapper } => (
            FamilyKey::MappedType {
                source: *source,
                mapper: mapper.clone(),
            },
            ModeSlot::Single,
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
        SemanticQueryKey::ProjectPath { base, path, mode } => (
            FamilyKey::ProjectPath {
                base: *base,
                path: Arc::clone(path),
            },
            mode_to_slot(*mode),
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

/// Returns `true` iff `sig` contains a dep-record that names `canonical_id`.
/// The single invalidation authority in B3: `invalidate_canonical` walks
/// every populated slot's stored dep-signature and evicts those whose
/// signature references the changed canonical. No structural short-cut on
/// family-key shape — the dep-sig is the only truth.
pub(super) fn dep_signature_references_canonical(sig: &DepSignature, canonical_id: &str) -> bool {
    sig.iter()
        .any(|(canonical, _)| canonical.as_ref() == canonical_id)
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
];
