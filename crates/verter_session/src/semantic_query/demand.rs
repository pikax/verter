//! `ProjectionDemand × EvalPolicy` lattice algebra (Deliverable #3 of
//! `docs/arch/u2-query-value-domain-design.md`, §3.1–§3.7).
//!
//! This is the SELF-CONTAINED value-domain foundation: the per-field
//! lattices, the stratified product order, `meet`/`join`, the §3.4
//! materialised-record satisfaction/backfill relations, and the §3.5
//! monotone path-composition helper. It has ZERO dependencies on the
//! memo, the dispatch, `FamilyKey`, or `ModeSlot` — it is pure algebra
//! over owned, `Send + Sync` data.
//!
//! ## The order
//!
//! `⊑` reads "is dominated by / less-or-equal-demand"; `a ⊒ b ≜ b ⊑ a`.
//! Lower in the order = LESS evaluation demanded. The five `ProjectionMode`
//! presets are points in this lattice (§3.7), bridged via
//! [`From<ProjectionMode>`]; a demand that fits no preset constructs a
//! [`Demand`] directly.
//!
//! ## Stratification (§3.1.1 / §3.2)
//!
//! Three fields are **antichains** (structural discriminators, NOT depths):
//! [`GenericOpenPolicy`], [`SurfaceRole`], [`MergeRole`]. Their tuple is the
//! [`Regime`]. Two demands are comparable ONLY IF their regimes are equal;
//! across regimes there is no order, no `meet`, no `join`. Within a regime
//! the poset is a bounded meet-semilattice (`meet` total; `join` partial via
//! the path-prefix order).
//!
//! ## `display_needs` (§14)
//!
//! `display_needs` is DISPLAY-ONLY: it is part of [`ProjectionDemand`] for
//! completeness but is NOT in the [`Regime`] tuple and is unconditionally
//! masked to `⊥` by [`apply_mask`] before a demand could enter any
//! typed-value cache key.

use std::sync::Arc;

// NON-`pub`: these pre-existing types are owned by `semantic_query` and stay on
// that owner path. The demand module does NOT re-export them (§3.6): the demand
// path publishes ONLY the new lattice vocabulary, never a second export route
// for `PathSegment`/`ProjectionMode`/`IndexKey`.
use crate::semantic_query::{PathSegment, ProjectionMode};

// ---------------------------------------------------------------------------
// ProjectionPath
// ---------------------------------------------------------------------------

/// A projection path built on the existing [`PathSegment`] vocabulary.
///
/// **`ProjectionPath` IS the `Arc<[PathSegment]>` representation** (§3.1) — a
/// thin, zero-cost newtype with the prefix-order methods attached, NOT a
/// parallel/dual path representation. `SemanticQueryKey::ProjectPath.path` is
/// itself an `Arc<[PathSegment]>`; the [`From`] conversions below are O(1)
/// `Arc` clones/moves, so the semantic-query key shares this exact
/// representation with zero conversion tax. There is no second path datum to
/// keep in sync.
///
/// Equality is **structural** — two paths are equal iff their segment slices
/// are element-wise equal. The design's "prefix-interned id equality" (§3.6)
/// is an *optimization* of exactly this structural equality (the interner is
/// the normal form), so structural equality is the sound pure semantics and
/// is what this module implements.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectionPath(Arc<[PathSegment]>);

impl From<Arc<[PathSegment]>> for ProjectionPath {
    /// O(1): adopts the existing `Arc` allocation verbatim (no copy, no
    /// re-interning). `ProjectionPath` carries no datum beyond this `Arc`.
    fn from(segments: Arc<[PathSegment]>) -> Self {
        ProjectionPath(segments)
    }
}

impl From<ProjectionPath> for Arc<[PathSegment]> {
    /// O(1): hands back the wrapped `Arc` by move — the round-trip
    /// `Arc<[PathSegment]> → ProjectionPath → Arc<[PathSegment]>` is identity.
    fn from(path: ProjectionPath) -> Self {
        path.0
    }
}

impl ProjectionPath {
    /// The empty path `[]` — the `⊥` of the prefix order.
    pub fn empty() -> Self {
        ProjectionPath(Arc::from([]))
    }

    /// Borrow the underlying `Arc<[PathSegment]>` — proves there is no second
    /// representation: this IS the stored datum (O(1), no allocation).
    pub fn as_arc(&self) -> &Arc<[PathSegment]> {
        &self.0
    }

    /// Move out the underlying `Arc<[PathSegment]>` (O(1), no copy). Pairs with
    /// [`From<Arc<[PathSegment]>>`] for an identity round-trip.
    pub fn into_arc(self) -> Arc<[PathSegment]> {
        self.0
    }

    /// Build a path from an iterator of segments.
    pub fn from_segments<I: IntoIterator<Item = PathSegment>>(segments: I) -> Self {
        ProjectionPath(segments.into_iter().collect())
    }

    /// The segment slice.
    pub fn as_slice(&self) -> &[PathSegment] {
        &self.0
    }

    /// Number of segments.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the path is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Prefix order (§3.1): `self ⊑ other` iff `self` is a prefix of `other`.
    pub fn is_prefix_of(&self, other: &ProjectionPath) -> bool {
        self.0.len() <= other.0.len() && self.0[..] == other.0[..self.0.len()]
    }

    /// Longest common prefix — TOTAL (§3.1: meet on the prefix order always
    /// exists).
    pub fn longest_common_prefix(&self, other: &ProjectionPath) -> ProjectionPath {
        let n = self
            .0
            .iter()
            .zip(other.0.iter())
            .take_while(|(a, b)| a == b)
            .count();
        ProjectionPath(self.0[..n].iter().cloned().collect())
    }

    /// Prefix join (§3.1): the longer path iff one is a prefix of the other,
    /// else `None` (divergent paths have no least upper bound).
    pub fn prefix_join(&self, other: &ProjectionPath) -> Option<ProjectionPath> {
        if self.is_prefix_of(other) {
            Some(other.clone())
        } else if other.is_prefix_of(self) {
            Some(self.clone())
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Bitset axes — SurfaceFacet / DisplayFacet
// ---------------------------------------------------------------------------

/// A surface facet a [`ProjectionDemand`] may request (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceFacet {
    Members,
    IndexSignatures,
    Heritage,
    Call,
    Construct,
}

impl SurfaceFacet {
    const fn bit(self) -> u32 {
        match self {
            SurfaceFacet::Members => 1 << 0,
            SurfaceFacet::IndexSignatures => 1 << 1,
            SurfaceFacet::Heritage => 1 << 2,
            SurfaceFacet::Call => 1 << 3,
            SurfaceFacet::Construct => 1 << 4,
        }
    }

    const ALL: u32 = (1 << 5) - 1;
}

/// A `BitSet<SurfaceFacet>` ordered by subset inclusion (§3.1). `join = ∪`,
/// `meet = ∩` — both total.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct SurfaceFacetSet(u32);

impl SurfaceFacetSet {
    /// The empty set — `⊥`.
    pub fn empty() -> Self {
        SurfaceFacetSet(0)
    }

    /// Every defined facet — `⊤`.
    pub fn full() -> Self {
        SurfaceFacetSet(SurfaceFacet::ALL)
    }

    /// A singleton set.
    pub fn single(facet: SurfaceFacet) -> Self {
        SurfaceFacetSet(facet.bit())
    }

    /// Set union (`∪`, the join).
    pub fn union(self, other: SurfaceFacetSet) -> Self {
        SurfaceFacetSet(self.0 | other.0)
    }

    /// Set intersection (`∩`, the meet).
    pub fn intersect(self, other: SurfaceFacetSet) -> Self {
        SurfaceFacetSet(self.0 & other.0)
    }

    /// Membership test.
    pub fn contains(self, facet: SurfaceFacet) -> bool {
        self.0 & facet.bit() != 0
    }

    /// Subset test: `self ⊆ other`.
    pub fn is_subset_of(self, other: SurfaceFacetSet) -> bool {
        self.0 & other.0 == self.0
    }
}

/// A display facet (§14 — `DisplayNeeds`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DisplayFacet {
    ExpandAliases,
    IncludeReadonlyModifier,
    TruncateLargeUnions,
    QualifyNames,
}

impl DisplayFacet {
    const fn bit(self) -> u32 {
        match self {
            DisplayFacet::ExpandAliases => 1 << 0,
            DisplayFacet::IncludeReadonlyModifier => 1 << 1,
            DisplayFacet::TruncateLargeUnions => 1 << 2,
            DisplayFacet::QualifyNames => 1 << 3,
        }
    }

    const ALL: u32 = (1 << 4) - 1;
}

/// A `BitSet<DisplayFacet>` ordered by subset inclusion. DISPLAY-ONLY: never
/// part of the [`Regime`], always masked to `⊥` by [`apply_mask`] (§14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct DisplayNeeds(u32);

impl DisplayNeeds {
    /// The empty set — `⊥`.
    pub fn empty() -> Self {
        DisplayNeeds(0)
    }

    /// Every defined display facet — `⊤`.
    pub fn full() -> Self {
        DisplayNeeds(DisplayFacet::ALL)
    }

    /// A singleton set.
    pub fn single(facet: DisplayFacet) -> Self {
        DisplayNeeds(facet.bit())
    }

    /// Set union (`∪`, the join).
    pub fn union(self, other: DisplayNeeds) -> Self {
        DisplayNeeds(self.0 | other.0)
    }

    /// Set intersection (`∩`, the meet).
    pub fn intersect(self, other: DisplayNeeds) -> Self {
        DisplayNeeds(self.0 & other.0)
    }

    /// Membership test.
    pub fn contains(self, facet: DisplayFacet) -> bool {
        self.0 & facet.bit() != 0
    }

    /// Subset test: `self ⊆ other`.
    pub fn is_subset_of(self, other: DisplayNeeds) -> bool {
        self.0 & other.0 == self.0
    }
}

// ---------------------------------------------------------------------------
// Total-chain EvalPolicy / ProjectionDemand fields
//
// Each derives Ord with variants declared in ASCENDING demand order, so
// `std::cmp::min`/`max` give the per-field glb/lub directly (§3.3).
// ---------------------------------------------------------------------------

/// Whether a member's body is demanded in addition to the member set (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MemberBodyDemand {
    /// Member set only (`⊥`).
    SetOnly,
    /// Member set plus each demanded member body (`⊤`).
    SetPlusBody,
}

/// Whether to keep an alias `Ref` or inline its body (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum AliasPreservation {
    /// Keep the `Ref{name}` (`⊥`).
    Keep,
    /// Inline the alias body (`⊤`).
    Inline,
}

/// Normalization depth — total chain (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NormalizationDepth {
    None,
    NavigateOnly,
    Terminal,
    Deep,
}

/// Operator reduction depth — total chain (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OperatorReduction {
    /// Leave the operator carrier (`Pick<…>` stays `Ref`) — `⊥`.
    Leave,
    /// Navigate through the carrier without materialising its surface.
    NavigateOnly,
    /// Reduce the operator into its produced surface — `⊤`.
    Reduce,
}

/// Whether evaluation continues past a carrier or stops at it (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CarrierStopPolicy {
    /// Stop at the carrier (`⊥`).
    StopAtCarrier,
    /// Continue past the carrier (`⊤`).
    Continue,
}

/// Whether declaration provenance is retained (§3.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ProvenanceNeed {
    /// Drop provenance (`⊥`).
    Drop,
    /// Retain provenance (`⊤`).
    Retain,
}

// ---------------------------------------------------------------------------
// Antichain (regime) fields — NO Ord (incomparable, §3.1.1)
// ---------------------------------------------------------------------------

/// Generic-opening regime (§3.1.1) — an **antichain**: `Bound` and
/// `TypeParamShells` answer different questions and are incomparable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericOpenPolicy {
    /// Instantiate type parameters with their bound substitution args.
    Bound,
    /// Leave unbound parameters as `TypeParam` shells (Skeleton regime).
    TypeParamShells,
}

/// Surface role (§3.1) — a **flat antichain**: a structural discriminator,
/// not a depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceRole {
    Plain,
    Prop,
    Emit,
    Model,
    Slot,
    Option,
}

/// Merge role (§3.1) — a **flat antichain** (structural discriminator).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MergeRole {
    Standalone,
    Heritage,
    WithDefaults,
    IntersectionArm,
}

/// The regime tuple `(generic_open, surface_role, merge_role)` (§3.2). Two
/// demands are comparable ONLY IF their regimes are equal.
pub type Regime = (GenericOpenPolicy, SurfaceRole, MergeRole);

// ---------------------------------------------------------------------------
// ProjectionDemand / EvalPolicy / Demand
// ---------------------------------------------------------------------------

/// The projection-shape half of a demand (§3.1 first table).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ProjectionDemand {
    /// Path under the prefix order.
    pub path: ProjectionPath,
    /// Requested surface facets (subset order).
    pub facets: SurfaceFacetSet,
    /// Member set vs member set + bodies.
    pub member_demand: MemberBodyDemand,
    pub call_signatures: bool,
    pub construct_signatures: bool,
    pub index_signatures: bool,
    /// DISPLAY-ONLY (§14): not in the [`Regime`], masked to `⊥` before any
    /// typed-value cache key.
    pub display_needs: DisplayNeeds,
}

/// The evaluation-policy half of a demand (§3.1 second table).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct EvalPolicy {
    pub alias_preservation: AliasPreservation,
    pub normalization_depth: NormalizationDepth,
    /// Antichain regime field (§3.1.1).
    pub generic_open: GenericOpenPolicy,
    pub operator_reduction: OperatorReduction,
    pub carrier_stop: CarrierStopPolicy,
    /// Antichain regime field.
    pub surface_role: SurfaceRole,
    pub provenance: ProvenanceNeed,
    /// Antichain regime field.
    pub merge_role: MergeRole,
}

/// A demand point: `(ProjectionDemand, EvalPolicy)` (§3.2).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Demand {
    pub projection: ProjectionDemand,
    pub policy: EvalPolicy,
}

impl Demand {
    /// The regime tuple (§3.2) — the three antichain fields.
    pub fn regime(&self) -> Regime {
        (
            self.policy.generic_open,
            self.policy.surface_role,
            self.policy.merge_role,
        )
    }

    /// `self.dominates(other)` ≡ `other ⊑ self` over the FULL §3.2 product
    /// order — componentwise (INCLUDING `display_needs`), and ONLY within a
    /// shared regime (cross-regime ⇒ never dominates). This is the order the
    /// display sub-lattice and `meet`/`join` operate over; for warm-hit reuse
    /// use [`Demand::semantically_dominates`] instead (§14.1: `display_needs`
    /// must not gate typed-value reuse).
    pub fn dominates(&self, other: &Demand) -> bool {
        // display_needs ⊆ test is the ONLY clause `semantically_dominates`
        // omits; the full order adds it back.
        self.semantically_dominates(other)
            && other
                .projection
                .display_needs
                .is_subset_of(self.projection.display_needs)
    }

    /// `self ⊒ other` on the TYPED-VALUE (semantic) axes only — identical to
    /// [`Demand::dominates`] but with the display-only `display_needs` clause
    /// REMOVED (§14.1 invariant: `display_needs` never drives resolution, so
    /// two demands differing only in `display_needs` must share the cached
    /// typed value). [`cached_satisfies`] uses THIS, never the full order.
    pub fn semantically_dominates(&self, other: &Demand) -> bool {
        if self.regime() != other.regime() {
            return false;
        }
        let sp = &self.projection;
        let op = &other.projection;
        // path: other ⊑ self ⟺ other.path is a prefix of self.path
        op.path.is_prefix_of(&sp.path)
            // facets: other ⊆ self
            && op.facets.is_subset_of(sp.facets)
            // total-chain / bool fields: self ⊒ other
            && sp.member_demand >= op.member_demand
            && sp.call_signatures >= op.call_signatures
            && sp.construct_signatures >= op.construct_signatures
            && sp.index_signatures >= op.index_signatures
            && self.policy.alias_preservation >= other.policy.alias_preservation
            && self.policy.normalization_depth >= other.policy.normalization_depth
            && self.policy.operator_reduction >= other.policy.operator_reduction
            && self.policy.carrier_stop >= other.policy.carrier_stop
            && self.policy.provenance >= other.policy.provenance
        // display_needs DELIBERATELY OMITTED (§14.1) — regime fields equal by
        // the regime() guard above.
    }

    /// Greatest lower bound within a regime; `None` across regimes (§3.3).
    /// Total within a regime (every component meet is total —
    /// `longest_common_prefix` always exists).
    pub fn meet(a: &Demand, b: &Demand) -> Option<Demand> {
        if a.regime() != b.regime() {
            return None;
        }
        Some(Demand {
            projection: ProjectionDemand {
                path: a.projection.path.longest_common_prefix(&b.projection.path),
                facets: a.projection.facets.intersect(b.projection.facets),
                member_demand: a.projection.member_demand.min(b.projection.member_demand),
                call_signatures: a.projection.call_signatures && b.projection.call_signatures,
                construct_signatures: a.projection.construct_signatures
                    && b.projection.construct_signatures,
                index_signatures: a.projection.index_signatures && b.projection.index_signatures,
                display_needs: a
                    .projection
                    .display_needs
                    .intersect(b.projection.display_needs),
            },
            policy: EvalPolicy {
                alias_preservation: a.policy.alias_preservation.min(b.policy.alias_preservation),
                normalization_depth: a
                    .policy
                    .normalization_depth
                    .min(b.policy.normalization_depth),
                generic_open: a.policy.generic_open, // equal by regime
                operator_reduction: a.policy.operator_reduction.min(b.policy.operator_reduction),
                carrier_stop: a.policy.carrier_stop.min(b.policy.carrier_stop),
                surface_role: a.policy.surface_role, // equal by regime
                provenance: a.policy.provenance.min(b.policy.provenance),
                merge_role: a.policy.merge_role, // equal by regime
            },
        })
    }

    /// Least upper bound (§3.3) — PARTIAL: `None` across regimes AND when the
    /// paths diverge ([`ProjectionPath::prefix_join`] returns `None`).
    pub fn join(a: &Demand, b: &Demand) -> Option<Demand> {
        if a.regime() != b.regime() {
            return None;
        }
        let path = a.projection.path.prefix_join(&b.projection.path)?;
        Some(Demand {
            projection: ProjectionDemand {
                path,
                facets: a.projection.facets.union(b.projection.facets),
                member_demand: a.projection.member_demand.max(b.projection.member_demand),
                call_signatures: a.projection.call_signatures || b.projection.call_signatures,
                construct_signatures: a.projection.construct_signatures
                    || b.projection.construct_signatures,
                index_signatures: a.projection.index_signatures || b.projection.index_signatures,
                display_needs: a.projection.display_needs.union(b.projection.display_needs),
            },
            policy: EvalPolicy {
                alias_preservation: a.policy.alias_preservation.max(b.policy.alias_preservation),
                normalization_depth: a
                    .policy
                    .normalization_depth
                    .max(b.policy.normalization_depth),
                generic_open: a.policy.generic_open,
                operator_reduction: a.policy.operator_reduction.max(b.policy.operator_reduction),
                carrier_stop: a.policy.carrier_stop.max(b.policy.carrier_stop),
                surface_role: a.policy.surface_role,
                provenance: a.policy.provenance.max(b.policy.provenance),
                merge_role: a.policy.merge_role,
            },
        })
    }

    // -- Presets (§3.7) ----------------------------------------------------

    /// `Identity` preset (§3.7 row 1): empty path, no member/body, no sigs;
    /// `Bound/Plain/Standalone`, `Keep`, `None`(norm), `Leave`(op),
    /// `Continue`. The least-demand published point of its regime (empty path,
    /// no member/body demand, `Keep`); note `carrier_stop = Continue` per the
    /// §3.7 Identity row, whereas `CarrierStopPolicy`'s declared bottom is
    /// `StopAtCarrier`, so Identity is NOT the componentwise `⊥` on the
    /// `carrier_stop` axis.
    pub fn identity() -> Demand {
        Demand {
            projection: ProjectionDemand {
                path: ProjectionPath::empty(),
                facets: SurfaceFacetSet::empty(),
                member_demand: MemberBodyDemand::SetOnly,
                call_signatures: false,
                construct_signatures: false,
                index_signatures: false,
                display_needs: DisplayNeeds::empty(),
            },
            policy: EvalPolicy {
                alias_preservation: AliasPreservation::Keep,
                normalization_depth: NormalizationDepth::None,
                generic_open: GenericOpenPolicy::Bound,
                operator_reduction: OperatorReduction::Leave,
                carrier_stop: CarrierStopPolicy::Continue,
                surface_role: SurfaceRole::Plain,
                provenance: ProvenanceNeed::Drop,
                merge_role: MergeRole::Standalone,
            },
        }
    }

    /// `Navigate` preset (§3.7 row 2 / §3.5 `NAVIGATE_PRESET`):
    /// `facets={Members}`, `SetOnly`; `Keep`, `NavigateOnly`(norm),
    /// `op=NavigateOnly`, `Continue`. The next-hop chooser; non-owning
    /// normalization only.
    pub fn navigate(path: ProjectionPath) -> Demand {
        Demand {
            projection: ProjectionDemand {
                path,
                facets: SurfaceFacetSet::single(SurfaceFacet::Members),
                member_demand: MemberBodyDemand::SetOnly,
                call_signatures: false,
                construct_signatures: false,
                index_signatures: false,
                display_needs: DisplayNeeds::empty(),
            },
            policy: EvalPolicy {
                alias_preservation: AliasPreservation::Keep,
                normalization_depth: NormalizationDepth::NavigateOnly,
                generic_open: GenericOpenPolicy::Bound,
                operator_reduction: OperatorReduction::NavigateOnly,
                carrier_stop: CarrierStopPolicy::Continue,
                surface_role: SurfaceRole::Plain,
                provenance: ProvenanceNeed::Drop,
                merge_role: MergeRole::Standalone,
            },
        }
    }

    /// `Shallow` preset (§3.7 row 3): empty path, `facets={Members}`,
    /// `SetOnly`; `Keep`, `None`(norm), `op=Leave`, `Continue`.
    pub fn shallow() -> Demand {
        Demand {
            projection: ProjectionDemand {
                path: ProjectionPath::empty(),
                facets: SurfaceFacetSet::single(SurfaceFacet::Members),
                member_demand: MemberBodyDemand::SetOnly,
                call_signatures: false,
                construct_signatures: false,
                index_signatures: false,
                display_needs: DisplayNeeds::empty(),
            },
            policy: EvalPolicy {
                alias_preservation: AliasPreservation::Keep,
                normalization_depth: NormalizationDepth::None,
                generic_open: GenericOpenPolicy::Bound,
                operator_reduction: OperatorReduction::Leave,
                carrier_stop: CarrierStopPolicy::Continue,
                surface_role: SurfaceRole::Plain,
                provenance: ProvenanceNeed::Drop,
                merge_role: MergeRole::Standalone,
            },
        }
    }

    /// `Expanded` preset (§3.7 row 4): terminal path, `facets⊇{Members}`,
    /// `SetPlusBody`; `Inline`, `Terminal`(norm), `op=Reduce`, `Continue`.
    pub fn expanded(path: ProjectionPath) -> Demand {
        Demand {
            projection: ProjectionDemand {
                path,
                facets: SurfaceFacetSet::single(SurfaceFacet::Members),
                member_demand: MemberBodyDemand::SetPlusBody,
                call_signatures: false,
                construct_signatures: false,
                index_signatures: false,
                display_needs: DisplayNeeds::empty(),
            },
            policy: EvalPolicy {
                alias_preservation: AliasPreservation::Inline,
                normalization_depth: NormalizationDepth::Terminal,
                generic_open: GenericOpenPolicy::Bound,
                operator_reduction: OperatorReduction::Reduce,
                carrier_stop: CarrierStopPolicy::Continue,
                surface_role: SurfaceRole::Plain,
                provenance: ProvenanceNeed::Drop,
                merge_role: MergeRole::Standalone,
            },
        }
    }

    /// `Skeleton` preset (§3.7 row 5): BFS surface, `SetOnly`;
    /// `TypeParamShells/Plain/Standalone`, `Keep`, `op=Leave`,
    /// `carrier_stop=StopAtCarrier`. `generic_open = TypeParamShells` ⇒ a
    /// DIFFERENT REGIME ⇒ incomparable to every preset above. It is NOT a
    /// special-cased sixth mode — it is exactly this point (§3.1.1).
    pub fn skeleton() -> Demand {
        Demand {
            projection: ProjectionDemand {
                path: ProjectionPath::empty(),
                facets: SurfaceFacetSet::empty(),
                member_demand: MemberBodyDemand::SetOnly,
                call_signatures: false,
                construct_signatures: false,
                index_signatures: false,
                display_needs: DisplayNeeds::empty(),
            },
            policy: EvalPolicy {
                alias_preservation: AliasPreservation::Keep,
                normalization_depth: NormalizationDepth::None,
                generic_open: GenericOpenPolicy::TypeParamShells,
                operator_reduction: OperatorReduction::Leave,
                carrier_stop: CarrierStopPolicy::StopAtCarrier,
                surface_role: SurfaceRole::Plain,
                provenance: ProvenanceNeed::Drop,
                merge_role: MergeRole::Standalone,
            },
        }
    }

    /// Copy the three regime fields from `terminal` onto `self` (§3.5
    /// `with_regime`). Everything else is preserved.
    pub fn with_regime(mut self, terminal: &Demand) -> Demand {
        self.policy.generic_open = terminal.policy.generic_open;
        self.policy.surface_role = terminal.policy.surface_role;
        self.policy.merge_role = terminal.policy.merge_role;
        self
    }
}

/// The preset bridge (§3.7 closing note): each [`ProjectionMode`] resolves to
/// its preset point. NON-INVASIVE — does not change `ProjectionMode` and does
/// not touch any cache key. Path-bearing presets (`Navigate`/`Expanded`) use
/// the empty path; the concrete path is the orthogonal `path` axis supplied at
/// key-construction time.
impl From<ProjectionMode> for Demand {
    fn from(mode: ProjectionMode) -> Self {
        match mode {
            ProjectionMode::Identity => Demand::identity(),
            ProjectionMode::Navigate => Demand::navigate(ProjectionPath::empty()),
            ProjectionMode::Shallow => Demand::shallow(),
            ProjectionMode::Expanded => Demand::expanded(ProjectionPath::empty()),
            ProjectionMode::Skeleton => Demand::skeleton(),
        }
    }
}

// ---------------------------------------------------------------------------
// Monotone path composition (§3.5)
// ---------------------------------------------------------------------------

/// The demand at hop `i` of an `n`-hop path projection (§3.5).
///
/// Intermediate hops (`i < n-1`) run the `NAVIGATE_PRESET` carrying the
/// CONCRETE walked prefix `terminal.path[0..=i]` (§3.5: `NAVIGATE_PRESET` has
/// `path = the single hop` — the prefix walked so far), with the terminal's
/// regime copied on (`with_regime`). The terminal hop (`i == n-1`) is the
/// caller's `terminal` demand verbatim. `n` MUST equal `terminal.path.len()`
/// (each hop consumes one terminal-path segment), so `[0..=i]` indexing is
/// always in range.
///
/// This is **order-preserving in the terminal** (§3.5 proof): for any two
/// terminals sharing a regime AND a path, every intermediate hop is IDENTICAL
/// (the prefix `[0..=i]` is the same, the regime is the same, and the eval
/// rungs come entirely from the constant `NAVIGATE_PRESET` — NOT from the
/// terminal's rungs), so widening the leaf eval never widens an intermediate
/// slice. The recorded `(prefix, Navigate)` hops are therefore reusable across
/// terminal modes (§3.4 corollary). See the
/// `demand_at_hop_is_monotone_in_the_terminal` guard.
pub fn demand_at_hop(i: usize, n: usize, terminal: &Demand) -> Demand {
    debug_assert_eq!(
        n,
        terminal.projection.path.len(),
        "demand_at_hop: n must equal the terminal path length (one hop per segment)"
    );
    if i + 1 < n {
        let prefix = ProjectionPath::from_segments(
            terminal.projection.path.as_slice()[..=i].iter().cloned(),
        );
        Demand::navigate(prefix).with_regime(terminal)
    } else {
        terminal.clone()
    }
}

// ---------------------------------------------------------------------------
// Axis minimality + normalization (§3.6)
// ---------------------------------------------------------------------------

/// A demand axis a family may branch on (§3.6). `display_needs` is
/// DELIBERATELY ABSENT: it can never be declared relevant, so it is
/// structurally impossible to keep it in a key — [`apply_mask`] always zeroes
/// it (§14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemandAxis {
    Path,
    Facets,
    MemberBody,
    CallSignatures,
    ConstructSignatures,
    IndexSignatures,
    AliasPreservation,
    NormalizationDepth,
    GenericOpen,
    OperatorReduction,
    CarrierStop,
    SurfaceRole,
    Provenance,
    MergeRole,
}

impl DemandAxis {
    const fn bit(self) -> u32 {
        match self {
            DemandAxis::Path => 1 << 0,
            DemandAxis::Facets => 1 << 1,
            DemandAxis::MemberBody => 1 << 2,
            DemandAxis::CallSignatures => 1 << 3,
            DemandAxis::ConstructSignatures => 1 << 4,
            DemandAxis::IndexSignatures => 1 << 5,
            DemandAxis::AliasPreservation => 1 << 6,
            DemandAxis::NormalizationDepth => 1 << 7,
            DemandAxis::GenericOpen => 1 << 8,
            DemandAxis::OperatorReduction => 1 << 9,
            DemandAxis::CarrierStop => 1 << 10,
            DemandAxis::SurfaceRole => 1 << 11,
            DemandAxis::Provenance => 1 << 12,
            DemandAxis::MergeRole => 1 << 13,
        }
    }

    const ALL: u32 = (1 << 14) - 1;

    /// Every axis in canonical declaration (bit) order. The single ordered
    /// enumeration any renderer iterates.
    ///
    /// What is enforced: [`bit`](Self::bit) and [`name`](Self::name) are
    /// exhaustive `match`es (each gains a compiler-forced arm per variant), and
    /// the `_ORDERED_COVERS_ALL` assertion below pins this slice's combined
    /// bit-union to the manually-maintained [`DemandAxis::ALL`]. The assertion
    /// catches an ORDERED/ALL mismatch (a variant appended to one but not the
    /// other). It does NOT prove enum cardinality: a variant added with `bit()`
    /// and `name()` updated but BOTH `ORDERED` and `ALL` forgotten still
    /// compiles.
    pub const ORDERED: &'static [DemandAxis] = &[
        DemandAxis::Path,
        DemandAxis::Facets,
        DemandAxis::MemberBody,
        DemandAxis::CallSignatures,
        DemandAxis::ConstructSignatures,
        DemandAxis::IndexSignatures,
        DemandAxis::AliasPreservation,
        DemandAxis::NormalizationDepth,
        DemandAxis::GenericOpen,
        DemandAxis::OperatorReduction,
        DemandAxis::CarrierStop,
        DemandAxis::SurfaceRole,
        DemandAxis::Provenance,
        DemandAxis::MergeRole,
    ];

    /// The canonical render token / name for this axis. Exhaustive `match`:
    /// adding a variant is a compile error until an arm is added here.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            DemandAxis::Path => "Path",
            DemandAxis::Facets => "Facets",
            DemandAxis::MemberBody => "MemberBody",
            DemandAxis::CallSignatures => "CallSignatures",
            DemandAxis::ConstructSignatures => "ConstructSignatures",
            DemandAxis::IndexSignatures => "IndexSignatures",
            DemandAxis::AliasPreservation => "AliasPreservation",
            DemandAxis::NormalizationDepth => "NormalizationDepth",
            DemandAxis::GenericOpen => "GenericOpen",
            DemandAxis::OperatorReduction => "OperatorReduction",
            DemandAxis::CarrierStop => "CarrierStop",
            DemandAxis::SurfaceRole => "SurfaceRole",
            DemandAxis::Provenance => "Provenance",
            DemandAxis::MergeRole => "MergeRole",
        }
    }
}

/// Compile-time gate: [`DemandAxis::ORDERED`] must list every axis whose bit
/// is in [`DemandAxis::ALL`]. A new variant added to the full-mask without
/// being appended to `ORDERED` (or vice versa) fails to compile here.
const _ORDERED_COVERS_ALL: () = {
    let mut union: u32 = 0;
    let mut i = 0;
    while i < DemandAxis::ORDERED.len() {
        union |= DemandAxis::ORDERED[i].bit();
        i += 1;
    }
    assert!(
        union == DemandAxis::ALL,
        "DemandAxis::ORDERED must list exactly the axes whose bits compose \
         DemandAxis::ALL"
    );
};

/// The set of demand axes a family branches on (§3.6). Axes NOT in the mask
/// are zeroed to their `⊥` before a demand enters that family's key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct AxisMask(u32);

impl AxisMask {
    /// No axes — every axis normalizes to `⊥`.
    pub fn empty() -> Self {
        AxisMask(0)
    }

    /// Every declarable axis (note: `display_needs` is never an axis, so it is
    /// still masked out by [`apply_mask`] even under `full`).
    pub fn full() -> Self {
        AxisMask(DemandAxis::ALL)
    }

    /// Add an axis.
    pub fn with(self, axis: DemandAxis) -> Self {
        AxisMask(self.0 | axis.bit())
    }

    /// Remove an axis.
    pub fn without(self, axis: DemandAxis) -> Self {
        AxisMask(self.0 & !axis.bit())
    }

    /// Membership test.
    pub fn contains(self, axis: DemandAxis) -> bool {
        self.0 & axis.bit() != 0
    }

    /// Union of two masks.
    pub fn union(self, other: AxisMask) -> Self {
        AxisMask(self.0 | other.0)
    }

    /// Intersection of two masks.
    pub fn intersect(self, other: AxisMask) -> Self {
        AxisMask(self.0 & other.0)
    }
}

/// Build an [`AxisMask`] from a family's declared relevant axes (§3.6). Since
/// `display_needs` is not a [`DemandAxis`], it can never be declared relevant.
pub fn relevant_demand_axes(declared: &[DemandAxis]) -> AxisMask {
    declared.iter().fold(AxisMask::empty(), |m, a| m.with(*a))
}

/// Normalize `d` against `mask` (§3.6): every axis NOT in `mask` is reset to
/// its `⊥`, and `display_needs` is ALWAYS reset to `⊥` (the §14 invariant).
///
/// IDEMPOTENT: `apply_mask(apply_mask(d, m), m) == apply_mask(d, m)` — a
/// reset field is already at `⊥`, and kept fields are unchanged.
///
/// This is the SHAPE of the benched `cache_key_axes_are_minimal_and_normalized`
/// contract; the cache-runtime guards exercise it under cache pressure.
pub fn apply_mask(d: &Demand, mask: &AxisMask) -> Demand {
    let mut out = d.clone();

    // §14: display_needs is masked out unconditionally — even under the full
    // mask — because it is never a semantic (typed-value) axis.
    out.projection.display_needs = DisplayNeeds::empty();

    if !mask.contains(DemandAxis::Path) {
        out.projection.path = ProjectionPath::empty();
    }
    if !mask.contains(DemandAxis::Facets) {
        out.projection.facets = SurfaceFacetSet::empty();
    }
    if !mask.contains(DemandAxis::MemberBody) {
        out.projection.member_demand = MemberBodyDemand::SetOnly;
    }
    if !mask.contains(DemandAxis::CallSignatures) {
        out.projection.call_signatures = false;
    }
    if !mask.contains(DemandAxis::ConstructSignatures) {
        out.projection.construct_signatures = false;
    }
    if !mask.contains(DemandAxis::IndexSignatures) {
        out.projection.index_signatures = false;
    }
    if !mask.contains(DemandAxis::AliasPreservation) {
        out.policy.alias_preservation = AliasPreservation::Keep;
    }
    if !mask.contains(DemandAxis::NormalizationDepth) {
        out.policy.normalization_depth = NormalizationDepth::None;
    }
    if !mask.contains(DemandAxis::GenericOpen) {
        out.policy.generic_open = GenericOpenPolicy::Bound;
    }
    if !mask.contains(DemandAxis::OperatorReduction) {
        out.policy.operator_reduction = OperatorReduction::Leave;
    }
    if !mask.contains(DemandAxis::CarrierStop) {
        out.policy.carrier_stop = CarrierStopPolicy::StopAtCarrier;
    }
    if !mask.contains(DemandAxis::SurfaceRole) {
        out.policy.surface_role = SurfaceRole::Plain;
    }
    if !mask.contains(DemandAxis::Provenance) {
        out.policy.provenance = ProvenanceNeed::Drop;
    }
    if !mask.contains(DemandAxis::MergeRole) {
        out.policy.merge_role = MergeRole::Standalone;
    }
    out
}

// ---------------------------------------------------------------------------
// §3.4 carrier types + PURE satisfaction / backfill
// ---------------------------------------------------------------------------

/// One materialised record the compute ACTUALLY produced (§3.4). The carried
/// [`Demand`] is regime-tagged, and the record's path IS the demand's
/// projection path.
///
/// **Invariant: the record's path and its demand's path are the same datum**
/// (§3.4). The illegal `path != point.projection.path` state is
/// UNREPRESENTABLE: the only stored field is the [`Demand`], and [`path`] is
/// DERIVED from `point.projection.path` rather than settable independently.
/// Because the two can never diverge, [`cached_satisfies`] may match path-exact
/// on [`path`] and then trust [`Demand::semantically_dominates`] (whose own
/// path test is prefix-based) without a deep inner demand forging a hit at a
/// shallow path it never materialised.
///
/// [`path`]: MaterializedPoint::path
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedPoint {
    point: Demand,
}

impl MaterializedPoint {
    /// Build a record from the demand its compute materialised. The record's
    /// path is the demand's own `projection.path` — the single source of truth,
    /// so the outer path can never diverge from the inner demand.
    pub fn new(point: Demand) -> Self {
        MaterializedPoint { point }
    }

    /// The materialised path — DERIVED from `point.projection.path` (the single
    /// source of truth). O(1) borrow, no allocation.
    pub fn path(&self) -> &ProjectionPath {
        &self.point.projection.path
    }

    /// The materialised demand point (regime-tagged).
    pub fn point(&self) -> &Demand {
        &self.point
    }
}

/// The set of points a cached candidate's compute actually produced (§3.4) —
/// NOT its nominal request demand. Recorded by the compute itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedSet(pub Arc<[MaterializedPoint]>);

impl Default for MaterializedSet {
    #[inline]
    fn default() -> Self {
        MaterializedSet::empty()
    }
}

impl MaterializedSet {
    /// The empty record set — a build that materialised nothing the memo
    /// can reuse (post-processing replaces an empty set with the single
    /// terminal point for the canonical key, so an empty set never reaches
    /// the warm-hit gate of a published entry).
    #[inline]
    #[must_use]
    pub fn empty() -> Self {
        MaterializedSet(Arc::from([] as [MaterializedPoint; 0]))
    }

    /// Build a set from a single materialised point.
    #[inline]
    #[must_use]
    pub fn single(point: MaterializedPoint) -> Self {
        MaterializedSet(Arc::from([point]))
    }

    /// Build a set from an iterator of materialised points.
    #[must_use]
    pub fn from_points<I: IntoIterator<Item = MaterializedPoint>>(points: I) -> Self {
        MaterializedSet(points.into_iter().collect())
    }

    /// The recorded points (borrow).
    #[inline]
    #[must_use]
    pub fn points(&self) -> &[MaterializedPoint] {
        &self.0
    }

    /// Whether the set is empty.
    #[inline]
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A warm hit serves `requested` iff SOME recorded materialised point
/// SEMANTICALLY dominates it AT THE SAME PATH (structural path equality — NOT
/// prefix; a `Navigate` hop a deep compute walked never proves it expanded that
/// path). Regime fields must be equal (incomparable regimes never satisfy).
///
/// Uses [`Demand::semantically_dominates`], NOT the full [`Demand::dominates`]:
/// `display_needs` is display-only and must NEVER gate typed-value reuse
/// (§14.1 — two queries differing only in `display_needs` share the cached
/// typed value). PURE — no host/cache state (§3.4).
pub fn cached_satisfies(satisfied: &MaterializedSet, requested: &MaterializedPoint) -> bool {
    satisfied.0.iter().any(|m| {
        m.path() == requested.path() && m.point().semantically_dominates(requested.point())
    })
}

/// Backfill writes the RECORDED materialised points verbatim — never a
/// meet-derived or nominal-request point (§3.4).
pub fn backfill_points(satisfied: &MaterializedSet) -> &[MaterializedPoint] {
    &satisfied.0
}
