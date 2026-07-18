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
//!    `TypeNode` value domain EXCEPT `Relate` (`Relation`),
//!    `ResolveOverloadSet` (`OverloadSet`), `ClassifyBroadRuntime`
//!    (`BroadRuntime`), and `FlowNarrowingAt` /
//!    `ContextualTypeAt` (`ProgramAnalysis`), which is the current-tree truth,
//!    and the
//!    [`SemanticQueryKeyTag::ALL`](crate::semantic_query::SemanticQueryKeyTag::ALL)
//!    set triangulates against both the spec set and the enum-scan set.
//!
//! # Current-tree honesty
//!
//! - Every live variant resolves to
//!   [`SemanticQueryValueTag::TypeNode`] EXCEPT `Relate`,
//!   `ResolveOverloadSet`, `ClassifyBroadRuntime`, `FlowNarrowingAt`, and
//!   `ContextualTypeAt`:
//!   `ProjectSemanticDispatch::execute` wraps the
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
//!   records [`SemanticQueryValueTag::OverloadSet`] as its LIVE value
//!   domain: the execute arm projects the callee's ordered VISIBLE
//!   signature group and the cold build converts the group-bearing node
//!   into the memoized `OverloadSet(Arc<[SignatureRef]>)` value — a
//!   signature-less callee is an honest `Miss`, never a fabricated empty
//!   set. `ClassifyBroadRuntime` records the live terminal
//!   [`SemanticQueryValueTag::BroadRuntime`] domain. `FlowNarrowingAt`
//!   and `ContextualTypeAt` both record
//!   [`SemanticQueryValueTag::ProgramAnalysis`] as a FORWARD-DECLARED
//!   value domain: each `execute` arm is non-producing (returns `Miss`,
//!   admission [`NonProducingPendingReducer`](AdmissionSpec::NonProducingPendingReducer))
//!   until the flow-narrowing / contextual-type reducers land in U6. No
//!   other value domain appears.
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
//!   two-tier env model: `parse_env` (`P`) enters a key when the value reads
//!   the parsed body skeleton (class-surface decorator lowering, namespace-
//!   member body analysis, flow/contextual body analysis) or real-file
//!   parse-derived input. The rows that carry a static `P` are the body-/
//!   decorator-reading surface keys `ResolveClassSurface` (§419) and
//!   `ResolveAmbientNamespace` (§414) and the program-analysis keys
//!   `FlowNarrowingAt` / `ContextualTypeAt` (`env_full`). On the surface keys
//!   `P` is FORWARD-DECLARED for their deferred reducers (decorator-reading /
//!   namespace-member) — the value carries its FULL planned identity now so the
//!   reducer needs no breaking re-key and no false warm-hit can cross a missing
//!   `P` axis in the interim. The `Instantiate` row is CONDITIONAL by the
//!   key's `InstantiateBodySource` ([`EnvDimSpec::Conditional`] — one query
//!   tag, no variant split): `FileBacked(P)` ⇒ `P R T L J` (the compute may
//!   read real-file parse-derived input, so the live `parse_env_hash` is
//!   family identity); `NonFile` ⇒ `R T L J` (per R21 an unconditional `P`
//!   would false-miss every parse-env-insensitive instantiation). The
//!   remaining rows that operate over already-lowered interned nodes
//!   (re-sourcing a file's `whole_hash` / reading an `IndexedReady`
//!   `TypeExpr` is content-version rooting through `ReadSetSignature`, NOT a
//!   `parse_env` dependency) do not carry `P`. The per-row minimal dimension
//!   set remains pending the design's §3.6 benched-minimality pass (U3/U15).
//! - `cross_context_guard` names the per-key `*_do_not_warm_hit` guard that
//!   pins the row's cross-context warm-hit isolation, or is empty (`""`) for a
//!   row that does not yet have one. Several rows name their guards
//!   (`resolve_*_do_not_warm_hit`, `flow_narrowing_at_*`,
//!   `contextual_type_at_*`, `apparent_type_*`, `template_literal_reduce_*`);
//!   any row carrying `""` does so as the ACCURATE present state, not a
//!   placeholder.

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

/// The env-dimension specification of one spec row: either a single STATIC
/// mask, or — for `Instantiate` only — a mask CONDITIONAL on the key's
/// [`InstantiateBodySource`](crate::semantic_query::InstantiateBodySource)
/// (one `Instantiate` query tag, no variant split): `FileBacked(P)` ⇒
/// `P R T L J`; `NonFile` ⇒ `R T L J`. A single static mask is wrong in both
/// directions — `P R T L J` over-claims for `NonFile` (per R21 an
/// unconditional `P` would false-miss every parse-env-insensitive
/// instantiation) and `R T L J` hides the file-backed parse-env dependency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EnvDimSpec {
    /// One static mask for every instance of the key.
    Static(EnvDimMask),
    /// Conditional by the key's `InstantiateBodySource`.
    Conditional {
        /// The mask when the base is `FileBacked(P)`.
        file_backed: EnvDimMask,
        /// The mask when the base is `NonFile`.
        non_file: EnvDimMask,
    },
}

impl EnvDimSpec {
    /// Stable textual render. A static spec renders its mask; a conditional
    /// spec renders BOTH cases, labelled by the body-source arm that selects
    /// each.
    #[must_use]
    pub fn render(self) -> String {
        match self {
            EnvDimSpec::Static(mask) => mask.render(),
            EnvDimSpec::Conditional {
                file_backed,
                non_file,
            } => format!(
                "FileBacked(P) ⇒ {}; NonFile ⇒ {}",
                file_backed.render(),
                non_file.render()
            ),
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
    ///
    /// Distinct from [`RelationMemo`](Self::RelationMemo) (implies a
    /// dedicated relation-memo producer) and
    /// [`Singleflight`](Self::Singleflight) (implies a real materialiser).
    /// This variant implies NO writer at all.
    NonProducingPendingReducer,
}

impl AdmissionSpec {
    fn render(self) -> &'static str {
        match self {
            AdmissionSpec::Singleflight => "Singleflight",
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
    /// The R21 env-hash dimensions the cached value depends on — static for
    /// every row except `Instantiate`, whose mask is conditional by the
    /// key's `InstantiateBodySource`.
    pub env_dims: EnvDimSpec,
    /// Which [`DemandAxis`] this family branches on.
    pub allowed_demand: AxisMask,
    /// The per-key `*_do_not_warm_hit` cross-context guard name — populated for
    /// every spine row whose query identity carries an env or context dimension
    /// (class-surface / ambient-namespace / enum / overload-set / apparent-type /
    /// template-literal-reduce / flow-narrowing / contextual-type — each carries
    /// its dedicated `*_do_not_warm_hit` guard) and empty (`—`) for the rest.
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

/// The FULL env set `{P, R, T, L, J}` — the widest-env tier (design §2.1
/// tier-1), shared by every key whose value READS THE PARSED BODY SKELETON on
/// its own step (so it depends on `parse_env`, unlike the structural reducers):
/// the program-analysis keys (`FlowNarrowingAt` / `ContextualTypeAt` walk the
/// program point's parsed body / control-flow skeleton) and the body-/
/// decorator-reading surface keys (`ResolveClassSurface` reads decorator
/// expressions, `ResolveAmbientNamespace` reads the namespace's inner
/// declarations — design §419/§414 `{P,R}`). All also resolve imported
/// references on their own step (`resolve_env`) and are governed by the type /
/// lib / project env. The `parse_env` axis on the surface keys is FORWARD-
/// DECLARED for their deferred reducers (decorator-reading / namespace-member).
fn env_full() -> EnvDimMask {
    EnvDimMask::from_dims(&[
        EnvDim::Parse,
        EnvDim::Resolve,
        EnvDim::Type,
        EnvDim::Lib,
        EnvDim::Project,
    ])
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
    // `merge_role` + `demand` (verified on `ProjectionReductionContext`).
    // provenance + merge_role are family-identity
    // discriminators (which merge arm / provenance regime this reduction
    // answers), so the family branches on them via `DemandAxis::Provenance` /
    // `DemandAxis::MergeRole` below.
    //
    // This applies to every `ProjectionReductionContext`-carrying family:
    // `Instantiate`, `KeyOf`, `MappedType`, and `ProjectPath` all carry
    // provenance + merge_role on their `FamilyKey` identity, while
    // `context_to_slot` keeps the orthogonal demand/mode slot selection.
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
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // Instantiate { base, args, context: InstantiateContext {
        // projection_reduction, resolve_env_hash, body_source } } —
        // instantiates a generic decl body after substitution; resolves
        // imported type-argument references and branches on the reduction
        // context. The `base` is the env-bearing content-free
        // `ResolvedDeclSlotIdentity` slot (J/T/L); the `R` dim and the
        // `body_source` source-kind axis ride the dedicated
        // `InstantiateContext`. The env row is CONDITIONAL by
        // `InstantiateBodySource`: a `FileBacked(P)` base may read real-file
        // parse-derived input (shallow state, prepared declarations, the lazy
        // decl-body memo), so the live `parse_env_hash` is family identity —
        // `P R T L J`; a true `NonFile` base (`""` / `"__builtin__"` /
        // `"<synthetic>"`) genuinely does not depend on the parse env, so
        // `R T L J` (an unconditional `P` would false-miss every
        // parse-env-insensitive instantiation, R21).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::Instantiate,
            lifecycle: KeyLifecycle::Live,
            context_shape: "InstantiateContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Conditional {
                file_backed: env_full(),
                non_file: env_resolve(),
            },
            allowed_demand: reduction_axes,
            cross_context_guard: "instantiate_same_base_different_env_or_context_do_not_warm_hit, decl_self_type_or_lib_env_change_produces_distinct_instantiate_key",
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
            env_dims: EnvDimSpec::Static(env_structural()),
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
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: mode_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // KeyOf { base, context } — structural keyspace reduction over an
        // already-resolved node. Its memo identity is
        // `FamilyKey::KeyOf { base, provenance, merge_role }`; demand/mode
        // selection still routes through `context_to_slot`.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::KeyOf,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: reduction_axes,
            cross_context_guard: "keyof_queries_differing_only_by_provenance_do_not_warm_hit, keyof_and_mapped_type_context_axes_do_not_alias_family_identity",
            admission: AdmissionSpec::Singleflight,
        },
        // MappedType { source, mapper, context } — structural mapped-type
        // rewrite over an already-resolved source. Its memo identity is
        // `FamilyKey::MappedType { source, mapper, provenance, merge_role }`;
        // demand/mode selection still routes through `context_to_slot`.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::MappedType,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProjectionReductionContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: reduction_axes,
            cross_context_guard: "mapped_type_queries_differing_only_by_merge_role_do_not_warm_hit, keyof_and_mapped_type_context_axes_do_not_alias_family_identity",
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
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // TypeOf { value_root, context: TypeOfContext {
        // projection_reduction, resolve_env_hash } } — resolves the type of
        // a value declaration; resolves the owning file's name resolution
        // (§2.1 tier-2: `R T L J`, no parsed-body-skeleton read at query
        // time). The `value_root` is the env-bearing content-free
        // `ValueRootSlotIdentity` slot (J/T/L); the `R` dim rides the
        // dedicated `TypeOfContext` — mirror of `Instantiate`. The embedded
        // `projection_reduction` carries the caller's projection-reduction
        // demand — `build_typeof` lowers the value's annotation / shape AT
        // that demand (parity with `KeyOf` / `MappedType`); demand/mode
        // selection routes through `context_to_slot`.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::TypeOf,
            lifecycle: KeyLifecycle::Live,
            context_shape: "TypeOfContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: reduction_axes,
            cross_context_guard: "typeof_same_root_different_env_or_context_do_not_warm_hit, typeof_queries_differing_only_by_provenance_do_not_warm_hit, typeof_published_and_transit_contexts_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // NormalizeUnion { members } — structural union normalization over
        // already-resolved member nodes; no demand payload.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::NormalizeUnion,
            lifecycle: KeyLifecycle::Live,
            context_shape: "(members)",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_structural()),
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
            env_dims: EnvDimSpec::Static(env_structural()),
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
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: project_path_axes,
            cross_context_guard: "",
            admission: AdmissionSpec::Singleflight,
        },
        // Relate { source, target, relation, policy, source_freshness,
        // inference_context, context } — full-identity relation judgement over
        // already-resolved nodes. Resolves to the `Relation` value domain
        // (`SemanticQueryValue::Relation(RelationPayload)`), NOT `TypeNode`.
        // The family `execute` path is intentionally non-producing (returns
        // `Opaque(Miss)`); the authoritative judgement is produced + cached by
        // `relate_nodes` in the dedicated dep-signature-fenced `relation_memo`,
        // now re-keyed on the full `RelateMemoKey` identity (admission
        // `RelationMemo`, not the family singleflight). `env_dims` is `R T L J`:
        // the `RelationContext` carries the `R T L J` env the relation outcome
        // depends on (relating imported surfaces resolves their references on
        // the relation's own step — the `R` the bare `{source,target}` key
        // lacked — plus the structural `T L J`); no `P`, since a relation
        // operates over already-lowered interned nodes, not a fresh parsed body
        // skeleton. The relation kind / policy / freshness / inference-context
        // axes are identity discriminators carried on the key.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::Relate,
            lifecycle: KeyLifecycle::Live,
            context_shape:
                "(source,target,relation,policy,source_freshness,inference_context,context)",
            value_domain: SemanticQueryValueTag::Relation,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "",
            admission: AdmissionSpec::RelationMemo,
        },
        // ResolveMacroPayload { owner, macro_index, macro_kind, type_args,
        // context: MacroPayloadContext { resolve_env_hash, mode } } — resolves
        // a Vue macro payload to its effective type; resolves the SFC owner's
        // imports and reads the `AnalyzedMacro` sidecar (no AST re-walk, §2.1
        // tier-2: `R T L J`). The `owner` is the env-bearing content-free
        // `ResolvedDeclSlotIdentity` slot (J/T/L); the `R` dim rides the
        // dedicated `MacroPayloadContext`. The context's `mode` selects a
        // projection rung for downstream lowering, so the family branches on
        // the axes the `mode` spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveMacroPayload,
            lifecycle: KeyLifecycle::Live,
            context_shape: "MacroPayloadContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: mode_axes,
            cross_context_guard: "resolve_macro_payload_same_owner_different_env_or_context_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // ResolveClassSurface { decl_slot, type_args, side, context } —
        // resolves the instance (TYPE-space) or static (VALUE-space) half
        // of a class via the shared dual-space algorithm. A class surface
        // reads the parsed body skeleton (decorator expressions on the class
        // / its members), so the FULL planned identity is `P R T L J`
        // (design §419 `{P,R}`); `P` is FORWARD-DECLARED for the deferred
        // decorator-reading reducer so it needs no breaking re-key and no
        // false warm-hit can cross a missing `P` axis. LIVE producer;
        // branches on the axes the `mode` spans (`side` is a FAMILY-IDENTITY
        // discriminator on `FamilyKey`, not a DemandAxis).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveClassSurface,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ClassSurfaceContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_full()),
            allowed_demand: mode_axes,
            cross_context_guard: "resolve_class_surface_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // ResolveAmbientNamespace { namespace_slot, type_args, context } —
        // resolves an ambient namespace surface. The namespace-member
        // surface reads the parsed body skeleton (the namespace's inner
        // declarations), so the FULL planned identity is `P R T L J` (design
        // §414 `{P,R}`); `P` is FORWARD-DECLARED for the deferred body-
        // reading namespace-member reducer. Non-producing: the execute arm
        // returns Miss and never admits/caches. Carries a projection `mode`,
        // so the family branches on the axes the `mode` spans.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveAmbientNamespace,
            lifecycle: KeyLifecycle::Live,
            context_shape: "AmbientNamespaceContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_full()),
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
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "resolve_enum_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // ResolveOverloadSet { callee, type_args, context } — resolves a
        // callee's overload set. Signature lowering resolves imported
        // references but reads no parsed body skeleton at query time, so
        // `R T L J` (no `P`). LIVE producer: the execute arm projects the
        // callee's ordered VISIBLE signature group (call bucket first, then
        // construct; trailing implementations already hidden by the typeof
        // projection's visibility rule), instantiating per candidate under
        // explicit `type_args`; a callee with no signature group is an
        // honest Miss. Value domain is `OverloadSet`, NOT `TypeNode` — the
        // boundary `execute` wrap converts the group-bearing node into
        // `OverloadSet(Arc<[SignatureRef]>)`. Carries no `mode` → no demand
        // axes (substitution is carried by `type_args` on the key).
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ResolveOverloadSet,
            lifecycle: KeyLifecycle::Live,
            context_shape: "OverloadSetContext",
            value_domain: SemanticQueryValueTag::OverloadSet,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "resolve_overload_set_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // ClassifyBroadRuntime { subject, context } — terminal, modeless
        // semantic classification. Carrier settling and global nominal
        // recognition read R T L J; the subject carries substitution identity.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ClassifyBroadRuntime,
            lifecycle: KeyLifecycle::Live,
            context_shape: "BroadRuntimeContext",
            value_domain: SemanticQueryValueTag::BroadRuntime,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "classify_broad_runtime_contexts_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // ApparentType { base, context } — resolves the apparent member
        // surface of an already-substituted node (a primitive widens to its
        // lib wrapper). The surface is a function of the base node + the
        // lib/type/project env, NOT of import resolution or the parsed body
        // skeleton, so `T L J` (no `R`, no `P` — keying on either would be a
        // dead axis; the substitution axis rides on the `base` node, not a
        // separate field). The key has NO slot, so these env dims ride IN
        // the context. Non-producing: the lib-member index reducer is
        // unimplemented, so the execute arm returns Miss and never
        // admits/caches (a fabricated apparent surface would be a stub). The
        // member-facet demand the apparent surface implies is the mode-axis
        // facet mask.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ApparentType,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ApparentTypeContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_structural()),
            allowed_demand: mode_axes,
            cross_context_guard: "apparent_type_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // TemplateLiteralReduce { pattern, args, context } — folds a
        // template-literal type to its surface through the shared deferred
        // evaluator. An arg expression may resolve imported references on
        // its own step, so `R T L J` (no `P` — the reduction operates over
        // already-lowered interned arg nodes, content-version rooted via
        // ReadSetSignature, not a fresh parsed body skeleton). The key has
        // NO slot, so these env dims ride IN the context. LIVE producer.
        // Carries no `mode` and no DemandAxis — the substitution axis rides
        // on the order-significant `args` (part of identity), which the
        // DemandAxis vocabulary does not express, so `allowed_demand` is
        // empty.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::TemplateLiteralReduce,
            lifecycle: KeyLifecycle::Live,
            context_shape: "TemplateLiteralReduceContext",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_resolve()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "template_literal_reduce_do_not_warm_hit",
            admission: AdmissionSpec::Singleflight,
        },
        // FlowNarrowingAt { point, flow, context } — resolves the flow-narrowed
        // type of the value referenced at a program point (the type after
        // control-flow guard narrowing). Program analysis is the
        // widest-env operation: it walks the program point's parsed body /
        // control-flow skeleton (`P`), resolves imported references on its
        // own step (`R`), and is governed by the type / lib / project env
        // (`T L J`) — so the FULL `P R T L J` set. The key has NO slot, so
        // these env dims ride IN the context. Value domain is
        // `ProgramAnalysis` (NOT `TypeNode`): the narrowed/contextual node
        // is the program-analysis carrier. Non-producing: the flow engine
        // lands in U6, so the execute arm returns Miss and never
        // admits/caches (a fabricated narrowed node would be a stub). No
        // `mode` and no DemandAxis — narrowing is not a projection-rung
        // operation, so `allowed_demand` is empty. The per-variant
        // `flow: FlowNarrowingKey` axis (a key field, NOT a DemandAxis) plus the
        // shared `substitution` axis on `ProgramAnalysisContext` complete the
        // identity.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::FlowNarrowingAt,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProgramAnalysisContext",
            value_domain: SemanticQueryValueTag::ProgramAnalysis,
            env_dims: EnvDimSpec::Static(env_full()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "flow_narrowing_at_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // ContextualTypeAt { point, contextual, context } — resolves the contextual
        // (expected) type at a program point. Same env tier and shape as
        // FlowNarrowingAt: FULL `P R T L J` (parses the surrounding syntax,
        // resolves imported contextual signatures), no slot (env in the
        // context), `ProgramAnalysis` value domain, empty `allowed_demand`.
        // Non-producing: the contextual-typing engine lands in U6. The
        // per-variant `contextual: ContextualTypingKey` axis (a key field, NOT a
        // DemandAxis) plus the shared `substitution` axis on
        // `ProgramAnalysisContext` complete the identity.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::ContextualTypeAt,
            lifecycle: KeyLifecycle::Live,
            context_shape: "ProgramAnalysisContext",
            value_domain: SemanticQueryValueTag::ProgramAnalysis,
            env_dims: EnvDimSpec::Static(env_full()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "contextual_type_at_do_not_warm_hit",
            admission: AdmissionSpec::NonProducingPendingReducer,
        },
        // LowerLocator { key: LocatorLoweringKey } — lowers the FIXED
        // authored SHAPE of a locator-addressed body through the retained
        // decl-body snapshot. `P R T L J` — with `T` / `L` / `J`
        // SLOT-TRANSITIVE, not standalone key fields: the sealed
        // `LocatorLoweringKey` carries exactly `slot + locator + P + R`, and
        // the type-env / lib-env / project dims participate through the
        // slot's own typed env tail (`SlotEnvIdentity`), so a mixed-env key
        // is unconstructible by shape. `P` is real family identity (the
        // worker phase re-borrows the retained parse snapshot, keyed
        // `(canonical, whole_hash, parse_env_hash)` — a parse-env-only move
        // must miss). LIVE producer; strictly unsubstituted, carrier-only,
        // role-free — no mode / demand / substitution / projection axis, so
        // `allowed_demand` is empty and the family lives in the `Single`
        // slot. Substituted demands route through `Instantiate { args }`.
        SemanticQueryKeySpec {
            variant: SemanticQueryKeyTag::LowerLocator,
            lifecycle: KeyLifecycle::Live,
            context_shape: "LocatorLoweringKey",
            value_domain: SemanticQueryValueTag::TypeNode,
            env_dims: EnvDimSpec::Static(env_full()),
            allowed_demand: AxisMask::empty(),
            cross_context_guard: "lower_locator_family_distinct_by_parse_env_and_locator",
            admission: AdmissionSpec::Singleflight,
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
        SemanticQueryValueTag::BroadRuntime => "BroadRuntime",
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

    /// The `Instantiate` env-dim row is CONDITIONAL by `InstantiateBodySource`
    /// (one `Instantiate` query tag — no variant split): `FileBacked(P)` ⇒
    /// `P R T L J`; `NonFile` ⇒ `R T L J`. A single static mask is wrong in
    /// both directions — `P R T L J` over-claims for `NonFile` (R21
    /// false-miss on every parse-env-insensitive instantiation) and
    /// `R T L J` hides the file-backed parse-env dependency. Every OTHER row
    /// stays static, and the rendered table shows BOTH cases on the
    /// Instantiate row.
    #[test]
    fn instantiate_env_dims_row_is_conditional_by_body_source() {
        let specs = semantic_query_key_specs();
        let row = specs
            .iter()
            .find(|s| s.variant == SemanticQueryKeyTag::Instantiate)
            .expect("missing spec row for Instantiate");
        match row.env_dims {
            EnvDimSpec::Conditional {
                file_backed,
                non_file,
            } => {
                assert_eq!(file_backed.render(), "P R T L J");
                assert_eq!(non_file.render(), "R T L J");
            }
            EnvDimSpec::Static(_) => {
                panic!("Instantiate env-dim row must be conditional by body_source")
            }
        }
        let rendered = row.env_dims.render();
        assert!(
            rendered.contains("P R T L J") && rendered.contains("R T L J"),
            "the rendered Instantiate row must show both body_source cases, got `{rendered}`"
        );
        for other in specs
            .iter()
            .filter(|s| s.variant != SemanticQueryKeyTag::Instantiate)
        {
            assert!(
                matches!(other.env_dims, EnvDimSpec::Static(_)),
                "{:?} env-dim row must stay static — only Instantiate is \
                 conditional by body_source",
                other.variant
            );
        }
    }

    /// `EnvDimMask::render` is canonical-order and uses `—` for empty.
    #[test]
    fn env_dim_mask_renders_in_canonical_order() {
        assert_eq!(env_resolve().render(), "R T L J");
        assert_eq!(env_structural().render(), "T L J");
        // The full five-hash render still works for a `P`-bearing mask. The
        // parse-env-bearing spine rows (`ResolveClassSurface`,
        // `ResolveAmbientNamespace`, `FlowNarrowingAt`, `ContextualTypeAt`) carry
        // `P` (forward-declared);
        // here the mask is constructed directly to exercise the render.
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
