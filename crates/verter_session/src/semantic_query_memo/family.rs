//! Family memo — mode-erased keys + per-mode slots.
//!
//! `FamilyKey` is the mode-erased identity for one [`SemanticQueryKey`]
//! family; per-mode results live in distinct slots inside `FamilySlots`.
//! `family_and_slot` projects a key onto its `(family, slot)` pair, and
//! `slot_domain_siblings` describes the same-domain slots the §3.4
//! recorded-point backfill rule may fill from a broader compute.

use std::sync::Arc;

use parking_lot::Mutex;
use rustc_hash::FxHashMap;

use crate::fact_signature_helpers::{ReadSetSignature, ReadSetSignatureExt as _};
use crate::semantic_query::demand::{
    cached_satisfies, Demand, MaterializedPoint, MaterializedSet, ProjectionPath,
};
use crate::semantic_query::{
    DepSignature, IndexKey, MapperKey, PathSegment, ProjectionMode, ProjectionReductionContext,
    PropertyKey, QueryResult, ReductionDemand, ResolveDeclKey, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue, VueHeritagePolicy,
};

#[derive(Clone)]
pub(super) struct MemoEntry {
    pub(super) result: QueryResult<SemanticQueryValue>,
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
    /// The §3.4 **materialised-record set** — the concrete `(path, point)`
    /// records this candidate's compute ACTUALLY produced (the terminal
    /// point plus one `Navigate` hop per walked intermediate for a path
    /// walk; the single terminal point for a non-path build). This is NOT the
    /// candidate's nominal request demand: a deep terminal that only
    /// `Navigate`-walked an intermediate records a `Navigate` point there,
    /// never the terminal mode it never expanded. The warm-hit gate is
    /// `cached_satisfies(satisfied_projection, requested_point)` — one of
    /// the two independent gates (the other is
    /// `read_set_signature.validate_with_self_roots`); BOTH must pass.
    /// Same-family backfill clones this entry into a sibling slot ONLY when
    /// a recorded point dominates that sibling's requested point — never by
    /// enum rank.
    pub(super) satisfied_projection: MaterializedSet,
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
        #[cfg(any(test, feature = "test-support"))]
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
/// use only the `single` slot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) enum FamilyKey {
    ResolveDecl(ResolveDeclKey),
    /// `base` is the env-bearing, content-free
    /// [`crate::semantic_query::ResolvedDeclSlotIdentity`] (R6 — carries
    /// the slot's intrinsic `T` / `L` / `J` env, NEVER a content/version
    /// hash); version-rooting lives on each candidate's
    /// `ReadSetSignature.facts` + `self_root_canonicals` inside the
    /// multi-candidate [`FamilySlots`]. `resolve_env_hash` (`R`) is folded
    /// here from the key's [`crate::semantic_query::InstantiateContext`]
    /// so two instantiations differing only in `R` never warm-hit; the
    /// embedded projection mode strips into the [`ModeSlot`].
    Instantiate {
        base: crate::semantic_query::ResolvedDeclSlotIdentity,
        args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
        /// The base body's SOURCE KIND (`FileBacked(P)` / `NonFile`),
        /// folded from the key's
        /// [`crate::semantic_query::InstantiateContext`]. A file-backed
        /// base folds the live `parse_env_hash` here, so two lowerings
        /// differing only in the FileBacked `P` are DISTINCT FAMILIES —
        /// a parse-env-only change (content unchanged) is not caught by
        /// the `FileWholeHash` self-root rail and must be caught by the
        /// key. A true non-file base folds NO `P` (an unconditional `P`
        /// would false-miss every parse-env-insensitive instantiation,
        /// R21).
        body_source: crate::semantic_query::InstantiateBodySource,
        /// Surface-provenance dimension. A
        /// macro-type-argument own-body instantiation and a plain
        /// structural instantiation of the SAME decl + args produce
        /// distinct surfaces (`declared_in_macro_type_arg` differs), so
        /// they MUST NOT collide on one family slot. The mode still maps
        /// to `ModeSlot`; this keeps the provenance variants apart at
        /// the family-identity level so per-mode backfill stays within
        /// one provenance family.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
        /// Member-merge role dimension. A heritage-arm instantiation
        /// (`MemberMergeRole::Heritage`) and a structural instantiation of
        /// the SAME decl + args produce distinct surfaces (the inherited
        /// members carry distinct merge roles), so they MUST NOT collide on
        /// one family slot. Orthogonal to `provenance`.
        merge_role: crate::semantic_query::MemberMergeRole,
        /// Orthogonal Vue heritage policy. Runtime carrier unwraps use the
        /// same StructuralTransit slot as ordinary unwraps, so this
        /// value-affecting axis must remain on the family identity.
        vue_heritage_policy: VueHeritagePolicy,
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
        /// Surface-provenance dimension. Key-space reduction threads this
        /// context through nested evaluation and encoded output bits, so a
        /// macro-own-body query and a structural query over the same base must
        /// never share one memo family.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
        /// Member-merge role dimension. Heritage/own-body/authored reductions
        /// can observe different member precedence and metadata, so they are
        /// part of the family identity rather than the mode slot.
        merge_role: crate::semantic_query::MemberMergeRole,
        /// Runtime heritage filtering is value-affecting even after demand
        /// demotion to StructuralTransit.
        vue_heritage_policy: VueHeritagePolicy,
    },
    MappedType {
        source: SemanticNodeId,
        mapper: MapperKey,
        /// Surface-provenance dimension. Mapped-type materialisation evaluates
        /// mapper values/names under the full projection-reduction context, so
        /// provenance is value-affecting identity.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
        /// Member-merge role dimension. Produced members and nested reductions
        /// depend on the merge-role regime and must not warm-hit across it.
        merge_role: crate::semantic_query::MemberMergeRole,
        /// Runtime heritage filtering is value-affecting even after demand
        /// demotion to StructuralTransit.
        vue_heritage_policy: VueHeritagePolicy,
    },
    Conditional {
        check: SemanticNodeId,
        extends: SemanticNodeId,
        true_branch: SemanticNodeId,
        false_branch: SemanticNodeId,
        distributive: bool,
    },
    /// `value_root` is the env-bearing, content-free
    /// [`crate::semantic_query::ValueRootSlotIdentity`] (R6 — carries the
    /// value-root scope canonical's intrinsic `T` / `L` / `J` env, NEVER a
    /// content/version hash); version-rooting lives on each candidate's
    /// `ReadSetSignature.facts` + `self_root_canonicals` inside the
    /// multi-candidate [`FamilySlots`]. `resolve_env_hash` (`R`) is folded
    /// here from the key's [`crate::semantic_query::TypeOfContext`] so two
    /// `typeof` resolutions differing only in `R` never warm-hit; the
    /// embedded projection demand strips into the [`ModeSlot`].
    TypeOf {
        value_root: crate::semantic_query::ValueRootSlotIdentity,
        resolve_env_hash: crate::semantic_query::HashValue,
        /// Surface-provenance dimension. `build_typeof` lowers the value's
        /// annotation / shape under the full projection-reduction context,
        /// so provenance is value-affecting identity (parity with `KeyOf` /
        /// `MappedType`).
        provenance: crate::semantic_query::SurfaceProvenanceContext,
        /// Member-merge role dimension. The lowered value surface and its
        /// nested reductions depend on the merge-role regime and must not
        /// warm-hit across it.
        merge_role: crate::semantic_query::MemberMergeRole,
        /// Runtime heritage filtering is value-affecting even after demand
        /// demotion to StructuralTransit.
        vue_heritage_policy: VueHeritagePolicy,
    },
    NormalizeUnion {
        members: Arc<[SemanticNodeId]>,
    },
    NormalizeIntersection {
        members: Arc<[SemanticNodeId]>,
    },
    /// Selector-aware object-spread projection family. The payload is boxed to
    /// keep the hot `FamilyKey` enum below its size rail. Only the established
    /// projection mode/demand dimensions are erased into `ModeSlot`; every
    /// other context dimension remains exact family identity.
    ProjectObjectSpread {
        identity: Box<ObjectSpreadProjectionFamilyIdentity>,
    },
    ProjectPath {
        base: SemanticNodeId,
        path: Arc<[PathSegment]>,
        /// Surface-provenance dimension. A
        /// macro-type-argument own-body path projection (the empty-path
        /// macro-payload surface read) and a plain structural path
        /// projection of the SAME `(base, path)` produce distinct
        /// surfaces (`declared_in_macro_type_arg` differs on the
        /// expanded DeclPlaceholder's own-body members), so they MUST
        /// NOT collide on one family slot.
        provenance: crate::semantic_query::SurfaceProvenanceContext,
        /// Member-merge role dimension. The empty-path Shallow projection
        /// of a heritage carrier under `MemberMergeRole::Heritage` produces a
        /// distinct surface from a structural projection of the same
        /// `(base, path)`, so they MUST NOT collide on one family slot.
        merge_role: crate::semantic_query::MemberMergeRole,
        /// Runtime heritage filtering is value-affecting even after demand
        /// demotion to StructuralTransit.
        vue_heritage_policy: VueHeritagePolicy,
    },
    /// Mode-erased ResolveMacroPayload identity. `owner` is the
    /// env-bearing, content-free
    /// [`crate::semantic_query::ResolvedDeclSlotIdentity`] (R6 — carries
    /// the slot's intrinsic `T` / `L` / `J` env); version-rooting lives on
    /// each candidate's `ReadSetSignature.facts` + `self_root_canonicals`
    /// inside the multi-candidate [`FamilySlots`]. `resolve_env_hash`
    /// (`R`) is folded here from the key's
    /// [`crate::semantic_query::MacroPayloadContext`]; the projection mode
    /// strips into the [`ModeSlot`].
    ResolveMacroPayload {
        owner: crate::semantic_query::ResolvedDeclSlotIdentity,
        macro_index: usize,
        macro_kind: verter_semantic::analysis::AnalyzedMacroKind,
        type_args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
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
    /// family slots and never collide. The context's EXTRA env dims
    /// (`parse_env_hash` = `P`, `resolve_env_hash` = `R`; env stays ON the
    /// key) are carried here so two queries differing only in a context
    /// env-hash do NOT collide. `P` is FORWARD-DECLARED for the deferred
    /// decorator-reading reducer (design §419 `{P,R}`). These are ENV
    /// hashes, NOT content/version hashes (R6-clean). The projection mode is
    /// stripped into the [`ModeSlot`].
    ResolveClassSurface {
        decl_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        side: crate::semantic_query::ClassSurfaceSide,
        parse_env_hash: crate::semantic_query::HashValue,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// Mode-erased `ResolveAmbientNamespace` identity. Carries the
    /// namespace slot (`symbol_space = Namespace`) + type args + the
    /// context's extra env dims (`parse_env_hash` = `P`, `resolve_env_hash` =
    /// `R`; `P` is FORWARD-DECLARED for the deferred body-reading namespace-
    /// member reducer, design §414 `{P,R}`). The execute path is
    /// non-producing (returns `Opaque(Miss)`); like `Relate`, nothing is ever
    /// admitted under this family — the variant exists so `family_and_slot`
    /// stays total and honest (a real distinct identity, never a placeholder
    /// reusing another family's shape).
    ResolveAmbientNamespace {
        namespace_slot: crate::semantic_query::ResolvedDeclSlotIdentity,
        type_args: Arc<[SemanticNodeId]>,
        parse_env_hash: crate::semantic_query::HashValue,
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
    /// extra env dim (`resolve_env_hash` = `R`). LIVE producer
    /// (`build_resolve_overload_set` — the ordered visible signature
    /// group, admitted `Singleflight`); the key carries no projection
    /// context, so the whole family is mode-erased and lives in the
    /// `Single` slot.
    ResolveOverloadSet {
        callee: SemanticNodeId,
        type_args: Arc<[SemanticNodeId]>,
        resolve_env_hash: crate::semantic_query::HashValue,
    },
    /// Mode-erased terminal broad-runtime-kind classifier identity. The
    /// SUBJECT payload is BOXED for the same keyspace-size discipline as
    /// [`FamilyKey::Relate`]: `BroadRuntimeSubjectLocator` embeds the
    /// env-bearing `ResolvedDeclSlotIdentity` owner slot plus a member
    /// route, which by value would make this the largest variant and
    /// inflate EVERY entry key of the hot `FamilyKey → FamilySlots`
    /// keyspace past the u2b8 128B bound. Hash/Eq semantics are
    /// unchanged — two classifier keys differing in any subject or env
    /// axis map to distinct family identities.
    ClassifyBroadRuntime {
        subject: Box<crate::locator_identity::BroadRuntimeSubjectLocator>,
        context: crate::semantic_query::BroadRuntimeContext,
    },
    /// DEDICATED, non-aliasing `Relate` family identity carrying the FULL
    /// relation identity [`crate::semantic_query::RelateMemoKey`] (source /
    /// target / relation kind / policy / source freshness / inference context /
    /// env+substitution+projection-reduction context).
    ///
    /// This is the LIVE relation-memo family. The production relation
    /// authority is `SemanticQueryApi::execute(SemanticQueryKey::Relate)`;
    /// root execution uses the shared cooperative value path to read and
    /// publish this family, and every sub-relation re-enters that authority.
    ///
    /// It carries the FULL relation identity (NOT just source/target): a
    /// `Relate` key can NEVER collide with a live [`Self::IndexedAccess`]
    /// slot over the same `(source, target)` nodes, and two `Relate` keys
    /// differing in any relation-identity axis map to distinct family
    /// identities.
    ///
    /// The `RelateMemoKey` payload is BOXED: a Rust
    /// enum is sized to its largest variant, and `RelateMemoKey` is 144B, so
    /// embedding it BY VALUE would inflate EVERY entry key of the hot
    /// single-node `FamilyKey → FamilySlots` keyspace. `Box<RelateMemoKey>`
    /// is 8 bytes and delegates `Hash`/`Eq`/`Clone` to the inner key, so the
    /// family IDENTITY (and `variant_label`) is UNCHANGED — two `Relate`
    /// keys differing in any relation-identity axis still map to distinct
    /// family identities.
    Relate {
        key: Box<crate::semantic_query::RelateMemoKey>,
    },
    /// Mode-erased `ApparentType` identity. `ApparentType` has no slot, so
    /// its R21 env dims (`type_env_hash` = `T`, `lib_env_hash` = `L`,
    /// `project_identity` = `J`) ride here ON the family key — these are
    /// ENV hashes, NOT content/version hashes (R6-clean). Two queries
    /// differing only in any env dim do NOT collide. `demand_scope` is the
    /// rootless-callable scope witness (content-free canonical identity,
    /// R6-clean): an `Anchored` demand and a `Rootless` demand over the
    /// SAME base node are DISTINCT family identities, as are two `Rootless`
    /// demands from different canonicals — the same interned rootless node
    /// can never cross-serve between projects. LIVE producer for the
    /// CALLABLE arm only; a rootless value is built `cache_suppress` so
    /// nothing rootless is ever admitted under this family. The
    /// primitive-to-wrapper arm (the lib-member index) is unimplemented and
    /// returns `Opaque(Miss)`.
    ApparentType {
        base: SemanticNodeId,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
        demand_scope: crate::semantic_query::ApparentDemandScope,
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
    /// (`canonical_id` + `offset`) is the identity core; the per-variant
    /// [`FlowNarrowingKey`] demand axis (`flow`, FORWARD-DECLARED for the U6
    /// flow engine) and the context's full `{P, R, T, L, J}` env dims +
    /// shared `substitution` axis ride here ON the family key (env hashes,
    /// NOT content/version hashes — R6-clean; `flow`/`substitution` are
    /// content-free SHAPE-only identities). Non-producing (the flow engine
    /// lands in U6): the execute path returns `Opaque(Miss)` and nothing is
    /// ever admitted under this family; like `Relate`, the variant exists so
    /// `family_and_slot` stays total and honest.
    ///
    /// [`ProgramPointId`]: crate::semantic_query::ProgramPointId
    /// [`FlowNarrowingKey`]: crate::semantic_query::FlowNarrowingKey
    FlowNarrowingAt {
        point: crate::semantic_query::ProgramPointId,
        flow: crate::semantic_query::FlowNarrowingKey,
        parse_env_hash: crate::semantic_query::HashValue,
        resolve_env_hash: crate::semantic_query::HashValue,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
        substitution: crate::semantic_query::SubstitutionCanonicalHash,
    },
    /// Mode-erased `ContextualTypeAt` identity. The [`ProgramPointId`] is the
    /// identity core; the per-variant [`ContextualTypingKey`] demand axis
    /// (`contextual`, FORWARD-DECLARED for the U6 contextual engine) and the
    /// full `{P, R, T, L, J}` env dims + shared `substitution` axis ride here
    /// (env hashes, R6-clean; `contextual`/`substitution` are content-free
    /// SHAPE-only identities). Non-producing (the contextual engine lands in
    /// U6).
    ///
    /// [`ProgramPointId`]: crate::semantic_query::ProgramPointId
    /// [`ContextualTypingKey`]: crate::semantic_query::ContextualTypingKey
    ContextualTypeAt {
        point: crate::semantic_query::ProgramPointId,
        contextual: crate::semantic_query::ContextualTypingKey,
        parse_env_hash: crate::semantic_query::HashValue,
        resolve_env_hash: crate::semantic_query::HashValue,
        type_env_hash: crate::semantic_query::HashValue,
        lib_env_hash: crate::semantic_query::HashValue,
        project_identity: u32,
        substitution: crate::semantic_query::SubstitutionCanonicalHash,
    },
    /// Mode-erased `LowerLocator` identity. The family fields are EXACTLY
    /// the sealed [`crate::locator_identity::LocatorLoweringKey`] — `slot`
    /// (whose typed env tail carries `T` / `L` / `J`) + `locator` +
    /// `parse_env_hash` (`P`) + `resolve_env_hash` (`R`) — and NOTHING
    /// else: the key is strictly unsubstituted and carries no caller
    /// projection axis, so the whole family is mode-free and lives in the
    /// `Single` slot. Carrying the sealed key verbatim keeps the family
    /// identity anchor-match-gated by construction (a mismatched
    /// slot/locator family cannot be fabricated). A parse-env-only move on
    /// the same locator is a DISTINCT family — a parse-env change with
    /// unchanged content is not caught by the `FileWholeHash` self-root
    /// rail, so it must be caught by the key (mirrors
    /// [`Self::Instantiate`]'s `body_source` rule).
    ///
    /// The payload is BOXED (mirroring [`Self::Relate`]'s
    /// `Box<RelateMemoKey>`): a Rust enum is sized to its largest variant,
    /// and the locator key's slot + locator composites would inflate EVERY
    /// entry of the hot single-node `FamilyKey → FamilySlots` keyspace.
    /// `Box` delegates `Hash`/`Eq`/`Clone` to the inner key, so the family
    /// IDENTITY (and `variant_label`) is unchanged.
    LowerLocator {
        key: Box<crate::locator_identity::LocatorLoweringKey>,
    },
    /// Mode-erased `ClassifyMaterializationCycleGate` identity. The family
    /// fields are EXACTLY the sealed
    /// [`crate::semantic_query::MaterializationCycleGateKey`] — the
    /// env-bearing root slot (`T` / `L` / `J`) + `parse_env_hash` (`P`) +
    /// `resolve_env_hash` (`R`) — and NOTHING else: the remaining axes
    /// (empty args, `StructuralTransit`, `Skeleton`, neutral
    /// policy/provenance) are FIXED inside the producer, so the family is
    /// mode-free and lives in the `Single` slot. No content hash,
    /// generation, `DeclIdentity`, or algorithm version enters the family
    /// identity (R6); version-rooting lives on the cached value's read-set
    /// facts + observed self-roots.
    ///
    /// The payload is BOXED (mirroring [`Self::Relate`]'s
    /// `Box<RelateMemoKey>`): the root slot composite would inflate EVERY
    /// entry of the hot single-node `FamilyKey → FamilySlots` keyspace.
    ClassifyMaterializationCycleGate {
        key: Box<crate::semantic_query::MaterializationCycleGateKey>,
    },
    /// Mode-erased `FlowReturn` identity. The family fields are EXACTLY
    /// the full [`crate::semantic_query::FlowReturnKey`] — function slot
    /// identity (declaration slot + part + overload ordinal), normalized
    /// type args, and the env/substitution/policy context — and NOTHING
    /// else: no demand path, projection mode, content hash, generation,
    /// or budget (R6); version-rooting lives on the cached value's
    /// `ProgramAnalysisFactRef::FlowBody` fact + consumed subquery facts
    /// + self roots.
    ///
    /// BOXED (mirroring [`Self::Relate`]): the key composite would
    /// inflate EVERY entry of the hot keyspace.
    FlowReturn {
        key: Box<crate::semantic_query::FlowReturnKey>,
    },
    /// Mode-erased `ResolveCall` identity. The family fields are EXACTLY
    /// the full [`crate::semantic_query::ResolveCallKey`] — call-site
    /// program point, callee / receiver / argument nodes, call-vs-construct
    /// kind, explicit type args, the sealed-empty flow axis, and the env +
    /// substitution context — and NOTHING else: no freshness field, no
    /// contextual / candidate-target axis, no budget, no content hash (R6);
    /// version-rooting lives on the cached value's read-set facts + self
    /// roots. The applicability executor is live, but its winner remains
    /// staged/ReturnOnly until the return-equation boundary commits it; the
    /// family identity is already final so `family_and_slot` stays total.
    ///
    /// BOXED (mirroring [`Self::Relate`]): the key composite would
    /// inflate EVERY entry of the hot keyspace.
    ResolveCall {
        key: Box<crate::semantic_query::ResolveCallKey>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(super) struct ObjectSpreadProjectionFamilyIdentity {
    program: SemanticNodeId,
    selector: crate::semantic_query::ObjectProjectionSelector,
    resolve_env_hash: crate::semantic_query::HashValue,
    type_env_hash: crate::semantic_query::HashValue,
    lib_env_hash: crate::semantic_query::HashValue,
    project_identity: crate::semantic_query::HashValue,
    substitution: crate::semantic_query::SubstitutionCanonicalHash,
    optional_property_policy: crate::semantic_query::ExactOptionalPropertyPolicy,
    provenance: crate::semantic_query::SurfaceProvenanceContext,
    merge_role: crate::semantic_query::MemberMergeRole,
    vue_heritage_policy: VueHeritagePolicy,
}

// Family-side R6 witness. `program`/`selector` are already-lowered semantic
// identities; the four raw hashes are named env dimensions. No content/version
// hash or parse-env dimension can be added without updating this exhaustive
// no-`..` destructure.
const _: fn(&ObjectSpreadProjectionFamilyIdentity) = w_object_spread_projection_family_identity;

#[allow(dead_code)]
fn w_object_spread_projection_family_identity(identity: &ObjectSpreadProjectionFamilyIdentity) {
    let ObjectSpreadProjectionFamilyIdentity {
        program,
        selector,
        resolve_env_hash,
        type_env_hash,
        lib_env_hash,
        project_identity,
        substitution,
        optional_property_policy,
        provenance,
        merge_role,
        vue_heritage_policy,
    } = identity;
    let _: &SemanticNodeId = program;
    let _: &crate::semantic_query::ObjectProjectionSelector = selector;
    let _: &crate::semantic_query::HashValue = resolve_env_hash;
    let _: &crate::semantic_query::HashValue = type_env_hash;
    let _: &crate::semantic_query::HashValue = lib_env_hash;
    let _: &crate::semantic_query::HashValue = project_identity;
    let _: &crate::semantic_query::SubstitutionCanonicalHash = substitution;
    let _: &crate::semantic_query::ExactOptionalPropertyPolicy = optional_property_policy;
    let _: &crate::semantic_query::SurfaceProvenanceContext = provenance;
    let _: &crate::semantic_query::MemberMergeRole = merge_role;
    let _: &VueHeritagePolicy = vue_heritage_policy;
}

impl FamilyKey {
    /// The stable variant label of this family identity. Used by the family-
    /// mapping guards (via the `for_tests` probe) to assert the domain a
    /// [`SemanticQueryKey`] maps to — e.g. that `Relate` maps to the dedicated
    /// `Relate` family and never aliases `IndexedAccess` — without exposing the
    /// `pub(super)` taxonomy outside the crate.
    #[cfg(any(test, feature = "test-support"))]
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
            FamilyKey::ProjectObjectSpread { .. } => "ProjectObjectSpread",
            FamilyKey::ProjectPath { .. } => "ProjectPath",
            FamilyKey::ResolveMacroPayload { .. } => "ResolveMacroPayload",
            FamilyKey::ResolveClassSurface { .. } => "ResolveClassSurface",
            FamilyKey::ResolveAmbientNamespace { .. } => "ResolveAmbientNamespace",
            FamilyKey::ResolveEnum { .. } => "ResolveEnum",
            FamilyKey::ResolveOverloadSet { .. } => "ResolveOverloadSet",
            FamilyKey::ClassifyBroadRuntime { .. } => "ClassifyBroadRuntime",
            FamilyKey::Relate { .. } => "Relate",
            FamilyKey::ApparentType { .. } => "ApparentType",
            FamilyKey::TemplateLiteralReduce { .. } => "TemplateLiteralReduce",
            FamilyKey::FlowNarrowingAt { .. } => "FlowNarrowingAt",
            FamilyKey::ContextualTypeAt { .. } => "ContextualTypeAt",
            FamilyKey::LowerLocator { .. } => "LowerLocator",
            FamilyKey::ClassifyMaterializationCycleGate { .. } => {
                "ClassifyMaterializationCycleGate"
            }
            FamilyKey::FlowReturn { .. } => "FlowReturn",
            FamilyKey::ResolveCall { .. } => "ResolveCall",
        }
    }

    /// Per-family bounded-retention candidate cap
    /// (`U3.ADAPTIVE_FAMILY_RETENTION`). Exhaustive and wildcard-free by
    /// contract: every family declares its own cap.
    ///
    /// The FLOOR is 4 — every family retains at least four concurrent
    /// view variants of one content-free identity. The LIVE
    /// inference/substitution-heavy families — `Instantiate`, `TypeOf`,
    /// `Conditional`, `MappedType` — receive a HIGHER cap (8): one
    /// content-free identity of those families legitimately coexists
    /// across many live substitution / inference-context / overlay
    /// variants, so the floor would thrash a hot inference loop.
    /// Content-light projection and modeless families — including the
    /// live `Relate` family (one content-free relation identity coexists
    /// across few view variants) — and every
    /// non-producing variant (nothing is ever admitted under
    /// `ApparentType` / `FlowNarrowingAt` / `ContextualTypeAt` /
    /// `ResolveAmbientNamespace` / `ResolveEnum`), keep the floor.
    ///
    /// This is the FAMILY-LOCAL bound only. The process-wide candidate
    /// memory ceiling and the typed non-admission (`ReturnOnly`) path are
    /// full-`U3.CACHE_FACT_MODEL` work, deliberately NOT modelled here:
    /// cacheability never depends on memory pressure, and a new cacheable
    /// candidate is ALWAYS admitted after local eviction.
    pub(super) fn candidate_cap(&self) -> usize {
        match self {
            FamilyKey::Instantiate { .. }
            | FamilyKey::TypeOf { .. }
            | FamilyKey::Conditional { .. }
            | FamilyKey::MappedType { .. }
            | FamilyKey::ProjectObjectSpread { .. }
            | FamilyKey::FlowReturn { .. } => 8,
            FamilyKey::ResolveDecl(_) => 4,
            FamilyKey::ProjectMember { .. } => 4,
            FamilyKey::IndexedAccess { .. } => 4,
            FamilyKey::KeyOf { .. } => 4,
            FamilyKey::NormalizeUnion { .. } => 4,
            FamilyKey::NormalizeIntersection { .. } => 4,
            FamilyKey::ProjectPath { .. } => 4,
            FamilyKey::ResolveMacroPayload { .. } => 4,
            FamilyKey::ResolveClassSurface { .. } => 4,
            FamilyKey::ResolveAmbientNamespace { .. } => 4,
            FamilyKey::ResolveEnum { .. } => 4,
            FamilyKey::ResolveOverloadSet { .. } => 4,
            FamilyKey::ClassifyBroadRuntime { .. } => 4,
            FamilyKey::Relate { .. } => 4,
            FamilyKey::ApparentType { .. } => 4,
            FamilyKey::TemplateLiteralReduce { .. } => 4,
            FamilyKey::FlowNarrowingAt { .. } => 4,
            FamilyKey::ContextualTypeAt { .. } => 4,
            FamilyKey::LowerLocator { .. } => 4,
            FamilyKey::ClassifyMaterializationCycleGate { .. } => 4,
            FamilyKey::ResolveCall { .. } => 4,
        }
    }
}

/// Per-family slot selector. For non-mode variants only `Single` is used;
/// for mode-bearing variants one of `Identity` / `Navigate` / `Shallow` /
/// `Expanded` is selected from the key's `ProjectionMode`.
///
/// Demand-driven reducer spec: the `Instantiate` / `KeyOf` /
/// `MappedType` families carry a [`ProjectionReductionContext`] in
/// their key, not just a `ProjectionMode`. Their slots are picked from
/// the `TransitShallow` / `TransitNavigate` / `TransitIdentity` /
/// `TransitExpanded` / `TransitSkeleton` set whenever the context's
/// `demand` is `StructuralTransit`, keeping transit results from
/// colliding with `Published` results on the same node.
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
    /// demand-driven reducer spec. Distinct from the
    /// publication slots; do NOT backfill the publication slots and
    /// are not backfilled by them.
    TransitIdentity,
    TransitNavigate,
    TransitShallow,
    TransitExpanded,
    /// `StructuralTransit` mirror of the `Skeleton` slot. The genuine
    /// Skeleton probe executors (the BFS cycle-guard probe and the
    /// slot-param symbolic probe) run under `StructuralTransit(Skeleton)`
    /// with a builtin-gate exemption that MATERIALIZES builtin bodies;
    /// `Published(Skeleton)` is wire-reachable (projection mode 4). The
    /// warm gates are demand-blind, so this dedicated slot is what keeps
    /// the probe results from warm-serving a published-Skeleton read.
    /// Like `Skeleton`, it never backfills and is never backfilled.
    TransitSkeleton,
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
    /// Vue runtime props/emits object-surface publication slot. This evaluates
    /// at Shallow depth like [`Self::MacroSurfaceShallow`], but applies typed
    /// `@vue-ignore` heritage suppression and therefore never shares or
    /// backfills the unfiltered macro slot.
    VueRuntimeSurfaceShallow,
}

/// SmallVec inline capacity for the per-slot candidate list — a
/// STORAGE optimization only, NOT the retention policy. The semantic
/// candidate cap is per-family ([`FamilyKey::candidate_cap`]); a slot
/// whose family cap exceeds the inline capacity spills to the heap.
pub(super) const CANDIDATE_LIST_INLINE_CAP: usize = 4;

/// Per-slot candidate list (semantic cap: [`FamilyKey::candidate_cap`]).
///
/// Slot order IS the LRU order: the least-recently admitted /
/// validated-hit candidate sits at the front (index 0). A publish
/// appends at the back; a validated warm hit promotes its candidate to
/// the back ([`FamilySlots::mark_validated_freshest`]). Eviction at the
/// family cap is invalid-first, then the front of this order — see
/// [`FamilySlots::publish_one`] and [`select_eviction_victim`].
pub(super) type CandidateList = smallvec::SmallVec<[MemoEntry; CANDIDATE_LIST_INLINE_CAP]>;

/// The eviction victim a publish site selected for a slot at its
/// family cap. Computed by [`select_eviction_victim`] OUTSIDE the
/// `entries` mutex (never validate fact rails while holding `entries`)
/// and applied by [`FamilySlots::publish_one`] under it.
pub(super) enum EvictionVictim {
    /// Evict the candidate carrying this `admission_seq` — the FIRST
    /// candidate (front-to-back LRU order) whose fact rail failed
    /// validation against the publishing caller's stable store view.
    /// [`FamilySlots::publish_one`] rechecks the candidate's identity
    /// under the lock and falls back to the LRU front when the seq no
    /// longer resolves (a concurrent invalidation drained it first).
    Invalid(u64),
    /// Every candidate validated against the caller's view (or no view
    /// was threaded — the legacy test publishes): evict the front of
    /// the LRU order, the least-recently validated-hit candidate.
    LruFront,
}

/// Invalid-first victim selection over a slot SNAPSHOT. The publish
/// site calls this AFTER releasing the `entries` mutex
/// (snapshot/validate/reacquire): returns the first candidate whose
/// `ReadSetSignature` fact rail fails validation against the publishing
/// caller's stable store view, else [`EvictionVictim::LruFront`]. An
/// invalid candidate can never warm-hit, so evicting it ahead of any
/// still-valid candidate is pure win.
pub(super) fn select_eviction_victim(
    candidates: &CandidateList,
    ctx: &dyn crate::resolver_core::ResolverContext,
) -> EvictionVictim {
    // Reached only when the slot is at its family cap and a publish must
    // displace a candidate, so this counts genuine cap pressure.
    verter_audit::attribute!(FamilyCandidateEvict);
    candidates
        .iter()
        .find(|candidate| !candidate.validate(ctx))
        .map_or(EvictionVictim::LruFront, |candidate| {
            EvictionVictim::Invalid(candidate.admission_seq)
        })
}

/// Eviction planning for a family-slot publish
/// (`U3.ADAPTIVE_FAMILY_RETENTION` per-family bounded retention).
///
/// Probes the PRIMARY slot under a SHORT `entries` hold: when the
/// incoming `entry` is a new discriminant and the slot already sits at
/// the family cap `cap`, the publish will evict — so snapshot the
/// candidate list, RELEASE the lock, and validate each candidate
/// against the publishing caller's stable store view
/// ([`select_eviction_victim`]), picking the first INVALID candidate
/// (front-to-back LRU order). Validation NEVER runs while holding
/// `entries` — the fact-rail walk against the resolver store view is
/// reentrant work that must not serialise unrelated warm reads and cold
/// publishes on the single global memo mutex (mirrors the warm-read
/// snapshot/validate/reacquire discipline). The publish sites re-check
/// their abort fences under the publish lock AFTER this plan, so an
/// invalidation racing the plan is still caught. The returned victim is
/// applied under the publish lock with an `admission_seq` identity
/// recheck, falling back to the LRU front; with no victim needed
/// (in-place replacement or below cap) this is a pure read. Lives in
/// `family.rs` (not on `SemanticGraphStore`) so the post-split
/// `mod.rs` stays under its line budget; takes the raw mutex and holds
/// it only for the probe — same brief-hold discipline as the warm-hit
/// fast path.
pub(super) fn plan_family_slot_eviction(
    entries: &Mutex<FxHashMap<FamilyKey, FamilySlots>>,
    family: &FamilyKey,
    slot: ModeSlot,
    entry: &MemoEntry,
    cap: usize,
    ctx: &dyn crate::resolver_core::ResolverContext,
) -> EvictionVictim {
    let snapshot: Option<CandidateList> = {
        let entries = entries.lock();
        entries.get(family).and_then(|slots| {
            if slots.primary_slot_needs_victim(slot, entry, cap) {
                Some(slots.snapshot_slot(slot))
            } else {
                None
            }
        })
    };
    match snapshot {
        Some(candidates) => select_eviction_victim(&candidates, ctx),
        None => EvictionVictim::LruFront,
    }
}

/// Outcome of [`FamilySlots::publish`]: the list of slots this publish
/// populated PLUS the candidates that were displaced during the
/// publish (same-discriminant replacements + per-family bounded-retention
/// cap-evictions). The caller drains each displaced candidate's
/// reverse-index registrations by its `admission_seq` so a surviving
/// sibling candidate in the same slot keeps its own seq registrations.
pub(super) struct FamilyPublishOutcome {
    pub(super) populated: smallvec::SmallVec<[ModeSlot; 6]>,
    pub(super) displaced: smallvec::SmallVec<[(ModeSlot, MemoEntry); 4]>,
}

/// Per-family per-slot warm storage.
///
/// Each slot independently holds an ORDERED LIST of [`MemoEntry`]
/// candidates (bounded at the family's [`FamilyKey::candidate_cap`]) —
/// see [`CandidateList`]. Each candidate carries its own
/// `read_set_signature` + `self_root_canonicals`, so two file-content
/// versions of the SAME content-free `SemanticQueryKey` (e.g.
/// `Instantiate { base: ResolvedDeclSlotIdentity { .. }, .. }` under a
/// base view and an overlay view) coexist as distinct candidates
/// inside the same slot — R20 overlay isolation.
///
/// Validity is decided EXCLUSIVELY by per-candidate
/// `read_set_signature.validate_with_self_roots`. The
/// `validated_at_generation` metadata is recency only.
///
/// Backfill on completion clones a successful broader-projection
/// compute into every EMPTY projection-depth-narrower sibling slot, but
/// ONLY when one of the broader entry's recorded materialised points
/// `cached_satisfies` the narrower slot's requested point — directional
/// AND gated, never by enum rank (see [`FamilySlots::publish`] and
/// [`slot_domain_siblings`]).
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
    /// `StructuralTransit` slot mirrors of the publication slots —
    /// demand-driven reducer spec. Independent from the publication slots. Transit
    /// backfill shares the SAME directional candidate ordering as the
    /// publication slots (`TransitExpanded → TransitShallow →
    /// TransitNavigate → TransitIdentity`), but every clone into a narrower
    /// transit sibling is `cached_satisfies`-gated exactly as the
    /// publication slots are — so e.g. a `TransitShallow → TransitNavigate`
    /// candidate is only a CANDIDATE and is REJECTED by the gate
    /// (`Shallow ⊅ Navigate`). It is never an unconditional enum-rank
    /// fan-out.
    transit_identity: CandidateList,
    transit_navigate: CandidateList,
    transit_shallow: CandidateList,
    transit_expanded: CandidateList,
    /// `StructuralTransit` mirror of the `skeleton` slot — the probe
    /// executors' demand. Like `skeleton`, independent from every other
    /// slot; no backfill in either direction.
    transit_skeleton: CandidateList,
    /// Vue macro object-surface publication slot. Independent of
    /// the publication + transit slots; no backfill in either direction.
    macro_surface_shallow: CandidateList,
    /// Vue runtime props/emits object-surface slot. Independent of the
    /// unfiltered macro surface so ignored heritage cannot cross-serve TSC or
    /// component-meta demand.
    vue_runtime_surface_shallow: CandidateList,
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
            ModeSlot::TransitSkeleton => &self.transit_skeleton,
            ModeSlot::MacroSurfaceShallow => &self.macro_surface_shallow,
            ModeSlot::VueRuntimeSurfaceShallow => &self.vue_runtime_surface_shallow,
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
            ModeSlot::TransitSkeleton => &mut self.transit_skeleton,
            ModeSlot::MacroSurfaceShallow => &mut self.macro_surface_shallow,
            ModeSlot::VueRuntimeSurfaceShallow => &mut self.vue_runtime_surface_shallow,
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
    /// during validation, which the multi-candidate substrate makes
    /// worse.
    pub(super) fn snapshot_slot(&self, slot: ModeSlot) -> CandidateList {
        self.slot_list(slot).clone()
    }

    /// Move the candidate matching `(validated_at_generation, facts)`
    /// to the back of `slot`'s LRU order — the bookkeeping the
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
    /// Moves the matching candidate to the back of the LRU order so
    /// the bounded-retention eviction policy treats it as freshest.
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

    /// Would publishing `entry` into `slot` require evicting a resident
    /// candidate? True iff `entry` is a NEW discriminant (not an
    /// in-place same-view replacement) and the slot already sits at the
    /// family cap `cap`. The publish site evaluates this UNDER the
    /// `entries` lock to decide whether eviction planning must snapshot
    /// the candidate list and validate it outside the lock
    /// ([`select_eviction_victim`]).
    pub(super) fn primary_slot_needs_victim(
        &self,
        slot: ModeSlot,
        entry: &MemoEntry,
        cap: usize,
    ) -> bool {
        let list = self.slot_list(slot);
        list.len() >= cap && !list.iter().any(|c| candidate_same_discriminant(c, entry))
    }

    /// Publish `entry` into `slot` and backfill every narrower slot
    /// whose candidate list is currently empty.
    ///
    /// Admission policy in the PRIMARY slot (`U3.ADAPTIVE_FAMILY_RETENTION`
    /// per-family bounded retention):
    /// - Same exact `(validated_at_generation, facts)` discriminant ⇒
    ///   replace in place (move to back; same-view re-publish) and
    ///   become freshest.
    /// - Different discriminant ⇒ ALWAYS admitted after local eviction:
    ///   at the family cap `cap`, evict `victim` FIRST (an
    ///   invalid-against-the-publishing-view candidate selected by
    ///   [`select_eviction_victim`] outside the `entries` lock), else
    ///   the front of the LRU order (the least-recently validated-hit
    ///   candidate), then append at the back.
    ///
    /// §3.4 **recorded-point backfill** into a projection-depth-narrower
    /// target slot: the broader compute's entry is cloned UNCHANGED into
    /// an EMPTY narrower slot ONLY when a recorded point in its
    /// `satisfied_projection` dominates that slot's requested point
    /// (`cached_satisfies`) — NEVER by enum rank alone, NEVER synthesising
    /// a target-slot point. `requested_path` is the projection path of the
    /// owning family (empty for non-path families); the target slot's
    /// requested point is `point_for_slot(target, requested_path)`. A
    /// narrower compute that wrote first survives (backfill writes only
    /// into an empty slot, so backfill never reaches the cap path).
    ///
    /// Returns the list of slots this publish populated AND the
    /// candidates that the publish displaced (same-discriminant
    /// replacements + per-family bounded-retention cap-eviction
    /// victims). The caller drains each displaced candidate's
    /// reverse-index registrations under its own admission_seq so a
    /// sibling candidate in the same slot keeps its registrations.
    pub(super) fn publish(
        &mut self,
        slot: ModeSlot,
        entry: MemoEntry,
        requested_path: &ProjectionPath,
        cap: usize,
        victim: EvictionVictim,
    ) -> FamilyPublishOutcome {
        let mut populated = smallvec::SmallVec::<[ModeSlot; 6]>::new();
        let mut displaced: smallvec::SmallVec<[(ModeSlot, MemoEntry); 4]> =
            smallvec::SmallVec::new();
        for evicted in self.publish_one(slot, entry.clone(), cap, victim) {
            displaced.push((slot, evicted));
        }
        populated.push(slot);
        for target in slot_domain_siblings(slot) {
            if !self.slot_list(*target).is_empty() {
                continue;
            }
            // §3.4 gate: the broader compute's recorded materialised set
            // must dominate the narrower target slot's requested point.
            // Enum rank alone is NOT sufficient — e.g. a `Shallow` record
            // does NOT satisfy a `Navigate` request (`Shallow ⊅ Navigate`:
            // `normalization_depth None < NavigateOnly`), so the legacy
            // `Shallow → Navigate` clone is rejected here.
            let target_point = MaterializedPoint::new(point_for_slot(*target, requested_path));
            if !cached_satisfies(&entry.satisfied_projection, &target_point) {
                continue;
            }
            for evicted in self.publish_one(*target, entry.clone(), cap, EvictionVictim::LruFront) {
                displaced.push((*target, evicted));
            }
            populated.push(*target);
        }
        FamilyPublishOutcome {
            populated,
            displaced,
        }
    }

    /// Internal: admit `entry` into a single slot's candidate list,
    /// applying the same-discriminant-replace / bounded-retention-evict
    /// rules. Returns the candidates that were displaced (replaced or
    /// evicted) — the caller drains their reverse-index registrations
    /// by per-candidate `admission_seq` so a surviving sibling
    /// candidate in the same slot keeps its own seq registrations.
    fn publish_one(
        &mut self,
        slot: ModeSlot,
        entry: MemoEntry,
        cap: usize,
        victim: EvictionVictim,
    ) -> smallvec::SmallVec<[MemoEntry; 2]> {
        let mut displaced: smallvec::SmallVec<[MemoEntry; 2]> = smallvec::SmallVec::new();
        let list = self.slot_list_mut(slot);
        if let Some(pos) = list
            .iter()
            .position(|c| candidate_same_discriminant(c, &entry))
        {
            // Same view re-publish: remove the previous candidate and
            // append the new one so it becomes the freshest in the LRU
            // order. A cap eviction then drops the least-recently
            // validated unrelated candidate, not the just-replaced one.
            // The displaced candidate's reverse-index registrations
            // (keyed under its own admission_seq) are orphan stamps
            // until the caller drains them.
            displaced.push(list.remove(pos));
            list.push(entry);
        } else {
            // Different view: make room BEFORE appending so the new
            // candidate is ALWAYS admitted after local eviction (never
            // dropped by the cap, never admission-gated). The FIRST
            // eviction honours the publish site's pre-selected `victim`
            // — an invalid-against-the-publishing-view candidate chosen
            // outside the `entries` lock — rechecked HERE by
            // `admission_seq` identity: if the seq no longer resolves
            // (a concurrent invalidation drained it between the
            // snapshot and this lock hold), fall back to the LRU front.
            // Any further iteration evicts the front of the LRU order
            // (the least-recently validated-hit candidate).
            let mut victim = victim;
            while list.len() >= cap {
                let index = match victim {
                    EvictionVictim::Invalid(seq) => list
                        .iter()
                        .position(|c| c.admission_seq == seq)
                        .unwrap_or(0),
                    EvictionVictim::LruFront => 0,
                };
                displaced.push(list.remove(index));
                victim = EvictionVictim::LruFront;
            }
            list.push(entry);
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
            &self.transit_skeleton,
            &self.macro_surface_shallow,
            &self.vue_runtime_surface_shallow,
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
    /// **By design — single-representative shape.** With the
    /// multi-candidate substrate a slot may hold several candidates
    /// (bounded at the family's [`FamilyKey::candidate_cap`]), but this
    /// audit row format yields ONE row per populated slot to preserve
    /// the legacy corpus-snapshot shape so existing audit fixtures stay
    /// stable. Tooling that needs the full per-candidate
    /// enumeration uses [`Self::iter_populated_slots_all`] (drain /
    /// reverse-index sweep paths), not this audit dump. The chosen
    /// representative is the FIRST candidate in the list — the front of
    /// the LRU order (the slot's validated-hit move-to-back operation
    /// reorders subsequent reads, so a recently validated candidate is
    /// at the back of the list, not the front).
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
        if let Some(e) = self.transit_skeleton.first() {
            out.push(("transit_skeleton", e));
        }
        if let Some(e) = self.macro_surface_shallow.first() {
            out.push(("macro_surface_shallow", e));
        }
        if let Some(e) = self.vue_runtime_surface_shallow.first() {
            out.push(("vue_runtime_surface_shallow", e));
        }
        out
    }

    /// Candidate count in a specific slot. Exposed for test probes
    /// (`SemanticGraphStore::slot_candidate_count_for_tests`); the
    /// integration tests use it to verify multi-candidate
    /// coexistence and per-family bounded retention. Cheap O(1) read.
    pub(super) fn slot_candidate_count_for_test(&self, slot: ModeSlot) -> usize {
        self.slot_list(slot).len()
    }

    /// The slot's candidate `validated_at_generation` stamps in slot
    /// order (front = least-recently admitted / validated-hit). Exposed
    /// for test probes
    /// (`SemanticGraphStore::slot_candidate_generations_for_tests`); the
    /// per-family bounded-retention guards use it to assert
    /// survivor/victim identity and LRU order.
    pub(super) fn slot_candidate_generations_for_test(&self, slot: ModeSlot) -> Vec<u64> {
        self.slot_list(slot)
            .iter()
            .map(|candidate| candidate.validated_at_generation)
            .collect()
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
        for e in &self.transit_skeleton {
            out.push((ModeSlot::TransitSkeleton, e));
        }
        for e in &self.macro_surface_shallow {
            out.push((ModeSlot::MacroSurfaceShallow, e));
        }
        for e in &self.vue_runtime_surface_shallow {
            out.push((ModeSlot::VueRuntimeSurfaceShallow, e));
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

/// The candidate backfill TARGET slots a publish into `slot` may fill —
/// the PROJECTION-DEPTH-narrower slots in the same domain. The ACTUAL
/// backfill of each is then gated by `cached_satisfies` in
/// [`FamilySlots::publish`]; this is only the candidate set.
///
/// The target set is DIRECTIONAL (broader-projection → narrower-projection
/// only), the same direction as the legacy `backfill_targets` enum-rank
/// fan-out (`Expanded → Shallow → Navigate → Identity`). The §3.4 change
/// is NOT the direction but the GATE: each candidate is backfilled ONLY
/// when a recorded materialised point in the publishing entry's
/// `satisfied_projection` `cached_satisfies` the candidate's requested
/// point. So the legacy `Shallow → Navigate` clone is now REJECTED
/// (`Shallow ⊅ Navigate` — `normalization_depth: None < NavigateOnly`),
/// while `Expanded → {Shallow, Navigate, Identity}` and
/// `Shallow/Navigate → Identity` survive the gate.
///
/// **Why directional, not the full lattice-dominance peer set.** The
/// landed lattice ALSO has `Navigate ⊒ Shallow` (Navigate's higher
/// normalization/operator rungs dominate Shallow's), so a naive
/// all-peers-gated backfill would clone a `Navigate` result into the
/// `Shallow` slot. That is OPERATIONALLY UNSOUND: `Navigate` is the
/// intermediate next-hop demand (it carrier-stops / does NOT materialise a
/// one-shell surface), so serving a `Shallow` surface request from a
/// `Navigate` result returns an under-materialised surface — e.g. it hides
/// a cyclic-heritage expansion the Shallow request would have surfaced.
/// Backfill therefore only ever flows toward strictly-shallower projection
/// depth; the gate prunes the unsound enum-rank cases within that
/// direction. The directional rule is pinned by
/// `super::tests::navigate_compute_does_not_serve_or_backfill_shallow_request`
/// (a Navigate cold build leaves the Shallow slot empty and a Shallow
/// request misses, while the narrower Identity slot is backfilled); the
/// no-sub-slot-mode-terminal soundness invariant it relies on is pinned by
/// the `warm_publish_one` `debug_assert!` plus
/// `super::tests::warm_publish_one_debug_asserts_against_sub_slot_mode_terminal`.
///
/// `Skeleton`, `TransitSkeleton`, `MacroSurfaceShallow`,
/// `VueRuntimeSurfaceShallow`, and `Single` are independent evaluations with
/// no backfill in either direction.
pub(super) fn slot_domain_siblings(slot: ModeSlot) -> &'static [ModeSlot] {
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
        ModeSlot::TransitSkeleton => &[],
        ModeSlot::MacroSurfaceShallow => &[],
        ModeSlot::VueRuntimeSurfaceShallow => &[],
    }
}

/// The [`ProjectionMode`] a [`ModeSlot`] stores results for — the inverse
/// of [`mode_to_slot`] / [`context_to_slot`] for the purpose of building
/// the slot's requested [`Demand`] point. The `Transit*` mirrors map to
/// the same mode as their publication peer; `MacroSurfaceShallow` is a
/// Shallow surface; `Single` has no mode.
fn mode_of_slot(slot: ModeSlot) -> Option<ProjectionMode> {
    match slot {
        ModeSlot::Identity | ModeSlot::TransitIdentity => Some(ProjectionMode::Identity),
        ModeSlot::Navigate | ModeSlot::TransitNavigate => Some(ProjectionMode::Navigate),
        ModeSlot::Shallow
        | ModeSlot::TransitShallow
        | ModeSlot::MacroSurfaceShallow
        | ModeSlot::VueRuntimeSurfaceShallow => Some(ProjectionMode::Shallow),
        ModeSlot::Expanded | ModeSlot::TransitExpanded => Some(ProjectionMode::Expanded),
        ModeSlot::Skeleton | ModeSlot::TransitSkeleton => Some(ProjectionMode::Skeleton),
        // Modeless families (`ResolveDecl` / `Conditional` /
        // `Normalize*` / …) carry no projection demand; their satisfaction
        // is decided purely by `validate_with_self_roots`. Represent their
        // point as the regime-`⊥` `Demand::identity()` at the empty path so
        // the recorded point and the requested point coincide (trivial
        // `cached_satisfies` pass — the gate never blocks a modeless hit).
        ModeSlot::Single => None,
    }
}

/// The [`Demand`] point a request targeting `slot` at `path` denotes —
/// the slot's mode preset with `projection.path = path`. Modeless
/// (`Single`) slots use `Demand::identity()` at `path`. Shared by the
/// warm-hit gate (`requested_point_for_key`) and the recorded-point
/// backfill gate in [`FamilySlots::publish`].
pub(super) fn point_for_slot(slot: ModeSlot, path: &ProjectionPath) -> Demand {
    let mut demand = match mode_of_slot(slot) {
        Some(mode) => Demand::from(mode),
        None => Demand::identity(),
    };
    demand.projection.path = path.clone();
    demand
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
/// use the `Transit*` mirrors. The unfiltered and runtime-filtered Vue macro
/// demands use separate shallow slots because their heritage policies differ.
pub(super) fn context_to_slot(ctx: ProjectionReductionContext) -> ModeSlot {
    match ctx.demand {
        ReductionDemand::Published => mode_to_slot(ctx.mode),
        ReductionDemand::StructuralTransit => match ctx.mode {
            ProjectionMode::Identity => ModeSlot::TransitIdentity,
            ProjectionMode::Navigate => ModeSlot::TransitNavigate,
            ProjectionMode::Shallow => ModeSlot::TransitShallow,
            ProjectionMode::Expanded => ModeSlot::TransitExpanded,
            // Skeleton mirrors the other transit modes: the probe
            // executors' `StructuralTransit(Skeleton)` results (which
            // materialize builtin bodies under the builtin-gate
            // exemption) must never share a slot with the wire-reachable
            // `Published(Skeleton)` demand — the warm gates are
            // demand-blind, so the slot split is the isolation.
            ProjectionMode::Skeleton => ModeSlot::TransitSkeleton,
        },
        // Vue macro object-surface publication. The macro
        // publication boundary always runs at Shallow mode (the
        // empty-path terminal-surface synthesis is where the union-arm
        // rule applies), so all modes land in the dedicated
        // `MacroSurfaceShallow` slot — distinct from the `Published`
        // slots so the union surface never collides with an ordinary
        // `Published` read of the same node.
        ReductionDemand::MacroObjectSurface => ModeSlot::MacroSurfaceShallow,
        // Runtime props/emits uses the same shallow union surface as the
        // ordinary macro demand, but applies typed `@vue-ignore` filtering.
        // Keeping it isolated prevents a filtered value from warm-serving a
        // TSC/component-meta query (and vice versa).
        ReductionDemand::VueRuntimeObjectSurface => ModeSlot::VueRuntimeSurfaceShallow,
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
        SemanticQueryKey::Instantiate(k) => {
            let prc = k.projection_reduction();
            (
                FamilyKey::Instantiate {
                    base: k.base().clone(),
                    args: Arc::clone(k.args()),
                    resolve_env_hash: k.resolve_env_hash(),
                    body_source: k.body_source(),
                    provenance: prc.provenance,
                    merge_role: prc.merge_role,
                    vue_heritage_policy: prc.vue_heritage_policy,
                },
                context_to_slot(prc),
            )
        }
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
        SemanticQueryKey::KeyOf { base, context } => (
            FamilyKey::KeyOf {
                base: *base,
                provenance: context.provenance,
                merge_role: context.merge_role,
                vue_heritage_policy: context.vue_heritage_policy,
            },
            context_to_slot(*context),
        ),
        SemanticQueryKey::MappedType {
            source,
            mapper,
            context,
        } => (
            FamilyKey::MappedType {
                source: *source,
                mapper: mapper.clone(),
                provenance: context.provenance,
                merge_role: context.merge_role,
                vue_heritage_policy: context.vue_heritage_policy,
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
        SemanticQueryKey::TypeOf {
            value_root,
            context,
        } => (
            FamilyKey::TypeOf {
                value_root: value_root.clone(),
                resolve_env_hash: context.resolve_env_hash,
                provenance: context.projection_reduction.provenance,
                merge_role: context.projection_reduction.merge_role,
                vue_heritage_policy: context.projection_reduction.vue_heritage_policy,
            },
            context_to_slot(context.projection_reduction),
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
        SemanticQueryKey::ProjectObjectSpread {
            program,
            selector,
            context,
        } => {
            let projection = context.projection_reduction();
            (
                FamilyKey::ProjectObjectSpread {
                    identity: Box::new(ObjectSpreadProjectionFamilyIdentity {
                        program: *program,
                        selector: selector.clone(),
                        resolve_env_hash: context.resolve_env_hash(),
                        type_env_hash: context.type_env_hash(),
                        lib_env_hash: context.lib_env_hash(),
                        project_identity: context.project_identity(),
                        substitution: context.substitution(),
                        optional_property_policy: context.optional_property_policy(),
                        provenance: projection.provenance,
                        merge_role: projection.merge_role,
                        vue_heritage_policy: projection.vue_heritage_policy,
                    }),
                },
                context_to_slot(projection),
            )
        }
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
                vue_heritage_policy: context.vue_heritage_policy,
            },
            context_to_slot(*context),
        ),
        // `Relate` maps to a DEDICATED, non-aliasing `FamilyKey::Relate`
        // carrying the FULL relation identity. Production relation roots enter
        // this arm through `SemanticQueryApi::execute(Relate)` and the shared
        // cooperative value path; sub-relations re-enter the same authority.
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
                key: Box::new(crate::semantic_query::RelateMemoKey {
                    source: *source,
                    target: *target,
                    relation: *relation,
                    policy: *policy,
                    source_freshness: *source_freshness,
                    inference_context: inference_context.clone(),
                    context: *context,
                }),
            },
            ModeSlot::Single,
        ),
        // Binding amendment — `ResolveMacroPayload`. The
        // mode is stripped into the slot per the standard mode-bearing
        // pattern; the family identity is the (owner, macro_index,
        // macro_kind, type_args, resolve_env_hash) tuple — the `R` env
        // dim rides the dedicated `MacroPayloadContext` and is folded onto
        // the family key (the `owner` slot itself carries the J/T/L dims).
        SemanticQueryKey::ResolveMacroPayload {
            owner,
            macro_index,
            macro_kind,
            type_args,
            context,
        } => (
            FamilyKey::ResolveMacroPayload {
                owner: owner.clone(),
                macro_index: *macro_index,
                macro_kind: *macro_kind,
                type_args: Arc::clone(type_args),
                resolve_env_hash: context.resolve_env_hash,
            },
            mode_to_slot(context.mode),
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
                parse_env_hash: context.parse_env_hash,
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
                parse_env_hash: context.parse_env_hash,
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
        // ResolveOverloadSet — LIVE producer with a mode-erased key: the
        // key carries no projection context, so the family uses the
        // `Single` slot (the WHY is mode-erasure, not non-production).
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
        SemanticQueryKey::ClassifyBroadRuntime { subject, context } => (
            FamilyKey::ClassifyBroadRuntime {
                subject: Box::new(subject.clone()),
                context: *context,
            },
            ModeSlot::Single,
        ),
        // ApparentType — LIVE producer for the callable arm. No projection
        // mode → the `Single` slot. The `{T, L, J}` env dims AND the
        // rootless demand-scope witness ride on the family key (the key
        // has no slot to carry them).
        SemanticQueryKey::ApparentType { base, context } => (
            FamilyKey::ApparentType {
                base: *base,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
                demand_scope: context.demand_scope.clone(),
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
        // slot. The per-variant `flow` demand axis, the full `{P, R, T, L, J}`
        // env dims, and the shared `substitution` axis ride on the family key
        // (the key has no slot to carry them); the `ProgramPointId` is
        // carried VERBATIM as the identity core.
        SemanticQueryKey::FlowNarrowingAt {
            point,
            flow,
            context,
        } => (
            FamilyKey::FlowNarrowingAt {
                point: point.clone(),
                flow: flow.clone(),
                parse_env_hash: context.parse_env_hash,
                resolve_env_hash: context.resolve_env_hash,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
                substitution: context.substitution,
            },
            ModeSlot::Single,
        ),
        // ContextualTypeAt — non-producing. Same shape as FlowNarrowingAt:
        // `Single` slot, the per-variant `contextual` demand axis, full
        // `{P, R, T, L, J}` env + shared `substitution` on the family key,
        // `ProgramPointId` as the verbatim identity core.
        SemanticQueryKey::ContextualTypeAt {
            point,
            contextual,
            context,
        } => (
            FamilyKey::ContextualTypeAt {
                point: point.clone(),
                contextual: contextual.clone(),
                parse_env_hash: context.parse_env_hash,
                resolve_env_hash: context.resolve_env_hash,
                type_env_hash: context.type_env_hash,
                lib_env_hash: context.lib_env_hash,
                project_identity: context.project_identity,
                substitution: context.substitution,
            },
            ModeSlot::Single,
        ),
        // LowerLocator — LIVE producer with a mode-erased key: the sealed
        // `LocatorLoweringKey` IS the family identity (slot + locator + P +
        // R, nothing else; T/L/J slot-carried), and the fixed locator-shape
        // lowering has no mode/demand axis, so the family uses the `Single`
        // slot.
        SemanticQueryKey::LowerLocator { key } => (
            FamilyKey::LowerLocator {
                key: Box::new(key.clone()),
            },
            ModeSlot::Single,
        ),
        // ClassifyMaterializationCycleGate — LIVE producer with a
        // mode-erased key: the sealed `MaterializationCycleGateKey` IS the
        // family identity (root slot + P + R, nothing else; T/L/J
        // slot-carried; args/demand/mode/policy fixed inside the
        // producer), so the family uses the `Single` slot.
        SemanticQueryKey::ClassifyMaterializationCycleGate(key) => (
            FamilyKey::ClassifyMaterializationCycleGate {
                key: Box::new(key.clone()),
            },
            ModeSlot::Single,
        ),
        // FlowReturn — LIVE whole-function producer with a mode-erased
        // key: the full `FlowReturnKey` IS the family identity (function
        // slot + type args + env/substitution/policy), so the family
        // uses the `Single` slot.
        SemanticQueryKey::FlowReturn(key) => {
            (FamilyKey::FlowReturn { key: key.clone() }, ModeSlot::Single)
        }
        // ResolveCall — complete mixed-equation results only. Mode-erased:
        // the full `ResolveCallKey` IS the family
        // identity (call-site point + callee/receiver/args + kind +
        // explicit type args + sealed-empty flow axis + env/substitution
        // context), so the family uses the `Single` slot.
        SemanticQueryKey::ResolveCall(key) => (
            FamilyKey::ResolveCall { key: key.clone() },
            ModeSlot::Single,
        ),
    }
}

/// The projection path a query targets — the path carried by the
/// path-bearing key variants (`ProjectPath` / `ProjectMember` /
/// `IndexedAccess`), empty for every other variant (`Instantiate`,
/// `KeyOf`, modeless families, …). This is the path of the owning
/// `FamilyKey`, so the §3.4 recorded-point backfill in
/// [`FamilySlots::publish`] builds each sibling slot's requested point at
/// the SAME path.
pub(super) fn requested_path_for_key(key: &SemanticQueryKey) -> ProjectionPath {
    match key {
        SemanticQueryKey::ProjectPath { path, .. } => ProjectionPath::from(Arc::clone(path)),
        SemanticQueryKey::ProjectMember { member, .. } => {
            ProjectionPath::from_segments([PathSegment::Member(PropertyKey::identifier(
                Arc::clone(member),
            ))])
        }
        SemanticQueryKey::IndexedAccess { index, .. } => {
            ProjectionPath::from_segments([PathSegment::Index(index.clone())])
        }
        // FlowReturn: the path is the key's OWN demand-axis projection
        // path (whole-return = empty). Kept in lockstep with
        // `requested_demand_override` so the requested point's path and
        // the requested path are the same datum.
        SemanticQueryKey::FlowReturn(key) => key.demand.point.projection.path.clone(),
        _ => ProjectionPath::empty(),
    }
}

/// The requested-DEMAND override for key variants that embed their own
/// demand point instead of denoting it through their `ModeSlot` preset.
/// `FlowReturn` carries the flow-typed `(ProjectionDemand, EvalPolicy)`
/// point as a KEY FIELD (`ReturnProjectionDemand`); a warm hit on such a
/// key must be gated against THAT point — the shared demand-lattice
/// dominance machinery over the key's own demand — never the modeless
/// `Single` preset (which would collapse every demand point to a trivial
/// pass). `None` keeps the `point_for_slot` formula.
pub(super) fn requested_demand_override(key: &SemanticQueryKey) -> Option<Demand> {
    match key {
        SemanticQueryKey::FlowReturn(key) => Some(key.demand.point.clone()),
        _ => None,
    }
}

/// The §3.4 **requested materialised point** for `key` — the demand point
/// a warm hit on `key` must be served by. Built from the key's
/// `(slot, path)`: the slot's mode preset at the key's projection path
/// (modeless `Single` families use `Demand::identity()`, so the gate is a
/// trivial pass — their satisfaction is decided purely by
/// `validate_with_self_roots`). Used by the warm-hit gate
/// (`cached_satisfies(entry.satisfied_projection, requested_point_for_key(key))`).
#[cfg(any(test, feature = "test-support"))]
pub(super) fn requested_point_for_key(key: &SemanticQueryKey) -> MaterializedPoint {
    if let Some(point) = requested_demand_override(key) {
        return MaterializedPoint::new(point);
    }
    let (_, slot) = family_and_slot(key);
    let path = requested_path_for_key(key);
    MaterializedPoint::new(point_for_slot(slot, &path))
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
            r.canonical_id() == Some(canonical_id)
        }
        crate::resolver_core::FactVersionRef::RouteSurface(r) => {
            r.canonical_id.as_str() == canonical_id
        }
        crate::resolver_core::FactVersionRef::FileSourceEnv {
            canonical_id: c, ..
        } => c.as_str() == canonical_id,
        crate::resolver_core::FactVersionRef::ProgramAnalysis(fact) => match fact {
            crate::resolver_core::ProgramAnalysisFactRef::FlowBody { function, .. } => {
                function.canonical_id.as_ref() == canonical_id
            }
        },
        // Not file-scoped — whole-project scalars reference no canonical.
        crate::resolver_core::FactVersionRef::ProjectGeneration { .. }
        | crate::resolver_core::FactVersionRef::DomainGeneration(_)
        | crate::resolver_core::FactVersionRef::StrictSelfRootWorld(_) => false,
    })
}

/// Every [`ModeSlot`] variant as a static slice. The per-canonical
/// reverse index drives `invalidate_canonical`'s slot sweep; this
/// constant is retained for invalidate-all and diagnostic paths that
/// need to enumerate every slot.
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
    ModeSlot::TransitSkeleton,
    ModeSlot::MacroSurfaceShallow,
    ModeSlot::VueRuntimeSurfaceShallow,
];
