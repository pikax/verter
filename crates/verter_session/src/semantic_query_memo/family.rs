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
    DeclKey, DepSignature, HostResolvedNamedTypeKey, IndexKey, MapperKey, PathSegment,
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
    /// LRU eviction-recency metadata for the multi-candidate slot
    /// vector — NOT a semantic-validity oracle. Validity is decided
    /// exclusively by `read_set_signature.validate_with_self_roots`
    /// against the caller's live view.
    pub(super) validated_at_generation: u64,
    /// Store-assigned per-candidate identity. Two distinct candidates
    /// in the same `(family, slot)` carry distinct `admission_seq`s;
    /// the reverse-index registration includes this seq so a
    /// `(family, slot)` with multiple candidates registers each
    /// candidate separately and a cross-canonical cleanup for an
    /// evicted candidate does NOT strip the surviving sibling's
    /// registrations. Set by [`SemanticGraphStore::warm_publish_one`] /
    /// `warm_publish_one_if_absent` / the test publish helpers — the
    /// only paths that create a `MemoEntry`.
    pub(super) admission_seq: u64,
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
        // Test-only process-global probe, serialised across test
        // threads via [`super::VALIDATE_RUNNING_PROBE_TEST_LOCK`].
        // Invoked WHILE this `validate` call is running. The warm-read
        // path calls `validate` AFTER releasing the `entries` lock
        // (snapshot + outside-lock validate); the probe lets a test
        // assert `entries.try_lock()` succeeds from a peer thread while
        // this validate is in progress. Disarmed by default; armed via
        // [`super::SemanticGraphStore::arm_validate_running_probe_for_tests`].
        #[cfg(any(test, debug_assertions))]
        super::validate_running_probe();
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
    /// `base` is a content-free [`DeclKey`] (R6) — version-rooting
    /// lives on each candidate's `ReadSetSignature.facts` +
    /// `self_root_canonicals` inside the multi-candidate
    /// [`FamilySlots`].
    Instantiate {
        base: DeclKey,
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
        /// Member-merge role dimension (codex BINDING design for the
        /// type-resolution unification). A heritage-arm instantiation
        /// (`MemberMergeRole::Heritage`) and a structural instantiation of
        /// the SAME decl + args produce distinct surfaces (the inherited
        /// members carry distinct merge roles), so they MUST NOT collide on
        /// one family slot. Orthogonal to `provenance`.
        merge_role: crate::semantic_query::MemberMergeRole,
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
        /// Member-merge role dimension (codex BINDING design for the
        /// type-resolution unification). The empty-path Shallow projection
        /// of a heritage carrier under `MemberMergeRole::Heritage` produces a
        /// distinct surface from a structural projection of the same
        /// `(base, path)`, so they MUST NOT collide on one family slot.
        merge_role: crate::semantic_query::MemberMergeRole,
    },
    /// Included for completeness so `family_and_slot` is total, but
    /// [`SemanticQueryKey::ResolvedNamedType`] bypasses the family memo at
    /// admission and never lands in the warm map.
    ResolvedNamedType {
        key: Arc<HostResolvedNamedTypeKey>,
    },
    /// Mode-erased ResolveMacroPayload identity. `owner` is a
    /// content-free [`DeclKey`] (R6) — version-rooting lives on each
    /// candidate's `ReadSetSignature.facts` + `self_root_canonicals`
    /// inside the multi-candidate [`FamilySlots`].
    ResolveMacroPayload {
        owner: DeclKey,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: Arc<[SemanticNodeId]>,
    },
    /// Mode-erased `ResolveClassSurface` identity. `decl_slot` is the
    /// content-free [`crate::semantic_query::ResolvedDeclSlotIdentity`]
    /// (env-bearing but content-free, R6 — it carries the slot's
    /// intrinsic `T`/`L`/`J` env), CANONICALIZED to a single
    /// `symbol_space` (`Type`) by `family_and_slot`: the incoming slot's
    /// symbol space must NOT fork the family identity, because `side`
    /// already selects the half and `build_class_surface` derives the
    /// composed sub-query from `(defining_canonical, merged_symbol_name)`
    /// regardless of `symbol_space` (path-independence — two slots
    /// differing only in `symbol_space` compute the same value and so
    /// share one slot). `side` is a MANDATORY identity discriminator —
    /// instance and static halves of the same class occupy DISTINCT
    /// family slots and never collide. The context's EXTRA env dim
    /// (`resolve_env_hash` = `R`; env stays ON the key) is carried here so
    /// two queries differing only in the context env-hash do NOT collide.
    /// This is an ENV hash, NOT a content/version hash (R6-clean). The
    /// projection mode is stripped into the [`ModeSlot`].
    ResolveClassSurface {
        decl_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        side: crate::semantic_query::ClassSurfaceSide,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// Mode-erased `ResolveAmbientNamespace` identity. Carries the
    /// namespace slot (`symbol_space = Namespace`) + type args + the
    /// context's extra env dim (`resolve_env_hash` = `R`). The execute
    /// path is non-producing (returns `Opaque(Miss)`); like `Relate`,
    /// nothing is ever admitted under this family — the variant exists so
    /// `family_and_slot` stays total and honest (a real distinct
    /// identity, never a placeholder reusing another family's shape).
    ResolveAmbientNamespace {
        namespace_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// Mode-erased `ResolveEnum` identity. Carries the context's extra
    /// env dim (`resolve_env_hash` = `R`). Non-producing (see
    /// [`Self::ResolveAmbientNamespace`]).
    ResolveEnum {
        enum_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// Mode-erased `ResolveOverloadSet` identity. Carries the context's
    /// extra env dim (`resolve_env_hash` = `R`). Non-producing (see
    /// [`Self::ResolveAmbientNamespace`]).
    ResolveOverloadSet {
        callee: SemanticNodeId,
        type_args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// DEDICATED, non-aliasing `Relate` family identity carrying the FULL
    /// relation identity [`crate::semantic_query::RelateMemoKey`] (source /
    /// target / relation kind / policy / source freshness / inference context /
    /// env+substitution+projection-reduction context).
    ///
    /// No production code constructs a [`SemanticQueryKey::Relate`] value, so
    /// this variant is never published into or read from the family memo at
    /// runtime — the production relation authority is `relate_nodes`, which keys
    /// the dedicated `relation_memo` on the same `RelateMemoKey` and never
    /// enters `execute_cooperative` / the family memo. The variant exists SOLELY
    /// so `family_and_slot` stays total and honest over every
    /// [`SemanticQueryKey`] variant (a real distinct identity, never a
    /// placeholder reusing another family's shape).
    ///
    /// It carries the FULL relation identity (NOT just source/target): even
    /// though nothing is admitted under it, a `Relate` key can NEVER collide
    /// with a live [`Self::IndexedAccess`] slot over the same `(source, target)`
    /// nodes — the prior arm aliased `IndexedAccess` and was a latent
    /// wrong-domain warm-hit hazard. Carrying the whole `RelateMemoKey` also
    /// keeps the family identity faithful to the relation memo's own key, so two
    /// `Relate` keys differing in any relation-identity axis map to distinct
    /// family identities.
    Relate {
        key: crate::semantic_query::RelateMemoKey,
    },
    /// Mode-erased `ApparentType` identity. `ApparentType` has no slot, so
    /// its R21 env dims (`type_env_hash` = `T`, `lib_env_hash` = `L`,
    /// `project_identity` = `J`) ride here ON the family key — these are
    /// ENV hashes, NOT content/version hashes (R6-clean). Two queries
    /// differing only in any env dim do NOT collide. Non-producing (the
    /// lib-member index is unimplemented): the execute path returns
    /// `Opaque(Miss)` and nothing is ever admitted under this family; like
    /// `Relate`, the variant exists so `family_and_slot` stays total and
    /// honest.
    ApparentType {
        base: SemanticNodeId,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
    },
    /// Mode-erased `TemplateLiteralReduce` identity. `pattern` (quasis) and
    /// the ORDER-SIGNIFICANT `args` (NEVER reordered — concatenation order
    /// is semantic) are the identity core; the context's `{R, T, L, J}` env
    /// dims ride here ON the family key (env hashes, R6-clean). LIVE
    /// producer (build folds via the shared deferred evaluator).
    TemplateLiteralReduce {
        pattern: Arc<[Arc<str>]>,
        args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
    },
    /// Mode-erased `FlowNarrowingAt` identity. The [`ProgramPointId`]
    /// (`canonical_id` + `offset`) is the identity core; the context's full
    /// `{P, R, T, L, J}` env dims ride here ON the family key (env hashes,
    /// NOT content/version hashes — R6-clean). Non-producing (the flow
    /// engine lands in U6): the execute path returns `Opaque(Miss)` and
    /// nothing is ever admitted under this family; like `Relate`, the
    /// variant exists so `family_and_slot` stays total and honest.
    ///
    /// [`ProgramPointId`]: crate::semantic_query::ProgramPointId
    FlowNarrowingAt {
        point: crate::semantic_query::ProgramPointId,
        parse_env_hash: crate::semantic_query::HashValue,
        resolve_env_hash: crate::semantic_query::HashValue,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
    },
    /// Mode-erased `ContextualTypeAt` identity. Same shape as
    /// [`FlowNarrowingAt`](Self::FlowNarrowingAt): the [`ProgramPointId`] is
    /// the identity core and the full `{P, R, T, L, J}` env dims ride here
    /// (env hashes, R6-clean). Non-producing (the contextual engine lands in
    /// U6).
    ///
    /// [`ProgramPointId`]: crate::semantic_query::ProgramPointId
    ContextualTypeAt {
        point: crate::semantic_query::ProgramPointId,
        parse_env_hash: crate::semantic_query::HashValue,
        resolve_env_hash: crate::semantic_query::HashValue,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
    },
}

impl FamilyKey {
    /// The stable variant label of this family identity. Used by the family-
    /// mapping guards (via the `for_tests` probe) to assert the domain a
    /// [`SemanticQueryKey`] maps to — e.g. that `Relate` maps to the dedicated
    /// `Relate` family and never aliases `IndexedAccess` — without exposing the
    /// `pub(super)` taxonomy outside the crate.
    pub(super) fn variant_label(&self) -> &'static str {
        match self {
            FamilyKey::ResolveDecl(_) => "ResolveDecl",
            FamilyKey::Instantiate { .. } => "Instantiate",
            FamilyKey::ProjectMember { .. } => "ProjectMember",
            FamilyKey::IndexedAccess { .. } => "IndexedAccess",
            FamilyKey::KeyOf { .. } => "KeyOf",
            FamilyKey::MappedType { .. } => "MappedType",
            FamilyKey::Conditional { .. } => "Conditional",
            FamilyKey::TypeOf { .. } => "TypeOf",
            FamilyKey::NormalizeUnion { .. } => "NormalizeUnion",
            FamilyKey::NormalizeIntersection { .. } => "NormalizeIntersection",
            FamilyKey::ProjectPath { .. } => "ProjectPath",
            FamilyKey::ResolvedNamedType { .. } => "ResolvedNamedType",
            FamilyKey::ResolveMacroPayload { .. } => "ResolveMacroPayload",
            FamilyKey::ResolveClassSurface { .. } => "ResolveClassSurface",
            FamilyKey::ResolveAmbientNamespace { .. } => "ResolveAmbientNamespace",
            FamilyKey::ResolveEnum { .. } => "ResolveEnum",
            FamilyKey::ResolveOverloadSet { .. } => "ResolveOverloadSet",
            FamilyKey::Relate { .. } => "Relate",
            FamilyKey::ApparentType { .. } => "ApparentType",
            FamilyKey::TemplateLiteralReduce { .. } => "TemplateLiteralReduce",
            FamilyKey::FlowNarrowingAt { .. } => "FlowNarrowingAt",
            FamilyKey::ContextualTypeAt { .. } => "ContextualTypeAt",
        }
    }
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
    /// Vue macro object-surface publication slot.
    /// `ReductionDemand::MacroObjectSurface` at the Shallow macro
    /// publication boundary lands here — the empty-path Shallow surface
    /// enumerates the UNION of object-arm members (Vue macro convention)
    /// rather than the common-member intersection. Distinct from the
    /// `Shallow` publication slot so a macro object-surface read and an
    /// ordinary `Published(Shallow)` read of the same node never collide.
    /// Does NOT backfill / is NOT backfilled by any other slot (the union
    /// surface is a different evaluation).
    MacroSurfaceShallow,
}

/// Per-slot candidate cap.
///
/// Two candidates in one (family, slot) belong to different views (a
/// base view and an overlay view, or two overlays of the SAME
/// content-free key under different file-content versions). Same-view
/// re-publish (matching signature) replaces in place; a different
/// view appends; an unrelated fifth candidate FIFO-evicts the
/// oldest. The cap prevents unbounded growth for keys queried under
/// many distinct overlays without losing R20 overlay isolation.
pub(super) const FAMILY_SLOT_CANDIDATE_CAP: usize = 4;

/// Per-slot candidate list (cap [`FAMILY_SLOT_CANDIDATE_CAP`]).
///
/// Insertion order is FIFO from front to back; the eldest candidate
/// sits at index 0. Eviction policy when an unmatched signature
/// appends a new candidate at cap: evict the oldest candidate at
/// index 0.
pub(super) type CandidateList = smallvec::SmallVec<[MemoEntry; FAMILY_SLOT_CANDIDATE_CAP]>;

/// Outcome of [`FamilySlots::publish`]: the list of slots this publish
/// populated PLUS the candidates that were displaced during the
/// publish (same-discriminant replacements + per-slot FIFO cap-evictions).
/// The caller drains each displaced candidate's reverse-index
/// registrations by its `admission_seq` so a surviving sibling
/// candidate in the same slot keeps its own seq registrations.
pub(super) struct FamilyPublishOutcome {
    pub(super) populated: smallvec::SmallVec<[ModeSlot; 6]>,
    pub(super) displaced: smallvec::SmallVec<[(ModeSlot, MemoEntry); 4]>,
}

/// Per-family per-slot warm storage.
///
/// Each slot independently holds an ORDERED LIST of [`MemoEntry`]
/// candidates (cap [`FAMILY_SLOT_CANDIDATE_CAP`]) — see
/// [`CandidateList`]. Each candidate carries its own
/// `read_set_signature` + `self_root_canonicals`, so two file-content
/// versions of the SAME content-free `SemanticQueryKey` (e.g.
/// `Instantiate { base: DeclKey { canonical, name }, .. }` under a
/// base view and an overlay view) coexist as distinct candidates
/// inside the same slot — R20 overlay isolation.
///
/// Validity is decided EXCLUSIVELY by per-candidate
/// `read_set_signature.validate_with_self_roots`. The
/// `validated_at_generation` metadata is recency only.
///
/// Backfill on completion fills every empty narrower slot from a
/// successful broader compute (see [`FamilySlots::publish`]).
#[derive(Default, Clone)]
pub(super) struct FamilySlots {
    single: CandidateList,
    identity: CandidateList,
    navigate: CandidateList,
    shallow: CandidateList,
    expanded: CandidateList,
    /// Skeleton mode slot. Independent from
    /// Navigate/Expanded; does NOT participate in backfill.
    skeleton: CandidateList,
    /// `StructuralTransit` slot mirrors of the four publication slots —
    /// Codex-hybrid spec. Independent from the
    /// publication slots; backfill within the transit family follows
    /// the same `Expanded → Shallow → Navigate → Identity` hierarchy.
    transit_identity: CandidateList,
    transit_navigate: CandidateList,
    transit_shallow: CandidateList,
    transit_expanded: CandidateList,
    /// Vue macro object-surface publication slot. Independent of
    /// the publication + transit slots; no backfill in either direction.
    macro_surface_shallow: CandidateList,
}

/// Discriminant identity used for in-place replacement on
/// [`FamilySlots::publish`].
///
/// A re-publish with the SAME fact-set + generation replaces the
/// existing candidate in place. A different view (different
/// generation OR different observed facts) appends a NEW candidate.
/// This is admission identity ONLY — read-side validity is decided by
/// the candidate's `ReadSetSignature.validate_with_self_roots` rail
/// against the caller's live view (R20).
fn candidate_same_discriminant(a: &MemoEntry, b: &MemoEntry) -> bool {
    a.validated_at_generation == b.validated_at_generation
        && a.read_set_signature.facts == b.read_set_signature.facts
}

impl FamilySlots {
    fn slot_list(&self, slot: ModeSlot) -> &CandidateList {
        match slot {
            ModeSlot::Single => &self.single,
            ModeSlot::Identity => &self.identity,
            ModeSlot::Navigate => &self.navigate,
            ModeSlot::Shallow => &self.shallow,
            ModeSlot::Expanded => &self.expanded,
            ModeSlot::Skeleton => &self.skeleton,
            ModeSlot::TransitIdentity => &self.transit_identity,
            ModeSlot::TransitNavigate => &self.transit_navigate,
            ModeSlot::TransitShallow => &self.transit_shallow,
            ModeSlot::TransitExpanded => &self.transit_expanded,
            ModeSlot::MacroSurfaceShallow => &self.macro_surface_shallow,
        }
    }

    fn slot_list_mut(&mut self, slot: ModeSlot) -> &mut CandidateList {
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
            ModeSlot::MacroSurfaceShallow => &mut self.macro_surface_shallow,
        }
    }

    /// Snapshot the candidate list for `slot`. Caller validates each
    /// candidate OUTSIDE the lock (via [`MemoEntry::validate`]) and, on a
    /// match, briefly reacquires the lock to call
    /// [`Self::mark_validated_freshest`] for LRU bookkeeping.
    ///
    /// Splitting the warm-hit path into snapshot, outside-lock
    /// validate, and brief LRU update keeps the single global memo
    /// `entries` mutex off the validation hot path. `validate` walks
    /// the path-precise fact rail against the resolver store view,
    /// which is itself reentrant work; holding `entries` across that
    /// walk serialises every unrelated warm read and cold publish
    /// during validation, which the multi-candidate cap-4 substrate
    /// makes worse.
    pub(super) fn snapshot_slot(&self, slot: ModeSlot) -> CandidateList {
        self.slot_list(slot).clone()
    }

    /// Move the candidate matching `(validated_at_generation, facts)`
    /// to the back of `slot`'s FIFO order — the LRU bookkeeping the
    /// snapshot/validate-outside-lock path's caller invokes after a
    /// successful match.
    ///
    /// Identifies the candidate by discriminant identity (the same
    /// `(validated_at_generation, facts)` pair `publish_one` uses for
    /// in-place replacement). If no matching candidate is still in the
    /// slot — a concurrent invalidation drained it between the
    /// snapshot and this update — this is a no-op; the caller has
    /// already returned the cloned `MemoEntry` from the snapshot.
    ///
    /// Moves the matching candidate to the back of the FIFO insertion
    /// order so the LRU eviction policy treats it as freshest.
    /// `validated_at_generation` is left unchanged (admission-time
    /// stamp, not access-time).
    pub(super) fn mark_validated_freshest(&mut self, slot: ModeSlot, entry: &MemoEntry) {
        let list = self.slot_list_mut(slot);
        if let Some(index) = list
            .iter()
            .position(|c| candidate_same_discriminant(c, entry))
        {
            if index + 1 < list.len() {
                let candidate = list.remove(index);
                list.push(candidate);
            }
        }
    }

    /// Unvalidated peek: return the first candidate for `slot`, if
    /// any. Used by code paths that report whether a slot is
    /// physically populated (independent of validity against any
    /// particular view) — e.g. instrumentation, in-flight admission
    /// gating, and reverse-index sanity checks. The validity oracle
    /// is `lookup`'s strict self-root validation rail.
    pub(super) fn slot_peek_any(&self, slot: ModeSlot) -> Option<&MemoEntry> {
        self.slot_list(slot).first()
    }

    /// Publish `entry` into `slot` and backfill every narrower slot
    /// whose candidate list is currently empty.
    ///
    /// Admission policy in the PRIMARY slot:
    /// - Same exact `(validated_at_generation, facts)` discriminant ⇒
    ///   replace in place (move to back; same-view re-publish).
    /// - Different discriminant ⇒ append at the back.
    /// - At cap ⇒ FIFO-evict the front (oldest by insertion).
    ///
    /// Backfill into a narrower slot is the conservative "broader
    /// satisfies narrower when no narrower compute landed first" rule
    /// — backfill writes ONLY if the narrower candidate list is
    /// empty, preserving any prior narrower-specific publish. A
    /// narrower compute that wrote first survives the broader
    /// compute's backfill (consistent with the legacy single-entry
    /// `if cell.is_none()` semantics).
    ///
    /// Returns the list of slots this publish populated AND the
    /// candidates that the publish displaced (same-discriminant
    /// replacements + per-slot FIFO cap-eviction victims). The caller
    /// drains each displaced candidate's reverse-index registrations
    /// under its own admission_seq so a sibling candidate in the same
    /// slot keeps its registrations.
    pub(super) fn publish(&mut self, slot: ModeSlot, entry: MemoEntry) -> FamilyPublishOutcome {
        let mut populated = smallvec::SmallVec::<[ModeSlot; 6]>::new();
        let mut displaced: smallvec::SmallVec<[(ModeSlot, MemoEntry); 4]> =
            smallvec::SmallVec::new();
        for victim in self.publish_one(slot, entry.clone()) {
            displaced.push((slot, victim));
        }
        populated.push(slot);
        for narrower in backfill_targets(slot) {
            if self.slot_list(*narrower).is_empty() {
                for victim in self.publish_one(*narrower, entry.clone()) {
                    displaced.push((*narrower, victim));
                }
                populated.push(*narrower);
            }
        }
        FamilyPublishOutcome {
            populated,
            displaced,
        }
    }

    /// Internal: admit `entry` into a single slot's candidate list,
    /// applying the same-discriminant-replace / FIFO-evict rules.
    /// Returns the candidates that were displaced (replaced or
    /// evicted) — the caller drains their reverse-index registrations
    /// by per-candidate `admission_seq` so a surviving sibling
    /// candidate in the same slot keeps its own seq registrations.
    fn publish_one(
        &mut self,
        slot: ModeSlot,
        entry: MemoEntry,
    ) -> smallvec::SmallVec<[MemoEntry; 2]> {
        let mut displaced: smallvec::SmallVec<[MemoEntry; 2]> = smallvec::SmallVec::new();
        let list = self.slot_list_mut(slot);
        if let Some(pos) = list
            .iter()
            .position(|c| candidate_same_discriminant(c, &entry))
        {
            // Same view re-publish: remove the previous candidate and
            // append the new one so it becomes the freshest by
            // insertion order. A FIFO eviction now drops the oldest
            // unrelated candidate, not the just-replaced one. The
            // displaced candidate's reverse-index registrations
            // (keyed under its own admission_seq) are orphan stamps
            // until the caller drains them.
            displaced.push(list.remove(pos));
            list.push(entry);
        } else {
            // Different view: append. If we overshoot the cap, drop
            // the oldest candidate at the front — and surface it so
            // the caller can drain its reverse-index registrations.
            list.push(entry);
            while list.len() > FAMILY_SLOT_CANDIDATE_CAP {
                displaced.push(list.remove(0));
            }
        }
        displaced
    }

    /// Total number of distinct slots that hold at least one
    /// candidate. Used by the per-store memo size accounting.
    pub(super) fn populated_count(&self) -> usize {
        [
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
            &self.macro_surface_shallow,
        ]
        .iter()
        .filter(|list| !list.is_empty())
        .count()
    }

    /// Audit-only iterator that yields one `(slot_label, &MemoEntry)`
    /// pair per populated slot — taking the FIRST candidate of each
    /// list as a representative. Used by
    /// [`super::SemanticGraphStore::audit_eager_key_dump`] to flatten
    /// family state into per-slot rows for the corpus snapshot.
    ///
    /// **By design — single-representative shape.** With the cap-4
    /// multi-candidate substrate a slot may hold up to 4 candidates,
    /// but this audit row format yields ONE row per populated slot
    /// to preserve the legacy corpus-snapshot shape so existing audit
    /// fixtures stay stable. Tooling that needs the full per-candidate
    /// enumeration uses [`Self::iter_populated_slots_all`] (drain /
    /// reverse-index sweep paths), not this audit dump. The chosen
    /// representative is the FIRST candidate in the list — the eldest
    /// by insertion order under the FIFO discipline (the slot's LRU
    /// move-to-back operation reorders subsequent reads, so a recently
    /// validated candidate is at the back of the list, not the front).
    pub(super) fn iter_populated_slots(&self) -> Vec<(&'static str, &MemoEntry)> {
        let mut out: Vec<(&'static str, &MemoEntry)> = Vec::new();
        if let Some(e) = self.single.first() {
            out.push(("single", e));
        }
        if let Some(e) = self.identity.first() {
            out.push(("identity", e));
        }
        if let Some(e) = self.navigate.first() {
            out.push(("navigate", e));
        }
        if let Some(e) = self.shallow.first() {
            out.push(("shallow", e));
        }
        if let Some(e) = self.expanded.first() {
            out.push(("expanded", e));
        }
        if let Some(e) = self.skeleton.first() {
            out.push(("skeleton", e));
        }
        if let Some(e) = self.transit_identity.first() {
            out.push(("transit_identity", e));
        }
        if let Some(e) = self.transit_navigate.first() {
            out.push(("transit_navigate", e));
        }
        if let Some(e) = self.transit_shallow.first() {
            out.push(("transit_shallow", e));
        }
        if let Some(e) = self.transit_expanded.first() {
            out.push(("transit_expanded", e));
        }
        if let Some(e) = self.macro_surface_shallow.first() {
            out.push(("macro_surface_shallow", e));
        }
        out
    }

    /// Candidate count in a specific slot. Exposed for test probes
    /// (`SemanticGraphStore::slot_candidate_count_for_tests`); the
    /// integration tests use it to verify multi-candidate
    /// coexistence and cap-4 FIFO eviction. Cheap O(1) read.
    pub(super) fn slot_candidate_count_for_test(&self, slot: ModeSlot) -> usize {
        self.slot_list(slot).len()
    }

    /// Walk `slot`'s candidate list and retain only those entries for
    /// which `keep` returns `true`. Removed entries do not appear in
    /// any subsequent lookup; the caller is responsible for fanning
    /// out per-candidate cleanup (reverse-index drains, memo-budget
    /// accounting). Used by `invalidate_canonical` to drop only those
    /// candidates whose fact rail genuinely references the touched
    /// canonical, leaving unrelated overlay candidates intact.
    pub(super) fn retain_candidates_in_slot_mut<F>(&mut self, slot: ModeSlot, mut keep: F)
    where
        F: FnMut(&MemoEntry) -> bool,
    {
        let list = self.slot_list_mut(slot);
        list.retain(|entry| keep(entry));
    }

    /// Walk EVERY candidate in EVERY slot, yielding `(ModeSlot,
    /// &MemoEntry)` per candidate. Used by the reverse-index drain
    /// path that must register / unregister every candidate the
    /// memo's per-canonical sweep can encounter — not just one
    /// representative per slot.
    pub(super) fn iter_populated_slots_all(&self) -> Vec<(ModeSlot, &MemoEntry)> {
        let mut out: Vec<(ModeSlot, &MemoEntry)> = Vec::new();
        for e in &self.single {
            out.push((ModeSlot::Single, e));
        }
        for e in &self.identity {
            out.push((ModeSlot::Identity, e));
        }
        for e in &self.navigate {
            out.push((ModeSlot::Navigate, e));
        }
        for e in &self.shallow {
            out.push((ModeSlot::Shallow, e));
        }
        for e in &self.expanded {
            out.push((ModeSlot::Expanded, e));
        }
        for e in &self.skeleton {
            out.push((ModeSlot::Skeleton, e));
        }
        for e in &self.transit_identity {
            out.push((ModeSlot::TransitIdentity, e));
        }
        for e in &self.transit_navigate {
            out.push((ModeSlot::TransitNavigate, e));
        }
        for e in &self.transit_shallow {
            out.push((ModeSlot::TransitShallow, e));
        }
        for e in &self.transit_expanded {
            out.push((ModeSlot::TransitExpanded, e));
        }
        for e in &self.macro_surface_shallow {
            out.push((ModeSlot::MacroSurfaceShallow, e));
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
        // The macro object-surface slot is an independent evaluation (union
        // of arm members) — it does NOT backfill the publication / transit
        // slots and is not backfilled by them.
        ModeSlot::MacroSurfaceShallow => &[],
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
        // Vue macro object-surface publication. The macro
        // publication boundary always runs at Shallow mode (the
        // empty-path terminal-surface synthesis is where the union-arm
        // rule applies), so all modes land in the dedicated
        // `MacroSurfaceShallow` slot — distinct from the `Published`
        // slots so the union surface never collides with an ordinary
        // `Published` read of the same node.
        ReductionDemand::MacroObjectSurface => ModeSlot::MacroSurfaceShallow,
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
                merge_role: context.merge_role,
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
                merge_role: context.merge_role,
            },
            context_to_slot(*context),
        ),
        SemanticQueryKey::ResolvedNamedType { key } => (
            FamilyKey::ResolvedNamedType {
                key: Arc::clone(key),
            },
            ModeSlot::Single,
        ),
        // `Relate` maps to a DEDICATED, non-aliasing `FamilyKey::Relate`
        // carrying the FULL relation identity. No production code constructs a
        // `SemanticQueryKey::Relate`, so this arm is exercised only by identity
        // guards; the production relation authority is `relate_nodes`, which
        // keys the dedicated `relation_memo` on the same `RelateMemoKey` and
        // never enters `execute_cooperative` / `family_and_slot`.
        //
        // `family_and_slot` is consulted UNCONDITIONALLY by
        // `try_warm_hit_fast_path` BEFORE any admission short-circuit, so this
        // arm cannot rely on a short-circuit to be safe — it is safe because it
        // maps to a dedicated family identity that can never collide with a live
        // `IndexedAccess` slot over the same `(source, target)` nodes.
        SemanticQueryKey::Relate {
            source,
            target,
            relation,
            policy,
            source_freshness,
            inference_context,
            context,
        } => (
            FamilyKey::Relate {
                key: crate::semantic_query::RelateMemoKey {
                    source: *source,
                    target: *target,
                    relation: *relation,
                    policy: *policy,
                    source_freshness: *source_freshness,
                    inference_context: inference_context.clone(),
                    context: *context,
                },
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
        // ResolveClassSurface — `side` is real family identity (instance
        // vs static halves never collide); the projection mode strips
        // into the slot. LIVE producer (build composes
        // `execute(Instantiate)` / `execute(TypeOf)`). The incoming
        // slot's `symbol_space` is CANONICALIZED to `Type` so it cannot
        // fork the family identity: `side` selects the half and the build
        // ignores `symbol_space`, so two slots differing only in
        // `symbol_space` compute the same value and must share one slot
        // (path-independence).
        SemanticQueryKey::ResolveClassSurface {
            decl_slot,
            type_args,
            side,
            context,
        } => (
            FamilyKey::ResolveClassSurface {
                decl_slot: decl_slot
                    .with_symbol_space(crate::semantic_query::SemanticSymbolSpace::Type),
                type_args: Arc::clone(type_args),
                side: *side,
                resolve_env_hash: context.resolve_env_hash,
            },
            mode_to_slot(context.mode),
        ),
        // ResolveAmbientNamespace — non-producing. Like `Relate`, the
        // execute build returns `Opaque(Miss)` (never admitted), but the
        // variant is REAL (not a placeholder reusing another family's
        // shape) so `family_and_slot` is total and honest. The namespace
        // surface carries a projection mode, so the mode strips into the
        // slot.
        SemanticQueryKey::ResolveAmbientNamespace {
            namespace_slot,
            type_args,
            context,
        } => (
            FamilyKey::ResolveAmbientNamespace {
                namespace_slot: namespace_slot.clone(),
                type_args: Arc::clone(type_args),
                resolve_env_hash: context.resolve_env_hash,
            },
            mode_to_slot(context.mode),
        ),
        // ResolveEnum — non-producing. No projection mode → the `Single`
        // slot.
        SemanticQueryKey::ResolveEnum { enum_slot, context } => (
            FamilyKey::ResolveEnum {
                enum_slot: enum_slot.clone(),
                resolve_env_hash: context.resolve_env_hash,
            },
            ModeSlot::Single,
        ),
        // ResolveOverloadSet — non-producing. No projection mode → the
        // `Single` slot.
        SemanticQueryKey::ResolveOverloadSet {
            callee,
            type_args,
            context,
        } => (
            FamilyKey::ResolveOverloadSet {
                callee: *callee,
                type_args: Arc::clone(type_args),
                resolve_env_hash: context.resolve_env_hash,
            },
            ModeSlot::Single,
        ),
        // ApparentType — non-producing. No projection mode → the `Single`
        // slot. The `{T, L, J}` env dims ride on the family key (the key
        // has no slot to carry them).
        SemanticQueryKey::ApparentType { base, context } => (
            FamilyKey::ApparentType {
                base: *base,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
            },
            ModeSlot::Single,
        ),
        // TemplateLiteralReduce — LIVE producer. No projection mode → the
        // `Single` slot. `args` is carried VERBATIM (NOT canonicalized /
        // reordered): concatenation order is semantic. The `{R, T, L, J}`
        // env dims ride on the family key.
        SemanticQueryKey::TemplateLiteralReduce {
            pattern,
            args,
            context,
        } => (
            FamilyKey::TemplateLiteralReduce {
                pattern: Arc::clone(pattern),
                args: Arc::clone(args),
                resolve_env_hash: context.resolve_env_hash,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
            },
            ModeSlot::Single,
        ),
        // FlowNarrowingAt — non-producing. No projection mode → the `Single`
        // slot. The full `{P, R, T, L, J}` env dims ride on the family key
        // (the key has no slot to carry them); the `ProgramPointId` is
        // carried VERBATIM as the identity core.
        SemanticQueryKey::FlowNarrowingAt { point, context } => (
            FamilyKey::FlowNarrowingAt {
                point: point.clone(),
                parse_env_hash: context.parse_env_hash,
                resolve_env_hash: context.resolve_env_hash,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
            },
            ModeSlot::Single,
        ),
        // ContextualTypeAt — non-producing. Same shape as FlowNarrowingAt:
        // `Single` slot, full `{P, R, T, L, J}` env on the family key,
        // `ProgramPointId` as the verbatim identity core.
        SemanticQueryKey::ContextualTypeAt { point, context } => (
            FamilyKey::ContextualTypeAt {
                point: point.clone(),
                parse_env_hash: context.parse_env_hash,
                resolve_env_hash: context.resolve_env_hash,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
            },
            ModeSlot::Single,
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
    ModeSlot::MacroSurfaceShallow,
];
