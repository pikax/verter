//! The `SemanticQueryKeySpec` table — one authoritative row per live
//! [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey) variant.
//!
//! # Generator is the sole writer
//!
//! The checked-in artifact `query_key_spec_table.txt` (sibling of this file)
//! is written ONLY by the generator binary `gen-query-key-spec`
//! (`cargo run -p verter_session --bin gen-query-key-spec`, wired as the pnpm
//! script `gen:query-key-spec`). The Rust test
//! `semantic_query_key_spec_table_equals_enum` NEVER writes the artifact — it
//! re-renders [`semantic_query_key_specs`] in memory and diffs. This is the
//! repo `generators_not_tests` rule: generated source artifacts are produced
//! by a dedicated `cargo run` target, and tests only assert.
//!
//! # The three discriminators the diff-test enforces
//!
//! 1. **Freshness** — `render_spec_table(&semantic_query_key_specs())`
//!    byte-equals the committed artifact (fails on a hand-edit, a stale
//!    artifact, or a generator that was not re-run).
//! 2. **Enum-equality** — the spec table's variant-name set equals the
//!    variant identifiers scanned from the live `pub enum SemanticQueryKey`
//!    source (fails when a variant is added/removed without regenerating).
//! 3. **Per-row sanity** — every row is `Live`; every row carries the
//!    `TypeNode` value domain EXCEPT `Relate` (`Relation`) and
//!    `ResolveOverloadSet` (`OverloadSet`), which is the current-tree truth,
//!    and the
//!    [`SemanticQueryKeyTag::ALL`](crate::semantic_query::SemanticQueryKeyTag::ALL)
//!    set triangulates against both the spec set and the enum-scan set.
//!
//! # Current-tree honesty
//!
//! - Every live variant resolves to
//!   [`SemanticQueryValueTag::TypeNode`] EXCEPT `Relate` and
//!   `ResolveOverloadSet`: `ProjectSemanticDispatch::execute` wraps the
//!   `TypeNode` keys' results as `SemanticQueryValue::TypeNode(node)`.
//!   `Relate` records its value domain as
//!   [`SemanticQueryValueTag::Relation`] — the tri-state assignability
//!   classification. Its formal `execute` arm is non-producing: it returns
//!   `QueryError::Miss` (`Opaque(Miss)`). The current PRODUCTION authority is
//!   `ProjectSemanticDispatch::relate_nodes(source, target) ->
//!   (RelationResult, DepSignature)`, which produces and dep-signature-fences
//!   every judgement in the dedicated `SemanticGraphStore::relation_memo` —
//!   NOT the family singleflight. That is why this row's `admission` is
//!   [`RelationMemo`](AdmissionSpec::RelationMemo). `ResolveOverloadSet`
//!   records [`SemanticQueryValueTag::OverloadSet`] as a FORWARD-DECLARED
//!   value domain: its `execute` arm is non-producing (returns `Miss`,
//!   admission [`NonProducingPendingReducer`](AdmissionSpec::NonProducingPendingReducer))
//!   until the overload-producing reducer that fills the `OverloadSet`
//!   carrier lands — it never fabricates an empty set. No other value
//!   domain appears.
//! - `allowed_demand` is a [`DemandAxis`]-vocabulary mask and does NOT capture
//!   the `ReductionDemand` slot-selection dimension. A key carrying a
//!   `ProjectionReductionContext` (`Instantiate` / `KeyOf` / `MappedType` /
//!   `ProjectPath`) also branches on `ReductionDemand`
//!   (`Published` / `StructuralTransit` / `MacroObjectSurface`), but that is a
//!   3-way MEMO-SLOT-SELECTION dimension resolved by `context_to_slot`
//!   (`semantic_query_memo::family`) — `StructuralTransit` routes to the
//!   `Transit*` slot mirrors and `MacroObjectSurface` to `MacroSurfaceShallow`,
//!   entirely separate from the `Demand.policy.carrier_stop` field the
//!   [`CarrierStop`](DemandAxis::CarrierStop) axis governs (`MacroObjectSurface`
//!   even reduces operators exactly like `Published`). Because the isolation
//!   lives in the slot layer OUTSIDE the `DemandAxis` vocabulary, it is
//!   correctly absent from `allowed_demand`; this column does not — and should
//!   not — express it. Beyond that structural exclusion, `allowed_demand` is a
//!   hand-classification of the [`DemandAxis`] fields a family's MEMO IDENTITY
//!   branches on, NOT yet a test-guarded reflection of the live `FamilyKey`
//!   axis set: the diff-test enforces freshness / enum-equality / per-row
//!   sanity but does NOT discriminate `allowed_demand` against the live
//!   `FamilyKey`. This hand-classification is pending the design's §3.6
//!   benched-minimality pass (U3/U15) that empirically pins each family's
//!   minimal axis set against its `FamilyKey`.
//! - `env_dims` is a current-tree classification per the design's §2.1
//!   two-tier env model: `parse_env` (`P`) enters a key ONLY when the value
//!   reads the parsed body skeleton (class-surface decorator lowering,
//!   flow/contextual body analysis). No current variant reads a body skeleton
//!   at query time — re-sourcing a file's `whole_hash` / reading an already-
//!   lowered `IndexedReady` `TypeExpr` is content-version rooting through
//!   `ReadSetSignature`, not a `parse_env` dependency — so no current row
//!   carries `P`. This hand-classification is pending the design's §3.6
//!   benched-minimality pass (U3/U15) that empirically pins each row's
//!   minimal dimension set.
//! - `cross_context_guard` names the per-key `*_do_not_warm_hit` guard that
//!   pins the row's cross-context warm-hit isolation, or is empty (`""`) for a
//!   row that does not yet have one. The four `Resolve{ClassSurface,
//!   AmbientNamespace,Enum,OverloadSet}` rows name their guards
//!   (`resolve_*_do_not_warm_hit`); the remaining rows carry `""` as the
//!   ACCURATE present state, not a placeholder.

use crate::semantic_query::demand::{relevant_demand_axes, AxisMask, DemandAxis};
use crate::semantic_query::{SemanticQueryKeyTag, SemanticQueryValueTag};

// ---------------------------------------------------------------------------
// Env dimensions (R21 five-hash split)
// ---------------------------------------------------------------------------

/// One R21 env-hash dimension a cached value may depend on. The five
/// dimensions are the orthogonal cache-key env hashes from the fact-based
/// cache architecture: `parse_env_hash`, `resolve_env_hash`, `type_env_hash`,
/// `lib_env_hash`, `project_identity`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EnvDim {
    /// `parse_env_hash` — `P`.
    Parse,
    /// `resolve_env_hash` — `R`.
    Resolve,
    /// `type_env_hash` — `T`.
    Type,
    /// `lib_env_hash` — `L`.
    Lib,
    /// `project_identity` — `J`.
    Project,
}

impl EnvDim {
    const fn bit(self) -> u8 {
        match self {
            EnvDim::Parse => 1 << 0,
            EnvDim::Resolve => 1 << 1,
            EnvDim::Type => 1 << 2,
            EnvDim::Lib => 1 << 3,
            EnvDim::Project => 1 << 4,
        }
    }

    /// The full `{P, R, T, L, J}` bit-mask.
    const ALL: u8 = (1 << 5) - 1;

    /// Every dimension in canonical render order: P, R, T, L, J. The single
    /// ordered enumeration [`EnvDimMask::render`] iterates.
    ///
    /// What is enforced: [`bit`](Self::bit) and [`token`](Self::token) are
    /// exhaustive `match`es (each gains a compiler-forced arm per variant), and
    /// the `_ENV_DIM_ORDERED_COVERS_ALL` assertion below pins this slice's
    /// combined bit-union to the manually-maintained [`EnvDim::ALL`]. The
    /// assertion catches an ORDERED/ALL mismatch (a variant appended to one but
    /// not the other). It does NOT prove enum cardinality: a variant added with
    /// `bit()` and `token()` updated but BOTH `ORDERED` and `ALL` forgotten
    /// still compiles. Symmetric to the [`DemandAxis::ORDERED`] /
    /// `_ORDERED_COVERS_ALL` gate.
    const ORDERED: &'static [EnvDim] = &[
        EnvDim::Parse,
        EnvDim::Resolve,
        EnvDim::Type,
        EnvDim::Lib,
        EnvDim::Project,
    ];

    /// The single-letter token used in the rendered table. Exhaustive `match`:
    /// adding a variant is a compile error until an arm is added here.
    fn token(self) -> &'static str {
        match self {
            EnvDim::Parse => "P",
            EnvDim::Resolve => "R",
            EnvDim::Type => "T",
            EnvDim::Lib => "L",
            EnvDim::Project => "J",
        }
    }
}

/// Compile-time gate: `bit()` and `token()` are exhaustive matches
/// (compiler-forced per variant); this const pins the [`EnvDim::ORDERED`]
/// slice's bit-union to the manually-maintained [`EnvDim::ALL`], so an
/// ORDERED/ALL mismatch fails to compile here. It does NOT prove enum
/// cardinality: a variant added with `bit()`/`token()` updated but BOTH
/// `ORDERED` and `ALL` forgotten still passes. Symmetric to
/// `demand::_ORDERED_COVERS_ALL` for [`DemandAxis`].
const _ENV_DIM_ORDERED_COVERS_ALL: () = {
    let mut union: u8 = 0;
    let mut i = 0;
    while i < EnvDim::ORDERED.len() {
        union |= EnvDim::ORDERED[i].bit();
        i += 1;
    }
    assert!(
        union == EnvDim::ALL,
        "EnvDim::ORDERED must list exactly the dimensions whose bits compose \
         EnvDim::ALL"
    );
};

/// The set of R21 env-hash dimensions a cached value depends on. Newtype over
/// a `u8` bitset, mirroring [`AxisMask`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct EnvDimMask(u8);

impl EnvDimMask {
    /// No dimensions.
    #[must_use]
    pub fn empty() -> Self {
        EnvDimMask(0)
    }

    /// Every R21 env-hash dimension — the full `{P, R, T, L, J}` mask. Mirrors
    /// [`AxisMask::full`].
    #[must_use]
    pub fn full() -> Self {
        EnvDimMask(EnvDim::ALL)
    }

    /// Add a dimension.
    #[must_use]
    pub fn with(self, dim: EnvDim) -> Self {
        EnvDimMask(self.0 | dim.bit())
    }

    /// Membership test.
    #[must_use]
    pub fn contains(self, dim: EnvDim) -> bool {
        self.0 & dim.bit() != 0
    }

    /// Union of two masks.
    #[must_use]
    pub fn union(self, other: EnvDimMask) -> Self {
        EnvDimMask(self.0 | other.0)
    }

    /// Build a mask from a slice of dimensions.
    #[must_use]
    pub fn from_dims(dims: &[EnvDim]) -> Self {
        dims.iter().fold(EnvDimMask::empty(), |m, d| m.with(*d))
    }

    /// Stable textual render in canonical P,R,T,L,J order, space-joined
    /// (e.g. `"P R T L J"` or `"T L J"`). The empty mask renders as `"—"` so
    /// every column in the table is non-blank.
    #[must_use]
    pub fn render(self) -> String {
        let tokens: Vec<&'static str> = EnvDim::ORDERED
            .iter()
            .filter(|d| self.contains(**d))
            .map(|d| d.token())
            .collect();
        if tokens.is_empty() {
            "—".to_string()
        } else {
            tokens.join(" ")
        }
    }
}

// ---------------------------------------------------------------------------
// Lifecycle + admission discriminants (design §2.5)
// ---------------------------------------------------------------------------

/// A spec row's lifecycle state (design §2.5 — "no fourth state"). Every
/// current-tree row is [`Live`](Self::Live); `Retired` / `Renamed` rows
/// describe a variant that was removed or renamed (a `Retired` row's name must
/// be absent from the live enum; a `Renamed` row's old name must be absent).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyLifecycle {
    Live,
    Retired,
    Renamed,
}

impl KeyLifecycle {
    fn render(self) -> &'static str {
        match self {
            KeyLifecycle::Live => "Live",
            KeyLifecycle::Retired => "Retired",
            KeyLifecycle::Renamed => "Renamed",
        }
    }
}

/// The admission / budget discriminant for a query family's cold build.
///
/// Only live, used arms exist. The design §2.5 sketch names `RelationBudget` /
/// `FlowSliceBudget` arms, which are NOT present here because no current key
/// routes through them — they are added if and when a key adopts that admission
/// regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AdmissionSpec {
    /// The default cooperative-admission cold build: concurrent cold requests
    /// for the same key collapse onto one singleflight materialisation, and
    /// non-cacheable outcomes (overflow / budget / cancellation / supersession
    /// / incomplete self-rooting) route through `ReturnOnly` without
    /// publishing.
    Singleflight,
    /// Read-dominant key with no `execute`-side producer: the formal
    /// [`SemanticQueryApi::execute`](crate::semantic_query::SemanticQueryApi)
    /// entry point returns a warm node id when the identity map has an entry and
    /// `QueryError::Miss` only WHEN UNWRITTEN — it never computes/writes a value;
    /// writes come from a dedicated adapter side, not from `execute`. Used by
    /// `ResolvedNamedType` (the parser's `NamedTypeCache` adapter writes; the hot
    /// path reads through `SemanticGraphStore::get_resolved_named_type`).
    ReadDominantNoExecute,
    /// Dedicated relation-memo path: the judgement is produced and cached by
    /// `ProjectSemanticDispatch::relate_nodes`, which memoises every outcome in
    /// the standalone `SemanticGraphStore::relation_memo` under dep-signature
    /// fencing. The family `execute` path for `Relate` is intentionally
    /// non-producing — it returns `QueryResult::Error(QueryError::Miss)`
    /// (`Opaque(Miss)`) — so this key never flows through the `Singleflight`
    /// family materialiser. Used by `Relate`.
    RelationMemo,
    /// Honest non-producing arm: this key has NO `execute`-side producer.
    /// The `execute` build arm returns
    /// `QueryResult::Error(QueryError::Miss)` (`Opaque(Miss)`) verbatim —
    /// it NEVER admits, caches, backfills, or fabricates an empty/unknown
    /// value as if it were semantic. The reducer that produces this key's
    /// value is unimplemented:
    /// - `ResolveAmbientNamespace` → the namespace-analysis reducer.
    /// - `ResolveEnum` → the enum value/type-duality reducer.
    /// - `ResolveOverloadSet` → the signature-lowering reducer.
    ///
    /// Distinct from [`RelationMemo`](Self::RelationMemo) (implies a
    /// dedicated relation-memo producer), [`Singleflight`](Self::Singleflight)
    /// (implies a real materialiser), and
    /// [`ReadDominantNoExecute`](Self::ReadDominantNoExecute) (implies an
    /// adapter writer). This variant implies NO writer at all.
    NonProducingPendingReducer,
}

impl AdmissionSpec {
    fn render(self) -> &'static str {
        match self {
            AdmissionSpec::Singleflight => "Singleflight",
            AdmissionSpec::ReadDominantNoExecute => "ReadDominantNoExecute",
            AdmissionSpec::RelationMemo => "RelationMemo",
            AdmissionSpec::NonProducingPendingReducer => "NonProducingPendingReducer",
        }
    }
}

// ---------------------------------------------------------------------------
// The spec row + table
// ---------------------------------------------------------------------------

/// One row of the `SemanticQueryKeySpec` table (field set per design §2.5).
#[derive(Debug, Clone)]
pub struct SemanticQueryKeySpec {
    /// The variant this row describes.
    pub variant: SemanticQueryKeyTag,
    /// Lifecycle state — `Live` for every current row.
    pub lifecycle: KeyLifecycle,
    /// The named context struct / payload shape the variant carries.
    pub context_shape: &'static str,
    /// The single value domain the variant resolves to.
    pub value_domain: SemanticQueryValueTag,
    /// The R21 env-hash dimensions the cached value depends on.
    pub env_dims: EnvDimMask,
    /// Which [`DemandAxis`] this family branches on.
    pub allowed_demand: AxisMask,
    /// The per-key `*_do_not_warm_hit` cross-context guard name — populated for
    /// the class-surface / ambient-namespace / enum / overload-set rows (each
    /// carries its dedicated `*_do_not_warm_hit` guard) and empty (`—`) for the
    /// rest.
    pub cross_context_guard: &'static str,
    /// The admission / budget discriminant for the cold build.
    pub admission: AdmissionSpec,
}

/// The env set for keys that resolve imports / name resolution on their own
/// step but do NOT read a parsed body skeleton: `{R, T, L, J}` (design §2.1
/// tier-2 — `parse_env` is excluded because re-sourcing a file's `whole_hash`
/// or reading an already-lowered `IndexedReady` `TypeExpr` is content-version
/// rooting via `ReadSetSignature`, not a `parse_env` dependency).
fn env_resolve() -> EnvDimMask {
    EnvDimMask::from_dims(&[EnvDim::Resolve, EnvDim::Type, EnvDim::Lib, EnvDim::Project])
}

/// The structural env set for keys that operate over already-resolved interned
/// nodes plus lib intrinsics, with no fresh parse/resolve on the key's own
/// step: `{T, L, J}` (design §2.1 tier-2 — no `parse_env`, no fresh
/// `resolve_env`).
fn env_structural() -> EnvDimMask {
    EnvDimMask::from_dims(&[EnvDim::Type, EnvDim::Lib, EnvDim::Project])
}

/// The demand axes a [`ProjectionMode`](crate::semantic_query::ProjectionMode)
/// spans — the union of the axes on which the five projection rungs differ.
///
/// A key carrying a `ProjectionMode` can take ANY rung
/// (Identity / Navigate / Shallow / Expanded / Skeleton), so its family
/// branches on this whole union, NOT just `NormalizationDepth`. Across the five
/// preset points (design §3.7 worked-examples) the rungs differ on exactly
/// these seven axes:
///
/// - `NormalizationDepth` / `OperatorReduction` / `MemberBody` / `Facets` /
///   `AliasPreservation` — the depth/surface rungs that separate
///   Navigate / Shallow / Expanded.
/// - `GenericOpen` — Skeleton uses `TypeParamShells`, an INCOMPARABLE regime
///   (design §3.1.1), so it differs on this antichain axis.
/// - `CarrierStop` — Skeleton uses `StopAtCarrier` while the others `Continue`.
fn mode_demand_axes() -> AxisMask {
    relevant_demand_axes(&[
        DemandAxis::NormalizationDepth,
        DemandAxis::OperatorReduction,
        DemandAxis::AliasPreservation,
        DemandAxis::MemberBody,
        DemandAxis::Facets,
        DemandAxis::GenericOpen,
        DemandAxis::CarrierStop,
    ])
}

/// The authoritative, hand-encoded spec table — one row per live
/// [`SemanticQueryKey`](crate::semantic_query::SemanticQueryKey) variant,
/// ordered by [`SemanticQueryKeyTag::ALL`] so the rendered artifact is
/// deterministic. This is the human-authored spec; the generator renders it
/// and the diff-test compares it against the live enum.
#[must_use]
pub fn semantic_query_key_specs() -> Vec<SemanticQueryKeySpec> {
    // The axes a `ProjectionMode` spans. A key carrying a `ProjectionMode`
    // (bare, or inside a `ProjectionReductionContext`) can take ANY of the five
    // rungs (Identity / Navigate / Shallow / Expanded / Skeleton), so its
    // family branches on the UNION of the axes those rungs differ on.
    let mode_axes = mode_demand_axes();
    // `ProjectionReductionContext` carries `mode` PLUS `provenance` +
    // `merge_role` + `demand` (verified on `ProjectionReductionContext`). Per
    // the design §2.1 FORK-A note, provenance + merge_role are family-identity
    // discriminators (which merge arm / provenance regime this reduction
    // answers), so the family branches on them via `DemandAxis::Provenance` /
    // `DemandAxis::MergeRole` below.
    //
    // IMPORTANT — this is true for ONLY the two families whose `FamilyKey`
    // actually carries those fields: `FamilyKey::Instantiate` (fields
    // `provenance` + `merge_role`) and `FamilyKey::ProjectPath` (same two
    // fields), both verified in `semantic_query_memo::family`. The other two
    // `ProjectionReductionContext`-carrying keys — `FamilyKey::KeyOf { base }`
    // and `FamilyKey::MappedType { source, mapper }` — carry NO
    // provenance / merge_role anywhere in the `(family, slot)` memo identity:
    // `context_to_slot` reads only `demand` + `mode`, and `ModeSlot` has no
    // provenance / merge_role dimension, so for these two families those fields
    // are DROPPED from the identity entirely (two KeyOf/MappedType queries
    // differing only in provenance / merge_role COLLIDE on one entry). Their
    // `allowed_demand` is therefore the bare `mode_axes` — only the mode axes
    // survive into the identity. `reduction_axes` below is consumed ONLY by
    // `Instantiate` and (via `project_path_axes`) `ProjectPath`, whose
    // `FamilyKey` carries provenance + merge_role precisely to keep those
    // variants apart.
    //
    // The `demand` field (`ReductionDemand::Published` / `StructuralTransit` /
    // `MacroObjectSurface`) is DELIBERATELY ABSENT from this `DemandAxis` mask:
    // it is a 3-way memo-SLOT-SELECTION dimension, not a `DemandAxis`. The
    // family separates its outputs by routing each `demand` to a distinct memo
    // slot in `context_to_slot` (`semantic_query_memo::family`) —
    // `StructuralTransit` to the `Transit*` slot mirrors, `MacroObjectSurface`
    // to `MacroSurfaceShallow` — NOT by setting any `Demand` axis. It is
    // independent of the `CarrierStop` axis: that axis governs the
    // `Demand.policy.carrier_stop` operator-reduction field, whereas
    // `MacroObjectSurface` reduces operators exactly like `Published`
    // (`carrier_stop = Continue`) and differs only in the union-arm merge rule.
    // Because `ReductionDemand` isolation lives in the slot layer OUTSIDE the
    // `DemandAxis` vocabulary, the `allowed_demand` column cannot and must not
    // express it.
    let reduction_axes = mode_axes
        .with(DemandAxis::Provenance)
        .with(DemandAxis::MergeRole);
    // `ProjectPath` additionally branches on the `Path` axis (it carries a
    // path the family walks).
    let project_path_axes = reduction_axes.with(DemandAxis::Path);

    vec![
        // ResolveDecl(ResolveDeclKey) — resolves a declaration: resolves the
        // owning file's imports / name resolution (no parsed-body-skeleton read
        // at query time, §2.1 tier-2), so `R T L J`. No demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveDecl,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ResolveDeclKey",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // Instantiate { base, args, context } — instantiates a generic decl
        // body after substitution; resolves imported type-argument references
        // (§2.1 tier-2: `R T L J`, no parsed-body-skeleton read at query time)
        // and branches on the reduction context.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::Instantiate,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: reduction_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // ProjectMember { base, member, mode } — structural projection over an
        // already-resolved interned node; branches on the axes the `mode`
        // spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ProjectMember,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionMode",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // IndexedAccess { base, index, mode } — structural indexed lookup over
        // an already-resolved node; branches on the axes the `mode` spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::IndexedAccess,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionMode",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // KeyOf { base, context } — structural keyspace reduction over an
        // already-resolved node. Its memo identity is `FamilyKey::KeyOf { base }`
        // (verified in `semantic_query_memo::family`), which carries NO
        // provenance / merge_role: `context_to_slot` reads only `demand` + `mode`
        // and `ModeSlot` has no provenance / merge_role dimension, so those
        // fields are dropped from the `(family, slot)` identity entirely
        // (provenance / merge_role variants collide on one entry). The family
        // branches solely on the mode axes; `allowed_demand = mode_axes` because
        // only the mode axes survive into the identity. Contrast
        // `Instantiate` / `ProjectPath`, whose `FamilyKey` carries
        // provenance + merge_role to keep variants apart.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::KeyOf,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // MappedType { source, mapper, context } — structural mapped-type
        // rewrite over an already-resolved source. Its memo identity is
        // `FamilyKey::MappedType { source, mapper }` (verified in
        // `semantic_query_memo::family`), which carries NO provenance /
        // merge_role: `context_to_slot` reads only `demand` + `mode` and
        // `ModeSlot` has no provenance / merge_role dimension, so those fields
        // are dropped from the `(family, slot)` identity entirely (provenance /
        // merge_role variants collide on one entry). The family branches solely
        // on the mode axes; `allowed_demand = mode_axes` because only the mode
        // axes survive into the identity. Contrast `Instantiate` / `ProjectPath`,
        // whose `FamilyKey` carries provenance + merge_role to keep variants
        // apart.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::MappedType,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // Conditional { check, extends, true, false, distributive } —
        // structural conditional decision over already-resolved nodes; no
        // demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::Conditional,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(check,extends,true_branch,false_branch,distributive)",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // TypeOf { value_root } — resolves the type of a value declaration;
        // resolves the owning file's name resolution (§2.1 tier-2: `R T L J`,
        // no parsed-body-skeleton read at query time). No demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::TypeOf,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ValueRootKey",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // NormalizeUnion { members } — structural union normalization over
        // already-resolved member nodes; no demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::NormalizeUnion,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(members)",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // NormalizeIntersection { members } — structural intersection
        // normalization over already-resolved member nodes; no demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::NormalizeIntersection,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(members)",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // ProjectPath { base, path, context } — path-precise projection over an
        // already-resolved base; branches on the reduction context plus the
        // Path axis it carries.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ProjectPath,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_structural(),
            allowed_demand: project_path_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // ResolvedNamedType { key } — read-dominant Vue-macro artifact identity;
        // `execute` returns Miss, writes come from the NamedTypeCache adapter.
        // The cached value depends on the resolution of the named type's
        // declaration graph (§2.1 tier-2: `R T L J`, no parsed-body-skeleton
        // read at query time).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolvedNamedType,
            lifecycle: KeyLifecycle::Live,
            context_shape: "HostResolvedNamedTypeKey",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::ReadDominantNoExecute,
        },
        // Relate { source, target } — assignability/relation judgement over
        // already-resolved nodes. Resolves to the `Relation` value domain
        // (`SemanticQueryValue::Relation(RelationPayload)`), NOT `TypeNode`.
        // The family `execute` path is intentionally non-producing (returns
        // `Opaque(Miss)`); the authoritative judgement is produced + cached by
        // `relate_nodes` in the dedicated dep-signature-fenced `relation_memo`
        // (admission `RelationMemo`, not the family singleflight). `env_dims`
        // stays `T L J` for the current bare `{source,target}` key (structural
        // over already-resolved nodes); it gains `R` only when the relation
        // upgrade lands.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::Relate,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(source,target)",
            value_domain: SemanticQueryValueTag::Relation,
            env_dims: env_structural(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::RelationMemo,
        },
        // ResolveMacroPayload { owner, macro_index, macro_kind, type_args,
        // mode } — resolves a Vue macro payload to its effective type; resolves
        // the SFC owner's imports and reads the `AnalyzedMacro` sidecar (no AST
        // re-walk, §2.1 tier-2: `R T L J`). The `mode` selects a projection rung
        // for downstream lowering, so the family branches on the axes the
        // `mode` spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveMacroPayload,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(owner,macro_index,macro_kind,type_args,mode)",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // ResolveClassSurface { decl_slot, type_args, side, context } —
        // resolves the instance (TYPE-space) or static (VALUE-space) half
        // of a class via the shared dual-space algorithm. The composed
        // surface identity-routes `execute(Instantiate)` /
        // `execute(TypeOf)` and reads no parsed body skeleton at query
        // time, so `R T L J` (no `P` — keying on `parse_env` would be a
        // dead axis; `P` enters with the decorator-reading reducer). LIVE
        // producer; branches on the axes the `mode` spans (`side` is a
        // FAMILY-IDENTITY discriminator on `FamilyKey`, not a DemandAxis).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveClassSurface,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ClassSurfaceContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: mode_axes,
            cross_context_guard: "resolve_class_surface_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // ResolveAmbientNamespace { namespace_slot, type_args, context } —
        // resolves an ambient namespace surface. The value reads no parsed
        // body skeleton at query time, so `R T L J` (no `P` — keying on
        // `parse_env` would be a dead axis; `P` enters with the
        // body-reading namespace-member reducer). Non-producing: the
        // execute arm returns Miss and never admits/caches. Carries a
        // projection `mode`, so the family branches on the axes the `mode`
        // spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveAmbientNamespace,
            lifecycle: KeyLifecycle::Live,
            context_shape: "AmbientNamespaceContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: mode_axes,
            cross_context_guard: "resolve_ambient_namespace_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // ResolveEnum { enum_slot, context } — resolves an enum surface.
        // An enum is not generic (no substitution axis) and enum-member
        // analysis does not read the parsed body skeleton at query time,
        // so `R T L J` (no `P`). Non-producing: the execute arm returns
        // Miss and never admits/caches. Carries no `mode` → no demand axes.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveEnum,
            lifecycle: KeyLifecycle::Live,
            context_shape: "EnumContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: env_resolve(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "resolve_enum_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // ResolveOverloadSet { callee, type_args, context } — resolves a
        // callee's overload set. Signature lowering resolves imported
        // references but reads no parsed body skeleton at query time, so
        // `R T L J` (no `P`). Non-producing: the execute arm returns Miss
        // and never admits/caches (returning an empty OverloadSet would be
        // a stub). Value domain is the forward-declared `OverloadSet`, NOT
        // `TypeNode`. Carries no `mode` → no demand axes (substitution is
        // carried by `type_args` on the key).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveOverloadSet,
            lifecycle: KeyLifecycle::Live,
            context_shape: "OverloadSetContext",
            value_domain: SemanticQueryValueTag::OverloadSet,
            env_dims: env_resolve(),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "resolve_overload_set_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
    ]
}

/// Render an [`AxisMask`] as a stable, comma-joined token list in canonical
/// [`DemandAxis`] declaration order. The empty mask renders as `"—"`.
///
/// Iteration order comes from [`DemandAxis::ORDERED`] and each token from the
/// exhaustive [`DemandAxis::name`] `match` (compiler-forced per variant). The
/// `_ORDERED_COVERS_ALL` gate in `demand.rs` pins the ORDERED bit-union to
/// `DemandAxis::ALL`; it does NOT prove enum cardinality, so a variant added
/// without updating `ORDERED` could still be dropped from a rendered row — the
/// `syn`-based diff-test is the backstop for that.
fn render_axis_mask(mask: AxisMask) -> String {
    let tokens: Vec<&'static str> = DemandAxis::ORDERED
        .iter()
        .copied()
        .filter(|axis| mask.contains(*axis))
        .map(DemandAxis::name)
        .collect();
    if tokens.is_empty() {
        "—".to_string()
    } else {
        tokens.join(",")
    }
}

fn render_value_domain(tag: SemanticQueryValueTag) -> &'static str {
    match tag {
        SemanticQueryValueTag::TypeNode => "TypeNode",
        SemanticQueryValueTag::ProgramAnalysis => "ProgramAnalysis",
        SemanticQueryValueTag::DeclarationAnalysis => "DeclarationAnalysis",
        SemanticQueryValueTag::OverloadSet => "OverloadSet",
        SemanticQueryValueTag::Relation => "Relation",
        SemanticQueryValueTag::DiagnosticAnalysis => "DiagnosticAnalysis",
    }
}

/// The artifact header — three comment lines documenting that the file is
/// generated, how to regenerate it, and the column order. Embedded in the
/// rendered output so the diff-test pins it too.
const TABLE_HEADER: &str = "\
# SemanticQueryKeySpec table — GENERATED by `cargo run -p verter_session --bin gen-query-key-spec`.
# DO NOT EDIT BY HAND. Regenerate via `pnpm gen:query-key-spec`, then commit this file.
# Columns: variant | lifecycle | value_domain | env_dims | allowed_demand | context_shape | cross_context_guard | admission
";

/// Deterministically render the spec table to the stable, human-auditable text
/// form stored in the checked-in artifact. One row per line in
/// [`SemanticQueryKeyTag::ALL`] order, columns ` | `-separated, ending with a
/// trailing newline. An empty `cross_context_guard` renders as `"—"` so every
/// column is non-blank.
#[must_use]
pub fn render_spec_table(specs: &[SemanticQueryKeySpec]) -> String {
    let mut out = String::from(TABLE_HEADER);
    for spec in specs {
        let guard = if spec.cross_context_guard.is_empty() {
            "—"
        } else {
            spec.cross_context_guard
        };
        out.push_str(&format!(
            "{} | {} | {} | {} | {} | {} | {} | {}\n",
            spec.variant.name(),
            spec.lifecycle.render(),
            render_value_domain(spec.value_domain),
            spec.env_dims.render(),
            render_axis_mask(spec.allowed_demand),
            spec.context_shape,
            guard,
            spec.admission.render(),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The hand-encoded table has exactly one row per live tag, in `ALL`
    /// order. (The cross-tree enum-equality check lives in the integration
    /// guard `semantic_query_key_spec_table_equals_enum`; this is the
    /// in-crate triangulation against `SemanticQueryKeyTag::ALL`.)
    #[test]
    fn spec_rows_cover_every_tag_in_order() {
        let specs = semantic_query_key_specs();
        let row_variants: Vec<SemanticQueryKeyTag> = specs.iter().map(|s| s.variant).collect();
        let all: Vec<SemanticQueryKeyTag> = SemanticQueryKeyTag::ALL.to_vec();
        assert_eq!(
            row_variants, all,
            "semantic_query_key_specs() must have exactly one row per \
             SemanticQueryKeyTag::ALL entry, in ALL order"
        );
    }

    /// Render is deterministic and round-trips its structure: a re-render of
    /// the same specs is byte-identical, ends with a newline, and the header
    /// is present.
    #[test]
    fn render_is_deterministic_and_well_formed() {
        let specs = semantic_query_key_specs();
        let a = render_spec_table(&specs);
        let b = render_spec_table(&specs);
        assert_eq!(a, b, "render_spec_table must be deterministic");
        assert!(a.ends_with('\n'), "rendered table must end with a newline");
        assert!(
            a.starts_with("# SemanticQueryKeySpec table"),
            "rendered table must carry the generated-artifact header"
        );
        // One header (3 lines) + one line per row.
        let row_lines = a.lines().filter(|l| !l.starts_with('#')).count();
        assert_eq!(
            row_lines,
            specs.len(),
            "rendered table must have exactly one non-comment line per spec row"
        );
    }

    /// `EnvDimMask::render` is canonical-order and uses `—` for empty.
    #[test]
    fn env_dim_mask_renders_in_canonical_order() {
        assert_eq!(env_resolve().render(), "R T L J");
        assert_eq!(env_structural().render(), "T L J");
        // The full five-hash render still works when a `P`-bearing mask is
        // constructed directly (no current row carries it — §2.1 tier-2).
        assert_eq!(
            EnvDimMask::from_dims(&[
                EnvDim::Parse,
                EnvDim::Resolve,
                EnvDim::Type,
                EnvDim::Lib,
                EnvDim::Project,
            ])
            .render(),
            "P R T L J"
        );
        // `full()` is the bit-mask path (uses `EnvDim::ALL`); it must agree
        // with the explicit five-dim construction above and the canonical
        // `EnvDim::ORDERED` token order.
        assert_eq!(EnvDimMask::full().render(), "P R T L J");
        assert_eq!(EnvDimMask::empty().render(), "—");
        // Order is canonical regardless of insertion order.
        let m = EnvDimMask::empty()
            .with(EnvDim::Project)
            .with(EnvDim::Parse);
        assert_eq!(m.render(), "P J");
    }
}
