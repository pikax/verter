//! Relation engine — the SOLE relation authority, riding
//! `execute(SemanticQueryKey::Relate)` on the cold-compute frame of the ONE
//! resolver (design `.claude/skills/type-resolution/SKILL.md`).
//!
//! Every relation judgement — top-level consumer asks, conditional branch
//! selection, `Extract`/`Exclude` per-arm filtering, the oracle adapter,
//! AND every recursive assignability sub-relation — re-enters the SAME
//! full-key authority [`ProjectSemanticDispatch::execute_relate`]. Comparable
//! member descent stays inside its root frame's iterative budget.
//!
//! The two descents are DELIBERATELY not one walker. Assignability answers
//! "is every source inhabitant a target inhabitant"; comparability answers
//! "is there one inhabitant of both". They agree on nothing below the root:
//! a union SOURCE conjoins for assignability and disjoins for comparability,
//! an extra source member is an excess-property question for one and
//! irrelevant to the other, and optional members are skipped by one and
//! variance-checked by the other. Sharing a descent would mean one walker
//! branching on the relation kind at every hop — a second engine wearing one
//! name. What they DO share is the single proven-disjoint tag oracle
//! ([`super::canonical_algebra::tag_level_disjoint`]) and the single nominal
//! leaf, which is where a drift between them would actually be a defect.
//! Extending object descent (index signatures, `ObjectSpreadProgram`,
//! `Alias` / `MergedDecl` decomposition) must therefore be decided per
//! descent, not assumed to propagate.
//!
//! The divergence extends past the descent SHAPE into cycle discipline.
//! Assignability re-enters the full authority for every sub-relation, so a
//! re-entered pair goes through the transaction's obligation/re-discharge
//! machinery. Comparability's member descent stays inside ONE frame, so it
//! carries its own coinductive assumption: a pair re-entered while still
//! being decided is assumed to OVERLAP. That direction is the safe one — both
//! folds propagate permissiveness, so an assumption can only miss a
//! disjointness proof, never mint one, and a frame-local memo entry finished
//! under an assumption carries only that same permissiveness forward. Only
//! the ROOT pair is published to the shared memo, so no assumed sub-result
//! escapes the frame that made the assumption.
//!
//! Disjointness proofs and intersection collapse are two different jobs.
//! The oracle SUPPLIES the proof — concrete tag conflicts (delegated to the
//! crate's sole proven-disjoint tag oracle, so it cannot drift from the
//! canonical intersection collapse), the nominal axis, and two structural
//! surfaces carrying the same REQUIRED member with disjoint values (a
//! COMPOSED root — an intersection body, an object-spread program — is
//! composed into its one-level surface first, so an `A & { kind: "a" }`
//! versus `A & { kind: "b" }` conflict is still proved, at any member
//! depth). Reducing a provably disjoint intersection to `never` is the
//! canonical algebra's decision: the proof carries the checker's collapse
//! class for the pair ([`DisjointnessProof::checker_reduces_intersection_to_never`]),
//! and a consumer narrows to `never` only on the checker-compatible
//! unit-discriminant criteria — disjoint tags, distinct `unique symbol`
//! identities, or a conflicting shared REQUIRED member whose values are
//! both unit types. A conflict reachable only through non-unit member
//! values keeps `A & B`, exactly as `tsc`'s `getNarrowedType` keeps it.
//!
//! There is one reentry substrate, not a second engine: the per-transaction
//! [`super::dispatch_txn::CheckerDispatchTransaction`] provides the
//! obligation/re-discharge machinery and the coinductive assumption, and
//! decided binary judgements admit into the `Relate` family slot and
//! warm-serve through the standard family read.
//!
//! Admission (design §2.3 / Decision 4): a pure non-binding SCC closes at
//! SCC-close (positive ⇒ `Assignable` + `CoinductiveCycle`; a negative
//! non-assumptive obligation ⇒ publishable `NotAssignable`); any
//! `Unknown` / budget edge routes the WHOLE component through `ReturnOnly`
//! — `Unknown` is NEVER admitted anywhere (memo / fact / reverse index),
//! and a public `BudgetExceeded` payload is returned to the caller with
//! admission suppressed (three-layer non-admission). The
//! `shallow_relation_check` prefilter survives ONLY as the O(tag) fast
//! reject INSIDE this authority (RI-5), never a parallel truth source.
//!
//! Reverse-mapped recovery's input preflight, precision boundary, opaque-state
//! polarity, and fixture ledger live in `/type-resolution` under
//! "Reverse-homomorphic mapped recovery".

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use super::dispatch_txn::{
    provisional_relate_step, redischarge_is_stable, select_inference_candidates,
    CompletedResolveCallMember, CompletedSccMember, FlowReturnPendingOutcome, InferenceInfoSetup,
    InferenceOccurrence, InferenceSession, InferenceSessionSetup, InferenceSessionState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
    PendingVerdict, ProvisionalSubstitution, ProvisionalVerdict, RelationEnvironment,
    RelationFrameState, RelationPendingState, RelationStep, ResolveCallPendingState,
    ReverseProjectionState, ReverseRecoveredEntry, SessionCheckpoint, StrictFamilyConfig,
};
use super::relation_predicates::*;
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, DeclIdentity, IndexKey, InferBinding,
    InferenceCandidatePriority, InferencePassKind, LiteralValue, NoInferMask, OptionalityMod,
    PrimitiveKind, ProjectionReductionContext, QueryError, QueryResult, ReadonlyMod,
    RecursionOrBudgetCap, RelateKeyId, RelateMemoKey, RelationContext, RelationFailureCode,
    RelationKind, RelationOutcome, RelationPayload, RelationPolicy, RelationProof, RelationResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue, SubRelationPosition, SubRelationRef, SurfaceView, VariancePhase,
};
use crate::semantic_query_memo::InlineMemberFlight;

#[cfg(test)]
std::thread_local! {
    static REDISCHARGE_EXECUTE_VISITS: std::cell::Cell<usize> =
        const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn redischarge_execute_visits_for_tests() -> usize {
    REDISCHARGE_EXECUTE_VISITS.get()
}

#[cfg(test)]
pub(super) fn record_redischarge_execute_visit_for_tests() {
    REDISCHARGE_EXECUTE_VISITS.set(REDISCHARGE_EXECUTE_VISITS.get() + 1);
}

/// The O(tag) fast-reject prefilter verdict (RI-5) — the retired
/// `shallow_relation_check`, surviving ONLY as a tag-only prefilter inside
/// the relation authority. `Unknown` falls through to the full structural
/// reducer; the decided arms short-circuit BEFORE any recursive
/// structural work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ShallowRelation {
    Assignable,
    NotAssignable,
    Unknown,
}

/// Which in-scope inference position a pattern-side `Infer` occupies —
/// drives the candidate's priority rung and combination variance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InferPosition {
    /// Covariant pattern position (object property, tuple/array element,
    /// bare) — the `Argument` rung.
    Covariant,
    /// Function parameter — the `Argument` rung, contravariant
    /// combination.
    ContravariantParam,
    /// Function return — the `ReturnType` rung, covariant combination.
    Return,
}

fn inference_occurrence_for_position(
    ambient: InferenceOccurrence,
    position: InferPosition,
) -> InferenceOccurrence {
    match position {
        // Structural object/array/tuple/index descent preserves the complete
        // occurrence selected by its enclosing relation position.
        InferPosition::Covariant => ambient,
        // Entering a function parameter flips orientation and starts the
        // ordinary argument-priority rung.
        InferPosition::ContravariantParam => InferenceOccurrence {
            priority: InferenceCandidatePriority::Argument,
            variance: match ambient.variance {
                VariancePhase::Covariant => VariancePhase::Contravariant,
                VariancePhase::Contravariant => VariancePhase::Covariant,
                VariancePhase::Invariant => VariancePhase::Invariant,
            },
        },
        // A return changes the priority rung while preserving the enclosing
        // orientation (including a return nested inside a parameter).
        InferPosition::Return => InferenceOccurrence {
            priority: InferenceCandidatePriority::ReturnType,
            variance: ambient.variance,
        },
    }
}

/// The shape of an in-scope conditional-`infer` pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferPatternShape {
    /// `T extends infer X`.
    Bare,
    /// `T extends { a: infer U, .. }` — direct `Infer` member values.
    ObjectProps,
    /// `T extends [infer H, .., ...infer Rest]` — direct `Infer` elements.
    TupleHeadTail,
    /// `T extends (infer U)[]` — a direct `Infer` array element (the
    /// `Flatten` class).
    ArrayElement,
    /// `T extends (p: infer U, ..) => infer R` — direct `Infer`
    /// parameter / return positions.
    Function,
    /// `{ [P in keyof infer T]: X }` with no key remap.
    ReverseHomomorphicMapped,
}

/// Mapped modifiers whose inverse metadata effect is applied while the
/// source shape is reconstructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ReverseMappedModifiers {
    pub(crate) optionality: OptionalityMod,
    pub(crate) readonly: ReadonlyMod,
}

/// Exact descriptor for a reverse-homomorphic mapped target.
#[derive(Debug, Clone)]
pub(crate) struct ReverseHomomorphicSpec {
    pub(crate) mapped_node: SemanticNodeId,
    pub(crate) base_infer: SemanticNodeId,
    pub(crate) mapper_parameter: SemanticNodeId,
    pub(crate) template: SemanticNodeId,
    pub(crate) modifiers: ReverseMappedModifiers,
}

enum ReverseSourceShape {
    Object,
    Array { readonly: bool },
    Tuple { readonly: bool },
}

fn reverse_optional(observed: bool, modifier: OptionalityMod) -> Option<bool> {
    match modifier {
        OptionalityMod::Add => Some(false),
        OptionalityMod::Keep => Some(observed),
        OptionalityMod::Remove => (!observed).then_some(false),
    }
}

fn reverse_readonly(observed: bool, modifier: ReadonlyMod) -> Option<bool> {
    match modifier {
        ReadonlyMod::Add => Some(false),
        ReadonlyMod::Keep => Some(observed),
        ReadonlyMod::Remove => Some(observed),
    }
}

/// One side of a signature relation's result: the return, and the type
/// predicate that replaces it when the target narrows.
#[derive(Debug, Clone, Copy)]
pub(super) struct FunctionResult {
    pub(super) return_type: SemanticNodeId,
    pub(super) predicate: Option<crate::semantic_query::SignaturePredicate>,
}

/// One inferable parameter discovered in a pattern.
#[derive(Debug, Clone)]
pub(super) struct InferParamSite {
    /// The `Infer` node (content-free parameter identity).
    node: SemanticNodeId,
    /// The parameter display name.
    name: Arc<str>,
    /// The highest rung this site's position admits.
    priority: InferenceCandidatePriority,
}

/// The detected pattern payload: shape plus the one frozen session setup
/// shared by key construction and session opening.
#[derive(Debug, Clone)]
pub(crate) struct InferPatternInfo {
    pub(crate) shape: InferPatternShape,
    setup: InferenceSessionSetup,
    reverse_homomorphic: Option<ReverseHomomorphicSpec>,
}

impl InferPatternInfo {
    fn new(
        shape: InferPatternShape,
        sites: Vec<InferParamSite>,
        reverse_homomorphic: Option<ReverseHomomorphicSpec>,
    ) -> Self {
        let pass_kind = if reverse_homomorphic.is_some() {
            InferencePassKind::ReverseHomomorphicMapped
        } else {
            InferencePassKind::Ordinary
        };
        let candidate_priority = sites
            .iter()
            .map(|site| site.priority)
            .max_by_key(|priority| crate::semantic_query::inference_candidate_precedence(*priority))
            .unwrap_or(InferenceCandidatePriority::Argument);
        let infos = Arc::from(
            sites
                .into_iter()
                .map(|site| InferenceInfoSetup::new(site.node, site.name))
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        Self {
            shape,
            setup: InferenceSessionSetup::new(
                infos,
                VariancePhase::Covariant,
                pass_kind,
                candidate_priority,
                NoInferMask::empty(),
                ConstParamPolicy::NonConst,
                ContextualInferenceMode::None,
            ),
            reverse_homomorphic,
        }
    }
}

/// The relation-payload bindings a binding-producing judgement fixed at
/// session close, plus the pattern shape that produced them (the
/// closedness classifiers widen non-`Bare` shapes to `Deferred`).
#[derive(Debug, Clone)]
pub(crate) struct RelationInferBindings {
    pub(crate) shape: InferPatternShape,
    pub(crate) bindings: Arc<[InferBinding]>,
}

/// The outcome of closing the machinery ROOT frame of a relation.
enum RootClose {
    /// A decided binary judgement — publish the payload.
    Decided(RelationPayload),
    /// A decided judgement whose mixed component consumed UNPROVEN
    /// flow-member values — PUBLIC payload, never admitted (the
    /// flow-poisoned twin of the budget arm's ReturnOnly-but-public).
    DecidedReturnOnly(RelationPayload),
    /// Budget exhaustion — PUBLIC payload, never admitted (three-layer
    /// non-admission).
    BudgetExceeded(RelationPayload),
    /// No public value-domain form (Unknown / poisoned SCC) — `Miss`.
    Undecided,
}

#[derive(Clone)]
struct ProjectedRelationMember {
    key: crate::semantic_query::PropertyKey,
    presence: crate::semantic_query::PositiveKeyPresence,
    value: crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
}

#[derive(Clone)]
struct ProjectedRelationIndex {
    key_type: SemanticNodeId,
    value: crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
}

#[derive(Clone)]
struct ProjectedRelationBranch {
    members: Vec<ProjectedRelationMember>,
    indices: Vec<ProjectedRelationIndex>,
    call_signatures: Vec<SemanticNodeId>,
    construct_signatures: Vec<SemanticNodeId>,
    open: bool,
}

pub(super) type DischargedMember = (
    RelateMemoKey,
    InferenceOccurrence,
    PendingVerdict,
    bool,
    bool,
    Option<super::dispatch_txn::SessionId>,
    Option<InlineMemberFlight>,
);

/// A relation-domain view over a drained tagged pending member: the SCC
/// close's verdict algebra operates on this shape; the tagged
/// `PendingObligation` storage lives in the generic ledger.
pub(super) struct DrainedRelationMember {
    pub(super) key: RelateMemoKey,
    pub(super) occurrence: InferenceOccurrence,
    pub(super) verdict: PendingVerdict,
    pub(super) session_delta: bool,
    pub(super) opened_session: Option<super::dispatch_txn::SessionId>,
    pub(super) inline_flight: Option<InlineMemberFlight>,
}

/// A flow-return-domain view over a drained tagged pending member. The
/// outcome is final at pop (a same-slot recursive backedge is a
/// coinductive hold decided by the seed check); the close fails the whole
/// tagged component on a `NoValue` outcome, and admits an evaluated member
/// ONLY through its own finalizer proof.
pub(super) struct DrainedFlowReturnMember {
    pub(super) key: crate::semantic_query::FlowReturnKey,
    pub(super) outcome: super::dispatch_txn::FlowReturnPendingOutcome,
    /// The refusal recorded when this member's own demand could not be
    /// planned — the batch reports it so the root classifies by the
    /// cause that actually refused, not by its own clean preparation.
    pub(super) plan_refusal: Option<super::dispatch_txn::flow_obligation_state::FlowPlanRefusal>,
    pub(super) inline_flight: Option<crate::semantic_query_memo::InlineMemberFlight>,
    /// The coinductive hold targets the member's evaluation met — the SCC
    /// close discharges an empty-cycle member on its targets' admitted
    /// returns.
    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
    /// The member's own file roots (unioned into the published component's
    /// self-roots).
    pub(super) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The materialised point set the member's compute ACTUALLY produced
    /// (§3.4) — carried to the fenced member publish.
    pub(super) materialized: crate::semantic_query::demand::MaterializedSet,
    /// Whether the member's own contributors were all FRESH literals —
    /// the post-convergence literal-widening input.
    pub(super) fresh_seed: bool,
    /// The member's own installed demand carrier (handle + plan +
    /// provenance), when its demand was prepared. The member finalizes
    /// against EXACTLY this demand at the close.
    pub(super) flow_demand: Option<super::dispatch_txn::flow_obligation_state::FlowDemandCarrier>,
    /// The member's typed discharge report, produced once by its
    /// evaluation and applied centrally at the close.
    pub(super) discharge: Option<super::dispatch_txn::flow_obligation_state::FlowDischargeReport>,
    /// The member's OWN evaluation provenance, carried through the
    /// deferral: finalization triangulates it against the carrier (a
    /// same-store, same-generation FOREIGN demand's evidence is refused)
    /// rather than reconstructing it from the carrier — that comparison
    /// would be tautological.
    pub(super) provenance: super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance,
}

type DrainedCallResult = (
    crate::semantic_query::ResolveCallKey,
    ResolveCallPendingState,
    crate::semantic_query::ResolvedCallResult,
);

/// The mixed component's discharge result: the prefix entries' outcomes,
/// the call members' results, and the runtime-observed convergence of the
/// joint fixed point (the flow-side passes the discharge actually ran).
type MixedDischargeResult = Result<
    (
        Vec<FlowReturnPendingOutcome>,
        Vec<DrainedCallResult>,
        super::dispatch_txn::flow_obligation_state::ObservedFlowConvergence,
    ),
    crate::semantic_query::ResolveCallFailure,
>;

/// The relation-root outcome of [`ProjectSemanticDispatch::relation_discharge_and_route`].
pub(super) struct RelationDischargeOutcome {
    /// The machinery relation root's family payload (its build output).
    pub(super) self_publish: Option<RelationPayload>,
    /// The caller-return step of an inline relation SCC root (or of a
    /// session-delta root, which never publishes).
    pub(super) self_step: Option<RelationStep>,
    /// One or more DRAINED flow members finalized UNPROVEN, so the whole
    /// member batch was refused (the torn-component rule). The mixed
    /// equation already consumed those members' evaluated values, so the
    /// root's own outcome is NON-ADMISSIBLE too: every consumer treats
    /// `self_publish` / `self_step` as ReturnOnly — the verdict still
    /// flows to the caller, and nothing warms around an unproven
    /// flow-derived value.
    pub(super) flow_batch_unproven: bool,
    /// The union of the partiality classes the refused members' recorded
    /// causes belong to — empty when the batch is proven, or when the
    /// refusal carried no cause. The root unions this into its own
    /// class: its consumers must see a member's budget edge or torn view
    /// as the faulting class it is, not as the contained unverified
    /// class the root's own clean preparation would report.
    pub(super) flow_batch_partial_reasons: crate::semantic_query::PartialReasonSet,
}

impl<'a> ProjectSemanticDispatch<'a> {
    // ──────────────────────────────────────────────────────────────────
    // The sole relation authority
    // ──────────────────────────────────────────────────────────────────

    /// Ergonomic pair constructor (design Decision 5 — a pure-delegation
    /// helper, owning ZERO memoization / cycle / assumption / admission
    /// logic): constructs the full default assignability key for
    /// `(source, target)` and delegates to [`Self::execute_relate`].
    pub(crate) fn execute_relate_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationStep {
        self.execute_relate(self.relate_key_for(source, target))
    }

    /// [`Self::execute_relate_pair`] for a NON-default relation kind — the
    /// same pure-delegation helper, keyed on the asked relation.
    pub(crate) fn execute_relate_pair_kind(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
    ) -> RelationStep {
        self.execute_relate(self.relate_key_for_kind(source, target, relation))
    }

    /// Ask the shared authority whether `a` and `b` can have a common
    /// inhabitant ([`RelationKind::Comparable`]).
    ///
    /// The three verdicts are exactly the relation's three, and a consumer
    /// must treat [`ComparabilityVerdict::Undecided`] as "no fact" — never
    /// as either answer — and must never re-derive the judgement locally.
    /// A `Disjoint` verdict carries the proof AND the checker's intersection
    /// collapse class for the pair; the consumer reads
    /// [`DisjointnessProof::checker_reduces_intersection_to_never`] instead
    /// of deciding collapse itself.
    pub(crate) fn nodes_comparable(
        &self,
        a: SemanticNodeId,
        b: SemanticNodeId,
    ) -> ComparabilityVerdict {
        match self.execute_relate_pair_kind(a, b, RelationKind::Comparable) {
            RelationStep::Assignable { .. } => ComparabilityVerdict::Overlaps,
            RelationStep::NotAssignable => ComparabilityVerdict::Disjoint(DisjointnessProof::new(
                self.checker_intersection_collapse(a, b),
            )),
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed(_) => {
                ComparabilityVerdict::Undecided
            }
        }
    }

    /// Classify how the checker reduces an intersection of a PROVED-disjoint
    /// pair — minted only for a pair the `Comparable` reduction already
    /// answered negative, as the payload of that proof.
    ///
    /// This is not a second disjointness oracle: it never decides WHETHER
    /// the pair is disjoint (that verdict arrived), only WHICH disjointness
    /// shapes the checker's intersection reducer collapses. It reads the
    /// pair's top-level node shapes through the same tag oracle, nominal
    /// identities, and one-level member surfaces the reduction used, so it
    /// stays warm-safe — the class is a pure function of the pair, not of
    /// the descent that proved it.
    ///
    /// The checker's criteria (`tsc` `getNarrowedType` /
    /// `isTypeDisjointTo`): disjoint primitive/literal tags and distinct
    /// `unique symbol` identities are unit-discriminant conflicts, as is a
    /// shared member that is not optional on both sides and whose types
    /// make a discriminant with an empty intersection
    /// ([`Self::discriminant_members_conflict`]). A conflict reachable only
    /// through member values that are not literal types — at ANY depth — is
    /// a real disjointness proof whose intersection the checker KEEPS.
    /// Unions distribute the decision: every alternative pair must satisfy
    /// a collapse criterion, else the intersection is kept.
    ///
    /// Runs OUTSIDE the relation budget, including for warm `Comparable`
    /// hits: the class is a pure function of the pair's top-level shapes —
    /// structurally bounded (at most two widen retries, flat union arms,
    /// unit conflicts bottom out) with family-memoized inner reads — and it
    /// runs only for a pair the reduction ALREADY answered negative, so it
    /// cannot widen the work any unanswered pair performs.
    fn checker_intersection_collapse(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> IntersectionCollapse {
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return IntersectionCollapse::Kept,
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return IntersectionCollapse::Kept,
        };
        if let Some(leaf) =
            self.relation_nominal_leaf(source, target, RelationKind::Comparable, &[])
        {
            // The nominal leaf DECIDED the pair negative exactly when the
            // two declaring identities are distinct — a unit-discriminant
            // conflict between the only nominal unit types TypeScript has.
            if let NominalLeaf::Decided(RelationResult::NotAssignable) = leaf {
                return IntersectionCollapse::ReducesToNever;
            }
            // A widened retry re-asks the pair on the ordinary lattice.
            if let NominalLeaf::Retry(widened_source, widened_target) = leaf {
                return self.checker_intersection_collapse(widened_source, widened_target);
            }
            return IntersectionCollapse::Kept;
        }
        let (Some(source_data), Some(target_data)) = (
            self.graph().node_data(source),
            self.graph().node_data(target),
        ) else {
            return IntersectionCollapse::Kept;
        };
        if let SemanticNodeData::Union(members) = &*source_data {
            let members = members.members_arc();
            return self.union_collapse_class(members.iter().copied(), target);
        }
        if let SemanticNodeData::Union(members) = &*target_data {
            let members = members.members_arc();
            return self.union_collapse_class(members.iter().copied(), source);
        }
        if super::canonical_algebra::tag_level_disjoint(self.graph(), source, target)
            || super::canonical_algebra::scalar_domain_provably_empty(
                self.graph(),
                &[source, target],
            )
        {
            // Disjoint tags at the TOP level of the two operands: an empty
            // primitive/literal intersection, which the checker's
            // intersection reducer collapses to `never` — as it does two
            // distinct unit types (an enum member's literal and a plain
            // literal of its value).
            return IntersectionCollapse::ReducesToNever;
        }
        // Two structural surfaces: the checker collapses only on a shared
        // member that makes an empty discriminant. Any other member conflict
        // — nested descent, non-literal values — leaves `A & B` standing.
        let (ComparableSurface::Object(source_view), ComparableSurface::Object(target_view)) = (
            self.comparable_surface(source),
            self.comparable_surface(target),
        ) else {
            return IntersectionCollapse::Kept;
        };
        for source_member in source_view.positive_members() {
            let Some(member_key) = source_member.key.cloned_known() else {
                continue;
            };
            let crate::semantic_query::SurfaceKeyProjection::Exact(target_member) =
                target_view.project_known_key(&member_key)
            else {
                continue;
            };
            // The intersection's member is optional only when both are, and
            // an optional member never reduces the intersection.
            if source_member.optional && target_member.optional {
                continue;
            }
            if self.discriminant_members_conflict(
                (source_member.value, source_member.optional),
                (target_member.value, target_member.optional),
            ) {
                return IntersectionCollapse::ReducesToNever;
            }
        }
        IntersectionCollapse::Kept
    }

    /// Every alternative must satisfy a collapse criterion for the union's
    /// intersection to reduce; one kept alternative leaves the intersection
    /// standing, exactly as the checker composes per-arm narrowing.
    fn union_collapse_class(
        &self,
        arms: impl Iterator<Item = SemanticNodeId>,
        other: SemanticNodeId,
    ) -> IntersectionCollapse {
        let mut arms = arms.peekable();
        if arms.peek().is_none() {
            return IntersectionCollapse::Kept;
        }
        for arm in arms {
            if matches!(
                self.checker_intersection_collapse(arm, other),
                IntersectionCollapse::Kept
            ) {
                return IntersectionCollapse::Kept;
            }
        }
        IntersectionCollapse::ReducesToNever
    }

    /// Whether a shared member of two surfaces is an EMPTY DISCRIMINANT —
    /// the checker's `isDiscriminantWithNeverType`: at least one side's
    /// type is a literal type (a unit type — a literal, a `unique symbol`,
    /// `null`, `undefined` — `boolean`, or a union of those) and the two
    /// types share no value, every pair of their arms being disjoint. An
    /// optional side contributes its type plus `undefined` under
    /// `strictNullChecks`. Measured on the pinned checker: `{ v: number }
    /// & { v: "b" }`, `{ v: "a" | undefined } & { v: "b" }`, `{ v?: "a" }
    /// & { v: "b" }` and `{ v: boolean } & { v: 1 }` are `never`, while
    /// `{ v: string } & { v: number }` (no literal side) and `{ v: "a" |
    /// "b" } & { v: "b" | "d" }` (a shared `"b"`) are kept. Bounded to one
    /// hop: an arm pair is decided by the tag / nominal arms, and an object
    /// arm is never disjoint from a literal (the pair is kept).
    fn discriminant_members_conflict(
        &self,
        (left, left_optional): (SemanticNodeId, bool),
        (right, right_optional): (SemanticNodeId, bool),
    ) -> bool {
        // A member value is the type the relation read for it: an indexed
        // access or a reference that reads a literal IS that literal
        // (`v: Boxed['k']` over `k: 'a'` is the unit `'a'`).
        let read = |node: SemanticNodeId| match self.unwrap_identity_carrier_for_relation(node) {
            IdentityCarrierUnwrap::Concrete(read) => read,
            IdentityCarrierUnwrap::Unresolvable => node,
        };
        let strict_null_checks = self
            .dispatch_txn
            .borrow()
            .relation
            .strict
            .unwrap_or(StrictFamilyConfig::TS_STRICT)
            .strict_null_checks;
        let arms_of = |node: SemanticNodeId, optional: bool| {
            let node = read(node);
            let mut arms: Vec<SemanticNodeId> = match self.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Union(members)) => {
                    members.members_arc().iter().map(|arm| read(*arm)).collect()
                }
                _ => vec![node],
            };
            if optional && strict_null_checks {
                arms.push(
                    self.graph()
                        .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)),
                );
            }
            arms
        };
        let (left, right) = (arms_of(left, left_optional), arms_of(right, right_optional));
        let unit = |node: &SemanticNodeId| {
            matches!(
                self.graph().node_data(*node).as_deref(),
                Some(
                    SemanticNodeData::Literal(_)
                        | SemanticNodeData::TypeOfNominal(_)
                        | SemanticNodeData::Primitive(
                            PrimitiveKind::Null | PrimitiveKind::Undefined | PrimitiveKind::Boolean
                        )
                )
            )
        };
        if !left.iter().all(unit) && !right.iter().all(unit) {
            return false;
        }
        left.iter().all(|a| {
            right.iter().all(|b| {
                matches!(
                    self.checker_intersection_collapse(*a, *b),
                    IntersectionCollapse::ReducesToNever
                )
            })
        })
    }

    /// Test-support adapter mapping the authority's step onto the
    /// reducer's tri-state lattice so legacy verdict assertions keep
    /// their shape. `Assumed` / `BudgetExceeded` collapse onto `Unknown`
    /// (both are non-decided from a consumer's perspective).
    #[cfg(any(test, feature = "test-support"))]
    #[doc(hidden)]
    pub fn execute_relate_pair_as_result_for_tests(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationResult {
        match self.execute_relate_pair(source, target) {
            RelationStep::Assignable { bindings } => RelationResult::Assignable { bindings },
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed(_) => {
                RelationResult::Unknown
            }
        }
    }

    #[cfg(test)]
    pub fn redischarge_execute_visits_for_tests(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> usize {
        let before = redischarge_execute_visits_for_tests();
        let _ = self.relation_redischarge(
            &self.relate_key_for(source, target),
            InferenceOccurrence::ARGUMENT_COVARIANT,
            &ProvisionalSubstitution::default(),
        );
        redischarge_execute_visits_for_tests() - before
    }

    /// Exercise a one-member cyclic binding judgement through the real
    /// frame-close/fixation/re-discharge path. `negative` changes only a
    /// fixed tuple obligation, so both polarities still collect and fix
    /// the same direct-infer candidate before SCC close.
    #[cfg(test)]
    pub fn binding_scc_discharge_for_tests(
        &self,
        negative: bool,
    ) -> (RelationOutcome, Arc<[InferBinding]>, usize) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("CyclicBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let tuple = |first, second| {
            graph.intern_node(SemanticNodeData::Tuple {
                elements: Arc::from(
                    vec![
                        crate::semantic_query::TupleElement {
                            label: None,
                            value: first,
                            optional: false,
                            rest: false,
                        },
                        crate::semantic_query::TupleElement {
                            label: None,
                            value: second,
                            optional: false,
                            rest: false,
                        },
                    ]
                    .into_boxed_slice(),
                ),
                readonly: true,
            })
        };
        let source = tuple(string, if negative { string } else { number });
        let target = tuple(infer, number);
        let key = self.relation_key_with_inference(self.relate_key_for(source, target));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let idx = self.relation_frame_open(&key, occurrence);
        {
            // A self edge is enough to select the cyclic discharge branch;
            // the reducer below remains the real positive/negative binding
            // judgement.
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.record_assumption(idx);
        }
        let mut bindings = Vec::new();
        let verdict = self.reduce_relation(&key, &mut bindings);
        let before = redischarge_execute_visits_for_tests();
        let payload = match self.relation_frame_close_root(idx, verdict, bindings) {
            RootClose::Decided(payload) => payload,
            other => panic!(
                "cyclic binding fixture must decide, got {}",
                match other {
                    RootClose::BudgetExceeded(_) => "BudgetExceeded",
                    RootClose::Undecided => "Undecided",
                    RootClose::DecidedReturnOnly(_) => "DecidedReturnOnly",
                    RootClose::Decided(_) => unreachable!(),
                }
            ),
        };
        (
            payload.outcome,
            Arc::clone(&payload.bindings),
            redischarge_execute_visits_for_tests() - before,
        )
    }

    /// Exercise a mixed SCC whose root fixes one binding while a nested
    /// non-binding member closes negative against an assumption edge.
    #[cfg(test)]
    pub fn mixed_binding_scc_discharge_for_tests(
        &self,
    ) -> (RelationOutcome, Arc<[InferBinding]>, usize) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("MixedCyclicBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let root_key = self.relation_key_with_inference(self.relate_key_for(string, infer));
        let member_key = self.relate_key_for(string, number);
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let root_idx = self.relation_frame_open(&root_key, occurrence);
        let member_idx = self.relation_frame_open(&member_key, occurrence);
        {
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.record_assumption(root_idx);
        }
        let mut member_bindings = Vec::new();
        let member_verdict = self.reduce_relation(&member_key, &mut member_bindings);
        let member_step = self.relation_frame_close(member_idx, member_verdict, member_bindings);
        assert!(matches!(member_step, RelationStep::NotAssignable));

        let mut root_bindings = Vec::new();
        let root_verdict = self.reduce_relation(&root_key, &mut root_bindings);
        let before = redischarge_execute_visits_for_tests();
        let payload = match self.relation_frame_close_root(root_idx, root_verdict, root_bindings) {
            RootClose::Decided(payload) => payload,
            other => panic!(
                "mixed cyclic binding fixture must decide, got {}",
                match other {
                    RootClose::BudgetExceeded(_) => "BudgetExceeded",
                    RootClose::Undecided => "Undecided",
                    RootClose::DecidedReturnOnly(_) => "DecidedReturnOnly",
                    RootClose::Decided(_) => unreachable!(),
                }
            ),
        };
        let result = (
            payload.outcome,
            Arc::clone(&payload.bindings),
            redischarge_execute_visits_for_tests() - before,
        );
        self.relation_abort_completed_members();
        result
    }

    /// Re-discharge a binding SCC consumer whose structural child edge is
    /// already fixed in the SCC substitution table. The returned tuple
    /// exposes the consumed binding snapshot and the real stability gate.
    #[cfg(test)]
    pub fn binding_scc_substitution_edge_for_tests(&self) -> (Arc<[InferBinding]>, bool) {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let unknown = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
        let infer = graph.intern_node(SemanticNodeData::Infer {
            name: Arc::from("SubstitutionEdgeBinding"),
            binder: graph.alloc_infer_binder_id(),
        });
        let member = |value, readonly| crate::semantic_query::SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string("value"),
            value,
            optional: false,
            readonly,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        };
        let source = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(vec![member(string, false)].into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ));
        let target = graph.intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(vec![member(unknown, true)].into_boxed_slice()),
                call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::<crate::semantic_query::IndexSignature>::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        ));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let binding = InferBinding {
            param: infer,
            name: Arc::from("SubstitutionEdgeBinding"),
            bound: string,
        };
        let fixed = Arc::from(vec![binding].into_boxed_slice());
        let substitution = ProvisionalSubstitution::from_iter([(
            ObligationIdentity::Relate {
                key: self.relate_key_for(string, unknown),
                occurrence,
            },
            ProvisionalVerdict::Relate(RelationStep::Assignable {
                bindings: Arc::clone(&fixed),
            }),
        )]);
        let rerun = self.relation_redischarge(
            &self.relate_key_for(source, target),
            occurrence,
            &substitution,
        );
        let bindings = match &rerun {
            PendingVerdict::Assignable { bindings } => Arc::clone(bindings),
            other => panic!("substitution-edge redischarge must stay assignable, got {other:?}"),
        };
        let provisional = PendingVerdict::Assignable { bindings: fixed };
        let stable = redischarge_is_stable(&provisional, &rerun);
        (bindings, stable)
    }

    /// Exercise the production nested-frame registration path for a
    /// non-binding relation member.
    #[cfg(test)]
    pub fn nested_nonbinding_frame_registers_inline_flight_for_tests(&self) -> bool {
        let graph = self.graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
        let occurrence = InferenceOccurrence::ARGUMENT_COVARIANT;
        let root_key = self.relate_key_for(string, string);
        let member_key = self.relate_key_for(string, number);
        let root_idx = self.relation_frame_open(&root_key, occurrence);
        let member_idx = self.relation_frame_open(&member_key, occurrence);
        let member_step =
            self.relation_frame_close(member_idx, RelationResult::NotAssignable, Vec::new());
        assert!(matches!(member_step, RelationStep::NotAssignable));
        let registered = self
            .dispatch_txn
            .borrow()
            .relation
            .completed_members
            .last()
            .is_some_and(|member| member.inline_flight.is_some());
        let root_close =
            self.relation_frame_close_root(root_idx, assignable(&Vec::new()), Vec::new());
        assert!(matches!(root_close, RootClose::Decided(_)));
        self.relation_abort_completed_members();
        registered
    }

    /// The full default relation identity for `(source, target)` under the
    /// REQUEST project's `R T L J` env ([`Self::relation_environment`]),
    /// the EMPTY canonical substitution, and the structural-transit
    /// reduction context. The project's effective type options reach the
    /// key twice, as two projections of one option set: the strictness
    /// family is folded into that project's `type_env_hash`, and the
    /// variance regime it selects into the policy.
    pub(crate) fn relate_key_for(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelateMemoKey {
        self.relate_key_for_kind(source, target, RelationKind::Assignable)
    }

    /// [`Self::relate_key_for`] for a NON-default relation kind. The env,
    /// substitution, reduction context, and strict-family policy are the
    /// SAME projection — only the relation axis differs, so the identity /
    /// comparability judgements over a node pair occupy their own memo
    /// slots beside the assignability one instead of aliasing it.
    pub(crate) fn relate_key_for_kind(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
    ) -> RelateMemoKey {
        let relation_env = self.relation_environment();
        let context = RelationContext {
            resolve_env_hash: relation_env.env.resolve_env_hash,
            type_env_hash: relation_env.env.type_env_hash,
            lib_env_hash: relation_env.env.lib_env_hash,
            project_identity: relation_env.project_identity,
            substitution: crate::semantic_query::SubstitutionCanonicalHash::empty(),
            projection_reduction: ProjectionReductionContext::structural_transit(),
            semantic_context: crate::semantic_query::SemanticContextId::production(),
        };
        let mut key = RelateMemoKey::for_kind(source, target, relation, context);
        key.policy = RelationPolicy {
            variance: relation_env.strict.variance_policy(),
            ..RelationPolicy::default()
        };
        key
    }

    /// The relation environment a judgement opened at this point is
    /// decided under — the `R/T/L/J` env and the effective strict-family
    /// configuration of the project owning the file whose answer is being
    /// decided.
    ///
    /// Inside a flow-return frame that file is the frame's function's own
    /// file (the innermost [`Self::relation_env_scope`] entry), so an
    /// inferred return is decided under its own project's options whichever
    /// request demanded it. Outside every frame it is the request's
    /// canonical, resolved ONCE per dispatch from the published project
    /// tables (the same tables the env hashes were composed from); a
    /// dispatch running outside any request (a bare test dispatch, a direct
    /// non-audited caller) runs the workspace default — TypeScript's
    /// default options under the workspace-default env — never a project it
    /// was not asked for.
    pub(crate) fn relation_environment(&self) -> RelationEnvironment {
        if let Some(scoped) = self.relation_env_scope.borrow().last() {
            return *scoped;
        }
        *self.relation_env.get_or_init(|| {
            let host = self.ctx.host_for_fact_tracer_install();
            match crate::request_context::current_request_canonical() {
                Some(canonical) => self.relation_environment_for(&canonical),
                None => RelationEnvironment {
                    env: host.host_view_env_hashes(),
                    project_identity: host.host_view_project_identity().0,
                    strict: StrictFamilyConfig::TS_STRICT,
                },
            }
        })
    }

    /// The relation environment of the project owning `canonical`: its
    /// published `R/T/L/J` env and its effective strict-family options.
    pub(super) fn relation_environment_for(&self, canonical: &str) -> RelationEnvironment {
        let host = self.ctx.host_for_fact_tracer_install();
        RelationEnvironment {
            env: host.host_view_env_hashes_for(canonical),
            project_identity: host.host_view_project_identity_for(canonical).0,
            strict: StrictFamilyConfig::from_options(
                &host.semantic_compiler_options_for(canonical),
            ),
        }
    }

    /// Decide every relation opened inside `scope` under the environment of
    /// the project owning `canonical` — the file whose answer the enclosed
    /// work decides.
    ///
    /// A relation root snapshots the strict-family configuration its
    /// reducer branches on, and a relation opened while an outer root is
    /// in flight reads that snapshot. The scope therefore also installs
    /// its own configuration as the snapshot for its length and restores
    /// the outer one after, so a judgement nested under an outer root is
    /// still decided under the options its key was built from.
    pub(super) fn with_relation_environment_of<T>(
        &self,
        canonical: &str,
        scope: impl FnOnce() -> T,
    ) -> T {
        let cached = self.relation_env_by_file.borrow().get(canonical).copied();
        let environment = cached.unwrap_or_else(|| {
            let environment = self.relation_environment_for(canonical);
            self.relation_env_by_file
                .borrow_mut()
                .insert(Arc::from(canonical), environment);
            environment
        });
        let _environment = super::RelationEnvironmentScope::push(
            &self.relation_env_scope,
            &self.dispatch_txn,
            environment,
        );
        scope()
    }

    /// The strict-family configuration in force for this dispatch — the
    /// request project's effective tsconfig options
    /// ([`Self::relation_environment`]), which the reducer snapshots at
    /// every relation root.
    pub(crate) fn relation_strict_config(&self) -> StrictFamilyConfig {
        self.relation_environment().strict
    }

    /// THE relation authority (design §2.1–§2.3). Every relation judgement
    /// enters here with a full §2.7 identity:
    ///
    /// 1. **Reentry intercept** — the identity is already in flight on
    ///    this transaction ⇒ record the scoped assumption edge and return
    ///    the `Assumed` sentinel (no recompute, no self-await, no warm
    ///    consult — the coinductive "assume it holds" step).
    /// 2. **Warm read** — a validated published payload (decided binary
    ///    outcomes only; `BudgetExceeded` / `Unknown` are never warm).
    /// 3. **Cold compute** — the machinery ROOT goes through the family
    ///    singleflight (`execute(Relate)` → `build_relate`); a nested
    ///    sub-relation computes INLINE on the transaction (its publish is
    ///    batched at its SCC's close and drained by the root).
    pub(crate) fn execute_relate(&self, key: RelateMemoKey) -> RelationStep {
        self.execute_relate_with_occurrence(key, InferenceOccurrence::ARGUMENT_COVARIANT)
    }

    /// Execute one relation under a transient inference occurrence. The
    /// occurrence is deliberately excluded from the persistent memo key:
    /// it changes only session-local candidate deposits. It is included in
    /// reentry identity so opposite-orientation visits cannot intercept one
    /// another while a reverse-inference session is active.
    fn execute_relate_with_occurrence(
        &self,
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
    ) -> RelationStep {
        let graph = self.graph();
        graph.record_relation_check();
        // Binding-producing upgrade: an in-scope `infer` pattern on the
        // target opens this judgement under the pattern's immutable
        // session-setup fingerprint. Reverse-projection sub-relations retain
        // their outer session context even though their immediate target is
        // no longer the mapped root.
        let key = self.relation_key_with_inference(key);
        // (1) Reentry intercept.
        {
            let identity = ObligationIdentity::Relate {
                key: key.clone(),
                occurrence,
            };
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(idx) = txn.reentry().find(&identity) {
                let evidence = txn.reentry().assumption_evidence(idx);
                txn.obligations.record_assumption(idx);
                return RelationStep::Assumed(evidence);
            }
        }
        // (2) Warm read (generation-gated, carrier-validated). An active
        // inference session must execute the relation so its transient
        // projection/direct-infer deposits occur; a persistent binary warm
        // verdict cannot stand in for those session-local effects.
        if self.dispatch_txn.borrow().active_session().is_none() {
            if let Some(payload) = graph.get_relation_payload(self.ctx, &key) {
                return relation_step_from_payload(&payload);
            }
        }
        // (3) Cold compute. Root versus inline is decided by the generic
        // obligation transaction: any open frame — of any domain — makes
        // this judgement inline.
        if self.dispatch_txn.borrow().obligations.decides_root() {
            self.execute_relate_root(key)
        } else {
            self.execute_relate_inline(key, occurrence)
        }
    }

    pub(super) fn relation_redischarge_active(&self) -> bool {
        self.dispatch_txn
            .borrow()
            .relation
            .redischarge_occurrence
            .is_some()
    }

    /// Producer body used only by the `SemanticQueryApi::execute(Relate)`
    /// redischarge branch. It deliberately runs inline and every frame opened
    /// under the transient redischarge context is ReturnOnly.
    pub(super) fn execute_relate_redischarge_from_api(
        &self,
        key: RelateMemoKey,
    ) -> QueryResult<SemanticQueryValue> {
        if self.relation_key_with_inference(key.clone()) != key {
            return QueryResult::Error(QueryError::Miss);
        }
        let occurrence = self
            .dispatch_txn
            .borrow()
            .relation
            .redischarge_occurrence
            .map(|(_, occurrence)| occurrence)
            .unwrap_or(InferenceOccurrence::ARGUMENT_COVARIANT);
        let step = self.execute_relate_inline(key.clone(), occurrence);
        let payload = match step {
            RelationStep::Assignable { bindings } => self.relation_payload(
                RelationOutcome::Assignable,
                bindings,
                RelationProof::Assignable {
                    witness: crate::semantic_query::DerivationTree {
                        sub_derivations: Arc::from(Vec::new().into_boxed_slice()),
                    },
                },
            ),
            RelationStep::NotAssignable => self.relation_payload(
                RelationOutcome::NotAssignable,
                Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                RelationProof::NotAssignable {
                    reason: RelationFailureCode::Structural,
                    failing_sub: SubRelationRef {
                        source: key.source,
                        target: key.target,
                        position: SubRelationPosition::Root,
                    },
                },
            ),
            RelationStep::BudgetExceeded(cap) => self.relation_payload(
                RelationOutcome::BudgetExceeded(cap.kind),
                Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                RelationProof::BudgetExceeded { cap },
            ),
            RelationStep::Unknown | RelationStep::Assumed(_) => {
                return QueryResult::Error(QueryError::Miss);
            }
        };
        QueryResult::Value(SemanticQueryValue::Relation(payload))
    }

    /// The machinery ROOT path: the full family singleflight
    /// (`execute(Relate)` → warm fast path / cross-thread join / traced
    /// cold build / publish). After a published cold build, drain the
    /// SCC-closed member batch onto the root's SCC-union carrier (design
    /// §2.3 step 4 R-a batched admission).
    fn execute_relate_root(&self, key: RelateMemoKey) -> RelationStep {
        let mut publication = None;
        let read = self.execute_via_cold_build_helper_capturing_publication(
            key.to_query_key(),
            &mut publication,
        );
        let step = match read.value {
            QueryResult::Value(SemanticQueryValue::Relation(payload)) => {
                relation_step_from_payload(&payload)
            }
            // An undecided judgement surfaces `Error(Miss)` — loud, never a
            // fallback, never admitted.
            _ => RelationStep::Unknown,
        };
        if let Some(publication) = publication {
            #[cfg(any(test, feature = "test-support"))]
            self.graph().wait_relation_root_pre_member_drain_gate();
            self.relation_drain_completed_members(&key, &publication);
        } else {
            // ReturnOnly exit (poisoned SCC / budget / undecided): the
            // deferred batch releases WITHOUT publish — no entry, no fact
            // signature, no backfill, no reverse-index metadata.
            self.relation_abort_completed_members();
        }
        step
    }

    /// Binding roots carry transient candidate deposits and therefore cannot
    /// join another transaction's in-flight inference session. Completed
    /// payloads are still eligible for the explicit warm read in
    /// `execute_relate_with_occurrence`; this policy controls only the cold
    /// build after that read misses.
    #[cfg(test)]
    pub(super) fn relate_root_uses_family_singleflight(key: &RelateMemoKey) -> bool {
        key.inference_context.is_none()
    }

    /// A nested sub-relation's INLINE cold compute: push a frame, run the
    /// reducer, close the frame through the SCC discharge. The publish is
    /// NEVER direct — it is batched at this frame's SCC close and drained
    /// by the machinery root onto the SCC-union carrier.
    fn execute_relate_inline(
        &self,
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
    ) -> RelationStep {
        let idx = self.relation_frame_open(&key, occurrence);
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(&key, &mut bindings);
        self.relation_frame_close(idx, verdict, bindings)
    }

    /// The family cold-build arm (the `execute(Relate)` reducer). Runs the
    /// root frame and maps the close onto the admission boundary: decided
    /// binary ⇒ publish; `BudgetExceeded` ⇒ public value, suppressed
    /// admission; undecided ⇒ `Error(Miss)`, never admitted.
    pub(super) fn build_relate(
        &self,
        key: &RelateMemoKey,
    ) -> crate::project_semantic_dispatch::walk::QueryBuildOutput<SemanticQueryValue> {
        verter_audit::attribute_scope!(RelationDecide);
        let fence = self.project_generation_signature();
        // A raw `SemanticQueryKey::Relate` can enter the family dispatcher
        // without passing through `execute_relate`. Refuse any such key whose
        // supplied inference context does not equal the target pattern's
        // immutable setup projection. Otherwise release builds could execute
        // one session setup while admitting under another fingerprint.
        if self.relation_key_with_inference(key.clone()) != *key {
            let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                SemanticQueryValue,
            > = (QueryResult::Error(QueryError::Miss), fence).into();
            output.cache_suppress = true;
            return output;
        }
        // Test-only fact-injection hook (ported from the retired
        // `relate_nodes` cold path): when the host's per-host
        // `relation_knobs.force_overflow_observations` knob is non-zero, emit
        // that many synthetic `FileWholeHash` observations onto the
        // active tracer so finalise reports `Overflow` once the
        // per-signature cap is exceeded — exercising the overflow
        // non-admission path without a pathological multi-file fixture.
        let host = self.ctx.host_for_fact_tracer_install();
        let force_n = host
            .relation_knobs
            .force_overflow_observations
            .load(std::sync::atomic::Ordering::Relaxed);
        if force_n > 0 {
            for n in 0..force_n {
                crate::resolver_core::resolver_context::observe_fan_out(
                    crate::resolver_core::FactVersionRef::FileWholeHash {
                        canonical_id: format!("__relation_force_overflow_{n}.ts"),
                        hash: [(n & 0xff) as u8; 16],
                    },
                );
            }
        }
        let idx = self.relation_frame_open(key, InferenceOccurrence::ARGUMENT_COVARIANT);
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(key, &mut bindings);
        #[cfg(any(test, feature = "test-support"))]
        self.inject_unproven_flow_member_for_tests(idx);
        match self.relation_frame_close_root(idx, verdict, bindings) {
            RootClose::Decided(payload) => {
                let observed_self_roots = self.relation_completed_publication_roots(key);
                crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                ))
                .with_observed_self_roots(observed_self_roots)
            }
            RootClose::DecidedReturnOnly(payload) => {
                let observed_self_roots =
                    self.observed_self_roots_from_nodes([key.source, key.target]);
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                    SemanticQueryValue,
                > = (
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                )
                    .into();
                // ReturnOnly-but-public: the verdict was computed around a
                // mixed component whose flow members finalized UNPROVEN —
                // the value flows to the caller, the memo refuses
                // admission (no warm entry, no fact signature, no
                // reverse-index metadata), and the frame close already
                // marked the request partial.
                output.cache_suppress = true;
                output.observed_self_roots = observed_self_roots;
                output
            }
            RootClose::BudgetExceeded(payload) => {
                let observed_self_roots =
                    self.observed_self_roots_from_nodes([key.source, key.target]);
                let mut output: crate::project_semantic_dispatch::walk::QueryBuildOutput<
                    SemanticQueryValue,
                > = (
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                )
                    .into();
                // ReturnOnly-but-public: the value flows to the caller, the
                // memo refuses admission (no warm entry, no fact signature,
                // no reverse-index metadata).
                output.cache_suppress = true;
                output.observed_self_roots = observed_self_roots;
                output
            }
            RootClose::Undecided => (QueryResult::Error(QueryError::Miss), fence).into(),
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Frames, sessions, and the SCC discharge
    // ──────────────────────────────────────────────────────────────────

    /// Push a reentry frame for `key`, opening an inference session when
    /// the key carries a fingerprint and no session is active (a binding
    /// root). Also snapshots the strict configuration at the root push.
    fn relation_frame_open(&self, key: &RelateMemoKey, occurrence: InferenceOccurrence) -> usize {
        // Snapshot the strict config + pattern BEFORE taking the borrow —
        // `relation_pattern_info` re-borrows the transaction (its
        // per-target cache lives there).
        let strict = self.relation_strict_config();
        let redischarge = self.relation_redischarge_active();
        let wants_inline_flight = !redischarge
            && key.inference_context.is_none()
            && !self.dispatch_txn.borrow().obligations.decides_root();
        let inline_flight = wants_inline_flight
            .then(|| self.graph().begin_inline_relation_flight(key))
            .flatten();
        let wants_session = key.inference_context.is_some()
            && self.dispatch_txn.borrow().active_session().is_none();
        let pattern = if wants_session {
            self.relation_pattern_info(key.target)
        } else {
            None
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        if txn.reentry().nearest_relate().is_none() {
            // Re-snapshot at every relation ROOT so the behavioral branch
            // and the key's strict fold can never diverge (the key reads
            // the live config; the reducer reads this snapshot).
            txn.relation.strict = Some(strict);
        }
        let watermark = txn.obligations.pending().pending_len();
        let idx = txn
            .reentry_mut()
            .push_relate(key.clone(), occurrence, watermark);
        txn.note_inline_flight(idx, inline_flight);
        if redischarge {
            txn.note_session_delta_range(idx, idx + 1);
        }
        if wants_session {
            if let Some(pattern) = pattern {
                let session_id = txn.push_collecting_session(
                    pattern.setup,
                    pattern.reverse_homomorphic.map(ReverseProjectionState::new),
                );
                // Both identities come from the same immutable setup value;
                // candidate collection cannot make them diverge.
                verter_debug_assert_eq!(
                    txn.relation
                        .sessions
                        .last()
                        .map(InferenceSession::context_key),
                    key.inference_context.as_ref(),
                    "the opened session must retain the relation key's frozen inference setup"
                );
                txn.note_opened_session(idx, session_id);
            }
        }
        idx
    }

    /// Close an INLINE frame: stage and commit an owned relation session,
    /// classify the pop (SCC-root vs provisional member), and run the SCC
    /// discharge at the root. Returns the caller-return step (PROVISIONAL
    /// for an unpublished member — never itself the published payload).
    fn relation_frame_close(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
    ) -> RelationStep {
        match self.relation_frame_pop(idx, verdict, bindings, false) {
            FramePop::Provisional(step) => step,
            FramePop::RootClose(close) => {
                // Poison surfaces (budget / undecided) reach the inline
                // caller as steps; a decided inline SCC root returned
                // through the provisional path above.
                match close {
                    RootClose::Decided(payload) => relation_step_from_payload(&payload),
                    RootClose::DecidedReturnOnly(payload) => relation_step_from_payload(&payload),
                    RootClose::BudgetExceeded(payload) => relation_step_from_payload(&payload),
                    RootClose::Undecided => RelationStep::Unknown,
                }
            }
        }
    }

    /// Close the machinery ROOT frame (same pop machinery, plus the
    /// public-outcome mapping).
    fn relation_frame_close_root(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
    ) -> RootClose {
        match self.relation_frame_pop(idx, verdict, bindings, true) {
            FramePop::RootClose(close) => close,
            FramePop::Provisional(_) => unreachable!(
                "the machinery root frame is always its SCC's root: the stack is \
                 empty below it, so no open assumption can target a deeper frame"
            ),
        }
    }

    /// The shared frame-pop + SCC-discharge engine. On a non-root pop the
    /// member defers PROVISIONALLY to the ledger and returns its
    /// caller-return step; on an SCC-root pop the whole component
    /// discharges (design §2.3 steps 3–4). `machinery_root` distinguishes
    /// the family singleflight's root frame (its payload returns as the
    /// build output) from an inline SCC root (its payload batch-publishes
    /// with the SCC drain).
    fn relation_frame_pop(
        &self,
        idx: usize,
        verdict: RelationResult,
        bindings: Vec<InferBinding>,
        machinery_root: bool,
    ) -> FramePop {
        // Session fixation: a binding root's session closes at the frame's
        // pop — after EVERY member's candidates have deposited (the
        // session's opener is the outermost frame of its deposits). A
        // budget edge inside the session ABANDONS it (design admission
        // row 8 — budget-exceeded abandon ⇒ ReturnOnly).
        let (popped, self_cycle) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let popped = txn.reentry_mut().pop();
            let self_cycle = popped.assumption_targets.contains(&idx);
            (popped, self_cycle)
        };
        // The popped frame is always a RELATION frame on this code path:
        // unpack its tagged identity and domain state into the relation
        // close's local shape.
        let (frame_key, frame_occurrence) = {
            let (key, occurrence) = popped.identity.expect_relate();
            (key.clone(), occurrence)
        };
        let budget_cap = popped.budget_cap;
        let min_open_target = popped.min_open_target;
        let pending_watermark = popped.pending_watermark;
        let self_assumptive = !popped.assumption_targets.is_empty();
        let RelationFrameState {
            session_delta,
            opened_session,
            inline_flight,
        } = match popped.domain {
            ObligationFrameDomain::Relate(state) => state,
            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => {
                unreachable!("a relation code path pops a relation frame")
            }
        };
        let mut session_bindings: Option<Arc<[InferBinding]>> = None;
        let mut session_abandoned = false;
        if let Some(sid) = opened_session {
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(position) = txn.relation.sessions.iter().position(|s| s.id == sid) {
                if budget_cap.is_some() {
                    txn.relation.sessions[position].abandon();
                    session_abandoned = true;
                } else {
                    let combine = |nodes: &[SemanticNodeId], variance: VariancePhase| {
                        self.relation_combine_candidates(nodes, variance)
                    };
                    let mut session = txn.relation.sessions.remove(position);
                    let fixed = session.stage_fixation(combine);
                    let committed = session.commit_completed();
                    let state = session.state;
                    txn.relation.sessions.insert(position, session);
                    match state {
                        InferenceSessionState::CommittedDeterministic => {
                            verter_debug_assert!(
                                committed,
                                "relation fixation commits at its safe pop"
                            );
                            session_bindings = fixed;
                        }
                        InferenceSessionState::Abandoned => session_abandoned = true,
                        InferenceSessionState::Collecting
                        | InferenceSessionState::StagedDeterministic => unreachable!(
                            "relation fixation stages and commits before the frame closes"
                        ),
                    }
                }
            }
        }
        let pending = pending_verdict_of(&verdict, &budget_cap, &mut session_bindings, bindings);
        let is_scc_root = match min_open_target {
            None => true,
            Some(target) => target >= idx,
        };
        if !is_scc_root {
            // PROVISIONAL member: defer to the ledger, propagate the still-
            // open lowlink to the parent, and return the caller-return
            // step. NEVER publishes here. A binding member additionally
            // records into its session's `SessionAdmissionLedger` (design
            // §2.3 step 4 — it admits only at its session's close, drained
            // at the SCC's batched-publish instant below).
            let step = relation_step_from_pending(&pending);
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.propagate_lowlink(min_open_target);
            if let Some(sid) = opened_session {
                txn.relation.session_admission.defer(sid, frame_key.clone());
            }
            txn.obligations.pending_mut().deposit(PendingObligation {
                identity: ObligationIdentity::Relate {
                    key: frame_key.clone(),
                    occurrence: frame_occurrence,
                },
                domain: PendingObligationDomain::Relate(RelationPendingState {
                    verdict: pending,
                    session_delta,
                    opened_session,
                    inline_flight,
                }),
            });
            return FramePop::Provisional(step);
        }

        // ── SCC close at this root (design §2.3 step 3) ──────────────
        // Drain by the frame's push-time watermark, NEVER by stack index —
        // indices recycle after pops, and a recycled index would let this
        // close steal a pending member of a still-open outer SCC (which
        // would then publish a stale provisional verdict).
        let mut flow_members: Vec<DrainedFlowReturnMember> = Vec::new();
        let mut members: Vec<DrainedRelationMember> = Vec::new();
        let mut call_members: Vec<(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )> = Vec::new();
        for member in self
            .dispatch_txn
            .borrow_mut()
            .obligations
            .pending_mut()
            .drain_scc(pending_watermark)
        {
            match member.domain {
                PendingObligationDomain::Relate(state) => {
                    let (key, occurrence) = member.identity.expect_relate();
                    members.push(DrainedRelationMember {
                        key: key.clone(),
                        occurrence,
                        verdict: state.verdict,
                        session_delta: state.session_delta,
                        opened_session: state.opened_session,
                        inline_flight: state.inline_flight,
                    });
                }
                PendingObligationDomain::FlowReturn(state) => {
                    let key = member
                        .identity
                        .as_flow_return()
                        .expect("flow-return pending member carries a flow identity")
                        .clone();
                    flow_members.push(DrainedFlowReturnMember {
                        key,
                        outcome: state.outcome,
                        plan_refusal: state.plan_refusal,
                        inline_flight: state.inline_flight,
                        holds: state.holds,
                        self_roots: state.self_roots,
                        materialized: state.materialized,
                        fresh_seed: state.fresh_seed,
                        flow_demand: state.flow_demand,
                        discharge: state.discharge,
                        provenance: state.provenance,
                    });
                }
                PendingObligationDomain::ResolveCall(state) => {
                    let state = *state;
                    let key = member
                        .identity
                        .as_resolve_call()
                        .expect("call pending member carries a call identity")
                        .clone();
                    call_members.push((key, state));
                }
            }
        }
        let cyclic = !members.is_empty()
            || !flow_members.is_empty()
            || !call_members.is_empty()
            || self_cycle;

        // Row 3 batched poison: ANY Unknown / budget / abandoned-session
        // edge anywhere in the component routes the WHOLE SCC through
        // ReturnOnly — nothing publishes.
        let budget_cap = budget_cap.or_else(|| {
            members.iter().find_map(|m| match &m.verdict {
                PendingVerdict::BudgetExceeded(cap) => Some(*cap),
                _ => None,
            })
        });
        // The mixed component discharges to ONE joint fixed point: the
        // flow members' callee-clause transfer / empty-cycle resurrection
        // and the call members' replay + return equation iterate against
        // each other's current values until neither moves — either
        // refusing poisons the whole component exactly like a degraded
        // flow member.
        let initial_substitution: ProvisionalSubstitution = std::iter::once((
            ObligationIdentity::Relate {
                key: frame_key.clone(),
                occurrence: frame_occurrence,
            },
            ProvisionalVerdict::Relate(relation_step_from_pending(&pending)),
        ))
        .chain(members.iter().map(|member| {
            (
                ObligationIdentity::Relate {
                    key: member.key.clone(),
                    occurrence: member.occurrence,
                },
                ProvisionalVerdict::Relate(relation_step_from_pending(&member.verdict)),
            )
        }))
        .collect();
        // A degraded flow member AFTER the joint discharge poisons the
        // WHOLE tagged component (atomic admission: nothing publishes,
        // every flight aborts). The check runs post-discharge because the
        // fixed point resurrects hold-only empty cycles — poisoning on
        // the pre-discharge outcome condemns members the close recovers.
        let (_prefix_outcomes, call_results, flow_convergence) = match self
            .discharge_mixed_component_to_fixed_point(
                Vec::new(),
                &mut flow_members,
                &mut call_members,
                &initial_substitution,
            ) {
            Ok(ok) => ok,
            Err(failure) => {
                self.abort_inline_flight(inline_flight.as_ref());
                for member in &members {
                    self.abort_inline_flight(member.inline_flight.as_ref());
                }
                self.flow_return_abort_drained_flights(&flow_members);
                for (_, member) in &call_members {
                    self.abort_inline_flight(member.inline_flight.as_ref());
                    if let Some(session) = member.staged_session {
                        self.abandon_session(session);
                    }
                }
                // Record the component failure on every installed flow
                // demand (failure detection on the ledger — never an
                // admission decision).
                let flow_failure = match failure {
                    crate::semantic_query::ResolveCallFailure::Budget => {
                        crate::semantic_query::FlowReturnFailure::Budget(
                            verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                        )
                    }
                    _ => crate::semantic_query::FlowReturnFailure::Unresolved,
                };
                for member in &flow_members {
                    let _ = self.fail_flow_demand(member.flow_demand.as_ref(), flow_failure);
                }
                return FramePop::RootClose(match failure {
                    crate::semantic_query::ResolveCallFailure::Budget => {
                        RootClose::BudgetExceeded(self.relation_payload(
                            RelationOutcome::BudgetExceeded(
                                crate::semantic_query::BudgetExceededKind::CallResolutionBudget,
                            ),
                            Arc::from([]),
                            RelationProof::BudgetExceeded {
                                cap: RecursionOrBudgetCap {
                                    kind: crate::semantic_query::BudgetExceededKind::CallResolutionBudget,
                                    limit: super::call_resolve::MAX_CANDIDATES_STARTED as u32,
                                },
                            },
                        ))
                    }
                    _ => RootClose::Undecided,
                });
            }
        };
        // Any poison edge (a post-discharge flow no-value, an abandoned
        // session, a budget verdict anywhere, an Unknown) routes the
        // WHOLE SCC through ReturnOnly — nothing publishes, every flight
        // aborts. The flow check runs POST-discharge: the joint fixed
        // point resurrects hold-only empty cycles, so poisoning on the
        // pre-discharge outcome would condemn members the close recovers.
        let poisoned = flow_members
            .iter()
            .any(|member| matches!(member.outcome, FlowReturnPendingOutcome::NoValue { .. }))
            || session_abandoned
            || budget_cap.is_some()
            || matches!(
                pending,
                PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_)
            )
            || members.iter().any(|m| {
                matches!(
                    m.verdict,
                    PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_)
                )
            });
        if poisoned {
            self.abort_inline_flight(inline_flight.as_ref());
            for member in &members {
                self.abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_results);
            // Record the failure on every installed flow demand of the
            // component (failure detection on the ledger — never an
            // admission decision).
            for member in &flow_members {
                let failure = match &member.outcome {
                    FlowReturnPendingOutcome::NoValue { failure, .. } => *failure,
                    FlowReturnPendingOutcome::EvaluatedValue(_) => {
                        crate::semantic_query::FlowReturnFailure::Unresolved
                    }
                };
                let _ = self.fail_flow_demand(member.flow_demand.as_ref(), failure);
            }
            // Release WITHOUT publish (no entry / fact signature /
            // backfill / reverse-index metadata). The machinery root
            // surfaces the public `BudgetExceeded` payload when a budget
            // edge drove the poison.
            if let Some(cap) = budget_cap {
                let payload = self.relation_payload(
                    RelationOutcome::BudgetExceeded(cap.kind),
                    Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                    RelationProof::BudgetExceeded { cap },
                );
                return FramePop::RootClose(RootClose::BudgetExceeded(payload));
            }
            return FramePop::RootClose(RootClose::Undecided);
        }

        match self.relation_discharge_and_route(
            machinery_root,
            Some((
                frame_key.clone(),
                frame_occurrence,
                pending,
                self_assumptive,
                session_delta,
                opened_session,
                inline_flight,
            )),
            members,
            flow_members,
            None,
            call_results,
            cyclic,
            &flow_convergence,
        ) {
            Ok(outcome) => {
                if outcome.flow_batch_unproven {
                    // The mixed equation consumed UNPROVEN flow-member
                    // values: the root's verdict still flows to the
                    // caller, but nothing composed over it may warm —
                    // the same funnel primitives the inline flow-root
                    // refusal folds (an answer composed around an
                    // unproven flow value must not warm).
                    // A member refused for a budget edge or a torn
                    // view faults consumers the contained unverified
                    // class does not reach: the root's own preparation
                    // being clean says nothing about why the batch was
                    // refused.
                    self.fold_cache_read_rails(
                        true,
                        true,
                        crate::semantic_query::PartialReasonSet::FLOW_RETURN_UNVERIFIED
                            .union(outcome.flow_batch_partial_reasons),
                    );
                    if let Some(step) = outcome.self_step {
                        return FramePop::Provisional(step);
                    }
                    return FramePop::RootClose(RootClose::DecidedReturnOnly(
                        outcome
                            .self_publish
                            .expect("the machinery root always produces its own payload"),
                    ));
                }
                if let Some(step) = outcome.self_step {
                    return FramePop::Provisional(step);
                }
                FramePop::RootClose(RootClose::Decided(
                    outcome
                        .self_publish
                        .expect("the machinery root always produces its own payload"),
                ))
            }
            Err(cap) => {
                // The component released WITHOUT publish (aborted session,
                // lost ledger member, non-stable redischarge, or a poisoned
                // relation member). The machinery root surfaces the public
                // `BudgetExceeded` payload when a budget edge drove it.
                if let Some(cap) = cap {
                    let payload = self.relation_payload(
                        RelationOutcome::BudgetExceeded(cap.kind),
                        Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                        RelationProof::BudgetExceeded { cap },
                    );
                    return FramePop::RootClose(RootClose::BudgetExceeded(payload));
                }
                FramePop::RootClose(RootClose::Undecided)
            }
        }
    }

    /// The relation-member half of a tagged SCC close (the generic
    /// coordinator's discharge step, design §2.3 steps 3–4): build the
    /// discharged set from the optional relation root plus the drained
    /// relation members, gate binding members on their session's drain,
    /// redischarge deepest-first/root-last against the ONE tagged
    /// provisional substitution table, and route every decided member —
    /// and every completed flow member — into the batched publish queue.
    ///
    /// `root_relation = None` is the FLOW-rooted shape: the close's root
    /// is a flow frame, so every relation member routes to the completed
    /// batch (no self publish, no self step) and the flow root publishes
    /// through its own family.
    ///
    /// Returns `Err(cap)` when the component releases WITHOUT publish
    /// (abandoned session, lost ledger member, non-stable redischarge, or
    /// a poisoned relation member); `cap` carries the budget edge when
    /// one drove the abort. The helper aborts every flight it owns; the
    /// caller aborts its own root flight.
    ///
    /// Flow members are proof-gated HERE: each drained flow member
    /// finalizes against its own installed demand (its typed discharge
    /// report applied centrally, the component's observed convergence
    /// replayed, the seal, the finalizer) and enters the publish queue
    /// ONLY with its own `CompleteFlowResult`. One unproven member refuses
    /// the WHOLE member batch — the torn-component rule — while the
    /// root's own admission is unaffected.
    #[allow(clippy::type_complexity)]
    pub(super) fn relation_discharge_and_route(
        &self,
        machinery_root: bool,
        root_relation: Option<(
            RelateMemoKey,
            InferenceOccurrence,
            PendingVerdict,
            bool,
            bool,
            Option<super::dispatch_txn::SessionId>,
            Option<InlineMemberFlight>,
        )>,
        members: Vec<DrainedRelationMember>,
        flow_members: Vec<DrainedFlowReturnMember>,
        provisional_call_root: Option<(
            crate::semantic_query::ResolveCallKey,
            crate::semantic_query::ResolvedCallResult,
            Option<super::dispatch_txn::SessionId>,
        )>,
        call_members: Vec<(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
            crate::semantic_query::ResolvedCallResult,
        )>,
        cyclic: bool,
        flow_convergence: &super::dispatch_txn::flow_obligation_state::ObservedFlowConvergence,
    ) -> Result<RelationDischargeOutcome, Option<RecursionOrBudgetCap>> {
        // Discharge verdicts — a member recorded POSITIVE that consumed
        // assumptions re-discharges against the converged state when ANY
        // member closed NEGATIVE (the collapsed-back-edge case, design
        // §2.3 step 3); a non-stable re-discharge (a flip to Unknown)
        // releases the whole batch without publish. Each record carries
        // the member's session-delta flag (row 7: a session-local delta
        // never publishes) and its opened-session token (a binding
        // member admits only through its session's
        // `SessionAdmissionLedger` drain below).
        let any_negative = root_relation
            .as_ref()
            .is_some_and(|(_, _, pending, _, _, _, _)| {
                matches!(pending, PendingVerdict::NotAssignable)
            })
            || members
                .iter()
                .any(|m| matches!(m.verdict, PendingVerdict::NotAssignable));
        let mut discharged: Vec<DischargedMember> = Vec::new();
        if let Some((
            key,
            occurrence,
            pending,
            self_assumptive,
            session_delta,
            opened_session,
            flight,
        )) = root_relation
        {
            if let Some(sid) = opened_session {
                self.dispatch_txn
                    .borrow_mut()
                    .relation
                    .session_admission
                    .defer(sid, key.clone());
            }
            discharged.push((
                key,
                occurrence,
                pending,
                self_assumptive,
                session_delta,
                opened_session,
                flight,
            ));
        }
        let has_relation_root = !discharged.is_empty();
        for member in members {
            discharged.push((
                member.key,
                member.occurrence,
                member.verdict,
                true,
                member.session_delta,
                member.opened_session,
                member.inline_flight,
            ));
        }
        let has_binding_member = discharged
            .iter()
            .any(|(key, _, _, _, _, opened_session, _)| {
                opened_session.is_some() || key.inference_context.is_some()
            });
        let has_return_member =
            !flow_members.is_empty() || provisional_call_root.is_some() || !call_members.is_empty();
        // Re-discharge is an SCC-close operation. A redischarge itself
        // opens an ordinary acyclic frame; allowing a merely-negative
        // binding result to enter this branch again would recursively
        // redischarge forever.
        if cyclic && (any_negative || has_binding_member || has_return_member) {
            let mut substitution: ProvisionalSubstitution = discharged
                .iter()
                .map(|(key, occurrence, verdict, _, _, _, _)| {
                    (
                        ObligationIdentity::Relate {
                            key: key.clone(),
                            occurrence: *occurrence,
                        },
                        ProvisionalVerdict::Relate(relation_step_from_pending(verdict)),
                    )
                })
                .collect();
            substitution.extend(call_members.iter().map(|(key, _, result)| {
                (
                    ObligationIdentity::ResolveCall(key.clone()),
                    ProvisionalVerdict::ResolveCall(result.clone()),
                )
            }));
            substitution.extend(provisional_call_root.iter().map(|(key, result, _)| {
                (
                    ObligationIdentity::ResolveCall(key.clone()),
                    ProvisionalVerdict::ResolveCall(result.clone()),
                )
            }));
            // Bottom-up over the condensation: re-discharge the POSITIVE
            // assumption-consuming members DEEPEST-FIRST so a shallower
            // member re-runs against the FINAL deeper verdicts. Layout:
            // `discharged[0]` is the SCC root (shallowest) when there is
            // a relation root; `discharged[1..]` are the drained members
            // in POP order — deepest-popped first — so deepest-first is
            // positions `1..len` in order, with the root LAST. With no
            // relation root the drained members themselves already run
            // deepest-first and none is privileged. (The reversed scan
            // froze a shallow member against a stale provisional deep
            // `Assignable` before the deep member flipped on its
            // collapsed back-edge.)
            let order: Vec<usize> = if has_relation_root {
                (1..discharged.len()).chain(std::iter::once(0)).collect()
            } else {
                (0..discharged.len()).collect()
            };
            for position in order {
                let (key, occurrence, verdict, assumptive, _, opened_session, _) =
                    &discharged[position];
                let binding_member = opened_session.is_some() || key.inference_context.is_some();
                let must_redischarge = if has_binding_member || has_return_member {
                    true
                } else {
                    *assumptive && matches!(verdict, PendingVerdict::Assignable { .. })
                };
                if !binding_member && !must_redischarge {
                    continue;
                }
                let key = key.clone();
                let occurrence = *occurrence;
                let rerun = self.relation_redischarge(&key, occurrence, &substitution);
                match rerun {
                    PendingVerdict::Unknown => {
                        // Non-stable re-discharge ⇒ release the whole batch
                        // WITHOUT publish (joiners recompute).
                        self.relation_abort_discharged_flights(&discharged);
                        self.flow_return_abort_drained_flights(&flow_members);
                        self.resolve_call_abort_drained_flights(&call_members);
                        return Err(None);
                    }
                    PendingVerdict::BudgetExceeded(cap) => {
                        self.relation_abort_discharged_flights(&discharged);
                        self.flow_return_abort_drained_flights(&flow_members);
                        self.resolve_call_abort_drained_flights(&call_members);
                        return Err(Some(cap));
                    }
                    stable => {
                        if (has_binding_member || has_return_member)
                            && !redischarge_is_stable(verdict, &stable)
                        {
                            // A binding SCC may publish only when every
                            // member retains its provisional polarity and the
                            // binding members retain their complete fixed
                            // binding snapshot. Pure non-binding SCCs instead
                            // converge bottom-up: a provisional positive may
                            // legitimately collapse to the final negative
                            // verdict carried by its dependency.
                            self.relation_abort_discharged_flights(&discharged);
                            self.flow_return_abort_drained_flights(&flow_members);
                            self.resolve_call_abort_drained_flights(&call_members);
                            return Err(None);
                        }
                        substitution.insert(
                            ObligationIdentity::Relate {
                                key: key.clone(),
                                occurrence,
                            },
                            ProvisionalVerdict::Relate(relation_step_from_pending(&stable)),
                        );
                        discharged[position].2 = stable;
                    }
                }
            }
        }

        let deferred_relation_sessions = discharged
            .iter()
            .filter_map(|(key, _, _, _, _, opened_session, _)| {
                opened_session.map(|session| (session, key.clone()))
            })
            .collect::<Vec<_>>();
        // Validate without consuming. From call-session commit through the
        // ledger drain and completed-member enqueue, no semantic work runs.
        let ledgers_ready = {
            let txn = self.dispatch_txn.borrow();
            deferred_relation_sessions.iter().all(|(session, key)| {
                let session_ok = txn
                    .relation
                    .sessions
                    .iter()
                    .find(|candidate| candidate.id == *session)
                    .is_some_and(|candidate| {
                        candidate.state == InferenceSessionState::CommittedDeterministic
                    });
                session_ok && txn.relation.session_admission.contains(*session, key)
            })
        };
        if !ledgers_ready {
            self.relation_abort_discharged_flights(&discharged);
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_members);
            return Err(None);
        }

        // Publish routing (design §2.3 step 4): decided members queue for
        // the root's batched publish onto the SCC-union carrier; a
        // session-local delta (row 7) never publishes.
        let scc_keys: Arc<[RelateKeyId]> = if cyclic {
            let keys: Vec<RelateKeyId> = discharged
                .iter()
                .map(|(key, _, _, _, _, _, _)| self.graph().intern_relate_key(key.clone()))
                .collect();
            Arc::from(keys.into_boxed_slice())
        } else {
            Arc::from(Vec::<RelateKeyId>::new().into_boxed_slice())
        };
        let mut self_publish: Option<RelationPayload> = None;
        let mut self_step: Option<RelationStep> = None;
        let mut completed: Vec<CompletedSccMember> = Vec::new();
        for (position, (key, _, verdict, _, session_delta, _, inline_flight)) in
            discharged.into_iter().enumerate()
        {
            let is_self = has_relation_root && position == 0;
            let payload = match &verdict {
                PendingVerdict::Assignable { bindings } => {
                    let proof = if cyclic {
                        RelationProof::CoinductiveCycle {
                            keys: Arc::clone(&scc_keys),
                        }
                    } else {
                        RelationProof::Assignable {
                            witness: crate::semantic_query::DerivationTree {
                                sub_derivations: Arc::from(
                                    vec![SubRelationRef {
                                        source: key.source,
                                        target: key.target,
                                        position: SubRelationPosition::Root,
                                    }]
                                    .into_boxed_slice(),
                                ),
                            },
                        }
                    };
                    self.relation_payload(RelationOutcome::Assignable, Arc::clone(bindings), proof)
                }
                PendingVerdict::NotAssignable => self.relation_payload(
                    RelationOutcome::NotAssignable,
                    Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
                    RelationProof::NotAssignable {
                        reason: RelationFailureCode::Structural,
                        failing_sub: SubRelationRef {
                            source: key.source,
                            target: key.target,
                            position: SubRelationPosition::Root,
                        },
                    },
                ),
                PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_) => {
                    unreachable!("poisoned SCCs return before the publish routing")
                }
            };
            if is_self {
                if session_delta {
                    // Admission row 7: a session-local delta never
                    // publishes — the caller gets the computed step.
                    self.abort_inline_flight(inline_flight.as_ref());
                    self_step = Some(relation_step_from_payload(&payload));
                } else if machinery_root {
                    // The machinery root publishes through the family
                    // singleflight (its build output IS this payload).
                    self_publish = Some(payload);
                } else {
                    // An inline SCC root: its payload batch-publishes with
                    // the SCC (drained by the machinery root); the caller
                    // consumes the computed step.
                    self_step = Some(relation_step_from_payload(&payload));
                    completed.push(CompletedSccMember {
                        key,
                        payload,
                        inline_flight,
                    });
                }
            } else if !session_delta {
                completed.push(CompletedSccMember {
                    key,
                    payload,
                    inline_flight,
                });
            } else {
                self.abort_inline_flight(inline_flight.as_ref());
            }
        }
        // The call sessions commit before the flow members finalize: the
        // session state feeds this transaction's remaining work regardless
        // of how the flow members close.
        let staged_call_sessions = call_members
            .iter()
            .filter_map(|(_, state, _)| state.staged_session)
            .chain(
                provisional_call_root
                    .iter()
                    .filter_map(|(_, _, session)| *session),
            )
            .collect::<Vec<_>>();
        if !self.commit_call_sessions(&staged_call_sessions) {
            for member in &completed {
                self.abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_members);
            return Err(None);
        }
        // Proof-gate the flow members BEFORE anything queues: each member
        // finalizes against its OWN installed demand (its typed discharge
        // report applied centrally, the component's observed convergence
        // replayed, the seal, the finalizer) and enters the publish queue
        // ONLY with its own `CompleteFlowResult`. One unproven member
        // refuses the WHOLE member batch (the torn-component rule): every
        // member flight aborts, nothing queues, and the ROOT's outcome is
        // marked non-admissible too — the mixed equation consumed the
        // members' evaluated values, so the root's verdict may still flow
        // to the caller but must never warm (`ReturnOnly`).
        let mut proven_flow_members = Vec::with_capacity(flow_members.len());
        let mut flow_batch_unproven = false;
        let mut flow_batch_partial_reasons = crate::semantic_query::PartialReasonSet::default();
        for member in flow_members {
            // Read before the outcome moves: the cause survives the
            // member, because the close is where it is finally needed.
            let member_plan_refusal = member.plan_refusal;
            // The member's per-key substitution applies BEFORE its value
            // leaves the component: the pop substituted only the
            // caller-return clone, so the value channel, the finalizer's
            // proof, and the publish all carry the INSTANTIATED value —
            // exactly as the root's own close substitutes before it
            // finalizes.
            let outcome = match member.outcome {
                FlowReturnPendingOutcome::EvaluatedValue(result) => {
                    // Per-key substitution, then the final idempotent
                    // pre-seal closure — the member's proof, the value
                    // channel and the batch publish all see the closed
                    // value — and the function-kind wrap materializes
                    // last, exactly as the root's own close orders it.
                    FlowReturnPendingOutcome::EvaluatedValue(self.materialize_flow_return_wrap(
                        self.close_flow_result_pre_seal(
                            self.apply_frame_key_substitution(&member.key, result),
                            member.key.context.policy.nullability,
                        ),
                    ))
                }
                no_value => no_value,
            };
            // The VALUE channel: every member whose close produced an
            // evaluated value records it for the shared return equation's
            // override reads — proven or not. Admission is the proof
            // typing below, never this channel.
            if let FlowReturnPendingOutcome::EvaluatedValue(result) = &outcome {
                self.dispatch_txn
                    .borrow_mut()
                    .flow
                    .closed_values
                    .push((member.key.clone(), result.clone()));
            }
            let verdict = match &outcome {
                FlowReturnPendingOutcome::EvaluatedValue(result) => {
                    // The member's OWN evaluation provenance, carried
                    // through the deferral — never reconstructed from the
                    // demand carrier, which would trivialize the
                    // triangulation.
                    self.finalize_flow_demand(
                        member.flow_demand.as_ref(),
                        member.discharge.as_ref(),
                        flow_convergence,
                        member.provenance,
                        result,
                    )
                }
                // A no-value member fails the component at the close
                // (both close paths poison it before routing); it never
                // enters the publish queue regardless.
                FlowReturnPendingOutcome::NoValue { failure, .. } => {
                    let _ = self.fail_flow_demand(member.flow_demand.as_ref(), *failure);
                    None
                }
            };
            match verdict {
                Some(super::flow_solve::FlowSolveOutcome::Complete(proof)) => {
                    proven_flow_members.push(super::dispatch_txn::CompletedFlowReturnMember {
                        key: member.key,
                        result: proof,
                        inline_flight: member.inline_flight,
                        self_roots: member.self_roots,
                        materialized: member.materialized,
                        // Closed inside a larger component: its value rests
                        // on frames outside its own, so it is never reused
                        // on the transaction.
                        reuse: None,
                    });
                }
                unproven => {
                    flow_batch_unproven = true;
                    // The member's OWN cause, unioned rather than
                    // ranked: two members refused for different reasons
                    // leave the batch both over budget and unstable, and
                    // picking one would drop a class a consumer needs.
                    // Both cause channels are read — the recorded plan
                    // refusal FIRST (it is the member's primary cause and
                    // the finalizer's undischarged echo must not shadow
                    // it), then the finalizer's typed partial. Two member
                    // states deliberately contribute NO class of their
                    // own: a cleanly-planned member whose obligations
                    // were merely left pending carries the batch close's
                    // OWN withholding signature (`IncompleteObligations`
                    // — a genuinely budget-refused obligation reaches
                    // here as a typed `Failed` budget record instead),
                    // and a NO-VALUE member is already represented
                    // POSITIONALLY in the value the root consumed (its
                    // typed marker names the exact position; blanketing
                    // the frame-wide missing-surface class here would
                    // erase the faithfully-typed sibling members a
                    // value-deriving consumer can still serve). The batch
                    // stays unproven either way — nothing warms.
                    let member_reasons = if member_plan_refusal.is_some() {
                        super::flow_return::plan_refusal_reason_class(member_plan_refusal)
                    } else {
                        match &unproven {
                            Some(super::flow_solve::FlowSolveOutcome::Partial(partial)) => {
                                match &partial.reason {
                                    super::flow_solve::FlowPartialReason::IncompleteObligations => {
                                        crate::semantic_query::PartialReasonSet::default()
                                    }
                                    _ => super::flow_return::flow_partial_reason_class(
                                        &partial.reason,
                                        partial.value.degradation(),
                                    ),
                                }
                            }
                            Some(super::flow_solve::FlowSolveOutcome::NoValue(_)) | None => {
                                crate::semantic_query::PartialReasonSet::default()
                            }
                            // Matched by the arm above.
                            Some(super::flow_solve::FlowSolveOutcome::Complete(_)) => {
                                unreachable!()
                            }
                        }
                    };
                    flow_batch_partial_reasons = flow_batch_partial_reasons.union(member_reasons);
                    self.abort_inline_flight(member.inline_flight.as_ref());
                }
            }
        }
        if flow_batch_unproven {
            for member in &completed {
                self.abort_inline_flight(member.inline_flight.as_ref());
            }
            for member in &proven_flow_members {
                self.abort_inline_flight(member.inline_flight.as_ref());
            }
            self.resolve_call_abort_drained_flights(&call_members);
            return Ok(RelationDischargeOutcome {
                self_publish,
                self_step,
                flow_batch_unproven: true,
                flow_batch_partial_reasons,
            });
        }
        let mut rootless_flights = Vec::new();
        {
            let mut txn = self.dispatch_txn.borrow_mut();
            for (session, _) in deferred_relation_sessions {
                let _ = txn.relation.session_admission.drain(session);
            }
            txn.relation.completed_members.extend(completed);
            txn.flow.completed_members.extend(proven_flow_members);
            for (key, state, result) in call_members {
                // Incomplete proof stays transaction-local. Origin is
                // provenance and is not the admission oracle.
                match crate::semantic_query::AdmissibleCallResult::new(result, state.proof_complete)
                {
                    Some(result) => txn.call.completed_members.push(CompletedResolveCallMember {
                        key,
                        result,
                        inline_flight: state.inline_flight,
                        self_roots: state.self_roots,
                    }),
                    _ => rootless_flights.push(state.inline_flight),
                }
            }
        }
        for flight in rootless_flights {
            self.abort_inline_flight(flight.as_ref());
        }
        Ok(RelationDischargeOutcome {
            self_publish,
            self_step,
            flow_batch_unproven: false,
            flow_batch_partial_reasons: crate::semantic_query::PartialReasonSet::default(),
        })
    }

    /// Re-discharge ONE member of a negatively-closed SCC against the
    /// converged state (design §2.3 step 4): the member's cold compute
    /// re-runs through the same `execute(Relate)` dispatch with the SCC's
    /// discharged verdicts as the substitution table, so a stale
    /// SCC-close snapshot is impossible by construction.
    fn relation_redischarge(
        &self,
        key: &RelateMemoKey,
        occurrence: InferenceOccurrence,
        substitution: &ProvisionalSubstitution,
    ) -> PendingVerdict {
        let saved_context = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let next_substitution = substitution
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            txn.replace_redischarge_context(next_substitution, occurrence)
        };
        let verdict = self.execute(key.to_query_key());
        self.dispatch_txn
            .borrow_mut()
            .restore_redischarge_context(saved_context);
        match verdict {
            QueryResult::Value(SemanticQueryOutput {
                value: SemanticQueryValue::Relation(payload),
                ..
            }) => match payload.outcome {
                RelationOutcome::Assignable => PendingVerdict::Assignable {
                    bindings: Arc::clone(&payload.bindings),
                },
                RelationOutcome::NotAssignable => PendingVerdict::NotAssignable,
                RelationOutcome::BudgetExceeded(_) => PendingVerdict::Unknown,
            },
            _ => PendingVerdict::Unknown,
        }
    }

    /// Fixation combinator (design §4.2 candidate combination): covariant
    /// candidates union (canonicalized), contravariant candidates
    /// intersect, a single candidate binds directly, and an unfixed
    /// parameter deterministically defaults to `unknown`.
    pub(super) fn relation_combine_candidates(
        &self,
        nodes: &[SemanticNodeId],
        variance: VariancePhase,
    ) -> SemanticNodeId {
        let graph = self.graph();
        match nodes {
            [] => graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
            [single] => *single,
            many => {
                let mut dedup: Vec<SemanticNodeId> = many.to_vec();
                crate::semantic_query::stable_key::sort_by_stable_key(graph, &mut dedup);
                dedup.dedup_by(|a, b| {
                    crate::semantic_query::stable_key::provably_equal(graph, *a, *b)
                });
                if dedup.len() == 1 {
                    return dedup[0];
                }
                if matches!(variance, VariancePhase::Contravariant) {
                    // Intersection combination: tag-level-disjoint
                    // candidate pairs (distinct primitives, distinct
                    // literals, literal against a mismatched base
                    // primitive) collapse the intersection to `never`;
                    // undecidable shapes conservatively keep the
                    // structural Intersection carrier.
                    let disjoint = dedup.iter().enumerate().any(|(i, &a)| {
                        dedup[i + 1..]
                            .iter()
                            .any(|&b| tag_level_disjoint(graph, a, b))
                    });
                    if disjoint {
                        return graph
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never));
                    }
                    return self.intern_normalized_union_or_intersection(&dedup, false);
                }
                self.intern_normalized_union_or_intersection(&dedup, true)
            }
        }
    }

    /// Release one claimed inline member flight, in any domain. `None`
    /// means the member was never deferred and has no flight to release.
    pub(super) fn abort_inline_flight(&self, flight: Option<&InlineMemberFlight>) {
        if let Some(flight) = flight {
            self.graph().abort_inline_member_flight(flight);
        }
    }

    pub(super) fn relation_abort_discharged_flights(&self, discharged: &[DischargedMember]) {
        for (_, _, _, _, _, _, flight) in discharged {
            self.abort_inline_flight(flight.as_ref());
        }
    }

    pub(super) fn flow_return_abort_drained_flights(&self, members: &[DrainedFlowReturnMember]) {
        for member in members {
            self.abort_inline_flight(member.inline_flight.as_ref());
        }
    }

    /// Hand one closed component's deferred members to the store's
    /// batched SCC publish, fenced on the root's published candidate.
    ///
    /// THE single drain shape both roots use: a member without a claimed
    /// flight was never deferred and has nothing to publish; every other
    /// member rides the root's SCC-union carrier and the root witness, so
    /// a superseded root releases the whole component with zero member
    /// publication.
    pub(super) fn publish_scc_member_batch(
        &self,
        required_root: crate::semantic_query_memo::SccRootWitness,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
        relation_members: Vec<CompletedSccMember>,
        flow_members: Vec<super::dispatch_txn::CompletedFlowReturnMember>,
        call_members: Vec<super::dispatch_txn::CompletedResolveCallMember>,
    ) {
        let relation_members: Vec<_> = relation_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingRelationMember {
                        key: member.key,
                        payload: member.payload,
                        flight,
                    }
                })
            })
            .collect();
        let flow_members: Vec<_> = flow_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingFlowReturnMember {
                        key: member.key,
                        result: member.result,
                        materialized: member.materialized,
                        flight,
                    }
                })
            })
            .collect();
        let call_members: Vec<_> = call_members
            .into_iter()
            .filter_map(|member| {
                member.inline_flight.map(|flight| {
                    crate::semantic_query_memo::PendingResolveCallMember {
                        key: member.key,
                        result: member.result,
                        flight,
                    }
                })
            })
            .collect();
        self.graph().publish_scc_members_fenced(
            Some(self.ctx),
            &required_root,
            &carrier.read_set_signature,
            &carrier.self_root_canonicals,
            carrier.validated_at_generation,
            relation_members,
            flow_members,
            call_members,
        );
    }

    /// Discharge a mixed flow/call component to ONE joint fixed point.
    ///
    /// The flow side owns the callee-clause transfer, empty-cycle
    /// resurrection, and freshness widening; the call side owns
    /// applicability replay and the return equation. Neither closes
    /// first: a flow member can hold an in-flight call (its result joins
    /// raw, already in the caller's terms) exactly as a call can hold a
    /// body-derived callee return (read from the just-discharged
    /// override map, never the store), so the two iterate against each
    /// other's current values — both monotone joins on the same leaf
    /// lattice — until the call results stop moving, at which point the
    /// flow side has already discharged against that same map and is
    /// final too. The pass bound is one per member plus one: both sides
    /// reach their join in strictly fewer passes, so exhausting the
    /// bound means the component cannot be trusted to be at its fixed
    /// point and fails closed.
    ///
    /// `prefix_entries` carries any extra flow entries ahead of the
    /// drained members (the flow-root close's own root); their outcomes
    /// come back in order. The drained members' outcomes update in
    /// place.
    pub(super) fn discharge_mixed_component_to_fixed_point(
        &self,
        prefix_entries: Vec<super::dispatch_txn::FlowDischargeEntry>,
        flow_members: &mut [DrainedFlowReturnMember],
        call_members: &mut [(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )],
        replay_substitution: &ProvisionalSubstitution,
    ) -> MixedDischargeResult {
        let mut call_result_map: rustc_hash::FxHashMap<
            crate::semantic_query::ResolveCallKey,
            SemanticNodeId,
        > = rustc_hash::FxHashMap::default();
        let mut prefix_outcomes: Vec<FlowReturnPendingOutcome> = prefix_entries
            .iter()
            .map(|entry| entry.outcome.clone())
            .collect();
        let bound = prefix_entries.len() + flow_members.len() + call_members.len() + 1;
        // The observed convergence of the joint fixed point: every
        // flow-side pass the discharge runs, accumulated across the mixed
        // passes. The loop exits only on a stable pass (the call results
        // stopped moving, with the flow side already final against them).
        let mut observed_iterations: u32 = 0;
        for _pass in 0..bound {
            if !prefix_entries.is_empty() || !flow_members.is_empty() {
                let mut entries = prefix_entries.clone();
                for (entry, outcome) in entries.iter_mut().zip(prefix_outcomes.iter()) {
                    entry.outcome = outcome.clone();
                }
                for member in flow_members.iter() {
                    entries.push(super::dispatch_txn::FlowDischargeEntry {
                        key: member.key.clone(),
                        outcome: member.outcome.clone(),
                        holds: member.holds.clone(),
                        fresh_seed: member.fresh_seed,
                    });
                }
                let observed =
                    self.discharge_flow_component_to_fixed_point(&mut entries, &call_result_map);
                observed_iterations = observed_iterations.saturating_add(observed.iterations);
                let split = entries.len() - flow_members.len();
                prefix_outcomes = entries[..split]
                    .iter()
                    .map(|entry| entry.outcome.clone())
                    .collect();
                for (member, entry) in flow_members.iter_mut().zip(entries[split..].iter()) {
                    member.outcome = entry.outcome.clone();
                }
            }
            if call_members.is_empty() {
                return Ok((
                    prefix_outcomes,
                    Vec::new(),
                    super::dispatch_txn::flow_obligation_state::ObservedFlowConvergence {
                        iterations: observed_iterations,
                        stable: true,
                    },
                ));
            }
            // The overrides the call equation reads its flow hold targets
            // from: the JUST-discharged drained members AND any prefix
            // entries (a flow root is a hold target like any other) —
            // final at this pass but not yet published, so never the
            // store.
            let mut flow_overrides: rustc_hash::FxHashMap<
                crate::semantic_query::FlowReturnKey,
                SemanticNodeId,
            > = flow_members
                .iter()
                .filter_map(|member| match &member.outcome {
                    FlowReturnPendingOutcome::EvaluatedValue(result) => {
                        Some((member.key.clone(), result.return_type()))
                    }
                    FlowReturnPendingOutcome::NoValue { .. } => None,
                })
                .collect();
            for (entry, outcome) in prefix_entries.iter().zip(prefix_outcomes.iter()) {
                if let FlowReturnPendingOutcome::EvaluatedValue(result) = outcome {
                    flow_overrides.insert(entry.key.clone(), result.return_type());
                }
            }
            let new_results = self.solve_drained_call_members(
                call_members,
                &flow_overrides,
                replay_substitution,
            )?;
            let new_map: rustc_hash::FxHashMap<
                crate::semantic_query::ResolveCallKey,
                SemanticNodeId,
            > = new_results
                .iter()
                .map(|(key, _, result)| {
                    (
                        key.clone(),
                        super::return_equation::resolved_call_return_type(result),
                    )
                })
                .collect();
            if new_map == call_result_map {
                return Ok((
                    prefix_outcomes,
                    new_results,
                    super::dispatch_txn::flow_obligation_state::ObservedFlowConvergence {
                        iterations: observed_iterations,
                        stable: true,
                    },
                ));
            }
            call_result_map = new_map;
        }
        Err(crate::semantic_query::ResolveCallFailure::Undecidable)
    }

    /// Replay + solve the drained call members of one closing component.
    ///
    /// Relation-only applicability assumptions replay against the
    /// caller's converged provisional table; the survivors solve their
    /// return equation with the JUST-discharged in-component flow results
    /// as overrides — those targets are final at the close but not yet
    /// published, so they must never be read from the store.
    pub(super) fn solve_drained_call_members(
        &self,
        call_members: &mut [(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
        )],
        flow_overrides: &rustc_hash::FxHashMap<
            crate::semantic_query::FlowReturnKey,
            SemanticNodeId,
        >,
        replay_substitution: &ProvisionalSubstitution,
    ) -> Result<Vec<DrainedCallResult>, crate::semantic_query::ResolveCallFailure> {
        if call_members.is_empty() {
            return Ok(Vec::new());
        }
        for (key, state) in call_members.iter_mut() {
            if !state.replay_applicability {
                continue;
            }
            *state = self.replay_resolve_call_pending(key, state, replay_substitution)?;
        }
        let equation: Vec<super::dispatch_txn::ReturnEquationMember> = call_members
            .iter()
            .map(|(key, state)| super::dispatch_txn::ReturnEquationMember {
                fresh_literal_returns: state.selection.fresh_literal_returns().to_vec(),
                identity: super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key.clone()),
                concrete_seeds: state.concrete_seeds.clone(),
                holds: state.holds.clone(),
                domain: super::dispatch_txn::ReturnDomainMetadata::ResolveCall,
            })
            .collect();
        let solved = self
            .solve_return_equation(&equation, flow_overrides)
            .map_err(|_| crate::semantic_query::ResolveCallFailure::Undecidable)?;
        Ok(call_members
            .iter()
            .zip(solved.iter().copied())
            .map(|((key, state), return_type)| {
                (
                    key.clone(),
                    state.clone(),
                    state.selection.with_return_type(self, return_type),
                )
            })
            .collect())
    }

    pub(super) fn resolve_call_abort_drained_flights(
        &self,
        members: &[(
            crate::semantic_query::ResolveCallKey,
            ResolveCallPendingState,
            crate::semantic_query::ResolvedCallResult,
        )],
    ) {
        for (_, state, _) in members {
            self.abort_inline_flight(state.inline_flight.as_ref());
            if let Some(session) = state.staged_session {
                self.abandon_session(session);
            }
        }
    }

    pub(super) fn relation_abort_completed_members(&self) {
        let (members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.flow.closed_values.clear();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        for member in &members {
            self.abort_inline_flight(member.inline_flight.as_ref());
        }
        for member in &flow_members {
            self.abort_inline_flight(member.inline_flight.as_ref());
        }
        for member in &call_members {
            self.abort_inline_flight(member.inline_flight.as_ref());
        }
    }

    fn relation_publication_roots(
        &self,
        root_key: &RelateMemoKey,
        member_keys: impl IntoIterator<Item = RelateMemoKey>,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        let mut nodes = vec![root_key.source, root_key.target];
        for member in member_keys {
            nodes.push(member.source);
            nodes.push(member.target);
        }
        self.observed_self_roots_from_nodes(nodes)
    }

    fn relation_completed_publication_roots(
        &self,
        root_key: &RelateMemoKey,
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        let (member_keys, flow_self_roots, call_self_roots) = {
            let txn = self.dispatch_txn.borrow();
            (
                txn.relation
                    .completed_members
                    .iter()
                    .map(|member| member.key.clone())
                    .collect::<Vec<_>>(),
                txn.flow
                    .completed_members
                    .iter()
                    .flat_map(|member| member.self_roots.iter().cloned())
                    .collect::<Vec<_>>(),
                txn.call
                    .completed_members
                    .iter()
                    .flat_map(|member| member.self_roots.iter().cloned())
                    .collect::<Vec<_>>(),
            )
        };
        // The published component's self-roots are the UNION of every
        // drained member's roots across BOTH domains: a flow member's file
        // roots ride the relation-rooted carrier, so a cross-file edit
        // invalidates the whole component.
        let mut roots = self.relation_publication_roots(root_key, member_keys);
        for root in flow_self_roots {
            if !roots.iter().any(|(canonical, _)| canonical == &root.0) {
                roots.push(root);
            }
        }
        for root in call_self_roots {
            if !roots.iter().any(|(canonical, _)| canonical == &root.0) {
                roots.push(root);
            }
        }
        roots
    }

    #[cfg(test)]
    pub(super) fn scc_publication_roots_for_tests(
        &self,
        root_key: &RelateMemoKey,
        member_keys: &[RelateMemoKey],
    ) -> Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> {
        self.relation_publication_roots(root_key, member_keys.iter().cloned())
    }

    #[cfg(test)]
    pub(super) fn publish_staged_scc_member_for_tests(
        &self,
        root_key: RelateMemoKey,
        member_key: RelateMemoKey,
    ) -> RelationStep {
        let inline_flight = self
            .graph()
            .begin_inline_relation_flight(&member_key)
            .expect("the staged member must claim its relation flight");
        let payload = self.relation_payload(
            RelationOutcome::Assignable,
            Arc::from(Vec::<InferBinding>::new().into_boxed_slice()),
            RelationProof::Assignable {
                witness: crate::semantic_query::DerivationTree {
                    sub_derivations: Arc::from(Vec::new().into_boxed_slice()),
                },
            },
        );
        self.dispatch_txn
            .borrow_mut()
            .relation
            .completed_members
            .push(CompletedSccMember {
                key: member_key,
                payload,
                inline_flight: Some(inline_flight),
            });
        self.execute_relate_root(root_key)
    }

    /// Drain the SCC-closed member batch onto the root's published
    /// SCC-union carrier (design §2.3: the published fact set is the UNION
    /// of all SCC members' observed facts, never the bare per-member set).
    ///
    /// The root's admitted publish is the component's COMMIT BOUNDARY.
    /// Every member here is independently fenced backfill: it revalidates
    /// at its own publish, and one the fence refuses stays cold and
    /// recomputes on demand rather than weakening the committed root.
    fn relation_drain_completed_members(
        &self,
        root_key: &RelateMemoKey,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
    ) {
        let (members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.flow.closed_values.clear();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        self.publish_scc_member_batch(
            crate::semantic_query_memo::SccRootWitness::relate(
                root_key.clone(),
                carrier.admission_seq,
            ),
            carrier,
            members,
            flow_members,
            call_members,
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // Inference pattern detection + session plumbing
    // ──────────────────────────────────────────────────────────────────

    /// Upgrade a plain relation key with the target pattern's immutable
    /// session-setup fingerprint. Session opening consumes the same setup
    /// value, so there is no second projection to drift.
    pub(super) fn relation_key_with_inference(&self, mut key: RelateMemoKey) -> RelateMemoKey {
        if key.relation != RelationKind::Assignable
            || self.dispatch_txn.borrow().binding_is_disabled()
        {
            return key;
        }
        // Only upgrade when a binding could actually occur: the pattern
        // scan is cached per target node on the transaction.
        let Some(pattern) = self.relation_pattern_info(key.target) else {
            return key;
        };
        // Canonicalize even a caller-supplied context: behavior and memo
        // identity are one projection of the same frozen setup.
        key.inference_context = Some(pattern.setup.context_key().clone());
        key
    }

    /// Raw family dispatch accepts only the target pattern's exact frozen
    /// inference context. A target without an inferable pattern accepts no
    /// caller-supplied context. Reverse-projection sub-relations do not enter
    /// through raw dispatch; they retain the active session context through
    /// [`Self::relation_sub_key`].
    pub(super) fn relation_raw_key_has_exact_inference_context(&self, key: &RelateMemoKey) -> bool {
        if key.relation != RelationKind::Assignable {
            return key.inference_context.is_none();
        }
        let expected = self
            .relation_pattern_info(key.target)
            .map(|pattern| pattern.setup.context_key().clone());
        key.inference_context == expected
    }

    /// Detect an in-scope conditional-`infer` pattern on `target`. Direct
    /// infer occupants are supported in bare, object, tuple, array, and
    /// function positions. An exact unremapped homomorphic mapped target
    /// enables reverse projection; all other deeper nesting stays deferred.
    /// Results are cached per target node on the transaction.
    pub(super) fn relation_pattern_info(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        if let Some(cached) = self
            .dispatch_txn
            .borrow()
            .relation
            .pattern_cache
            .get(&target)
        {
            return cached.clone();
        }
        let computed = self.relation_pattern_info_uncached(target);
        self.dispatch_txn
            .borrow_mut()
            .relation
            .pattern_cache
            .insert(target, computed.clone());
        computed
    }

    fn relation_pattern_info_uncached(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        let graph = self.graph();
        if let Some(spec) = self.reverse_homomorphic_spec(target) {
            let name = match graph.node_data(spec.base_infer).as_deref() {
                Some(SemanticNodeData::Infer { name, .. }) => Arc::clone(name),
                _ => return None,
            };
            return Some(InferPatternInfo::new(
                InferPatternShape::ReverseHomomorphicMapped,
                vec![InferParamSite {
                    node: spec.base_infer,
                    name,
                    priority: InferenceCandidatePriority::HomomorphicMapped,
                }],
                Some(spec),
            ));
        }
        match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Infer { name, .. }) => Some(InferPatternInfo::new(
                InferPatternShape::Bare,
                vec![InferParamSite {
                    node: target,
                    name: Arc::clone(name),
                    priority: InferenceCandidatePriority::NakedTypeParameter,
                }],
                None,
            )),
            Some(SemanticNodeData::Object(view)) => {
                let mut sites = Vec::new();
                for member in view.positive_members().iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(member.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: member.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::ObjectProps, sites, None))
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let mut sites = Vec::new();
                for element in elements.iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(element.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: element.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::TupleHeadTail, sites, None))
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                if let Some(SemanticNodeData::Infer { name, .. }) =
                    graph.node_data(*element).as_deref()
                {
                    Some(InferPatternInfo::new(
                        InferPatternShape::ArrayElement,
                        vec![InferParamSite {
                            node: *element,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        }],
                        None,
                    ))
                } else {
                    None
                }
            }
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                predicate,
                ..
            }) => {
                let mut sites = Vec::new();
                for param in params.iter() {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(param.ty).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: param.ty,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    } else {
                        // A function parameter may itself be a variadic
                        // tuple/array inference pattern (`...args:
                        // [...infer R]`). The function relation flips the
                        // occurrence to contravariant; the nested
                        // container reducer deposits into these exact
                        // sites.
                        match graph.node_data(param.ty).as_deref() {
                            Some(SemanticNodeData::Tuple { elements, .. }) => {
                                for element in elements.iter() {
                                    if let Some(SemanticNodeData::Infer { name, .. }) =
                                        graph.node_data(element.value).as_deref()
                                    {
                                        sites.push(InferParamSite {
                                            node: element.value,
                                            name: Arc::clone(name),
                                            priority: InferenceCandidatePriority::Argument,
                                        });
                                    }
                                }
                            }
                            Some(SemanticNodeData::Array { element, .. }) => {
                                if let Some(SemanticNodeData::Infer { name, .. }) =
                                    graph.node_data(*element).as_deref()
                                {
                                    sites.push(InferParamSite {
                                        node: *element,
                                        name: Arc::clone(name),
                                        priority: InferenceCandidatePriority::Argument,
                                    });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                if let Some(SemanticNodeData::Infer { name, .. }) =
                    graph.node_data(*return_type).as_deref()
                {
                    sites.push(InferParamSite {
                        node: *return_type,
                        name: Arc::clone(name),
                        priority: InferenceCandidatePriority::ReturnType,
                    });
                }
                // `x is infer U`: the predicate target is inferred from the
                // source predicate where the return would be.
                if let Some(target) = predicate.and_then(|predicate| predicate.ty) {
                    if let Some(SemanticNodeData::Infer { name, .. }) =
                        graph.node_data(target).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: target,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::ReturnType,
                        });
                    }
                }
                (!sites.is_empty())
                    .then(|| InferPatternInfo::new(InferPatternShape::Function, sites, None))
            }
            _ => None,
        }
    }

    /// Recognize only the exact `{ [P in keyof infer T]: X }` descriptor.
    fn reverse_homomorphic_spec(
        &self,
        mapped_node: SemanticNodeId,
    ) -> Option<ReverseHomomorphicSpec> {
        let graph = self.graph();
        let mapped = graph.node_data(mapped_node)?;
        let (source, mapper) = match mapped.as_ref() {
            SemanticNodeData::Mapped { source, mapper } if mapper.name_remap.is_none() => {
                (*source, mapper.clone())
            }
            _ => return None,
        };
        drop(mapped);

        let source = self.peel_relation_alias(source)?;
        let key_space = self.peel_relation_alias(mapper.key_space)?;
        let key_base = match graph.node_data(key_space).as_deref() {
            Some(SemanticNodeData::KeyOf { base }) => self.peel_relation_alias(*base)?,
            _ => return None,
        };
        if source != key_base
            || !matches!(
                graph.node_data(key_base).as_deref(),
                Some(SemanticNodeData::Infer { .. })
            )
            || !matches!(
                graph.node_data(mapper.parameter_node).as_deref(),
                Some(SemanticNodeData::TypeParam { .. })
            )
        {
            return None;
        }
        Some(ReverseHomomorphicSpec {
            mapped_node,
            base_infer: key_base,
            mapper_parameter: mapper.parameter_node,
            template: mapper.value_expr,
            modifiers: ReverseMappedModifiers {
                optionality: mapper.optionality,
                readonly: mapper.readonly,
            },
        })
    }

    fn peel_relation_alias(&self, node: SemanticNodeId) -> Option<SemanticNodeId> {
        let mut current = node;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            match self.graph().node_data(current).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => current = *inner,
                Some(_) => return Some(current),
                None => return None,
            }
        }
        None
    }

    /// The transient inference occurrence of the current reducer. A popped
    /// SCC member re-discharges through a virtual root occurrence until it
    /// opens a nested real frame; ordinary structural frames read their
    /// occurrence from the nearest open RELATE ancestor on the shared
    /// reentry stack.
    fn relation_current_occurrence(&self) -> InferenceOccurrence {
        let txn = self.dispatch_txn.borrow();
        if let Some((virtual_depth, occurrence)) = txn.relation.redischarge_occurrence {
            if txn.reentry().depth() <= virtual_depth {
                return occurrence;
            }
        }
        txn.reentry()
            .nearest_relate()
            .map(|(_, occurrence)| occurrence)
            .unwrap_or(InferenceOccurrence::ARGUMENT_COVARIANT)
    }

    fn relation_occurrence(&self, position: InferPosition) -> InferenceOccurrence {
        inference_occurrence_for_position(self.relation_current_occurrence(), position)
    }

    /// Deposit an inference candidate into the active session (a
    /// session-local delta — the deposit itself is ReturnOnly, never
    /// published). The current top frame records the delta flag ONLY when
    /// the session belongs to an OUTER frame (admission row 7); the
    /// binding root's own deposits into its OWN session do not suppress
    /// its publish (its payload carries the session's fixed bindings).
    fn relation_deposit(
        &self,
        param_node: SemanticNodeId,
        mut bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let (call_policy, deposit_is_top_level) = {
            let txn = self.dispatch_txn.borrow();
            (
                txn.active_session()
                    .and_then(|session| session.call_const_policy(param_node))
                    .zip(txn.call_argument_literal_mode()),
                txn.call_argument_target_is_top_level(param_node),
            )
        };
        if let Some((policy, literal_mode)) = call_policy {
            // A bare literal argument's candidate widens under the
            // inferring parameter's own const policy; an argument whose
            // authored form already pins its type deposits as authored.
            // A NAKED top-level inference position preserves a primitive
            // literal (the constraint is an upper-bound check, not a
            // widening target: `cstr<T extends string>("a")` is `"a"`);
            // nested positions — an array element, an object member —
            // widen as before.
            if literal_mode == crate::semantic_query::ArgumentLiteralMode::Widened {
                let preserve_top_literal = deposit_is_top_level
                    && policy == crate::semantic_query::ConstParamPolicy::NonConst
                    && matches!(
                        self.graph().node_data(bound).as_deref(),
                        Some(SemanticNodeData::Literal(_))
                    );
                if !preserve_top_literal {
                    bound = self.call_inference_candidate(bound, policy);
                } else {
                    // A preserved bare literal at a naked position is FRESH
                    // provenance for an unconstrained parameter (the note
                    // is a no-op for a constrained one, whose preserved
                    // literal is regular).
                    if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                        session.note_fresh_literal_deposit(param_node, bound);
                    }
                }
            }
        }
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit(param_node, bound, occurrence.priority, occurrence.variance)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    fn relation_projection_target(&self, node: SemanticNodeId) -> bool {
        self.dispatch_txn
            .borrow()
            .active_session()
            .is_some_and(|session| session.is_projection_target(node))
    }

    /// Deposit the assembled reverse candidate through the same frame/session
    /// ownership gate as ordinary and projection candidates. A nested frame
    /// mutating an outer session is a session-local delta and therefore cannot
    /// publish an otherwise context-free relation payload.
    fn relation_reverse_aggregate_deposit(
        &self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
    ) -> bool {
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let accepted = txn.active_session_mut().is_some_and(|session| {
            session.deposit_reverse_aggregate(param_node, candidate, priority)
        });
        if !accepted {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    /// Deposit into a registered reverse projection. The indexed access is
    /// only a projection target; it never becomes an `Infer` declaration.
    fn relation_projection_deposit(
        &self,
        projection: SemanticNodeId,
        bound: SemanticNodeId,
        occurrence: InferenceOccurrence,
    ) -> bool {
        let bound = match self.unwrap_identity_carrier_for_relation(bound) {
            IdentityCarrierUnwrap::Concrete(bound) => bound,
            IdentityCarrierUnwrap::Unresolvable => return false,
        };
        if self.relation_subtree_contains_semantically_unresolved(bound)
            || super::raise::node_is_unknown_materializing_failure(self, bound)
            || super::raise::node_contains_semantic_miss_with_dispatch(self, bound) != Some(false)
        {
            return false;
        }
        let Some(bound_data) = self.graph().node_data(bound) else {
            return false;
        };
        if is_deferred(&bound_data)
            || matches!(
                bound_data.as_ref(),
                SemanticNodeData::TypeParam { .. }
                    | SemanticNodeData::Infer { .. }
                    | SemanticNodeData::InferRef { .. }
            )
        {
            return false;
        }
        drop(bound_data);
        let mut txn = self.dispatch_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        let deposited = txn.active_session_mut().is_some_and(|session| {
            session.deposit_projection(projection, bound, occurrence.priority, occurrence.variance)
        });
        if !deposited {
            return false;
        }
        txn.relation.accepted_inference_deposits += 1;
        txn.note_candidate_write(active_id);
        true
    }

    fn relation_subtree_contains_semantically_unresolved(&self, root: SemanticNodeId) -> bool {
        self.relation_subtree_matches(root, |_, data| data.means_type_is_not_yet_known())
    }

    fn relation_reverse_input_is_semantically_resolved(&self, root: SemanticNodeId) -> bool {
        !self.relation_subtree_contains_semantically_unresolved(root)
            && !super::raise::node_is_unknown_materializing_failure(self, root)
            && super::raise::node_contains_semantic_miss_with_dispatch(self, root) == Some(false)
    }

    /// Enforces the assembled-input preflight documented in `/type-resolution`
    /// under "Reverse-homomorphic mapped recovery".
    fn relation_reverse_source_inputs_are_semantically_resolved(
        &self,
        source: SemanticNodeId,
    ) -> bool {
        let Some(data) = self.graph().node_data(source) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::Object(surface) => {
                surface.positive_members().iter().all(|member| {
                    self.relation_reverse_input_is_semantically_resolved(member.value)
                }) && surface.index_signatures.iter().all(|signature| {
                    self.relation_reverse_input_is_semantically_resolved(signature.key_type)
                        && self
                            .relation_reverse_input_is_semantically_resolved(signature.value_type)
                })
            }
            SemanticNodeData::Array { element, .. } => {
                self.relation_reverse_input_is_semantically_resolved(*element)
            }
            SemanticNodeData::Tuple { elements, .. } => elements
                .iter()
                .all(|element| self.relation_reverse_input_is_semantically_resolved(element.value)),
            _ => false,
        }
    }

    fn relation_subtree_matches(
        &self,
        root: SemanticNodeId,
        mut matches: impl FnMut(SemanticNodeId, &SemanticNodeData) -> bool,
    ) -> bool {
        let graph = self.graph();
        let mut visited = FxHashSet::default();
        let mut stack = vec![root];
        while let Some(node) = stack.pop() {
            if !visited.insert(node) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            if matches(node, data.as_ref()) {
                return true;
            }
            match data.as_ref() {
                SemanticNodeData::IntrinsicApplication { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                SemanticNodeData::Alias(inner) => stack.push(*inner),
                SemanticNodeData::ClassExpressionInstance {
                    type_arguments,
                    surface,
                    ..
                } => {
                    stack.extend(type_arguments.iter().copied());
                    stack.push(*surface);
                }
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    let members = composite.composite_members().expect("composite arm");
                    stack.extend(members.iter().copied());
                }
                SemanticNodeData::Array { element, .. } => stack.push(*element),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| element.value));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(surface.positive_members().iter().map(|member| member.value));
                    stack.extend(surface.call_signatures.iter().copied());
                    stack.extend(surface.construct_signatures.iter().copied());
                    for signature in surface.index_signatures.iter() {
                        stack.push(signature.key_type);
                        stack.push(signature.value_type);
                    }
                    if let Some(keyspace) = surface.keyspace {
                        stack.push(keyspace);
                    }
                }
                SemanticNodeData::ObjectSpreadProgram(program) => {
                    stack.extend(program.child_nodes());
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    predicate,
                    ..
                } => {
                    stack.extend(params.iter().map(|parameter| parameter.ty));
                    stack.push(*return_type);
                    for parameter in type_parameters.iter() {
                        stack.extend(parameter.constraint);
                        stack.extend(parameter.default);
                    }
                    stack.extend(predicate.and_then(|predicate| predicate.ty));
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().copied());
                }
                SemanticNodeData::KeyOf { base } => stack.push(*base),
                SemanticNodeData::IndexedAccess { object, index } => {
                    stack.push(*object);
                    if let IndexKey::Computed(index) = index {
                        stack.push(*index);
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push(*source);
                    stack.push(mapper.key_space);
                    stack.push(mapper.value_expr);
                    stack.extend(mapper.name_remap);
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    pending,
                    ..
                } => {
                    if let Some(pending) = pending {
                        stack.extend(pending.argument_nodes());
                    }
                    stack.extend([*check, *extends, *true_branch_ref, *false_branch_ref]);
                }
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().copied());
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(contributors.iter().copied());
                }
                SemanticNodeData::SyntheticBinding { value_node, .. } => {
                    stack.push(SemanticNodeId(*value_node));
                }
                SemanticNodeData::TypeParam {
                    constraint,
                    default,
                    ..
                } => {
                    stack.extend(constraint.iter().copied());
                    stack.extend(default.iter().copied());
                }
                SemanticNodeData::TypeOf(_)
                | SemanticNodeData::BareRef(_)
                | SemanticNodeData::ImportType(_) => {
                    stack.extend(data.carrier_type_args().iter().copied());
                }
                SemanticNodeData::Primitive(_)
                | SemanticNodeData::Literal(_)
                | SemanticNodeData::EnumLiteral(_)
                | SemanticNodeData::Opaque(_)
                | SemanticNodeData::Infer { .. }
                | SemanticNodeData::InferRef { .. }
                | SemanticNodeData::DeclRef { .. }
                // The nominal terminal's payload is a scalar identity — no
                // infer-bearing child and no carrier args to descend.
                | SemanticNodeData::TypeOfNominal(_)
                // The sealed callable carrier never carries an `infer`
                // placeholder.
                | SemanticNodeData::DeferredCallable(_)
                | SemanticNodeData::RawFallback { .. } => {}
            }
        }
        false
    }

    fn try_relation_projection(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut [InferBinding],
        occurrence: InferenceOccurrence,
    ) -> Option<RelationResult> {
        let projection = match occurrence.variance {
            VariancePhase::Covariant => self
                .relation_projection_target(target)
                .then_some((target, source)),
            VariancePhase::Contravariant => self
                .relation_projection_target(source)
                .then_some((source, target)),
            VariancePhase::Invariant => {
                if self.relation_projection_target(target) {
                    Some((target, source))
                } else {
                    self.relation_projection_target(source)
                        .then_some((source, target))
                }
            }
        };
        let projection = projection?;
        Some(
            if self.relation_projection_deposit(projection.0, projection.1, occurrence) {
                assignable(bindings)
            } else {
                RelationResult::Unknown
            },
        )
    }

    /// Whether an inference session is currently active.
    fn relation_session_active(&self) -> bool {
        self.dispatch_txn.borrow().active_session().is_some()
    }

    /// Checkpoint the ACTIVE inference session's deposits (`None` when no
    /// session is active). The alternative-scoping half of the
    /// losing-alternative rule: a first-match loop over overload /
    /// signature-group alternatives brackets each alternative with a
    /// checkpoint and rolls back on failure, so a LOSING alternative's
    /// deposits never reach fixation (`{ (a: number, b: number): void;
    /// (a: string, b: string): void } extends (a: infer U, b: string) =>
    /// void` fixes `U := string`, never `number ∧ string`).
    fn relation_session_checkpoint(&self) -> Option<SessionCheckpoint> {
        self.dispatch_txn
            .borrow()
            .active_session()
            .map(InferenceSession::checkpoint)
    }

    /// Roll the ACTIVE session's deposits back to `checkpoint` (no-op when
    /// no session is active or no checkpoint was taken).
    fn relation_session_rollback(&self, checkpoint: &Option<SessionCheckpoint>) {
        if let Some(checkpoint) = checkpoint {
            if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                session.rollback_to(checkpoint);
            }
        }
    }

    fn relate_pair_alternatives(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_pair_alternatives_with_freshness(alternatives, bindings, position, false)
    }

    fn relate_union_target_alternatives(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_pair_alternatives_with_freshness(alternatives, bindings, position, true)
    }

    /// The checker's `typeRelatedToDiscriminatedType`: an object `source`
    /// whose discriminant properties — those a member of the target union
    /// `members` declares with a unit type — hold unions of unit types is
    /// related once per combination of those units, each combination
    /// narrowing the source's discriminants to it (`{ kind: "cat" | "dog" }`
    /// fits `{ kind: "cat" } | { kind: "dog" }`). At most 25 combinations
    /// are generated, as the checker's limit. `None` when the source is not
    /// an object, declares no such discriminant, or has too many
    /// combinations.
    fn relate_discriminated_object_source(
        &self,
        source: SemanticNodeId,
        members: &[SemanticNodeId],
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        const MAX_DISCRIMINATED_COMBINATIONS: usize = 25;
        let graph = self.graph();
        let source_view = match graph.node_data(source)?.as_ref() {
            SemanticNodeData::Object(view) => view.clone(),
            _ => return None,
        };
        let transit = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let target_views: Vec<SurfaceView> = members
            .iter()
            .filter_map(|member| {
                match self.normalize_node_for_structural_fact_demand(*member, transit) {
                    super::evaluate::StructuralFactDemandOutcome::Complete(node) => {
                        match graph.node_data(node)?.as_ref() {
                            SemanticNodeData::Object(view) => Some(view.clone()),
                            _ => None,
                        }
                    }
                    _ => None,
                }
            })
            .collect();
        let is_unit = |node: SemanticNodeId| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(
                    SemanticNodeData::Literal(_)
                        | SemanticNodeData::EnumLiteral(_)
                        | SemanticNodeData::Primitive(
                            PrimitiveKind::Null | PrimitiveKind::Undefined
                        )
                )
            )
        };
        // The unit constituents of a discriminant value: its literals, or
        // `boolean` as `true | false`.
        let units_of = |node: SemanticNodeId| -> Option<Vec<SemanticNodeId>> {
            let literal = |value: bool| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(value)))
            };
            let arms: Vec<SemanticNodeId> = match graph.node_data(node)?.as_ref() {
                SemanticNodeData::Union(arms) => arms.iter().copied().collect(),
                _ => vec![node],
            };
            let mut units = Vec::new();
            for arm in arms {
                if matches!(
                    graph.node_data(arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
                ) {
                    units.push(literal(true));
                    units.push(literal(false));
                } else if is_unit(arm) {
                    units.push(arm);
                } else {
                    return None;
                }
            }
            Some(units)
        };
        let mut discriminants: Vec<(usize, Vec<SemanticNodeId>)> = Vec::new();
        for (index, member) in source_view.positive_members().iter().enumerate() {
            if member.optional || member.method_kind.is_some() {
                continue;
            }
            let discriminant = target_views.iter().any(|view| {
                view.positive_members().iter().any(|target| {
                    target.key == member.key
                        && target.method_kind.is_none()
                        && units_of(target.value).is_some()
                })
            });
            if !discriminant {
                continue;
            }
            let Some(units) = units_of(member.value) else {
                continue;
            };
            if units.len() > 1 {
                discriminants.push((index, units));
            }
        }
        if discriminants.is_empty() {
            return None;
        }
        let combinations = discriminants
            .iter()
            .try_fold(1usize, |count, (_, units)| count.checked_mul(units.len()))?;
        if combinations > MAX_DISCRIMINATED_COMBINATIONS {
            return None;
        }
        let mut any_unknown = false;
        for combination in 0..combinations {
            let mut rest = combination;
            let mut narrowed: Vec<crate::semantic_query::SurfaceMember> =
                source_view.positive_members().to_vec();
            for (index, units) in &discriminants {
                narrowed[*index].value = units[rest % units.len()];
                rest /= units.len();
            }
            let narrowed = graph.intern_node(SemanticNodeData::Object(
                source_view
                    .clone()
                    .with_positive_members(Arc::from(narrowed.into_boxed_slice())),
            ));
            let alternatives: Vec<_> = members.iter().map(|member| (narrowed, *member)).collect();
            match self.relate_union_target_alternatives(
                &alternatives,
                bindings,
                InferPosition::Covariant,
            ) {
                RelationResult::Assignable { .. } => {}
                RelationResult::Unknown => any_unknown = true,
                RelationResult::NotAssignable => return Some(RelationResult::NotAssignable),
            }
        }
        Some(if any_unknown {
            RelationResult::Unknown
        } else {
            assignable(bindings)
        })
    }

    fn relate_pair_alternatives_with_freshness(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
        excess_prepass_completed: bool,
    ) -> RelationResult {
        let mut any_unknown = false;
        for (source, target) in alternatives {
            let checkpoint = self.relation_session_checkpoint();
            let bindings_len = bindings.len();
            let result = if excess_prepass_completed {
                self.relate_union_arm_after_excess_prepass(*source, *target, bindings, position)
            } else {
                self.relate_member(*source, *target, bindings, position)
            };
            match result {
                result @ RelationResult::Assignable { .. } => return result,
                RelationResult::Unknown => {
                    self.relation_session_rollback(&checkpoint);
                    bindings.truncate(bindings_len);
                    any_unknown = true;
                }
                RelationResult::NotAssignable => {
                    self.relation_session_rollback(&checkpoint);
                    bindings.truncate(bindings_len);
                }
            }
        }
        if any_unknown {
            RelationResult::Unknown
        } else {
            RelationResult::NotAssignable
        }
    }

    /// Relate an intersection `source` none of whose constituents is
    /// assignable to the object `target` on its own, as the one object the
    /// constituents compose — TypeScript's structural fallback for an
    /// intersection source (measured on the pinned checker: over `interface
    /// QA { qa: 1 }` and `interface QB { qb: 2 }`, `QA & QB extends { qa:
    /// 1; qb: 2 }` is true though neither arm is). The composition is the
    /// shared surface reader's; one it cannot produce leaves the pair
    /// undecided, since no constituent's failure proves the whole fails.
    fn relate_composed_intersection(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let Some((_, composed)) = self.resolve_typeinfo_surface_view_with_node(
            source,
            ProjectionReductionContext::structural_transit(),
        ) else {
            return RelationResult::Unknown;
        };
        if composed == source
            || !matches!(
                self.graph().node_data(composed).as_deref(),
                Some(SemanticNodeData::Object(_))
            )
        {
            return RelationResult::Unknown;
        }
        self.relate_member(composed, target, bindings, InferPosition::Covariant)
    }

    fn relate_signature_alternatives(
        &self,
        source_signatures: &[SemanticNodeId],
        target_signature: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let alternatives: Vec<_> = source_signatures
            .iter()
            .map(|source| (*source, target_signature))
            .collect();
        if self.infers_from_last_source_signature(target_signature) {
            return self.relate_overloads_inferring_from_last(
                &alternatives,
                bindings,
                InferPosition::Covariant,
            );
        }
        self.relate_pair_alternatives(&alternatives, bindings, InferPosition::Covariant)
    }

    /// An overloaded source against an inferring signature target
    /// (`inferFromSignatures`): only the source's LAST signature infers.
    /// It is related first and keeps its deposits when it holds; any
    /// other overload then decides assignability alone, its deposits
    /// rolled back — `((x: unknown) => x is A) & ((x: unknown) => false)`
    /// is assignable to `(x: unknown) => x is S` and infers nothing, so
    /// `S` is `unknown`.
    fn relate_overloads_inferring_from_last(
        &self,
        alternatives: &[(SemanticNodeId, SemanticNodeId)],
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        let Some(((last_source, last_target), rest)) = alternatives.split_last() else {
            return RelationResult::NotAssignable;
        };
        let mut any_unknown = false;
        let checkpoint = self.relation_session_checkpoint();
        let bindings_len = bindings.len();
        match self.relate_member(*last_source, *last_target, bindings, position) {
            result @ RelationResult::Assignable { .. } => return result,
            RelationResult::Unknown => any_unknown = true,
            RelationResult::NotAssignable => {}
        }
        self.relation_session_rollback(&checkpoint);
        bindings.truncate(bindings_len);
        for (source, target) in rest {
            let checkpoint = self.relation_session_checkpoint();
            let result = self.relate_member(*source, *target, bindings, position);
            self.relation_session_rollback(&checkpoint);
            bindings.truncate(bindings_len);
            match result {
                RelationResult::Assignable { .. } => return assignable(bindings),
                RelationResult::Unknown => any_unknown = true,
                RelationResult::NotAssignable => {}
            }
        }
        if any_unknown {
            RelationResult::Unknown
        } else {
            RelationResult::NotAssignable
        }
    }

    /// Whether relating an overloaded source to `target` infers the
    /// target's `infer` sites or a call's type parameters: the checker's
    /// `inferFromSignatures` reads the source's LAST signature, so `(() =>
    /// A) & (() => B)` against `() => infer R` infers `B`, `((x: unknown)
    /// => x is A) & ((x: unknown) => x is B)` against `(x: any) => x is
    /// infer U` infers `B`, and the same source handed to `takes<S>(g: (x:
    /// unknown) => x is S)` infers `S` as `B`. The overloads are then
    /// tried last first.
    fn infers_from_last_source_signature(&self, target: SemanticNodeId) -> bool {
        self.relation_session_active()
            && (matches!(
                self.graph().node_data(target).as_deref(),
                Some(SemanticNodeData::Signature { .. })
            ) || self
                .relation_pattern_info(target)
                .is_some_and(|pattern| pattern.shape == InferPatternShape::Function))
    }

    /// Recover the input of an exact homomorphic mapped target. The only
    /// externally visible output is the normal relation verdict; recovered
    /// values and the aggregate candidate stay in the active session.
    fn relate_reverse_homomorphic(
        &self,
        source: SemanticNodeId,
        spec: &ReverseHomomorphicSpec,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(source) => source,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        if !self.relation_reverse_source_inputs_are_semantically_resolved(source) {
            return RelationResult::Unknown;
        }
        let overall_checkpoint = self.relation_session_checkpoint();
        let Some(checkpoint) = overall_checkpoint.as_ref() else {
            return RelationResult::Unknown;
        };
        let bindings_len = bindings.len();
        let graph = self.graph();
        let source_shape = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                if view.has_known_index_signature() && view.index_signatures.is_empty() {
                    self.relation_session_rollback(&overall_checkpoint);
                    return RelationResult::Unknown;
                }
                ReverseSourceShape::Object
            }
            Some(SemanticNodeData::Array { readonly, .. }) => ReverseSourceShape::Array {
                readonly: *readonly,
            },
            Some(SemanticNodeData::Tuple { readonly, .. }) => ReverseSourceShape::Tuple {
                readonly: *readonly,
            },
            _ => {
                self.relation_session_rollback(&overall_checkpoint);
                return RelationResult::Unknown;
            }
        };

        let relation = match graph.node_data(source).as_deref() {
            Some(SemanticNodeData::Object(view)) => {
                let members = view.positive_members().to_vec();
                let index_signatures = view.index_signatures.to_vec();
                let mut verdict = assignable(bindings);
                for member in members {
                    let Some(optional) =
                        reverse_optional(member.optional, spec.modifiers.optionality)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    let Some(readonly) = reverse_readonly(member.readonly, spec.modifiers.readonly)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    let Some(known_key) = member.key.cloned_known() else {
                        verdict = RelationResult::Unknown;
                        break;
                    };
                    let key = match known_key {
                        crate::semantic_query::PropertyKey::String(name) => graph.intern_node(
                            SemanticNodeData::Literal(LiteralValue::String(name.to_string())),
                        ),
                        crate::semantic_query::PropertyKey::Number(number) => graph.intern_node(
                            SemanticNodeData::Literal(LiteralValue::Number(number.get() as f64)),
                        ),
                        crate::semantic_query::PropertyKey::UniqueSymbol(_) => {
                            verdict = RelationResult::Unknown;
                            break;
                        }
                    };
                    verdict = self.recover_reverse_projection(
                        member.value,
                        key,
                        spec,
                        bindings,
                        move |value| {
                            let mut recovered = member;
                            recovered.value = value;
                            recovered.optional = optional;
                            recovered.readonly = readonly;
                            ReverseRecoveredEntry::ObjectMember { member: recovered }
                        },
                    );
                    if !matches!(verdict, RelationResult::Assignable { .. }) {
                        break;
                    }
                }
                if matches!(verdict, RelationResult::Assignable { .. }) {
                    for signature in index_signatures {
                        let Some(readonly) =
                            reverse_readonly(signature.readonly, spec.modifiers.readonly)
                        else {
                            verdict = RelationResult::NotAssignable;
                            break;
                        };
                        let key = signature.key_type;
                        verdict = self.recover_reverse_projection(
                            signature.value_type,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = signature;
                                recovered.value_type = value;
                                recovered.readonly = readonly;
                                ReverseRecoveredEntry::IndexSignature {
                                    signature: recovered,
                                }
                            },
                        );
                        if !matches!(verdict, RelationResult::Assignable { .. }) {
                            break;
                        }
                    }
                }
                verdict
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                let number_key =
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                self.recover_reverse_projection(*element, number_key, spec, bindings, |value| {
                    ReverseRecoveredEntry::ArrayElement { value }
                })
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let elements = elements.to_vec();
                let mut verdict = assignable(bindings);
                let mut variadic_key_domain = false;
                for (index, element) in elements.into_iter().enumerate() {
                    let Some(optional) =
                        reverse_optional(element.optional, spec.modifiers.optionality)
                    else {
                        verdict = RelationResult::NotAssignable;
                        break;
                    };
                    if element.rest {
                        variadic_key_domain = true;
                        let rest = match self.unwrap_identity_carrier_for_relation(element.value) {
                            IdentityCarrierUnwrap::Concrete(rest) => rest,
                            IdentityCarrierUnwrap::Unresolvable => {
                                verdict = RelationResult::Unknown;
                                break;
                            }
                        };
                        let Some(rest_data) = graph.node_data(rest) else {
                            verdict = RelationResult::Unknown;
                            break;
                        };
                        let (rest_element, rest_readonly) = match rest_data.as_ref() {
                            SemanticNodeData::Array { element, readonly } => (*element, *readonly),
                            _ => {
                                verdict = RelationResult::Unknown;
                                break;
                            }
                        };
                        drop(rest_data);
                        let key =
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
                        verdict = self.recover_reverse_projection(
                            rest_element,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = element;
                                recovered.value = graph.intern_node(SemanticNodeData::Array {
                                    element: value,
                                    readonly: rest_readonly,
                                });
                                recovered.optional = optional;
                                ReverseRecoveredEntry::TupleElement { element: recovered }
                            },
                        );
                    } else {
                        let key = if variadic_key_domain {
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number))
                        } else {
                            graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(
                                index.to_string(),
                            )))
                        };
                        verdict = self.recover_reverse_projection(
                            element.value,
                            key,
                            spec,
                            bindings,
                            move |value| {
                                let mut recovered = element;
                                recovered.value = value;
                                recovered.optional = optional;
                                ReverseRecoveredEntry::TupleElement { element: recovered }
                            },
                        );
                    }
                    if !matches!(verdict, RelationResult::Assignable { .. }) {
                        break;
                    }
                }
                verdict
            }
            _ => RelationResult::Unknown,
        };
        if !matches!(relation, RelationResult::Assignable { .. }) {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return relation;
        }

        let (recovered, partial) = self
            .dispatch_txn
            .borrow()
            .active_session()
            .map(|session| {
                (
                    session.recovered_since(checkpoint),
                    session.reverse_is_partial(),
                )
            })
            .unwrap_or_default();
        let Some(aggregate) = self.assemble_reverse_candidate(source_shape, recovered) else {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        };
        let priority = if partial {
            InferenceCandidatePriority::PartialHomomorphicMapped
        } else {
            InferenceCandidatePriority::HomomorphicMapped
        };
        if partial {
            if let Some(session) = self.dispatch_txn.borrow_mut().active_session_mut() {
                session.mark_reverse_partial();
            }
        }
        let deposited =
            self.relation_reverse_aggregate_deposit(spec.base_infer, aggregate, priority);
        if !deposited {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        }
        assignable(bindings)
    }

    fn recover_reverse_projection<F>(
        &self,
        source: SemanticNodeId,
        key: SemanticNodeId,
        spec: &ReverseHomomorphicSpec,
        bindings: &mut Vec<InferBinding>,
        recover: F,
    ) -> RelationResult
    where
        F: FnOnce(SemanticNodeId) -> ReverseRecoveredEntry,
    {
        let property_checkpoint = self.relation_session_checkpoint();
        let Some(checkpoint) = property_checkpoint.as_ref() else {
            return RelationResult::Unknown;
        };
        let bindings_len = bindings.len();
        let template =
            self.substitute_semantic_type_param(spec.template, spec.mapper_parameter, key);
        let projection_probe = self.graph().intern_node(SemanticNodeData::IndexedAccess {
            object: spec.base_infer,
            index: IndexKey::Computed(spec.mapper_parameter),
        });
        let projection_probe =
            self.substitute_semantic_type_param(projection_probe, spec.mapper_parameter, key);
        let expected_index = match self.graph().node_data(projection_probe).as_deref() {
            Some(SemanticNodeData::IndexedAccess { index, .. }) => index.clone(),
            _ => {
                self.relation_session_rollback(&property_checkpoint);
                return RelationResult::Unknown;
            }
        };
        let projection_targets =
            self.discover_reverse_projection_targets(template, spec.base_infer, &expected_index);
        let registered = self
            .dispatch_txn
            .borrow_mut()
            .active_session_mut()
            .is_some_and(|session| session.register_projection_targets(&projection_targets));
        if !registered {
            self.relation_session_rollback(&property_checkpoint);
            return RelationResult::Unknown;
        }

        let relation = self.relate_member(source, template, bindings, InferPosition::Covariant);
        if !matches!(relation, RelationResult::Assignable { .. }) {
            self.relation_session_rollback(&property_checkpoint);
            bindings.truncate(bindings_len);
            return relation;
        }
        let candidates = self
            .dispatch_txn
            .borrow()
            .active_session()
            .map(|session| session.projection_candidates_since(checkpoint))
            .unwrap_or_default();
        let (candidate_nodes, variance) = select_inference_candidates(&candidates);
        let projection_recovered = !candidate_nodes.is_empty();
        let recovered = if projection_recovered {
            self.relation_combine_candidates(&candidate_nodes, variance)
        } else {
            self.graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown))
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        let Some(session) = txn.active_session_mut() else {
            drop(txn);
            self.relation_session_rollback(&property_checkpoint);
            bindings.truncate(bindings_len);
            return RelationResult::Unknown;
        };
        if !projection_recovered {
            session.mark_reverse_partial();
        }
        session.push_recovered(recover(recovered));
        assignable(bindings)
    }

    fn assemble_reverse_candidate(
        &self,
        source: ReverseSourceShape,
        recovered: Vec<ReverseRecoveredEntry>,
    ) -> Option<SemanticNodeId> {
        let graph = self.graph();
        match source {
            ReverseSourceShape::Object => {
                let mut members = Vec::new();
                let mut index_signatures = Vec::new();
                for entry in recovered {
                    match entry {
                        ReverseRecoveredEntry::ObjectMember { member, .. } => {
                            members.push(member);
                        }
                        ReverseRecoveredEntry::IndexSignature { signature, .. } => {
                            index_signatures.push(signature);
                        }
                        _ => return None,
                    }
                }
                let has_index_signature = !index_signatures.is_empty();
                Some(graph.intern_node(SemanticNodeData::Object(
                    crate::semantic_query::surface_view! {
                        members: Arc::from(members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                        index_signatures: Arc::from(index_signatures.into_boxed_slice()),
                        keyspace: None,
                        has_index_signature,
                    },
                )))
            }
            ReverseSourceShape::Array { readonly } => match recovered.as_slice() {
                [ReverseRecoveredEntry::ArrayElement { value, .. }] => {
                    Some(graph.intern_node(SemanticNodeData::Array {
                        element: *value,
                        readonly,
                    }))
                }
                _ => None,
            },
            ReverseSourceShape::Tuple { readonly } => {
                let mut elements = Vec::with_capacity(recovered.len());
                for entry in recovered {
                    match entry {
                        ReverseRecoveredEntry::TupleElement { element, .. } => {
                            elements.push(element);
                        }
                        _ => return None,
                    }
                }
                match self.normalize_tuple_spread(&elements, readonly) {
                    super::build::NormalizedTupleShape::Tuple(elements) => {
                        Some(graph.intern_node(SemanticNodeData::Tuple {
                            elements: Arc::from(elements.into_boxed_slice()),
                            readonly,
                        }))
                    }
                    super::build::NormalizedTupleShape::Array(array) => Some(array),
                }
            }
        }
    }

    fn discover_reverse_projection_targets(
        &self,
        root: SemanticNodeId,
        base_infer: SemanticNodeId,
        expected_index: &IndexKey,
    ) -> Vec<SemanticNodeId> {
        let graph = self.graph();
        let Some(SemanticNodeData::Infer {
            name: base_name,
            binder: base_binder,
        }) = graph.node_data(base_infer).as_deref().cloned()
        else {
            return Vec::new();
        };
        let mut targets = Vec::new();
        let mut visited: FxHashSet<(SemanticNodeId, bool)> = FxHashSet::default();
        let mut stack = vec![(root, false)];
        while let Some((node, shadowed)) = stack.pop() {
            if !visited.insert((node, shadowed)) {
                continue;
            }
            let Some(data) = graph.node_data(node) else {
                continue;
            };
            match data.as_ref() {
                SemanticNodeData::IndexedAccess { object, index } => {
                    if !shadowed
                        && index == expected_index
                        && self.reverse_projection_object_matches(
                            *object,
                            base_infer,
                            base_binder.clone(),
                        )
                    {
                        targets.push(node);
                    }
                    stack.push((*object, shadowed));
                    if let IndexKey::Computed(index) = index {
                        stack.push((*index, shadowed));
                    }
                }
                SemanticNodeData::Mapped { source, mapper } => {
                    stack.push((*source, shadowed));
                    stack.push((mapper.key_space, shadowed));
                    let mapper_shadows = matches!(
                        graph.node_data(mapper.parameter_node).as_deref(),
                        Some(SemanticNodeData::TypeParam { display_name, .. })
                            if display_name.as_ref() == base_name.as_ref()
                    );
                    stack.push((mapper.value_expr, shadowed || mapper_shadows));
                    if let Some(remap) = mapper.name_remap {
                        stack.push((remap, shadowed || mapper_shadows));
                    }
                }
                SemanticNodeData::Conditional {
                    check,
                    extends,
                    true_branch_ref,
                    false_branch_ref,
                    pending,
                    ..
                } => {
                    let conditional_shadows =
                        self.extends_pattern_declares_infer(*extends, base_infer);
                    let (selection, _) = self.conditional_branch_selection(*check, *extends);
                    if selection != super::ConditionalBranchSelection::Deferred {
                        if let Some(Some(selected)) = self.reduce_relation_conditional(node) {
                            stack.push((
                                selected,
                                shadowed
                                    || (conditional_shadows
                                        && selection == super::ConditionalBranchSelection::True),
                            ));
                        }
                        continue;
                    }
                    stack.push((*check, shadowed));
                    stack.push((*extends, shadowed || conditional_shadows));
                    stack.push((
                        self.apply_conditional_branch_pending(
                            *true_branch_ref,
                            pending.as_deref(),
                            true,
                        ),
                        shadowed || conditional_shadows,
                    ));
                    stack.push((
                        self.apply_conditional_branch_pending(
                            *false_branch_ref,
                            pending.as_deref(),
                            false,
                        ),
                        shadowed,
                    ));
                }
                SemanticNodeData::Signature {
                    params,
                    return_type,
                    type_parameters,
                    predicate,
                    ..
                } => {
                    if shadowed
                        || type_parameters
                            .iter()
                            .any(|parameter| parameter.name.as_ref() == base_name.as_ref())
                    {
                        continue;
                    }
                    for parameter in params.iter() {
                        stack.push((parameter.ty, false));
                    }
                    stack.push((*return_type, false));
                    if let Some(target) = predicate.and_then(|predicate| predicate.ty) {
                        stack.push((target, false));
                    }
                    for parameter in type_parameters.iter() {
                        if let Some(constraint) = parameter.constraint {
                            stack.push((constraint, false));
                        }
                        if let Some(default) = parameter.default {
                            stack.push((default, false));
                        }
                    }
                }
                SemanticNodeData::Alias(inner) => stack.push((*inner, shadowed)),
                composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)) => {
                    let members = composite.composite_members().expect("composite arm");
                    stack.extend(members.iter().map(|member| (*member, shadowed)));
                }
                SemanticNodeData::Array { element, .. } => stack.push((*element, shadowed)),
                SemanticNodeData::Tuple { elements, .. } => {
                    stack.extend(elements.iter().map(|element| (element.value, shadowed)));
                }
                SemanticNodeData::Object(surface) => {
                    stack.extend(
                        surface
                            .positive_members()
                            .iter()
                            .map(|member| (member.value, shadowed)),
                    );
                    stack.extend(
                        surface
                            .call_signatures
                            .iter()
                            .chain(surface.construct_signatures.iter())
                            .map(|signature| (*signature, shadowed)),
                    );
                    for signature in surface.index_signatures.iter() {
                        stack.push((signature.key_type, shadowed));
                        stack.push((signature.value_type, shadowed));
                    }
                    if let Some(keyspace) = surface.keyspace {
                        stack.push((keyspace, shadowed));
                    }
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    stack.extend(
                        contributors
                            .iter()
                            .map(|contributor| (*contributor, shadowed)),
                    );
                }
                SemanticNodeData::TemplateLiteral { expressions, .. } => {
                    stack.extend(expressions.iter().map(|expression| (*expression, shadowed)));
                }
                SemanticNodeData::KeyOf { base } => stack.push((*base, shadowed)),
                SemanticNodeData::InstantiationRef { args, .. } => {
                    stack.extend(args.iter().map(|argument| (*argument, shadowed)));
                }
                other => {
                    stack.extend(
                        other
                            .carrier_type_args()
                            .iter()
                            .map(|argument| (*argument, shadowed)),
                    );
                }
            }
        }
        crate::semantic_query::stable_key::sort_by_stable_key(graph, &mut targets);
        targets.dedup_by(|a, b| {
            crate::semantic_query::stable_key::stable_key_for_node(graph, *a)
                == crate::semantic_query::stable_key::stable_key_for_node(graph, *b)
        });
        targets
    }

    fn reverse_projection_object_matches(
        &self,
        object: SemanticNodeId,
        base_infer: SemanticNodeId,
        base_binder: crate::semantic_query::InferBinderId,
    ) -> bool {
        let Some(object) = self.peel_relation_alias(object) else {
            return false;
        };
        if object == base_infer {
            return true;
        }
        let Some(data) = self.graph().node_data(object) else {
            return false;
        };
        match data.as_ref() {
            SemanticNodeData::InferRef { binder, .. } => *binder == base_binder,
            _ => false,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The lattice adapter: full-key sub-relations from the reducer
    // ──────────────────────────────────────────────────────────────────

    /// The full identity of a sub-relation inside the current frame:
    /// inherits the relation kind / policy / freshness / env context of the
    /// nearest open RELATE ancestor — never the untyped top of a mixed
    /// stack. Ordinary direct-infer member judgements remain
    /// session-independent. Reverse-projection judgements retain the frozen
    /// inference context because registered indexed-access targets alter
    /// their reduction and therefore their memo identity.
    fn relation_sub_key(&self, source: SemanticNodeId, target: SemanticNodeId) -> RelateMemoKey {
        let txn = self.dispatch_txn.borrow();
        match txn.reentry().nearest_relate() {
            Some((top, _)) => {
                let inference_context = top.inference_context.as_ref().and_then(|context| {
                    (context.pass_kind == InferencePassKind::ReverseHomomorphicMapped)
                        .then(|| context.clone())
                });
                RelateMemoKey {
                    source,
                    target,
                    relation: top.relation,
                    policy: top.policy,
                    source_freshness: top.source_freshness,
                    inference_context,
                    context: top.context,
                }
            }
            None => self.relate_key_for(source, target),
        }
    }

    /// The reducer's sub-relation step (the 8 recursion sites of the
    /// retired hidden path): an in-scope `Infer` occupant binds through
    /// the active session; every other sub-judgement re-enters the SAME
    /// full-key authority ([`Self::execute_relate`]) and folds onto the
    /// reducer's lattice.
    pub(super) fn relate_member(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_member_with_freshness(source, target, bindings, position, None, false)
    }

    /// Relate an ordinary union arm after the enclosing fresh-source frame
    /// has completed its one excess-property prepass. Freshness is consumed
    /// by that enclosing check; carrying it into each arm would rerun a
    /// branch-local excess check and reject names known by sibling arms.
    fn relate_union_arm_after_excess_prepass(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
    ) -> RelationResult {
        self.relate_member_with_freshness(
            source,
            target,
            bindings,
            position,
            Some(crate::semantic_query::FreshnessKey::Regular),
            false,
        )
    }

    /// Relate one arm of an intersection target on its own, as the checker
    /// relates it under `IntersectionState.Target`: the whole intersection
    /// already passed the weak-type check, so the arm skips it.
    fn relate_intersection_target_arm(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        self.relate_member_with_freshness(
            source,
            target,
            bindings,
            InferPosition::Covariant,
            None,
            true,
        )
    }

    fn relate_member_with_freshness(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        position: InferPosition,
        source_freshness: Option<crate::semantic_query::FreshnessKey>,
        intersection_target_arm: bool,
    ) -> RelationResult {
        let occurrence = self.relation_occurrence(position);
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            return result;
        }
        let graph = self.graph();
        if self.relation_session_active() {
            match occurrence.variance {
                VariancePhase::Covariant | VariancePhase::Invariant => {
                    if matches!(
                        graph.node_data(target).as_deref(),
                        Some(SemanticNodeData::Infer { .. } | SemanticNodeData::TypeParam { .. })
                    ) {
                        if !self.relation_deposit(target, source, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
                VariancePhase::Contravariant => {
                    if matches!(
                        graph.node_data(source).as_deref(),
                        Some(SemanticNodeData::Infer { .. } | SemanticNodeData::TypeParam { .. })
                    ) {
                        if !self.relation_deposit(source, target, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
            }
        }
        // The discharge substitution rail (re-discharge, design §2.3 step
        // 4): a member of a negatively-closed SCC re-runs against the
        // converged verdicts.
        let mut key = self.relation_sub_key(source, target);
        if let Some(source_freshness) = source_freshness {
            key.source_freshness = source_freshness;
        }
        // Only an intersection target's arm itself skips the weak-type
        // check; a relation it opens, a property's, never does.
        key.policy.intersection_target_arm = intersection_target_arm;
        {
            let txn = self.dispatch_txn.borrow();
            if !txn.obligations.substitution().is_empty() {
                if let Some(step) =
                    provisional_relate_step(txn.obligations.substitution(), &key, occurrence)
                {
                    return match step {
                        RelationStep::Assignable { bindings: sub } => {
                            for binding in sub.iter() {
                                if !bindings
                                    .iter()
                                    .any(|existing| existing.param == binding.param)
                                {
                                    bindings.push(binding.clone());
                                }
                            }
                            assignable(bindings)
                        }
                        RelationStep::NotAssignable => RelationResult::NotAssignable,
                        _ => RelationResult::Unknown,
                    };
                }
            }
        }
        match self.execute_relate_with_occurrence(key, occurrence) {
            RelationStep::Assumed(_) => {
                // The coinductive hypothesis: assumed to hold; the edge is
                // recorded on the frame.
                assignable(bindings)
            }
            RelationStep::Assignable { bindings: sub } => {
                for binding in sub.iter() {
                    if !bindings.iter().any(|b| b.param == binding.param) {
                        bindings.push(binding.clone());
                    }
                }
                assignable(bindings)
            }
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown => RelationResult::Unknown,
            RelationStep::BudgetExceeded(cap) => {
                let mut txn = self.dispatch_txn.borrow_mut();
                if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                    txn.reentry_mut().note_budget_edge(depth, cap);
                }
                RelationResult::Unknown
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The nominal axis: `unique symbol` declaring identity
    // ──────────────────────────────────────────────────────────────────

    /// The NOMINAL identity a node denotes, or `None` when the node carries
    /// no nominal identity of its own (a structural or tag-level subject the
    /// structural half of the relation decides).
    ///
    /// The only nominal type TypeScript has is `unique symbol`: two
    /// `unique symbol` declarations denote DIFFERENT types even though both
    /// widen to the same `symbol` primitive, and one declaration denotes ONE
    /// type however many aliases, imports, or re-exports a reference
    /// travelled through. The declaring
    /// [`verter_type_expr::facts::ValueDeclIdentityPart`] IS that identity.
    /// It is minted from the existing unique-symbol lookup and carried on
    /// the `TypeOf` node, so relation reads are O(1): there is no second
    /// unique-symbol identity type or resolution path.
    pub(crate) fn relation_nominal_identity(
        &self,
        node: SemanticNodeId,
    ) -> Option<verter_type_expr::facts::ValueDeclIdentityPart> {
        self.graph()
            .node_data(node)?
            .typeof_nominal_identity()
            .cloned()
    }

    /// The widened (non-nominal) type a nominal subject inhabits — the bare
    /// `symbol` primitive a `unique symbol` declaration widens to.
    ///
    /// Used for a nominal source in assignability, and symmetrically for
    /// comparability. A non-nominal source is never accepted by widening a
    /// nominal assignability target.
    ///
    /// The primitive is INTERNED, never re-queried: the `typeof` key now
    /// answers a `unique symbol` root with the nominal carrier itself (the
    /// carrier IS the type), so re-asking it here would return the carrier
    /// again. The widened inhabitant of a `unique symbol` is by definition
    /// the `symbol` primitive — the exact node the annotation lowers to —
    /// and structural interning mints or finds that one node, with no
    /// second resolution pass.
    fn symbol_primitive(&self) -> SemanticNodeId {
        self.graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Symbol))
    }

    /// Whether a node is a nominal (`unique symbol`) `typeof` carrier —
    /// [`Self::relation_nominal_identity`] without the identity clone, for
    /// the tag-test sites that only need the boolean.
    fn node_is_nominal_typeof(&self, node: SemanticNodeId) -> bool {
        self.graph()
            .node_data(node)
            .is_some_and(|data| data.typeof_nominal_identity().is_some())
    }

    /// The nominal leaf of the structural lattice, asked once per pair
    /// BEFORE the deferred gate would swallow a preserved `typeof` carrier.
    ///
    /// * both sides nominal — the DECLARING identities decide: equal is one
    ///   type, distinct are two different types.
    /// * exactly one side nominal - comparability widens either side;
    ///   assignability widens only a nominal source.
    /// * neither side nominal — `None`: this leaf has nothing to say and the
    ///   caller continues (a `typeof` carrier over a NON-unique value is an
    ///   ordinary deferred shell, decided by the gate below it).
    ///
    /// The widen step is DIRECTION-SYMMETRIC in what it declines. Widening a
    /// nominal side against a composite would erase the declaring identity
    /// every distributed arm still needs, and for a symmetric question like
    /// comparability that makes the answer depend on operand order:
    /// `Comparable(typeof A | typeof B, typeof C)` would widen the target to
    /// `symbol` and report an overlap, while the same pair asked the other
    /// way round distributes and proves disjointness. Both directions
    /// therefore decline and let the composite distribute first.
    fn relation_nominal_leaf(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        relation: RelationKind,
        bindings: &[InferBinding],
    ) -> Option<NominalLeaf> {
        let source_nominal = self.relation_nominal_identity(source);
        let target_nominal = self.relation_nominal_identity(target);
        // Frames the leaf must NOT pre-empt, on WHICHEVER side they sit: a
        // composite still has to distribute (widening first erases the
        // declaring identity every distributed arm needs), and an
        // assignability `Infer` position still has to receive the CARRIER as
        // its inference candidate — depositing the widened `symbol` would
        // lose the identity for every later ask on the bound parameter.
        // Comparability runs no inference, so `Infer` is not its deferral.
        let declines_widening = |node: SemanticNodeId| {
            matches!(
                self.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_))
            ) || (relation != RelationKind::Comparable
                && matches!(
                    self.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Infer { .. })
                ))
        };
        match (source_nominal, target_nominal) {
            (Some(a), Some(b)) => Some(NominalLeaf::Decided(if a == b {
                assignable(bindings)
            } else {
                RelationResult::NotAssignable
            })),
            (Some(_), None) if declines_widening(target) => None,
            (Some(_), None) => Some(NominalLeaf::Retry(self.symbol_primitive(), target)),
            (None, Some(_)) if declines_widening(source) => None,
            (None, Some(_)) if relation == RelationKind::Comparable => {
                Some(NominalLeaf::Retry(source, self.symbol_primitive()))
            }
            (None, Some(_)) => Some(NominalLeaf::Decided(
                match self.graph().node_data(source).as_deref() {
                    Some(SemanticNodeData::Primitive(
                        PrimitiveKind::Any | PrimitiveKind::Never,
                    )) => assignable(bindings),
                    None | Some(SemanticNodeData::Opaque(_)) => RelationResult::Unknown,
                    Some(data) if is_deferred(data) => RelationResult::Unknown,
                    Some(_) => RelationResult::NotAssignable,
                },
            )),
            (None, None) => None,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // `RelationKind::Identity` — bounded to the nominal axis
    // ──────────────────────────────────────────────────────────────────

    /// Reduce a [`RelationKind::Identity`] judgement.
    ///
    /// BOUNDED: this authority owns the NOMINAL half of type identity —
    /// whether two subjects denote the same `unique symbol` declaration.
    /// Scope-insensitive STRUCTURAL constituent identity (the exhaustive
    /// comparator the canonical union / intersection algebra needs) is a
    /// DIFFERENT authority; answering it here would be two engines for one
    /// question, so a non-nominal subject stays undecided — `ReturnOnly`,
    /// never warm — rather than borrowing the assignability lattice.
    fn reduce_identity(&self, key: &RelateMemoKey) -> RelationResult {
        if !self.relation_reads_node_pair_only(key) {
            return RelationResult::Unknown;
        }
        let source = match self.unwrap_identity_carrier_for_relation(key.source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        let target = match self.unwrap_identity_carrier_for_relation(key.target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        match (
            self.relation_nominal_identity(source),
            self.relation_nominal_identity(target),
        ) {
            (Some(a), Some(b)) => {
                if a == b {
                    assignable(&[])
                } else {
                    RelationResult::NotAssignable
                }
            }
            // A nominal subject against anything else is left to the
            // structural-identity authority: `typeof K` versus `symbol` is a
            // STRUCTURAL question this bounded reduction does not answer.
            _ => RelationResult::Unknown,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // `RelationKind::Comparable` — the overlap oracle
    // ──────────────────────────────────────────────────────────────────

    /// Reduce a [`RelationKind::Comparable`] judgement — "can these two
    /// types have a common inhabitant?".
    ///
    /// Three outcomes, and the NEGATIVE one is the load-bearing fact:
    ///
    /// * `NotAssignable` — a PROOF of empty overlap. A consumer may act on
    ///   it (a narrowing consumer treats it as its disjointness proof).
    ///   Whether a provably disjoint INTERSECTION collapses to `never` is the
    ///   canonical type algebra's decision, not this relation's: supplying
    ///   the proof and reducing a type are two different jobs.
    /// * `Assignable` — the oracle found NO proof of empty overlap, so the
    ///   two are treated as comparable. This is the permissive arm, exactly
    ///   as the checker's comparability relation is permissive.
    /// * `Unknown` — a subject the oracle cannot answer on: an opaque node,
    ///   an unsubstituted `infer` reference, a bare / imported / declaration
    ///   reference that survived the identity unwrap (a name that did not
    ///   resolve), OR an operator the reduction did not itself expand — an
    ///   unreduced `typeof` / `keyof` / indexed access / mapped /
    ///   conditional / raw-fallback operand. Missing knowledge is never
    ///   converted into a positive fact: the permissive arm PROMISES "no
    ///   proof of empty overlap was found", and an operand whose content
    ///   was never read cannot back that promise. `ReturnOnly`; the
    ///   consumer records its typed gap and the enclosing batch stays cold
    ///   until the operand is resolvable.
    ///
    /// The proof itself is DELIBERATELY conservative and stays inside this
    /// one authority: concrete tag conflicts (delegated to the crate's sole
    /// proven-disjoint tag oracle, so this cannot drift from the canonical
    /// intersection collapse), the nominal axis above, and two structural
    /// surfaces carrying the same REQUIRED member with disjoint values —
    /// where a COMPOSED root (an intersection body, an object-spread
    /// program) is composed into its one-level surface first, so an
    /// `A & { kind: "a" }` versus `A & { kind: "b" }` conflict is still
    /// proved. Two surfaces are also disjoint when each requires a property
    /// the other proves absent — the checker's comparable relation refuses
    /// an unmatched required property in either direction — once their
    /// shared members are read; any other difference in key sets overlaps.
    /// Two arrays relate by their elements.
    fn reduce_comparable(&self, key: &RelateMemoKey) -> RelationResult {
        if !self.relation_reads_node_pair_only(key) {
            return RelationResult::Unknown;
        }
        self.comparable_worklist(key.source, key.target)
    }

    /// Whether a key's non-pair axes are at the values these two reductions
    /// were written for.
    ///
    /// [`Self::reduce_identity`] and [`Self::reduce_comparable`] read ONLY
    /// `source` and `target`. The substitution, inference-context, and
    /// freshness axes are part of the memo slot's identity, so a key that
    /// varies one of them would be answered by a value that never looked at
    /// it — a decision published under a substitution it ignored. Refusing
    /// (undecided, `ReturnOnly`, zero admission) is the same axis-refusal
    /// discipline the reducer applies to an unimplemented relation kind.
    fn relation_reads_node_pair_only(&self, key: &RelateMemoKey) -> bool {
        key.inference_context.is_none()
            && key.source_freshness == crate::semantic_query::FreshnessKey::Regular
            && key.context.substitution == crate::semantic_query::SubstitutionCanonicalHash::empty()
    }

    /// The one-level member surface the disjointness proof descends, or why
    /// there is none.
    ///
    /// A terminal `Object` node already IS its surface. A COMPOSED root —
    /// an intersection body, an object-spread program — is not: its members
    /// exist only once the shared empty-path `Shallow` synthesiser merges
    /// its arms. Reading only the terminal tag would answer every
    /// `type A = Base & { kind: "a" }` pair permissively and lose the
    /// conflicting-discriminant proof that the whole oracle exists to
    /// supply, so a composed root is composed HERE through the single
    /// shared surface reader (which owns the own-body-shadows-heritage
    /// merge, the declaration-placeholder unwrap, and the cross-file
    /// carrier resolution) before the member descent runs. Alias and
    /// merged-declaration roots never reach this point: the relation's
    /// identity unwrap already flattened them.
    ///
    /// A composition that does not yield a surface (a partial or errored
    /// projection) is `NonObject`, i.e. permissive — the same answer the
    /// pair had before any composition was attempted, never a proof minted
    /// from a surface the oracle could not read.
    fn comparable_surface(&self, node: SemanticNodeId) -> ComparableSurface {
        let composed = match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Object(view)) => return ComparableSurface::Object(view.clone()),
            None | Some(SemanticNodeData::Opaque(_)) => {
                return ComparableSurface::Unresolvable;
            }
            Some(SemanticNodeData::Intersection(_) | SemanticNodeData::ObjectSpreadProgram(_)) => {
                true
            }
            Some(_) => false,
        };
        if !composed {
            return ComparableSurface::NonObject;
        }
        match self
            .resolve_typeinfo_surface_view(node, ProjectionReductionContext::structural_transit())
        {
            Some(view) => ComparableSurface::Object(view),
            None => ComparableSurface::NonObject,
        }
    }

    fn relation_budget_limit(&self) -> u64 {
        (self.graph().node_count() as u64)
            .saturating_mul(10)
            .max(4096)
    }

    fn note_relation_budget_exceeded(&self, budget_limit: u64) {
        let cap = RecursionOrBudgetCap {
            kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
            limit: budget_limit.min(u64::from(u32::MAX)) as u32,
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        if let Some(depth) = txn.reentry().depth().checked_sub(1) {
            txn.reentry_mut().note_budget_edge(depth, cap);
        }
    }

    fn comparable_worklist(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelationResult {
        enum Work {
            Eval(SemanticNodeId, SemanticNodeId),
            /// Disjoin the results above `base` (`source`-union alternatives).
            CombineAnyFrom(usize),
            /// Conjoin the results above `base` (shared required members).
            CombineAllFrom(usize),
            /// Conjoin the results above `base` (the shared members of a
            /// pair its property sets prove disjoint) and publish the
            /// disjointness proof unless one is undecided: the proof's
            /// collapse class reads those members.
            DisjointUnlessUndecidedFrom(usize),
            Finish((SemanticNodeId, SemanticNodeId)),
        }

        // Pair descent, union alternatives, and member enumeration share one
        // iterative envelope; descendants never open a fresh relation budget.
        let graph = self.graph();
        let budget_limit = if self
            .ctx
            .host_for_fact_tracer_install()
            .relation_knobs
            .force_budget_exhaustion
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            0
        } else {
            self.relation_budget_limit()
        };
        // The pair is canonicalized (lesser node first) once, before any
        // descent: comparability is answer-symmetric, so the frame-local
        // memo and the shared memo key (canonicalized at the
        // `RelateMemoKey` constructor) agree on one entry per unordered
        // pair — a reversed ask reuses the frame's own work instead of
        // retaining a second, redundant entry.
        //
        // An operand whose identity unwrap cannot resolve is UNREAD — the
        // same doctrine the worklist body applies: falling back to the raw
        // pair would let the fast path decide `Overlaps` (or a tag verdict)
        // for a carrier it never read, while the slow path reports
        // `Unknown` — a verdict that flips with the budget knob. No fact:
        // `Unknown`, before any fast-path arm runs.
        let (mut source, mut target) = match (|| {
            let source = match self.unwrap_identity_carrier_for_relation(source) {
                IdentityCarrierUnwrap::Concrete(id) => id,
                IdentityCarrierUnwrap::Unresolvable => return None,
            };
            let target = match self.unwrap_identity_carrier_for_relation(target) {
                IdentityCarrierUnwrap::Concrete(id) => id,
                IdentityCarrierUnwrap::Unresolvable => return None,
            };
            Some(if source > target {
                (target, source)
            } else {
                (source, target)
            })
        })() {
            Some(pair) => pair,
            None => return RelationResult::Unknown,
        };

        // ALLOCATION-FREE FAST PATH — runs before the work/results vectors
        // and the active/memo tables exist. These are the operand shapes
        // every narrowing consumer asks about on its hot edge (a nominal
        // carrier against a literal or another carrier, the same node
        // twice, a tag-level mismatch), and each is decided from at most
        // two node-data borrows with no allocation and no budget charge
        // (each is O(1) graph reads, the floor the budget exists to
        // bound). A nominal-leaf retry widens one side and re-runs the
        // fast path exactly once before any slow-path allocation. The
        // forced-exhaustion knob bypasses the fast path entirely so a
        // tripped budget is still reported as the TYPED cap outcome, never
        // silently decided.
        if budget_limit > 0 {
            // bounded-loop: at most one nominal widen retry — O(1) graph reads, no allocation.
            for _ in 0..2 {
                if let Some(leaf) =
                    self.relation_nominal_leaf(source, target, RelationKind::Comparable, &[])
                {
                    match leaf {
                        NominalLeaf::Decided(result) => return result,
                        NominalLeaf::Retry(widened_source, widened_target) => {
                            (source, target) = (widened_source, widened_target);
                            continue;
                        }
                    }
                }
                break;
            }
            // Comparability is symmetric: a top type on either side decides.
            if self.relation_reads_unread_marker(source, target)
                && self.relation_reads_unread_marker(target, source)
            {
                return RelationResult::Unknown;
            }
            if source == target {
                return assignable(&[]);
            }
            if let (Some(source_data), Some(target_data)) =
                (graph.node_data(source), graph.node_data(target))
            {
                if !matches!(&*source_data, SemanticNodeData::Union(_))
                    && !matches!(&*target_data, SemanticNodeData::Union(_))
                    && (super::canonical_algebra::tag_level_disjoint(graph, source, target)
                        || comparable_root_kinds_disjoint(&source_data, &target_data))
                {
                    return RelationResult::NotAssignable;
                }
            }
        }

        let mut budget_used = 0u64;
        let mut work = vec![Work::Eval(source, target)];
        let mut results = Vec::new();
        let mut active = FxHashSet::default();
        let mut memo: FxHashMap<(SemanticNodeId, SemanticNodeId), RelationResult> =
            FxHashMap::default();

        while let Some(item) = work.pop() {
            match item {
                Work::CombineAnyFrom(base) => {
                    let combined = results
                        .drain(base..)
                        .fold(RelationResult::NotAssignable, result_or);
                    results.push(combined);
                }
                Work::CombineAllFrom(base) => {
                    let combined = results.drain(base..).fold(assignable(&[]), result_and);
                    results.push(combined);
                }
                Work::DisjointUnlessUndecidedFrom(base) => {
                    let combined = results.drain(base..).fold(assignable(&[]), result_and);
                    results.push(match combined {
                        RelationResult::Unknown => RelationResult::Unknown,
                        _ => RelationResult::NotAssignable,
                    });
                }
                Work::Finish(pair) => {
                    let result = results
                        .pop()
                        .expect("a comparable pair must publish one result");
                    active.remove(&pair);
                    memo.insert(pair, result.clone());
                    results.push(result);
                }
                Work::Eval(source, target) => {
                    budget_used = budget_used.saturating_add(1);
                    if budget_used > budget_limit {
                        self.note_relation_budget_exceeded(budget_limit);
                        return RelationResult::Unknown;
                    }
                    let source = match self.unwrap_identity_carrier_for_relation(source) {
                        IdentityCarrierUnwrap::Concrete(id) => id,
                        IdentityCarrierUnwrap::Unresolvable => {
                            results.push(RelationResult::Unknown);
                            continue;
                        }
                    };
                    let target = match self.unwrap_identity_carrier_for_relation(target) {
                        IdentityCarrierUnwrap::Concrete(id) => id,
                        IdentityCarrierUnwrap::Unresolvable => {
                            results.push(RelationResult::Unknown);
                            continue;
                        }
                    };
                    // Comparability is answer-symmetric: canonicalizing the
                    // pair makes the frame-local memo and the coinductive
                    // `active` set agree for both operand orders, so a
                    // re-entered reversed pair hits the assumption rail
                    // instead of duplicating the descent.
                    let pair = if source > target {
                        (target, source)
                    } else {
                        (source, target)
                    };
                    let (source, target) = pair;
                    if let Some(result) = memo.get(&pair) {
                        results.push(result.clone());
                        continue;
                    }
                    if !active.insert(pair) {
                        // Coinductive ASSUMPTION: a pair re-entered while it
                        // is still being decided is assumed to overlap, so a
                        // cyclic type terminates on the permissive arm rather
                        // than fabricating a disjointness proof. Both folds
                        // propagate permissiveness (`result_or` for union
                        // alternatives, `result_and` for member conjunction),
                        // so the assumption can only MISS a proof, never mint
                        // one. A result finished under it is recorded in the
                        // frame-local memo like any other, which is sound for
                        // the same reason: reusing it can only carry the
                        // permissive answer forward, never a proof.
                        results.push(assignable(&[]));
                        continue;
                    }
                    work.push(Work::Finish(pair));

                    if let Some(leaf) =
                        self.relation_nominal_leaf(source, target, RelationKind::Comparable, &[])
                    {
                        match leaf {
                            NominalLeaf::Decided(result) => results.push(result),
                            NominalLeaf::Retry(source, target) => {
                                work.push(Work::Eval(source, target));
                            }
                        }
                        continue;
                    }
                    if self.relation_reads_unread_marker(source, target)
                        && self.relation_reads_unread_marker(target, source)
                    {
                        results.push(RelationResult::Unknown);
                        continue;
                    }
                    if source == target {
                        results.push(assignable(&[]));
                        continue;
                    }
                    let (Some(source_data), Some(target_data)) =
                        (graph.node_data(source), graph.node_data(target))
                    else {
                        results.push(RelationResult::Unknown);
                        continue;
                    };

                    if let SemanticNodeData::Union(members) = &*source_data {
                        let members = members.members_arc();
                        work.push(Work::CombineAnyFrom(results.len()));
                        for member in members.iter() {
                            work.push(Work::Eval(*member, target));
                        }
                        continue;
                    }
                    if let SemanticNodeData::Union(members) = &*target_data {
                        let members = members.members_arc();
                        work.push(Work::CombineAnyFrom(results.len()));
                        for member in members.iter() {
                            work.push(Work::Eval(source, *member));
                        }
                        continue;
                    }
                    if super::canonical_algebra::tag_level_disjoint(graph, source, target)
                        || comparable_root_kinds_disjoint(&source_data, &target_data)
                    {
                        results.push(RelationResult::NotAssignable);
                        continue;
                    }

                    // A subject the oracle CANNOT obtain, as opposed to one
                    // it merely did not reduce. An opaque node carries no
                    // type; an unsubstituted infer REFERENCE denotes nothing
                    // yet; and a bare / imported / declaration REFERENCE
                    // that survived the identity unwrap above is a name that
                    // did not resolve — a missing dependency, not a shape.
                    // For these the oracle never read the subject at all, so
                    // it reports no fact: undecided, `ReturnOnly`, and the
                    // consumer records its typed gap.
                    //
                    // A bare `Infer` / `TypeParam` operand is deliberately
                    // NOT in this list: a binder's inhabitant EXISTS by
                    // binding — the oracle is not missing knowledge about
                    // it, it merely cannot bound it — so the pair takes the
                    // permissive `Overlaps` arm rather than a typed gap.
                    // Asymmetric with the doctrine above by design; revisit
                    // only if a consumer ever needs a no-fact verdict from
                    // an unbound parameter pair.
                    let unresolvable = |data: &SemanticNodeData| {
                        matches!(
                            data,
                            SemanticNodeData::Opaque(_)
                                | SemanticNodeData::InferRef { .. }
                                | SemanticNodeData::BareRef(_)
                                | SemanticNodeData::ImportType(_)
                                | SemanticNodeData::DeclRef { .. }
                                | SemanticNodeData::InstantiationRef { .. }
                        )
                    };
                    if unresolvable(&source_data) || unresolvable(&target_data) {
                        results.push(RelationResult::Unknown);
                        continue;
                    }
                    // A DEFERRED operator or carrier is UNREAD. The
                    // permissive arm is a PROMISE — "no proof of empty
                    // overlap exists" — and an operand whose content was
                    // never read cannot back it: answering `Overlaps`
                    // would convert missing knowledge into a positive,
                    // memo-admissible fact (a consumer could warm-serve a
                    // completeness the oracle never had). The oracle
                    // reports no fact instead: undecided, `ReturnOnly`,
                    // zero admission, and the consumer records its typed
                    // gap until the operand resolves.
                    let unreduced_operator = |data: &SemanticNodeData| {
                        matches!(
                            data,
                            SemanticNodeData::TypeOf(_)
                                | SemanticNodeData::KeyOf { .. }
                                | SemanticNodeData::IndexedAccess { .. }
                                | SemanticNodeData::Mapped { .. }
                                | SemanticNodeData::Conditional { .. }
                                | SemanticNodeData::RawFallback { .. }
                        )
                    };
                    if unreduced_operator(&source_data) || unreduced_operator(&target_data) {
                        results.push(RelationResult::Unknown);
                        continue;
                    }
                    // Two arrays are comparable exactly when their elements
                    // are, in either direction and whichever is readonly
                    // (the mutable one relates to the readonly one): the
                    // checker relates `Array<T>` by its element's variance
                    // (measured: `x: string[] | number[]` reads `number[]`
                    // inside `if (x === arr)` over `arr: number[]`).
                    if let (
                        SemanticNodeData::Array {
                            element: source_element,
                            ..
                        },
                        SemanticNodeData::Array {
                            element: target_element,
                            ..
                        },
                    ) = (&*source_data, &*target_data)
                    {
                        work.push(Work::Eval(*source_element, *target_element));
                        continue;
                    }

                    let (source_view, target_view) = match (
                        self.comparable_surface(source),
                        self.comparable_surface(target),
                    ) {
                        (ComparableSurface::Object(source), ComparableSurface::Object(target)) => {
                            (source, target)
                        }
                        (ComparableSurface::Unresolvable, _)
                        | (_, ComparableSurface::Unresolvable) => {
                            results.push(RelationResult::Unknown);
                            continue;
                        }
                        _ => {
                            results.push(assignable(&[]));
                            continue;
                        }
                    };
                    // The member descent charges the work it actually
                    // performs: one unit per source member step PLUS the
                    // target-surface width for each key projection (the
                    // projection scans the target's members for the
                    // element-access collision). A flat one-unit charge per
                    // member would let a single descent perform
                    // O(source-width × target-width) comparisons inside a
                    // linear budget; charging the scan keeps the budget an
                    // honest bound on comparisons performed.
                    let target_width = target_view.positive_members().len() as u64;
                    // A member both sides declare is compared as the
                    // checker's comparable relation reads it: each side's
                    // type plus `undefined` where that side is optional
                    // under `strictNullChecks` (measured on the pinned
                    // checker: `{ v?: 'a' }` narrowed by `x is { v: 'b' }`
                    // is `never` with the option on and off, while `{ v?:
                    // 'a' }` against `{ v?: 'b' }` overlaps on `undefined`).
                    let strict_null_checks = self
                        .dispatch_txn
                        .borrow()
                        .relation
                        .strict
                        .unwrap_or(StrictFamilyConfig::TS_STRICT)
                        .strict_null_checks;
                    let read = |value: SemanticNodeId, optional: bool| {
                        if !(optional && strict_null_checks) {
                            return value;
                        }
                        let undefined = graph
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                        self.intern_normalized_union_or_intersection(&[value, undefined], true)
                    };
                    budget_used = budget_used.saturating_add(
                        (source_view.positive_members().len() as u64)
                            .saturating_mul(target_width)
                            .saturating_mul(2),
                    );
                    if budget_used > budget_limit {
                        self.note_relation_budget_exceeded(budget_limit);
                        return RelationResult::Unknown;
                    }
                    work.push(
                        if surfaces_require_members_the_other_lacks(&source_view, &target_view) {
                            Work::DisjointUnlessUndecidedFrom(results.len())
                        } else {
                            Work::CombineAllFrom(results.len())
                        },
                    );
                    for source_member in source_view.positive_members() {
                        budget_used = budget_used.saturating_add(1);
                        if budget_used > budget_limit {
                            self.note_relation_budget_exceeded(budget_limit);
                            return RelationResult::Unknown;
                        }
                        let Some(member_key) = source_member.key.cloned_known() else {
                            continue;
                        };
                        budget_used = budget_used.saturating_add(target_width);
                        if budget_used > budget_limit {
                            self.note_relation_budget_exceeded(budget_limit);
                            return RelationResult::Unknown;
                        }
                        let crate::semantic_query::SurfaceKeyProjection::Exact(target_member) =
                            target_view.project_known_key(&member_key)
                        else {
                            continue;
                        };
                        // The property pair's accessibility, as the
                        // checker's comparable relation reads it through
                        // `propertyRelatedTo` in either direction: a pair
                        // neither direction admits proves the types
                        // disjoint (TS2367 / TS2352 between `D0` and a
                        // `Q0` redeclaring its private member).
                        if let Some(result) =
                            self.comparable_property_accessibility(source_member, target_member)
                        {
                            results.push(result);
                            continue;
                        }
                        work.push(Work::Eval(
                            read(source_member.value, source_member.optional),
                            read(target_member.value, target_member.optional),
                        ));
                    }
                }
            }
        }

        results.pop().unwrap_or(RelationResult::Unknown)
    }

    // The reducer: prefilter → carrier unwrap → structural worklist
    // ──────────────────────────────────────────────────────────────────

    /// Run one frame's reduction: the O(tag) prefilter first (RI-5 — never
    /// a parallel truth source), then the dispatch-aware structural
    /// judgement. `bindings` accumulates sub-relation bindings onto the
    /// caller's lattice.
    fn reduce_relation(
        &self,
        key: &RelateMemoKey,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // Axis refusal: a key on a not-yet-implemented axis (`Subtype` /
        // `StrictSubtype`, a non-default overload-selection policy) must
        // REFUSE — undecided, ReturnOnly, zero admission — never route the
        // ask through a NEIGHBOURING relation's lattice (a `Subtype` ask
        // answered by the `(_, unknown) => Assignable` assignability arm
        // would publish a false verdict). Both strict variance regimes ARE
        // implemented (RI-10).
        if key.policy.overload_selection != crate::semantic_query::OverloadSelectionPolicy::All {
            return RelationResult::Unknown;
        }
        // Test-only forced budget knob (D6): trips the work budget on the
        // first driver pass so the typed `BudgetExceeded` outcome and its
        // three-layer non-admission are exercised deterministically.
        let host = self.ctx.host_for_fact_tracer_install();
        if key.relation != RelationKind::Comparable
            && host
                .relation_knobs
                .force_budget_exhaustion
                .load(std::sync::atomic::Ordering::Relaxed)
        {
            let cap = RecursionOrBudgetCap {
                kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
                limit: 0,
            };
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                txn.reentry_mut().note_budget_edge(depth, cap);
            }
            return RelationResult::Unknown;
        }
        match key.relation {
            RelationKind::Assignable => {}
            RelationKind::Identity => return self.reduce_identity(key),
            RelationKind::Comparable => return self.reduce_comparable(key),
            RelationKind::Subtype | RelationKind::StrictSubtype => {}
        }
        let occurrence = self.relation_current_occurrence();
        if let Some(result) =
            self.try_relation_projection(key.source, key.target, bindings, occurrence)
        {
            return result;
        }
        let reverse_spec = self
            .dispatch_txn
            .borrow()
            .active_session()
            .and_then(InferenceSession::reverse_spec)
            .cloned()
            .filter(|spec| spec.mapped_node == key.target);
        if let Some(spec) = reverse_spec {
            return self.relate_reverse_homomorphic(key.source, &spec, bindings);
        }
        match self.reduce_relation_conditional(key.source) {
            Some(Some(reduced)) => {
                return self.relate_member(reduced, key.target, bindings, InferPosition::Covariant);
            }
            Some(None) => return RelationResult::Unknown,
            None => {}
        }
        match self.reduce_relation_conditional(key.target) {
            Some(Some(reduced)) => {
                return self.relate_member(key.source, reduced, bindings, InferPosition::Covariant);
            }
            Some(None) => return RelationResult::Unknown,
            None => {}
        }
        // The binding root's bare-`Infer` arm: `check extends infer X`
        // binds `X := check` for any check through the active session.
        if self.relation_session_active() {
            match occurrence.variance {
                VariancePhase::Covariant | VariancePhase::Invariant => {
                    if let Some(SemanticNodeData::Infer { .. }) =
                        self.graph().node_data(key.target).as_deref()
                    {
                        if !self.relation_deposit(key.target, key.source, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
                VariancePhase::Contravariant => {
                    if let Some(SemanticNodeData::Infer { .. }) =
                        self.graph().node_data(key.source).as_deref()
                    {
                        if !self.relation_deposit(key.source, key.target, occurrence) {
                            return RelationResult::Unknown;
                        }
                        return assignable(bindings);
                    }
                }
            }
        }
        // The fresh excess-property prepass (once per frame, BEFORE ordinary
        // union-arm distribution): gate = Fresh source + excess policy; a
        // rejection decides the frame, an undecidable check stays Unknown
        // (never collapsed), a pass continues into the ordinary relation.
        if key.source_freshness == crate::semantic_query::FreshnessKey::Fresh
            && key.policy.excess_property_check
        {
            match self.excess_property_prepass(key, bindings) {
                super::relation_excess::ExcessPrepassOutcome::Reject => {
                    return RelationResult::NotAssignable;
                }
                super::relation_excess::ExcessPrepassOutcome::Undecided => {
                    return RelationResult::Unknown;
                }
                super::relation_excess::ExcessPrepassOutcome::Pass => {}
            }
        }
        // Object-spread programs are formulas, not ordinary graph-node
        // surfaces. Relate them before the identity fast path and before any
        // legacy Object handling so an unresolved program is never accepted
        // merely because both sides carry the same node id.
        if let Some(result) =
            self.try_object_spread_program_relation(key.source, key.target, bindings)
        {
            return result;
        }
        // Two applications of one generic declaration whose every type
        // parameter carries a variance annotation relate by their arguments
        // under those annotations, before any structural comparison
        // (`structuredTypeRelatedTo`'s reference variance check, where
        // `getVariances` reads an annotation instead of measuring).
        if let Some(pairs) = self.annotated_variance_argument_pairs(key.source, key.target) {
            let mut acc = assignable(bindings);
            for (source, target) in pairs {
                let result = self.relate_member(source, target, bindings, InferPosition::Covariant);
                acc = result_and(acc, result);
                if matches!(acc, RelationResult::NotAssignable) {
                    return RelationResult::NotAssignable;
                }
            }
            return acc;
        }
        // A source with no inferable index — a declared interface or class
        // instance among them — takes an index signature only through an
        // index signature of its own.
        if self.implicit_index_rejects(key.source, key.target) {
            return RelationResult::NotAssignable;
        }
        match self.shallow_relation_check(key.source, key.target) {
            ShallowRelation::Assignable => return assignable(bindings),
            ShallowRelation::NotAssignable => return RelationResult::NotAssignable,
            ShallowRelation::Unknown => {}
        }
        self.decide_relation_with_dispatch(
            key.source,
            key.target,
            bindings,
            key.policy.intersection_target_arm,
        )
    }

    /// The `(source, target)` argument pairs two applications of ONE
    /// generic declaration relate by when every one of its type parameters
    /// carries a variance annotation: an `out` argument pair as written, an
    /// `in` pair reversed, an `in out` pair both ways. `None` for any other
    /// pair — different declarations, an unannotated parameter (whose
    /// variance the checker measures; the structural comparison answers
    /// it), or a declaration whose header is not read.
    fn annotated_variance_argument_pairs(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<Vec<(SemanticNodeId, SemanticNodeId)>> {
        use verter_type_expr::facts::TypeParamVariance;
        let graph = self.graph();
        let source_data = graph.node_data(source)?;
        let target_data = graph.node_data(target)?;
        let (
            SemanticNodeData::InstantiationRef {
                base: source_base,
                args: source_args,
            },
            SemanticNodeData::InstantiationRef {
                base: target_base,
                args: target_args,
            },
        ) = (&*source_data, &*target_data)
        else {
            return None;
        };
        if source_base.canonical_id != target_base.canonical_id
            || source_base.owner != target_base.owner
            || source_base.decl_name != target_base.decl_name
            || source_args.len() != target_args.len()
        {
            return None;
        }
        let prepared = self.ctx.prepared_type_decl_return_only(
            source_base.canonical_id.as_ref(),
            source_base.owner,
            source_base.decl_name.as_ref(),
        )?;
        if prepared.type_parameters.len() != source_args.len() {
            return None;
        }
        let mut pairs = Vec::with_capacity(source_args.len());
        for ((param, s), t) in prepared
            .type_parameters
            .iter()
            .zip(source_args.iter())
            .zip(target_args.iter())
        {
            match param.variance {
                TypeParamVariance::Unannotated => return None,
                TypeParamVariance::Out => pairs.push((*s, *t)),
                TypeParamVariance::In => pairs.push((*t, *s)),
                TypeParamVariance::InOut => {
                    pairs.push((*s, *t));
                    pairs.push((*t, *s));
                }
            }
        }
        Some(pairs)
    }

    fn try_object_spread_program_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        // Transparent aliases are followed before program recognition so an
        // aliased program is still a program side.
        let source = self.follow_relation_aliases(source);
        let target = self.follow_relation_aliases(target);
        let source_is_program = matches!(
            self.graph().node_data(source).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        );
        let target_is_program = matches!(
            self.graph().node_data(target).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        );
        if !source_is_program && !target_is_program {
            return None;
        }

        // One side is a program: resolve identity carriers on both sides so a
        // `DeclRef` / `InstantiationRef` counterpart reaches its Object surface
        // instead of collapsing to Unknown.
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return Some(RelationResult::Unknown),
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return Some(RelationResult::Unknown),
        };
        // Top/bottom rules apply before projection: `any` / `unknown` accept
        // from either side, `never` is bottom, an error type swallows, and the
        // `object` nonprimitive accepts every program (a construction program
        // always produces an object). These mirror `expand_pair`'s wildcard
        // and `object` arms so root and worklist agree.
        let source_data = self.graph().node_data(source);
        let target_data = self.graph().node_data(target);
        let error_swallows = |data: &SemanticNodeData| matches!(data, SemanticNodeData::Opaque(err) if err.is_error_type());
        match (source_data.as_deref(), target_data.as_deref()) {
            (Some(data), _) | (_, Some(data)) if error_swallows(data) => {
                return Some(assignable(bindings));
            }
            (Some(SemanticNodeData::Primitive(PrimitiveKind::Never)), _) => {
                return Some(assignable(bindings));
            }
            (
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any)),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown)),
            ) if self.strict_subtype_mode() => {
                return Some(RelationResult::NotAssignable);
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Unknown))) => {
                return Some(assignable(bindings));
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Any))) => {
                return Some(assignable(bindings));
            }
            (Some(SemanticNodeData::Primitive(PrimitiveKind::Any)), _) => {
                if self.subtype_mode() {
                    return Some(RelationResult::NotAssignable);
                }
                return Some(assignable(bindings));
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Never))) => {
                return Some(RelationResult::NotAssignable);
            }
            (_, Some(SemanticNodeData::Primitive(PrimitiveKind::Object))) => {
                return Some(assignable(bindings));
            }
            _ => {}
        }
        let Some(source_branches) = self.projected_relation_branches(source) else {
            return Some(RelationResult::Unknown);
        };
        let Some(target_branches) = self.projected_relation_branches(target) else {
            return Some(RelationResult::Unknown);
        };
        let overall_checkpoint = self.relation_session_checkpoint();
        let overall_bindings_len = bindings.len();
        let mut universal_unknown = false;

        for source_branch in &source_branches {
            let source_checkpoint = self.relation_session_checkpoint();
            let source_bindings_len = bindings.len();
            let mut existential_unknown = false;
            let mut accepted = false;
            for target_branch in &target_branches {
                let alternative_checkpoint = self.relation_session_checkpoint();
                let alternative_bindings_len = bindings.len();
                match self.relate_projected_object_branch(source_branch, target_branch, bindings) {
                    RelationResult::Assignable { .. } => {
                        accepted = true;
                        break;
                    }
                    RelationResult::Unknown => {
                        self.relation_session_rollback(&alternative_checkpoint);
                        bindings.truncate(alternative_bindings_len);
                        existential_unknown = true;
                    }
                    RelationResult::NotAssignable => {
                        self.relation_session_rollback(&alternative_checkpoint);
                        bindings.truncate(alternative_bindings_len);
                    }
                }
            }
            if accepted {
                continue;
            }
            self.relation_session_rollback(&source_checkpoint);
            bindings.truncate(source_bindings_len);
            if existential_unknown {
                universal_unknown = true;
                continue;
            }
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(overall_bindings_len);
            return Some(RelationResult::NotAssignable);
        }
        if universal_unknown {
            self.relation_session_rollback(&overall_checkpoint);
            bindings.truncate(overall_bindings_len);
            Some(RelationResult::Unknown)
        } else {
            Some(assignable(bindings))
        }
    }

    /// Follow transparent `Alias` indirection (with a cycle guard) without
    /// touching declaration carriers — the cheap half of relation
    /// normalization.
    fn follow_relation_aliases(&self, node: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let mut current = node;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            match graph.node_data(current).as_deref() {
                Some(SemanticNodeData::Alias(inner)) => current = *inner,
                _ => return current,
            }
        }
        current
    }

    /// When `node == node` unwraps (alias follow + identity-carrier unwrap) to
    /// an object-spread program, the identical pair answers through the
    /// program protocol: node identity is not a completeness proof, so an
    /// open program stays non-publishing `Unknown` while a closed one
    /// decides. `None` when the node is not (or does not unwrap to) a
    /// program — the ordinary identity shortcut applies.
    fn try_identical_open_program_result(
        &self,
        node: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let followed = self.follow_relation_aliases(node);
        // A class's instance type declares its members one by one and is
        // never a spread program: it relates to itself without building the
        // body, which a member body relating its own class (`h.m(h)` inside
        // the class) would re-enter.
        if self.references_class_declaration(followed) {
            return None;
        }
        let normalized = match self.unwrap_identity_carrier_for_relation(followed) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return None,
        };
        let normalized = match self.graph().node_data(normalized).as_deref() {
            Some(SemanticNodeData::BareRef(_) | SemanticNodeData::ImportType(_)) => {
                let transit = ProjectionReductionContext::structural_transit();
                let (resolved, _, _) =
                    self.resolve_carrier_subject_node_capturing_suppress(normalized, transit);
                resolved
            }
            _ => normalized,
        };
        if !matches!(
            self.graph().node_data(normalized).as_deref(),
            Some(SemanticNodeData::ObjectSpreadProgram(_))
        ) {
            return None;
        }
        self.try_object_spread_program_relation(normalized, normalized, bindings)
    }

    /// Whether `node` references a class declaration's instance type — a
    /// `DeclRef`, an application of one, or the declaration's lowering-time
    /// self-reference.
    fn references_class_declaration(&self, node: SemanticNodeId) -> bool {
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return false;
        };
        let (canonical, owner, name) = match data.as_ref() {
            SemanticNodeData::DeclRef { identity } => (
                Arc::clone(&identity.canonical_id),
                identity.owner,
                Arc::clone(&identity.decl_name),
            ),
            SemanticNodeData::InstantiationRef { base, .. } => (
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            ),
            SemanticNodeData::Opaque(QueryError::RecursiveRef { name, .. }) => {
                match graph.node_scope(node) {
                    Some(crate::semantic_query::NodeScopeId::File {
                        canonical_id,
                        owner,
                        ..
                    }) => (canonical_id, owner, Arc::clone(name)),
                    _ => return false,
                }
            }
            _ => return false,
        };
        drop(data);
        self.ctx
            .prepared_type_decl_return_only(canonical.as_ref(), owner, name.as_ref())
            .is_some_and(|prepared| {
                prepared.kind == verter_semantic::analysis::type_eval::TypeDeclKind::Class
            })
    }

    fn projected_relation_branches(
        &self,
        node: SemanticNodeId,
    ) -> Option<Vec<ProjectedRelationBranch>> {
        let node = self.follow_relation_aliases(node);
        let data = self.graph().node_data(node)?;
        match data.as_ref() {
            SemanticNodeData::Union(arms) => {
                // Target disjunction: each arm is one accepting alternative.
                let arms = arms.members_arc();
                drop(data);
                let mut branches = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    let arm = match self.unwrap_identity_carrier_for_relation(*arm) {
                        IdentityCarrierUnwrap::Concrete(id) => id,
                        IdentityCarrierUnwrap::Unresolvable => return None,
                    };
                    branches.extend(self.projected_relation_branches(arm)?);
                }
                Some(branches)
            }
            SemanticNodeData::ObjectSpreadProgram(_) => {
                drop(data);
                let formula = match self.project_object_spread_for_consumer(
                    node,
                    crate::semantic_query::ObjectProjectionSelector::Surface,
                    ProjectionReductionContext::structural_transit(),
                ) {
                    QueryResult::Value(formula) => formula,
                    QueryResult::Recursive(_) | QueryResult::Error(_) => return None,
                };
                Some(
                    formula
                        .alternatives()
                        .iter()
                        .map(|alternative| {
                            let mut members = Vec::new();
                            alternative.positive().visit(|fact| {
                                members.push(ProjectedRelationMember {
                                    key: fact.key().clone(),
                                    presence: fact.presence(),
                                    value: fact.value().clone(),
                                });
                            });
                            let mut call_signatures = Vec::new();
                            let mut construct_signatures = Vec::new();
                            for signature in alternative.signatures() {
                                match signature.kind() {
                                    crate::semantic_query::ObjectSignatureKind::Call => {
                                        call_signatures.push(signature.node());
                                    }
                                    crate::semantic_query::ObjectSignatureKind::Construct => {
                                        construct_signatures.push(signature.node());
                                    }
                                }
                            }
                            ProjectedRelationBranch {
                                members,
                                indices: alternative
                                    .indices()
                                    .iter()
                                    .map(|index| ProjectedRelationIndex {
                                        key_type: index.key_type(),
                                        value: index.value().clone(),
                                    })
                                    .collect(),
                                call_signatures,
                                construct_signatures,
                                open: alternative.closed().is_none(),
                            }
                        })
                        .collect(),
                )
            }
            SemanticNodeData::Object(surface) => {
                let branch = ProjectedRelationBranch {
                    members: surface
                        .positive_members()
                        .iter()
                        .filter_map(|member| {
                            Some(ProjectedRelationMember {
                                key: member.key.cloned_known()?,
                                presence: if member.optional {
                                    crate::semantic_query::PositiveKeyPresence::Optional
                                } else {
                                    crate::semantic_query::PositiveKeyPresence::Required
                                },
                                value: crate::semantic_query::ProjectionEvidence::Proven(
                                    member.value,
                                ),
                            })
                        })
                        .collect(),
                    indices: surface
                        .index_signatures
                        .iter()
                        .map(|index| ProjectedRelationIndex {
                            key_type: index.key_type,
                            value: crate::semantic_query::ProjectionEvidence::Proven(
                                index.value_type,
                            ),
                        })
                        .collect(),
                    call_signatures: surface.call_signatures.to_vec(),
                    construct_signatures: surface.construct_signatures.to_vec(),
                    open: false,
                };
                Some(vec![branch])
            }
            _ => None,
        }
    }

    fn relate_projected_object_branch(
        &self,
        source: &ProjectedRelationBranch,
        target: &ProjectedRelationBranch,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let mut acc = assignable(bindings);
        for target_member in &target.members {
            // Element-access identity: a numeric key and its canonical
            // string spelling address the same property.
            let source_member = source
                .members
                .iter()
                .find(|member| member.key.element_access_collides(&target_member.key));
            let pair = match source_member {
                Some(source_member)
                    if target_member.presence
                        == crate::semantic_query::PositiveKeyPresence::Required
                        && source_member.presence
                            == crate::semantic_query::PositiveKeyPresence::Optional =>
                {
                    // Optional-to-required: under `strictNullChecks` the
                    // implied `undefined` cannot relate to the required value;
                    // with null checks relaxed the pair relates on the value
                    // types alone (mirrors `relate_property_pair`).
                    let strict = self
                        .dispatch_txn
                        .borrow()
                        .relation
                        .strict
                        .unwrap_or(StrictFamilyConfig::TS_STRICT);
                    if strict.strict_null_checks {
                        RelationResult::NotAssignable
                    } else {
                        self.relate_projected_values(
                            &source_member.value,
                            &target_member.value,
                            bindings,
                        )
                    }
                }
                Some(source_member) => self.relate_projected_values(
                    &source_member.value,
                    &target_member.value,
                    bindings,
                ),
                None => {
                    // An index signature constrains the value a key would
                    // carry; it never manufactures named presence. Required
                    // named target keys therefore need a named source fact.
                    if target_member.presence
                        == crate::semantic_query::PositiveKeyPresence::Required
                    {
                        if source.open {
                            RelationResult::Unknown
                        } else {
                            RelationResult::NotAssignable
                        }
                    } else {
                        // An optional target member the source does not
                        // name is satisfied without relating a source index
                        // signature to it (the checker's
                        // `getUnmatchedProperty`); a source whose key set is
                        // open may still carry the key with any value.
                        let indexed = source.indices.iter().any(|index| {
                            index_signature_applies_to_property(
                                self.graph(),
                                index.key_type,
                                &target_member.key,
                            )
                        });
                        if source.open && !indexed {
                            RelationResult::Unknown
                        } else {
                            assignable(bindings)
                        }
                    }
                }
            };
            acc = result_and(acc, pair);
            if matches!(acc, RelationResult::NotAssignable) {
                return acc;
            }
        }

        for target_index in &target.indices {
            // Broad index obligations are universal conditional value
            // obligations: relate EVERY source index fact whose domain
            // overlaps the target's (a number index satisfies a string
            // obligation; a string index covers the number domain for
            // value relating — numeric keys are strings at runtime) and
            // every known named contribution the target's key type covers.
            // The legacy authority (`relate_target_index_signature`)
            // relates all overlaps; relating only the first would let an
            // `any` index swallow a refuting narrower index (order-
            // dependent false Accept). A closed branch with only
            // compatible known contributions satisfies the obligation
            // without owning an index signature itself. A source index
            // whose domain does not overlap is SKIPPED — domain
            // non-overlap alone never rejects (the legacy rule; tsc
            // agrees for string-to-number). The payload relation still
            // rejects value-mismatched overlapping indices.
            for source_index in source.indices.iter().filter(|index| {
                index_domains_overlap(self.graph(), index.key_type, target_index.key_type)
            }) {
                acc = result_and(
                    acc,
                    self.relate_projected_values(
                        &source_index.value,
                        &target_index.value,
                        bindings,
                    ),
                );
                if matches!(acc, RelationResult::NotAssignable) {
                    return acc;
                }
            }
            for source_member in source.members.iter().filter(|member| {
                index_signature_applies_to_property(
                    self.graph(),
                    target_index.key_type,
                    &member.key,
                )
            }) {
                acc = result_and(
                    acc,
                    self.relate_projected_values(
                        &source_member.value,
                        &target_index.value,
                        bindings,
                    ),
                );
            }
            if source.open {
                // A live residual needs an exact enumerable-value envelope;
                // without one the universal obligation cannot close.
                acc = result_and(acc, RelationResult::Unknown);
            }
            if matches!(acc, RelationResult::NotAssignable) {
                return acc;
            }
        }

        for target_signature in &target.call_signatures {
            let pair = if source.call_signatures.is_empty() && source.open {
                RelationResult::Unknown
            } else {
                self.relate_signature_alternatives(
                    &source.call_signatures,
                    *target_signature,
                    bindings,
                )
            };
            acc = result_and(acc, pair);
        }
        for target_signature in &target.construct_signatures {
            let pair = if source.construct_signatures.is_empty() && source.open {
                RelationResult::Unknown
            } else {
                self.relate_signature_alternatives(
                    &source.construct_signatures,
                    *target_signature,
                    bindings,
                )
            };
            acc = result_and(acc, pair);
        }

        if target.open {
            acc = result_and(acc, RelationResult::Unknown);
        }
        acc
    }

    fn relate_projected_values(
        &self,
        source: &crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
        target: &crate::semantic_query::ProjectionEvidence<SemanticNodeId>,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        match (source, target) {
            (
                crate::semantic_query::ProjectionEvidence::Proven(source),
                crate::semantic_query::ProjectionEvidence::Proven(target),
            ) => self.relate_member(*source, *target, bindings, InferPosition::Covariant),
            _ => RelationResult::Unknown,
        }
    }

    /// Reduce one conditional shell through the canonical conditional query.
    /// The outer option distinguishes a non-conditional node; the inner
    /// option distinguishes a decided reduction from an undecided shell.
    fn reduce_relation_conditional(&self, node: SemanticNodeId) -> Option<Option<SemanticNodeId>> {
        let data = self.graph().node_data(node)?;
        let SemanticNodeData::Conditional {
            check,
            extends,
            true_branch_ref,
            false_branch_ref,
            distributive,
            pending,
        } = data.as_ref()
        else {
            return None;
        };
        let key = SemanticQueryKey::Conditional {
            check: *check,
            extends: *extends,
            true_branch: *true_branch_ref,
            false_branch: *false_branch_ref,
            distributive: *distributive,
            pending: pending.clone(),
        };
        drop(data);
        Some(match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) if value != node => Some(value),
            _ => None,
        })
    }

    /// The pair a relation decides in place of `(source, target)` when
    /// either side is a written intersection whose canonical intersection
    /// differs from it (`getIntersectionType`: `string & ('a' | 1)` IS
    /// `'a'`, `number & string` IS `never`); `None` when neither is.
    fn reduced_authored_relation_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> Option<(SemanticNodeId, SemanticNodeId)> {
        let is_intersection = |node: SemanticNodeId| {
            matches!(
                self.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Intersection(_))
            )
        };
        if !is_intersection(source) && !is_intersection(target) {
            return None;
        }
        let nullability = crate::semantic_query::NullabilityPolicy::from_strict_null_checks(
            self.dispatch_txn
                .borrow()
                .relation
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT)
                .strict_null_checks,
        );
        let graph = self.graph();
        // A source intersection over a union IS the distributed union of
        // intersections (`getIntersectionType`), even where its written
        // form stays the printed origin: `(A | B) & C` relates as
        // `(A & C) | (B & C)`.
        if let Some(SemanticNodeData::Intersection(arms)) = graph.node_data(source).as_deref() {
            let arms = arms.members_arc();
            if arms.iter().any(|arm| {
                matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Union(_))
                )
            }) {
                if let Some(distributed) = self.distributed_intersection(&arms) {
                    if distributed != source {
                        return Some((distributed, target));
                    }
                }
            }
        }
        let reduced_source =
            super::canonical_algebra::reduced_authored_intersection(graph, source, nullability);
        let reduced_target =
            super::canonical_algebra::reduced_authored_intersection(graph, target, nullability);
        (reduced_source.is_some() || reduced_target.is_some()).then(|| {
            (
                reduced_source.unwrap_or(source),
                reduced_target.unwrap_or(target),
            )
        })
    }

    /// Whether either side is a typed marker for a value Verter did not read,
    /// or the pair is one node that reaches such a marker anywhere inside it
    /// — an absence, an unmodelled position, an unsupported surface, a
    /// partial or control carrier. No relation over one is a fact: two
    /// markers are never the same type because they are the same marker,
    /// and a marker is never below `unknown` or above `never` because the
    /// relation cannot read it. The checker's error type (the
    /// `CheckerRecovery` and `Failure` dispositions) relates as `any`
    /// does, and a recursion or declaration carrier is unwrapped before it
    /// is compared, so neither is a marker here.
    ///
    /// A relation to a top type (`unknown`, `any`) holds for every type,
    /// whatever a marker stands for, so it stays decided.
    pub(super) fn relation_reads_unread_marker(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        use super::query_error_disposition::{query_error_disposition, QueryErrorDisposition};
        let marker = |node: SemanticNodeId| match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Opaque(err)) => !matches!(
                query_error_disposition(err),
                QueryErrorDisposition::Failure
                    | QueryErrorDisposition::CheckerRecovery
                    | QueryErrorDisposition::RecursionCarrier
                    | QueryErrorDisposition::ExpandableDecl
            ),
            _ => false,
        };
        let top = |node: SemanticNodeId| {
            matches!(
                self.graph().node_data(node).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::Unknown | PrimitiveKind::Any
                ))
            )
        };
        // The same node relates to itself on identity only when it is known
        // throughout: `[marker]` is no more `[marker]` than the marker is
        // itself.
        let identical_unread = source == target && self.graph().node_reaches_unresolved(source);
        (marker(source) || marker(target) || identical_unread) && !top(target)
    }

    /// The O(tag) fast-reject prefilter (RI-5): decides the trivial
    /// primitive/identity/top/bottom cases inline BEFORE any recursive
    /// structural work. Non-trivial pairs return `Unknown` and fall
    /// through to the structural reducer.
    pub(super) fn shallow_relation_check(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> ShallowRelation {
        if self.relation_reads_unread_marker(source, target) {
            return ShallowRelation::Unknown;
        }
        if source == target {
            let mut no_bindings = Vec::new();
            if let Some(result) = self.try_identical_open_program_result(source, &mut no_bindings) {
                return match result {
                    RelationResult::Assignable { .. } => ShallowRelation::Assignable,
                    RelationResult::NotAssignable => ShallowRelation::NotAssignable,
                    RelationResult::Unknown => ShallowRelation::Unknown,
                };
            }
            return ShallowRelation::Assignable;
        }
        let graph = self.graph();
        let Some(source_data) = graph.node_data(source) else {
            return ShallowRelation::Unknown;
        };
        let Some(target_data) = graph.node_data(target) else {
            return ShallowRelation::Unknown;
        };
        if let Some((source, target)) = self.reduced_authored_relation_pair(source, target) {
            drop(source_data);
            drop(target_data);
            return self.shallow_relation_check(source, target);
        }
        match (&*source_data, &*target_data) {
            // The error-type wildcard fires BEFORE the `(_, Never)` bottom
            // arm — `error` relates bidirectionally like `any` (the same
            // arm order the structural reducer applies).
            (SemanticNodeData::Opaque(err), _) if err.is_error_type() => {
                ShallowRelation::Assignable
            }
            (_, SemanticNodeData::Opaque(err)) if err.is_error_type() => {
                ShallowRelation::Assignable
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => ShallowRelation::Assignable,
            (
                SemanticNodeData::Primitive(PrimitiveKind::Any),
                SemanticNodeData::Primitive(PrimitiveKind::Unknown),
            ) if self.strict_subtype_mode() => ShallowRelation::NotAssignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => ShallowRelation::Assignable,
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => {
                if self.subtype_mode() {
                    ShallowRelation::NotAssignable
                } else {
                    ShallowRelation::Assignable
                }
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                ShallowRelation::NotAssignable
            }
            // Strict-family behavioral branch (RI-10), mirrored from the
            // structural reducer's arm order: with `strictNullChecks` OFF,
            // `null` / `undefined` are assignable to every remaining target
            // (`never` already rejected above).
            (SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined), _)
                if !self
                    .dispatch_txn
                    .borrow()
                    .relation
                    .strict
                    .unwrap_or(StrictFamilyConfig::TS_STRICT)
                    .strict_null_checks =>
            {
                ShallowRelation::Assignable
            }
            (SemanticNodeData::Primitive(a), SemanticNodeData::Primitive(b)) => {
                if a == b || (*a == PrimitiveKind::Undefined && *b == PrimitiveKind::Void) {
                    ShallowRelation::Assignable
                } else {
                    ShallowRelation::NotAssignable
                }
            }
            _ => ShallowRelation::Unknown,
        }
    }

    /// Construct a public payload and intern its proof into the store's
    /// payload-side proof table (Decision 4 — the proof rides the table
    /// BY ID, never embedded on the value / type-values surface).
    fn relation_payload(
        &self,
        outcome: RelationOutcome,
        bindings: Arc<[InferBinding]>,
        proof: RelationProof,
    ) -> RelationPayload {
        let relation_proof = self.graph().intern_relation_proof(proof);
        RelationPayload {
            outcome,
            bindings,
            relation_proof,
        }
    }

    /// Dispatch-aware relation judgement: the Object-vs-Record arm first,
    /// then the identity-carrier unwrap, then the structural worklist.
    fn decide_relation_with_dispatch(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        intersection_target_arm: bool,
    ) -> RelationResult {
        if let Some(r) = self.try_object_vs_record_relation(source, target, bindings) {
            return r;
        }
        let source = match self.unwrap_identity_carrier_for_relation(source) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        let target = match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(id) => id,
            IdentityCarrierUnwrap::Unresolvable => return RelationResult::Unknown,
        };
        // Function-inference demand point (the pre-relation function-infer
        // case, now inside the authority): with an active session and a
        // Function pattern, a check still riding a deferred /
        // `InstantiationRef` shell materialises through the oracle demand
        // so positional binding can zip its signature.
        if self.relation_session_active() {
            if let Some(pattern) = self.relation_pattern_info(target) {
                if pattern.shape == InferPatternShape::Function {
                    let materialised = self.materialise_function_infer_check(source);
                    if materialised != source {
                        return self.decide_relation(
                            materialised,
                            target,
                            bindings,
                            intersection_target_arm,
                        );
                    }
                }
            }
        }
        self.decide_relation(source, target, bindings, intersection_target_arm)
    }

    /// Materialise a function-infer check through the oracle's transit
    /// demand (the retired pre-relation path): deferred-shell evaluation,
    /// then a one-level demanded `Instantiate` for an `InstantiationRef`
    /// carrier. Returns the input unchanged when no materialisation fires.
    fn materialise_function_infer_check(&self, check: SemanticNodeId) -> SemanticNodeId {
        let graph = self.graph();
        let oracle_demand = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let mut resolved = self
            .evaluate_deferred_semantic_node_with_context(check, oracle_demand)
            .into_active_query_build_node(self);
        if let Some(SemanticNodeData::InstantiationRef { base, args }) =
            graph.node_data(resolved).as_deref()
        {
            let owner_canonical = Arc::clone(&base.canonical_id);
            let slot = self.type_slot_for(
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            );
            let args: Arc<[SemanticNodeId]> = Arc::from(
                args.iter()
                    .map(|arg| {
                        self.evaluate_deferred_semantic_node_with_context(*arg, oracle_demand)
                            .into_active_query_build_node(self)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            let read = self.execute_read(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    slot,
                    args,
                    self.instantiate_context_for(&owner_canonical, oracle_demand),
                ),
            ));
            crate::request_context::observe_component_meta_read_suppress(&read);
            if let QueryResult::Value(id) = read.value {
                resolved = self.evaluate_deferred_semantic_node(id);
            }
        }
        resolved
    }

    /// The iterative structural worklist driver. Consumes a worklist of
    /// pairs and reducers, combining the final [`RelationResult`].
    ///
    /// **Termination budget.** The driver caps total work at
    /// `10 × graph.node_count()` with a minimum floor of 4096 entries.
    /// Exceeding the budget poisons the frame with the typed
    /// [`RecursionOrBudgetCap`] (the public `BudgetExceeded` outcome) and
    /// yields `Unknown` — the SCC gate routes the whole component through
    /// ReturnOnly.
    pub(super) fn decide_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        intersection_target_arm: bool,
    ) -> RelationResult {
        // Program recognition precedes the identity shortcut here exactly as
        // at the root and in `expand_pair`: an open program is never accepted
        // on node identity.
        if let Some(result) = self.try_object_spread_program_relation(source, target, bindings) {
            return result;
        }
        let occurrence = self.relation_current_occurrence();
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            return result;
        }
        if self.relation_reads_unread_marker(source, target) {
            return RelationResult::Unknown;
        }
        if source == target {
            if let Some(result) = self.try_identical_open_program_result(source, bindings) {
                return result;
            }
            return assignable(bindings);
        }
        let budget_limit = self.relation_budget_limit();
        let mut budget_used: u64 = 0;
        let mut work: Vec<RelateWork> = Vec::new();
        let mut results: Vec<RelationResult> = Vec::new();
        work.push(RelateWork::Expand(source, target, intersection_target_arm));
        while let Some(item) = work.pop() {
            budget_used = budget_used.saturating_add(1);
            if budget_used > budget_limit {
                self.note_relation_budget_exceeded(budget_limit);
                return RelationResult::Unknown;
            }
            match item {
                RelateWork::Expand(s, t, intersection_target_arm) => {
                    self.expand_pair(
                        s,
                        t,
                        intersection_target_arm,
                        bindings,
                        &mut work,
                        &mut results,
                    );
                }
                RelateWork::Eval(s, t) => {
                    if self.relation_eval_requires_canonical_frame(s, t) {
                        results.push(self.relate_member(s, t, bindings, InferPosition::Covariant));
                    } else {
                        self.expand_pair(s, t, false, bindings, &mut work, &mut results);
                    }
                }
                RelateWork::Arm(s, t) => {
                    if self.relation_eval_requires_canonical_frame(s, t)
                        || self.arm_pair_names_a_declaration_carrier(s, t)
                    {
                        results.push(self.relate_member(s, t, bindings, InferPosition::Covariant));
                    } else {
                        self.expand_pair(s, t, false, bindings, &mut work, &mut results);
                    }
                }
                RelateWork::TargetArm(s, t) => {
                    if self.relation_eval_requires_canonical_frame(s, t)
                        || self.arm_pair_names_a_declaration_carrier(s, t)
                    {
                        results.push(self.relate_intersection_target_arm(s, t, bindings));
                    } else {
                        self.expand_pair(s, t, true, bindings, &mut work, &mut results);
                    }
                }
                RelateWork::ReduceAnd(n) => {
                    let combined = reduce_and_from_results(&mut results, n);
                    results.push(combined);
                }
            }
        }
        results.pop().unwrap_or(RelationResult::Unknown)
    }

    /// Whether a recursive sub-pair must re-enter the full memoized relation
    /// authority instead of expanding inline.
    ///
    /// A `Conditional` always must: its branch selection is itself a
    /// relation, so expanding it inline would run a second selection outside
    /// the memo.
    ///
    /// A declaration CARRIER (a `DeclRef` / `InstantiationRef` / an
    /// unexpanded declaration or recursive-reference placeholder), a
    /// mapped type, a `keyof` or an indexed access on either side must too:
    /// the checker relates the type a carrier names — an indexed access over
    /// a type that is not generic IS the property type it reads, so a tuple
    /// element `Rec["a"]` relates as `any` — which only the canonical
    /// frame's identity unwrap reveals — an intersection target `QA & QB` distributes into pairs
    /// whose arms are declarations, a nominal pair compares DECLARING
    /// identities, and expanded inline a carrier answers `Unknown`, which a
    /// subtype reduction reads as undecided (`A1[]` below `{ x: string }[]`
    /// relates `A1` to `{ x: string }`, `{ v: A1 }` below `{ v: D1 }`
    /// relates `A1` to `D1`). The canonical frame's memo also closes a
    /// recursive declaration's cycle coinductively. An intersection source
    /// whose members relate to the target alone relates through the object
    /// they compose after them ([`Self::relate_composed_intersection`]).
    ///
    /// This runs on EVERY `RelateWork::Eval`, so its cost is the relation
    /// engine's per-pair floor: each side's node data is read AT MOST ONCE.
    /// A `Conditional` source short-circuits after a single read.
    fn relation_eval_requires_canonical_frame(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let graph = self.graph();
        let Some(source_data) = graph.node_data(source) else {
            return false;
        };
        if matches!(&*source_data, SemanticNodeData::Conditional { .. }) {
            return true;
        }
        let Some(target_data) = graph.node_data(target) else {
            return false;
        };
        if matches!(&*target_data, SemanticNodeData::Conditional { .. }) {
            return true;
        }
        let is_carrier = |data: &SemanticNodeData| {
            matches!(
                data,
                SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::Mapped { .. }
                    | SemanticNodeData::KeyOf { .. }
                    | SemanticNodeData::IndexedAccess { .. }
                    | SemanticNodeData::Opaque(
                        QueryError::DeclPlaceholder { .. } | QueryError::RecursiveRef { .. }
                    )
            )
        };
        is_carrier(&source_data) || is_carrier(&target_data)
    }

    /// Whether `target` is a declaration carrier naming an object type —
    /// a target an intersection source relates to through the object its
    /// members compose ([`Self::relate_composed_intersection`]), as it does
    /// to an object literal type.
    fn carrier_names_object(&self, target: SemanticNodeId) -> bool {
        match self.unwrap_identity_carrier_for_relation(target) {
            IdentityCarrierUnwrap::Concrete(resolved) => {
                resolved != target
                    && matches!(
                        self.graph().node_data(resolved).as_deref(),
                        Some(SemanticNodeData::Object(_))
                    )
            }
            IdentityCarrierUnwrap::Unresolvable => false,
        }
    }

    /// Whether either side of a composite arm pair is a declaration
    /// carrier (a `DeclRef` / `InstantiationRef` / an unexpanded
    /// declaration placeholder) the inline deferred gate would answer
    /// `Unknown` for.
    fn arm_pair_names_a_declaration_carrier(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> bool {
        let graph = self.graph();
        [source, target].into_iter().any(|node| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(
                    SemanticNodeData::DeclRef { .. }
                        | SemanticNodeData::InstantiationRef { .. }
                        | SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
                )
            )
        })
    }

    /// The checker's weak-type check (`isWeakType` with
    /// `hasCommonProperties`): a target with at least one property, every
    /// property optional and no call, construct or index signature — an
    /// intersection when every arm is one — rejects a source that has a
    /// property or a signature and shares no property with it. A primitive,
    /// an array or a tuple offers its apparent type's properties, an
    /// intersection source every member's, and the global `Object`
    /// interface is never checked. `false` whenever the check passes, does
    /// not apply or cannot be read.
    fn weak_target_rejects(&self, source: SemanticNodeId, target: SemanticNodeId) -> bool {
        let Some(targets) = self.weak_target_surfaces(target) else {
            return false;
        };
        let Some((keys, has_signatures)) = self.weak_check_source_members(source, target, true)
        else {
            return false;
        };
        if keys.is_empty() && !has_signatures {
            return false;
        }
        let shares_property = keys.iter().any(|key| {
            targets.iter().any(|surface| {
                matches!(
                    surface.project_known_key(key),
                    crate::semantic_query::SurfaceKeyProjection::Exact(_)
                )
            })
        });
        !shares_property && !self.is_global_object_surface(source, target)
    }

    /// The surfaces of a weak target: the object itself, or every arm of an
    /// intersection whose arms are all weak objects. `None` otherwise.
    fn weak_target_surfaces(&self, target: SemanticNodeId) -> Option<Vec<SurfaceView>> {
        let graph = self.graph();
        let is_weak = |surface: &SurfaceView| {
            let members = surface.closed().complete_members();
            !members.is_empty()
                && members.iter().all(|member| member.optional)
                && surface.call_signatures.is_empty()
                && surface.construct_signatures.is_empty()
                && surface.index_signatures.is_empty()
                && !surface.closed().has_index_signature()
        };
        match graph.node_data(target).as_deref()? {
            SemanticNodeData::Object(surface) => is_weak(surface).then(|| vec![surface.clone()]),
            SemanticNodeData::Intersection(members) => {
                let members = members.members_arc();
                let mut surfaces = Vec::with_capacity(members.len());
                for member in members.iter() {
                    let IdentityCarrierUnwrap::Concrete(resolved) =
                        self.unwrap_identity_carrier_for_relation(*member)
                    else {
                        return None;
                    };
                    let resolved = self.follow_relation_aliases(resolved);
                    match graph.node_data(resolved).as_deref() {
                        Some(SemanticNodeData::Object(surface)) if is_weak(surface) => {
                            surfaces.push(surface.clone());
                        }
                        _ => return None,
                    }
                }
                Some(surfaces)
            }
            _ => None,
        }
    }

    /// The property names a source offers the weak-type check, and whether
    /// it has a call or construct signature (`getPropertiesOfType`,
    /// `typeHasCallOrConstructSignatures`). `None` when they cannot be read.
    /// An intersection source reads its members one level deep: a canonical
    /// intersection's members are never intersections themselves.
    fn weak_check_source_members(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        split_intersection: bool,
    ) -> Option<(Vec<crate::semantic_query::PropertyKey>, bool)> {
        let graph = self.graph();
        let surface_members = |surface: &SurfaceView| {
            let keys = surface
                .positive_members()
                .iter()
                .map(|member| member.key.cloned_known())
                .collect::<Option<Vec<_>>>()?;
            let has_signatures =
                !surface.call_signatures.is_empty() || !surface.construct_signatures.is_empty();
            Some((keys, has_signatures))
        };
        let data = graph.node_data(source)?;
        match &*data {
            SemanticNodeData::Object(surface) => surface_members(surface),
            SemanticNodeData::Signature { .. } => Some((Vec::new(), true)),
            SemanticNodeData::Intersection(members) if split_intersection => {
                let mut keys = Vec::new();
                let mut has_signatures = false;
                for member in members.members_arc().iter() {
                    let IdentityCarrierUnwrap::Concrete(resolved) =
                        self.unwrap_identity_carrier_for_relation(*member)
                    else {
                        return None;
                    };
                    let resolved = self.follow_relation_aliases(resolved);
                    let (member_keys, member_signatures) =
                        self.weak_check_source_members(resolved, target, false)?;
                    keys.extend(member_keys);
                    has_signatures |= member_signatures;
                }
                Some((keys, has_signatures))
            }
            _ => {
                let Some((name, args)) = self.apparent_wrapper_of(source) else {
                    // `null`, `undefined`, `void` and `object` have no
                    // properties.
                    return matches!(&*data, SemanticNodeData::Primitive(_))
                        .then(|| (Vec::new(), false));
                };
                let canonical = self.relation_wrapper_canonical(target)?;
                let wrapper = match self.global_wrapper_surface(name, &args, canonical.as_ref()) {
                    super::apparent_type::GlobalWrapper::Surface(surface) => Some(surface),
                    super::apparent_type::GlobalWrapper::Absent => None,
                    super::apparent_type::GlobalWrapper::Unsettled => return None,
                };
                let apparent = match &*data {
                    SemanticNodeData::Tuple { elements, readonly } => {
                        Some(self.tuple_apparent_surface(wrapper, elements, *readonly))
                    }
                    _ => wrapper,
                };
                let Some(apparent) = apparent else {
                    return Some((Vec::new(), false));
                };
                match graph.node_data(apparent).as_deref() {
                    Some(SemanticNodeData::Object(surface)) => surface_members(surface),
                    _ => None,
                }
            }
        }
    }

    /// Whether `source` is the global `Object` interface, which the
    /// weak-type check never applies to.
    fn is_global_object_surface(&self, source: SemanticNodeId, target: SemanticNodeId) -> bool {
        self.relation_wrapper_canonical(target)
            .is_some_and(|canonical| {
                matches!(
                    self.global_wrapper_surface("Object", &[], canonical.as_ref()),
                    super::apparent_type::GlobalWrapper::Surface(surface) if surface == source
                )
            })
    }

    /// Whether a source that declares no index signature applicable to one
    /// of `target`'s is refused it outright, as the checker's
    /// `typeRelatedToIndexInfo` refuses it when the source is not an
    /// object type with an inferable index
    /// ([`Self::infers_index_signature`]); only such a source relates its
    /// properties to the index signature instead. A string index signature
    /// of type `any` takes every source.
    fn implicit_index_rejects(&self, source: SemanticNodeId, target: SemanticNodeId) -> bool {
        let graph = self.graph();
        if !matches!(
            graph.node_data(source).as_deref(),
            Some(
                SemanticNodeData::DeclRef { .. }
                    | SemanticNodeData::InstantiationRef { .. }
                    | SemanticNodeData::Intersection(_)
                    | SemanticNodeData::MergedDecl { .. }
                    | SemanticNodeData::ClassExpressionInstance { .. }
                    | SemanticNodeData::Primitive(PrimitiveKind::Object)
                    | SemanticNodeData::Object(_)
            )
        ) {
            return false;
        }
        if self.infers_index_signature(source, &mut FxHashSet::default()) {
            return false;
        }
        let IdentityCarrierUnwrap::Concrete(target) =
            self.unwrap_identity_carrier_for_relation(target)
        else {
            return false;
        };
        let target = self.follow_relation_aliases(target);
        let target_indexes = match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Object(surface)) if !surface.index_signatures.is_empty() => {
                surface.index_signatures.clone()
            }
            _ => return false,
        };
        let required: Vec<SemanticNodeId> = target_indexes
            .iter()
            .filter(|index| {
                !matches!(
                    (
                        graph.node_data(index.key_type).as_deref(),
                        graph.node_data(index.value_type).as_deref(),
                    ),
                    (
                        Some(SemanticNodeData::Primitive(PrimitiveKind::String)),
                        Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
                    )
                )
            })
            .map(|index| index.key_type)
            .collect();
        if required.is_empty() {
            return false;
        }
        let Some(declared) = self.declared_index_keys(source, &mut FxHashSet::default()) else {
            return false;
        };
        required.iter().any(|target_key| {
            !declared
                .iter()
                .any(|source_key| index_key_applies(graph, *source_key, *target_key))
        })
    }

    /// The checker's `isObjectTypeWithInferableIndex`: an object type from a
    /// type literal, an object literal or a mapped type relates to an index
    /// signature through its properties; a declared interface or class
    /// instance, `object`, a type with a call or construct signature, and an
    /// intersection holding any of them do not. An alias reads as the type it
    /// names, and an interface that declares no member of its own and extends
    /// one type reads as that type (measured: `interface Q extends { x:
    /// number } {}` takes `{ [k: string]: number }`, with a member of its own
    /// or a second base it does not). `true` whenever undecided.
    fn infers_index_signature(
        &self,
        node: SemanticNodeId,
        seen: &mut FxHashSet<SemanticNodeId>,
    ) -> bool {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        if !seen.insert(node) {
            return true;
        }
        let graph = self.graph();
        let Some(data) = graph.node_data(node) else {
            return true;
        };
        match &*data {
            SemanticNodeData::Alias(inner) => self.infers_index_signature(*inner, seen),
            SemanticNodeData::Primitive(PrimitiveKind::Object)
            | SemanticNodeData::MergedDecl { .. }
            | SemanticNodeData::ClassExpressionInstance { .. }
            | SemanticNodeData::Signature { .. } => false,
            SemanticNodeData::Object(surface) => {
                surface.call_signatures.is_empty() && surface.construct_signatures.is_empty()
            }
            SemanticNodeData::Intersection(members) => members
                .members_arc()
                .iter()
                .all(|member| self.infers_index_signature(*member, seen)),
            SemanticNodeData::DeclRef { identity }
            | SemanticNodeData::InstantiationRef { base: identity, .. } => {
                let kind = self.prepared_decl_kind(identity);
                drop(data);
                let IdentityCarrierUnwrap::Concrete(body) =
                    self.unwrap_identity_carrier_one_step(node)
                else {
                    return true;
                };
                match kind {
                    Some(TypeDeclKind::Alias) => self.infers_index_signature(body, seen),
                    Some(TypeDeclKind::Interface) => match self.sole_extended_type(body) {
                        Some(base) => self.infers_index_signature(base, seen),
                        None => false,
                    },
                    Some(TypeDeclKind::Class) => false,
                    None => true,
                }
            }
            _ => true,
        }
    }

    /// The one type an interface body extends when it declares no member of
    /// its own: the body is that type's carrier, or an intersection of it
    /// and empty objects.
    fn sole_extended_type(&self, body: SemanticNodeId) -> Option<SemanticNodeId> {
        let graph = self.graph();
        let is_carrier = |node: SemanticNodeId| {
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::DeclRef { .. } | SemanticNodeData::InstantiationRef { .. })
            )
        };
        if is_carrier(body) {
            return Some(body);
        }
        let members = match graph.node_data(body).as_deref() {
            Some(SemanticNodeData::Intersection(members)) => members.members_arc(),
            _ => return None,
        };
        let mut base = None;
        for member in members.iter() {
            let empty = matches!(
                graph.node_data(*member).as_deref(),
                Some(SemanticNodeData::Object(surface)) if surface.closed().is_empty()
            );
            if empty {
                continue;
            }
            if base.is_some() || !is_carrier(*member) {
                return None;
            }
            base = Some(*member);
        }
        base
    }

    /// The key types of the index signatures `source` declares, its
    /// intersection's members', heritage's and merged declarations'
    /// included. `None` when they cannot be read.
    fn declared_index_keys(
        &self,
        source: SemanticNodeId,
        seen: &mut FxHashSet<SemanticNodeId>,
    ) -> Option<Vec<SemanticNodeId>> {
        if !seen.insert(source) {
            return Some(Vec::new());
        }
        let IdentityCarrierUnwrap::Concrete(resolved) =
            self.unwrap_identity_carrier_for_relation(source)
        else {
            return None;
        };
        let resolved = self.follow_relation_aliases(resolved);
        let graph = self.graph();
        match graph.node_data(resolved).as_deref()? {
            SemanticNodeData::Object(surface) => {
                if surface.has_known_index_signature() && surface.index_signatures.is_empty() {
                    return None;
                }
                Some(
                    surface
                        .index_signatures
                        .iter()
                        .map(|index| index.key_type)
                        .collect(),
                )
            }
            SemanticNodeData::Intersection(members) => {
                let mut keys = Vec::new();
                for member in members.members_arc().iter() {
                    keys.extend(self.declared_index_keys(*member, seen)?);
                }
                Some(keys)
            }
            SemanticNodeData::Primitive(PrimitiveKind::Object) => Some(Vec::new()),
            _ => None,
        }
    }

    /// The file whose library an apparent type is read from while relating
    /// to `target`: the demand's, else the one the target was declared in.
    fn relation_wrapper_canonical(&self, target: SemanticNodeId) -> Option<Arc<str>> {
        self.wrapper_demand_canonical().or_else(|| {
            self.graph()
                .node_scope(target)
                .and_then(|scope| scope.canonical_file())
        })
    }

    /// Expand a single relate pair into direct result(s) or sub-work
    /// items. Pushes exactly one net result onto `results` by the time all
    /// sub-work drains.
    /// Relate a string literal against a template-literal pattern by the
    /// pattern's quasi skeleton, deciding ONLY what the skeleton proves;
    /// every undecidable shape returns `None` and stays deferred.
    ///
    /// The skeleton match treats every placeholder as an arbitrary
    /// string — an OVER-approximation of the template's denoted set, so
    /// a failed match is a proof of NON-membership while a successful
    /// match alone proves nothing. Verdicts:
    ///
    /// - literal → template: no skeleton match ⇒ `NotAssignable`
    ///   (over-approximated set excludes the literal). A match with
    ///   every placeholder typed `string` ⇒ `Assignable` (each gap
    ///   slice is a string). Any other match ⇒ `None` (a `number`
    ///   placeholder constrains its slice beyond the skeleton).
    /// - template → literal: only when every placeholder provably
    ///   denotes a NONEMPTY string set (a scalar primitive or a
    ///   literal — `never` denotes the empty template, which IS
    ///   assignable to anything): no skeleton match ⇒ `NotAssignable`
    ///   (two disjoint nonempty sets); a match ⇒ `None` (subset of a
    ///   singleton needs the template to BE that singleton).
    /// - template → template: the same quasis over structurally identical
    ///   placeholders are ONE type (the checker interns a template by its
    ///   texts and types) ⇒ `Assignable`; any other pair ⇒ `None`.
    ///
    /// Quasis are stored as RAW source text; a quasi carrying an escape
    /// (`\\`) is not cooked-comparable and bails to `None`.
    fn relate_string_literal_and_template(
        &self,
        source_data: &SemanticNodeData,
        target_data: &SemanticNodeData,
        bindings: &[InferBinding],
    ) -> Option<RelationResult> {
        fn skeleton_comparable(quasis: &[Arc<str>], expressions: &[SemanticNodeId]) -> bool {
            quasis.len() == expressions.len() + 1
                && quasis.iter().all(|quasi| !quasi.contains('\\'))
        }
        /// Whether `text` is producible from the quasi skeleton with
        /// every placeholder read as an arbitrary (possibly empty)
        /// string. Leftmost placement of each middle quasi is complete:
        /// both candidate remainders are suffixes of the same text, so
        /// the final `ends_with` verdict is placement-independent.
        fn skeleton_matches(text: &str, quasis: &[Arc<str>]) -> bool {
            let first = quasis.first().map(|q| q.as_ref()).unwrap_or("");
            let last = quasis.last().map(|q| q.as_ref()).unwrap_or("");
            if quasis.len() == 1 {
                return text == first;
            }
            let Some(mut rest) = text.strip_prefix(first) else {
                return false;
            };
            for quasi in &quasis[1..quasis.len() - 1] {
                match rest.find(quasi.as_ref()) {
                    Some(pos) => rest = &rest[pos + quasi.len()..],
                    None => return false,
                }
            }
            rest.ends_with(last)
        }
        let placeholder_is_any_string = |id: SemanticNodeId| {
            matches!(
                self.graph().node_data(id).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::String))
            )
        };
        let placeholder_denotes_nonempty = |id: SemanticNodeId| {
            matches!(
                self.graph().node_data(id).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::String
                        | PrimitiveKind::Number
                        | PrimitiveKind::BigInt
                        | PrimitiveKind::Boolean
                        | PrimitiveKind::Null
                        | PrimitiveKind::Undefined
                )) | Some(SemanticNodeData::Literal(_))
            )
        };
        match (source_data, target_data) {
            (
                SemanticNodeData::Literal(LiteralValue::String(text)),
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                },
            ) if skeleton_comparable(quasis, expressions) => {
                if !skeleton_matches(text, quasis) {
                    return Some(RelationResult::NotAssignable);
                }
                if expressions.iter().copied().all(placeholder_is_any_string) {
                    return Some(assignable(bindings));
                }
                None
            }
            (
                SemanticNodeData::TemplateLiteral {
                    quasis,
                    expressions,
                },
                SemanticNodeData::Literal(LiteralValue::String(text)),
            ) if skeleton_comparable(quasis, expressions) => {
                if expressions.is_empty() {
                    return Some(if skeleton_matches(text, quasis) {
                        assignable(bindings)
                    } else {
                        RelationResult::NotAssignable
                    });
                }
                if !expressions
                    .iter()
                    .copied()
                    .all(placeholder_denotes_nonempty)
                {
                    return None;
                }
                if !skeleton_matches(text, quasis) {
                    return Some(RelationResult::NotAssignable);
                }
                None
            }
            (
                SemanticNodeData::TemplateLiteral {
                    quasis: source_quasis,
                    expressions: source_expressions,
                },
                SemanticNodeData::TemplateLiteral {
                    quasis: target_quasis,
                    expressions: target_expressions,
                },
            ) if source_quasis == target_quasis
                && source_expressions.len() == target_expressions.len() =>
            {
                let mut evidence = super::canonical_algebra::CanonicalEvidence::default();
                let mut budget = super::canonical_algebra::COMPARE_WORK_BUDGET;
                let identical = source_expressions
                    .iter()
                    .zip(target_expressions.iter())
                    .all(|(source, target)| {
                        matches!(
                            super::canonical_algebra::compare_structural(
                                self.graph(),
                                *source,
                                *target,
                                &mut evidence,
                                &mut budget,
                            ),
                            super::canonical_algebra::StructuralIdentity::Equal
                        )
                    });
                identical.then(|| assignable(bindings))
            }
            _ => None,
        }
    }

    fn expand_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        intersection_target_arm: bool,
        bindings: &mut Vec<InferBinding>,
        work: &mut Vec<RelateWork>,
        results: &mut Vec<RelationResult>,
    ) {
        // A pair the checker relates as this very one — an alias or a merged
        // declaration unwrapped, an apparent type read — stays an
        // intersection target's arm when this pair is one.
        let same_pair = |source, target| {
            if intersection_target_arm {
                RelateWork::TargetArm(source, target)
            } else {
                RelateWork::Eval(source, target)
            }
        };
        // Program recognition precedes the identity shortcut, structural
        // shortcuts, inference deposits, and distribution — identical to the
        // root protocol. An open program is never accepted on node identity.
        let graph = self.graph();
        let source_data = match graph.node_data(source) {
            Some(d) => d,
            None => {
                results.push(RelationResult::Unknown);
                return;
            }
        };
        let target_data = match graph.node_data(target) {
            Some(d) => d,
            None => {
                results.push(RelationResult::Unknown);
                return;
            }
        };

        // ── Alias: unwrap transparently on either side ─────────────────
        if let SemanticNodeData::Alias(inner) = &*source_data {
            let inner = *inner;
            drop(source_data);
            drop(target_data);
            work.push(same_pair(inner, target));
            return;
        }
        if let SemanticNodeData::Alias(inner) = &*target_data {
            let inner = *inner;
            drop(source_data);
            drop(target_data);
            work.push(same_pair(source, inner));
            return;
        }

        // ── Object-spread programs are formulas: distributed and aliased
        //    program operands relate through the SAME protocol as the root
        //    (there is no second matcher). ───────────────────────────────
        if let Some(result) = self.try_object_spread_program_relation(source, target, bindings) {
            results.push(result);
            return;
        }

        let occurrence = self.relation_current_occurrence();
        if let Some(result) = self.try_relation_projection(source, target, bindings, occurrence) {
            results.push(result);
            return;
        }
        if self.relation_reads_unread_marker(source, target) {
            results.push(RelationResult::Unknown);
            return;
        }
        if source == target {
            if let Some(result) = self.try_identical_open_program_result(source, bindings) {
                results.push(result);
                return;
            }
            results.push(assignable(bindings));
            return;
        }

        // ── MergedDecl: reduce to its peer-merged object surface ───────
        if let SemanticNodeData::MergedDecl { contributors } = &*source_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(same_pair(merged, target));
            return;
        }
        if let SemanticNodeData::MergedDecl { contributors } = &*target_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(same_pair(source, merged));
            return;
        }

        // ── A written intersection is the type the checker constructs from
        //    it (`getIntersectionType`): `string & ('a' | 1)` IS `'a'`,
        //    `number & string` IS `never`. A shell whose canonical
        //    intersection differs relates as that type. ──────────────────
        if let Some((source, target)) = self.reduced_authored_relation_pair(source, target) {
            drop(source_data);
            drop(target_data);
            work.push(same_pair(source, target));
            return;
        }

        // ── Top / bottom + error-type wildcard ─────────────────────────
        match (&*source_data, &*target_data) {
            (SemanticNodeData::Opaque(err), _) if err.is_error_type() => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Opaque(err)) if err.is_error_type() => {
                results.push(assignable(bindings));
                return;
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Never), _) => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => {
                results.push(assignable(bindings));
                return;
            }
            (
                SemanticNodeData::Primitive(PrimitiveKind::Any),
                SemanticNodeData::Primitive(PrimitiveKind::Unknown),
            ) if self.strict_subtype_mode() => {
                results.push(RelationResult::NotAssignable);
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => {
                results.push(assignable(bindings));
                return;
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => {
                if self.subtype_mode() {
                    results.push(RelationResult::NotAssignable);
                } else {
                    results.push(assignable(bindings));
                }
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                results.push(RelationResult::NotAssignable);
                return;
            }
            _ => {}
        }

        // ── Strict-family behavioral branch (RI-10): with
        //    `strictNullChecks` OFF, `null` / `undefined` are assignable
        //    to every remaining target (`never` already returned above). ──
        {
            let strict = self
                .dispatch_txn
                .borrow()
                .relation
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT);
            if !strict.strict_null_checks
                && matches!(
                    &*source_data,
                    SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined)
                )
            {
                results.push(assignable(bindings));
                return;
            }
            // `unknown` against an object type or a union: without
            // `strictNullChecks` it relates as `{}` (`null` and `undefined`
            // inhabit every type); with it, it fits a union holding `null`,
            // `undefined` and the empty object type `{}` itself — the
            // checker's unknown union — and nothing narrower
            // (`{ a?: 1 } | null | undefined` does not take it).
            if matches!(
                (&*source_data, &*target_data),
                (
                    SemanticNodeData::Primitive(PrimitiveKind::Unknown),
                    SemanticNodeData::Object(_) | SemanticNodeData::Union(_)
                )
            ) && !self.subtype_mode()
            {
                if !strict.strict_null_checks {
                    // Only an object type takes it: never `object`, a
                    // primitive or `null` / `undefined` arm.
                    let object_arms: Vec<SemanticNodeId> = match &*target_data {
                        SemanticNodeData::Union(members) => members
                            .iter()
                            .copied()
                            .filter(|member| {
                                matches!(
                                    graph.node_data(*member).as_deref(),
                                    Some(SemanticNodeData::Object(_))
                                )
                            })
                            .collect(),
                        _ => vec![target],
                    };
                    if !object_arms.is_empty() {
                        drop(source_data);
                        drop(target_data);
                        let object_target =
                            self.intern_normalized_union_or_intersection(&object_arms, true);
                        work.push(RelateWork::Eval(self.empty_object(), object_target));
                        return;
                    }
                }
                if let SemanticNodeData::Union(members) = &*target_data {
                    let holds = |wanted: &dyn Fn(&SemanticNodeData) -> bool| {
                        members
                            .iter()
                            .any(|member| graph.node_data(*member).as_deref().is_some_and(wanted))
                    };
                    if holds(&|data| {
                        matches!(data, SemanticNodeData::Primitive(PrimitiveKind::Null))
                    }) && holds(&|data| {
                        matches!(data, SemanticNodeData::Primitive(PrimitiveKind::Undefined))
                    }) && holds(&|data| {
                        matches!(data, SemanticNodeData::Object(view)
                            if view.closed().is_empty()
                                && view.call_signatures.is_empty()
                                && view.construct_signatures.is_empty()
                                && view.index_signatures.is_empty())
                    }) {
                        drop(source_data);
                        drop(target_data);
                        results.push(assignable(bindings));
                        return;
                    }
                }
            }
        }

        // ── Type parameters: call-owned sessions bind their exact declared
        // parameter nodes; otherwise Unknown unless identical. Runs BEFORE
        // the deferred-shell arm: a deposit records the source node AS the
        // inference candidate, so a carrier source (`DeclRef` /
        // `InstantiationRef` — an interface-typed argument against a naked
        // binder) deposits verbatim and resolves at the bound's own demand
        // points, exactly like any other candidate. ──────────────────────
        if matches!(&*source_data, SemanticNodeData::TypeParam { .. })
            || matches!(&*target_data, SemanticNodeData::TypeParam { .. })
        {
            if self.relation_session_active() {
                let deposited = match occurrence.variance {
                    VariancePhase::Covariant | VariancePhase::Invariant => {
                        matches!(&*target_data, SemanticNodeData::TypeParam { .. })
                            && self.relation_deposit(target, source, occurrence)
                    }
                    VariancePhase::Contravariant => {
                        matches!(&*source_data, SemanticNodeData::TypeParam { .. })
                            && self.relation_deposit(source, target, occurrence)
                    }
                };
                if deposited {
                    results.push(assignable(bindings));
                    return;
                }
            }
            results.push(RelationResult::Unknown);
            return;
        }

        // ── String literal vs template-literal pattern: decided by the
        //    quasi skeleton where that is provably sound, BEFORE the
        //    deferred gate silently defers the pair. An undecidable pair
        //    still falls through to Unknown. ─────────────────────────────
        // A template literal type relates as the type the checker builds for
        // it — its holes settled and its unions distributed — and a pattern
        // accepts a string literal or template whose slices fit its holes.
        let mut settled_pair = None;
        let mut template_budget_exceeded = false;
        for (side, data) in [(source, &source_data), (target, &target_data)] {
            if settled_pair.is_some() || template_budget_exceeded {
                break;
            }
            if let SemanticNodeData::TemplateLiteral {
                quasis,
                expressions,
            } = &**data
            {
                let reduced = self.reduce_template_literal_nodes(
                    quasis,
                    expressions,
                    ProjectionReductionContext::published(
                        crate::semantic_query::ProjectionMode::Expanded,
                    ),
                );
                if reduced.keyspace_budget_exceeded {
                    template_budget_exceeded = true;
                    continue;
                }
                if reduced.node != side
                    && !matches!(
                        graph.node_data(reduced.node).as_deref(),
                        Some(SemanticNodeData::TemplateLiteral { quasis: q, expressions: e })
                            if q == quasis && e == expressions
                    )
                {
                    settled_pair = Some(if side == source {
                        (reduced.node, target)
                    } else {
                        (source, reduced.node)
                    });
                }
            }
        }
        if template_budget_exceeded {
            results.push(RelationResult::Unknown);
            return;
        }
        if let Some((source, target)) = settled_pair {
            drop(source_data);
            drop(target_data);
            work.push(RelateWork::Eval(source, target));
            return;
        }
        if let Some(accepted) = self.string_mapping_relation(source, target) {
            results.push(if accepted {
                assignable(bindings)
            } else {
                RelationResult::NotAssignable
            });
            return;
        }
        if matches!(&*target_data, SemanticNodeData::TemplateLiteral { .. }) {
            if let Some(accepted) = self.template_pattern_accepts(source, target) {
                results.push(if accepted {
                    assignable(bindings)
                } else {
                    RelationResult::NotAssignable
                });
                return;
            }
        }
        if let Some(result) =
            self.relate_string_literal_and_template(&source_data, &target_data, bindings)
        {
            results.push(result);
            return;
        }
        // ── A template literal type is a string type: below `string`,
        //    and never related to a primitive or literal of another kind
        //    in either direction (`number` is not assignable to
        //    ``item-${string}``), decided before the deferred gate. ────────
        if let Some(result) = template_literal_kind_verdict(&source_data, &target_data) {
            results.push(match result {
                true => assignable(bindings),
                false => RelationResult::NotAssignable,
            });
            return;
        }

        // ── Nominal (`unique symbol`) leaf, BEFORE the deferred gate ───
        //    A preserved `typeof K` carrier is a deferred shell by shape,
        //    but its DECLARING identity is exactly what makes it a type, so
        //    the gate below must not swallow it. Two nominal identities
        //    decide; one nominal side widens to its inhabited type and the
        //    pair re-enters the ordinary lattice.
        //
        //    Gated on the shapes ALREADY read above: a pair with no `typeof`
        //    carrier on either side costs one tag test and never reaches the
        //    declaration lookup, so the nominal axis adds no work to the hot
        //    structural path — the arms below keep their ordering verbatim
        //    for every pair the nominal axis has nothing to say about.
        let mut nominal_defers_gate = false;
        if matches!(
            &*source_data,
            SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_)
        ) || matches!(
            &*target_data,
            SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_)
        ) {
            match self.relation_nominal_leaf(source, target, RelationKind::Assignable, bindings) {
                Some(leaf) => {
                    drop(source_data);
                    drop(target_data);
                    match leaf {
                        NominalLeaf::Decided(result) => results.push(result),
                        NominalLeaf::Retry(source, target) => {
                            work.push(RelateWork::Eval(source, target))
                        }
                    }
                    return;
                }
                // The leaf DECLINED a pair it recognised: exactly one side
                // carries a nominal identity and the other is the composite
                // or inference frame that must run FIRST. That frame lives
                // below the deferred gate, and the nominal carrier is a
                // deferred shell by shape, so this ONE pair would need to
                // step over the gate to reach it.
                //
                // DEFENSE-IN-DEPTH only: under the current arm ordering this
                // bypass is unreachable — `is_deferred` excludes the nominal
                // terminal, and every shape that reaches this arm with a
                // declined leaf arrives non-deferred (composites and
                // inference frames are dispatched by earlier arms, plain
                // deferred shells still defer below). It is kept so a future
                // arm reorder cannot silently swallow a nominal-declined
                // pair into `Unknown` without this gate having to be
                // reinvented; an ordinary deferred shell (including a
                // non-unique `typeof`) still defers.
                None => {
                    nominal_defers_gate =
                        self.node_is_nominal_typeof(source) || self.node_is_nominal_typeof(target);
                }
            }
        }

        // ── The lib `Function` global, BEFORE the deferred gate ────────
        //    Its carrier is a TERMINAL nominal: the `__builtin__` base
        //    names no declaration the `Instantiate` dispatch can serve,
        //    so `unwrap_identity_carrier*` hands the carrier through
        //    verbatim and the gate below would answer `Unknown` for the
        //    two pairs the checker decides by tag alone. Deciding them
        //    here keeps the carrier terminal — no `Instantiate`, no body
        //    it does not have.
        if let Some(result) =
            self.relate_global_function_carrier(&source_data, &target_data, bindings)
        {
            results.push(result);
            return;
        }

        // ── Deferred shells on either side → Unknown ───────────────────
        //    A settled template literal type (every hole a placeholder) is
        //    a terminal string type, not a deferred shell: a union on the
        //    other side distributes over it below.
        let deferred = |node: SemanticNodeId, data: &SemanticNodeData| {
            is_deferred(data)
                && !(matches!(data, SemanticNodeData::TemplateLiteral { .. })
                    && self.template_is_settled(node))
        };
        if !nominal_defers_gate
            && (deferred(source, &source_data) || deferred(target, &target_data))
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Remaining opaque carriers → Unknown ────────────────────────
        if matches!(&*source_data, SemanticNodeData::Opaque(_))
            || matches!(&*target_data, SemanticNodeData::Opaque(_))
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Infer: bind through the active session (RI-6); without one,
        //    defensive Unknown. ──────────────────────────────────────────
        match occurrence.variance {
            VariancePhase::Covariant | VariancePhase::Invariant => {
                if let SemanticNodeData::Infer { .. } = &*target_data {
                    if self.relation_session_active()
                        && self.relation_deposit(target, source, occurrence)
                    {
                        results.push(assignable(bindings));
                    } else {
                        results.push(RelationResult::Unknown);
                    }
                    return;
                }
                if matches!(&*source_data, SemanticNodeData::Infer { .. }) {
                    results.push(RelationResult::Unknown);
                    return;
                }
            }
            VariancePhase::Contravariant => {
                if let SemanticNodeData::Infer { .. } = &*source_data {
                    if self.relation_session_active()
                        && self.relation_deposit(source, target, occurrence)
                    {
                        results.push(assignable(bindings));
                    } else {
                        results.push(RelationResult::Unknown);
                    }
                    return;
                }
                if matches!(&*target_data, SemanticNodeData::Infer { .. }) {
                    results.push(RelationResult::Unknown);
                    return;
                }
            }
        }

        // ── InferRef: an in-scope infer REFERENCE that reached the relate
        //    unsubstituted is undecidable (it is never a deposit target —
        //    only the declaration site binds). ─────────────────────────────
        if matches!(&*source_data, SemanticNodeData::InferRef { .. })
            || matches!(&*target_data, SemanticNodeData::InferRef { .. })
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Union/Intersection distribution ────────────────────────────
        if let SemanticNodeData::Union(members) = &*source_data {
            let members = members.members_arc();
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, RelateWork::Arm, |m| (*m, target));
            return;
        }
        // `boolean` IS the union `true | false` to the checker: against a
        // union target each of its literals relates on its own
        // (`eachTypeRelatedToSomeType`), so `boolean` fits `true | false`.
        if matches!(
            (&*source_data, &*target_data),
            (
                SemanticNodeData::Primitive(PrimitiveKind::Boolean),
                SemanticNodeData::Union(_)
            )
        ) {
            drop(source_data);
            drop(target_data);
            let literals = [true, false].map(|value| {
                graph.intern_node(SemanticNodeData::Literal(LiteralValue::Boolean(value)))
            });
            distribute_and(work, results, &literals, RelateWork::Arm, |literal| {
                (*literal, target)
            });
            return;
        }
        if let SemanticNodeData::Union(members) = &*target_data {
            let members = members.members_arc();
            drop(source_data);
            drop(target_data);
            let alternatives: Vec<_> = members.iter().map(|member| (source, *member)).collect();
            let result = self.relate_union_target_alternatives(
                &alternatives,
                bindings,
                InferPosition::Covariant,
            );
            // An object source no member takes whole may still split on its
            // discriminants (`typeRelatedToDiscriminatedType`).
            let result = match result {
                RelationResult::NotAssignable => self
                    .relate_discriminated_object_source(source, &members, bindings)
                    .unwrap_or(RelationResult::NotAssignable),
                result => result,
            };
            results.push(result);
            return;
        }
        // A weak target — every property optional — takes no source that
        // has members but shares none of its properties, checked on the
        // whole target before an intersection target splits into arms.
        if !intersection_target_arm
            && matches!(
                &*target_data,
                SemanticNodeData::Object(_) | SemanticNodeData::Intersection(_)
            )
            && self.weak_target_rejects(source, target)
        {
            results.push(RelationResult::NotAssignable);
            return;
        }
        // An intersection TARGET is related arm by arm before an
        // intersection source is split, as the checker orders
        // `unionOrIntersectionRelatedTo`: `QA & Z` against `(QA | QB) & Z`
        // needs the whole source against each target arm.
        if let SemanticNodeData::Intersection(members) = &*target_data {
            let members = members.members_arc();
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, RelateWork::TargetArm, |m| {
                (source, *m)
            });
            return;
        }
        if let SemanticNodeData::Intersection(members) = &*source_data {
            let members = members.members_arc();
            let object_target = matches!(&*target_data, SemanticNodeData::Object(_));
            drop(source_data);
            drop(target_data);
            // An intersection infers an index signature only when every
            // member does, and no member alone stands in for it then.
            if self.implicit_index_rejects(source, target) {
                results.push(RelationResult::NotAssignable);
                return;
            }
            let alternatives: Vec<_> = members.iter().map(|member| (*member, target)).collect();
            let result = if self.infers_from_last_source_signature(target) {
                self.relate_overloads_inferring_from_last(
                    &alternatives,
                    bindings,
                    InferPosition::Covariant,
                )
            } else {
                self.relate_pair_alternatives(&alternatives, bindings, InferPosition::Covariant)
            };
            // No member relates alone: the one object the members compose
            // still may, as the checker relates an intersection source
            // structurally after its members — against an object target or
            // a declaration carrier naming one.
            results.push(match result {
                RelationResult::NotAssignable
                    if object_target || self.carrier_names_object(target) =>
                {
                    self.relate_composed_intersection(source, target, bindings)
                }
                other => other,
            });
            return;
        }

        // ── `object` nonprimitive target: every object-like source
        //    (surface / array / tuple / bare signature) is assignable —
        //    the TS `object` semantics. Non-object sources fall through to
        //    the primitive/literal arms below (which reject them). ────────
        if matches!(
            &*target_data,
            SemanticNodeData::Primitive(PrimitiveKind::Object)
        ) && matches!(
            &*source_data,
            SemanticNodeData::Object(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. }
        ) {
            results.push(assignable(bindings));
            return;
        }

        // ── Enum member literals ───────────────────────────────────────
        //    An enum member's literal is NOMINAL: another enum's member
        //    (whatever its value) is not assignable to it. Against any
        //    other target it relates as the value it stands for. Into one,
        //    `number` is assignable when the member is numeric, and a plain
        //    number literal when it has the member's value (or the member's
        //    value is not a constant) — the checker's bit-flag rule. No
        //    other literal is.
        if let SemanticNodeData::EnumLiteral(source_literal) = &*source_data {
            if matches!(&*target_data, SemanticNodeData::EnumLiteral(_)) {
                // Distinct nodes (the identical pair returned above).
                results.push(RelationResult::NotAssignable);
                return;
            }
            let base = source_literal.base;
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &[base], RelateWork::Arm, |base| {
                (*base, target)
            });
            return;
        }
        if let SemanticNodeData::EnumLiteral(target_literal) = &*target_data {
            let target_base = graph.node_data(target_literal.base);
            let numeric_member = matches!(
                target_base.as_deref(),
                Some(
                    SemanticNodeData::Literal(LiteralValue::Number(_))
                        | SemanticNodeData::Primitive(PrimitiveKind::Number)
                )
            );
            match &*source_data {
                SemanticNodeData::Primitive(PrimitiveKind::Number) => {
                    results.push(if numeric_member {
                        assignable(bindings)
                    } else {
                        RelationResult::NotAssignable
                    });
                    return;
                }
                SemanticNodeData::Literal(source_value) => {
                    let related = match (source_value, target_base.as_deref()) {
                        (
                            LiteralValue::Number(_),
                            Some(SemanticNodeData::Literal(target_value)),
                        ) => literals_equal(source_value, target_value),
                        (
                            LiteralValue::Number(_),
                            Some(SemanticNodeData::Primitive(PrimitiveKind::Number)),
                        ) => true,
                        _ => false,
                    };
                    results.push(if related {
                        assignable(bindings)
                    } else {
                        RelationResult::NotAssignable
                    });
                    return;
                }
                // Another primitive relates as it relates to the member's
                // value (`string` is not a `"p"`), and a template literal
                // is never a member.
                SemanticNodeData::Primitive(_) => {
                    let base = target_literal.base;
                    drop(target_base);
                    drop(source_data);
                    drop(target_data);
                    distribute_and(work, results, &[base], RelateWork::Arm, |base| {
                        (source, *base)
                    });
                    return;
                }
                SemanticNodeData::TemplateLiteral { .. } => {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                _ => {}
            }
        }

        // ── Primitives / literals ──────────────────────────────────────
        if let (SemanticNodeData::Primitive(s), SemanticNodeData::Primitive(t)) =
            (&*source_data, &*target_data)
        {
            results.push(relate_primitives(*s, *t, bindings));
            return;
        }
        if let (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim)) =
            (&*source_data, &*target_data)
        {
            results.push(relate_literal_to_primitive(lit, *prim, bindings));
            return;
        }
        if let (SemanticNodeData::Literal(s), SemanticNodeData::Literal(t)) =
            (&*source_data, &*target_data)
        {
            results.push(if literals_equal(s, t) {
                assignable(bindings)
            } else {
                RelationResult::NotAssignable
            });
            return;
        }
        if matches!(&*source_data, SemanticNodeData::Primitive(_))
            && matches!(&*target_data, SemanticNodeData::Literal(_))
        {
            results.push(RelationResult::NotAssignable);
            return;
        }

        // ── Array / Tuple ──────────────────────────────────────────────
        match (&*source_data, &*target_data) {
            (
                SemanticNodeData::Array {
                    element: s_el,
                    readonly: s_ro,
                },
                SemanticNodeData::Array {
                    element: t_el,
                    readonly: t_ro,
                },
            ) => {
                let (s_el, s_ro, t_el, t_ro) = (*s_el, *s_ro, *t_el, *t_ro);
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                // An array relates its element types COVARIANTLY, mutable
                // arrays included: the checker's measured variance of
                // `Array<T>` and `ReadonlyArray<T>` is covariant
                // (`string[]` is assignable to `(string | number)[]`, the
                // reverse is not).
                work.push(RelateWork::Eval(s_el, t_el));
                return;
            }
            (
                SemanticNodeData::Tuple {
                    elements: s_els,
                    readonly: s_ro,
                },
                SemanticNodeData::Tuple {
                    elements: t_els,
                    readonly: t_ro,
                },
            ) => {
                let s_els = Arc::clone(s_els);
                let t_els = Arc::clone(t_els);
                let s_ro = *s_ro;
                let t_ro = *t_ro;
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                // Tuple-inference rest tail (RI-6 in-scope): a trailing
                // `...infer Rest` element binds the remaining source
                // elements as a tuple through the active session.
                let rest_on_source = matches!(occurrence.variance, VariancePhase::Contravariant);
                let session_rest = if self.relation_session_active() {
                    let inference_elements = if rest_on_source { &s_els } else { &t_els };
                    inference_elements.iter().position(|e| {
                        e.rest
                            && matches!(
                                graph.node_data(e.value).as_deref(),
                                Some(SemanticNodeData::Infer { .. })
                            )
                    })
                } else {
                    None
                };
                if session_rest.is_some() {
                    let required_source_len =
                        s_els.iter().filter(|e| !e.optional && !e.rest).count();
                    let required_target_len =
                        t_els.iter().filter(|e| !e.optional && !e.rest).count();
                    let (required_inference_len, required_remainder_len) = if rest_on_source {
                        (required_source_len, required_target_len)
                    } else {
                        (required_target_len, required_source_len)
                    };
                    if required_remainder_len < required_inference_len {
                        results.push(RelationResult::NotAssignable);
                        return;
                    }
                }
                let mut pairs: Vec<(SemanticNodeId, SemanticNodeId)> = Vec::new();
                if let Some(rest_index) = session_rest {
                    let (inference_elements, remainder_elements) = if rest_on_source {
                        (&s_els, &t_els)
                    } else {
                        (&t_els, &s_els)
                    };
                    let infer_element = inference_elements[rest_index].value;
                    let prefix = &inference_elements[..rest_index];
                    let suffix = &inference_elements[rest_index + 1..];
                    let required_prefix_len =
                        prefix.iter().filter(|element| !element.optional).count();
                    let required_suffix_len =
                        suffix.iter().filter(|element| !element.optional).count();
                    if remainder_elements.len() < required_prefix_len + required_suffix_len {
                        results.push(RelationResult::NotAssignable);
                        return;
                    }
                    // Reserve the full fixed suffix when present; when the
                    // concrete tuple is shorter, only optional trailing suffix
                    // slots may disappear. The same rule applies to the fixed
                    // prefix before the variadic capture.
                    let suffix_len = suffix
                        .len()
                        .min(remainder_elements.len().saturating_sub(required_prefix_len));
                    let prefix_len = prefix
                        .len()
                        .min(remainder_elements.len().saturating_sub(suffix_len));
                    if prefix[prefix_len..].iter().any(|element| !element.optional)
                        || suffix[suffix_len..].iter().any(|element| !element.optional)
                    {
                        results.push(RelationResult::NotAssignable);
                        return;
                    }
                    let remainder_end = remainder_elements.len() - suffix_len;
                    let remainder: Vec<crate::semantic_query::TupleElement> = remainder_elements
                        .iter()
                        .skip(prefix_len)
                        .take(remainder_end - prefix_len)
                        .cloned()
                        .collect();
                    let remainder_tuple = graph.intern_node(SemanticNodeData::Tuple {
                        elements: Arc::from(remainder.into_boxed_slice()),
                        readonly: if rest_on_source { t_ro } else { s_ro },
                    });
                    if !self.relation_deposit(infer_element, remainder_tuple, occurrence) {
                        results.push(RelationResult::Unknown);
                        return;
                    }
                    for position in 0..prefix_len {
                        pairs.push((s_els[position].value, t_els[position].value));
                    }
                    for offset in 0..suffix_len {
                        let inference_position = rest_index + 1 + offset;
                        let remainder_position = remainder_elements.len() - suffix_len + offset;
                        if rest_on_source {
                            pairs.push((
                                inference_elements[inference_position].value,
                                remainder_elements[remainder_position].value,
                            ));
                        } else {
                            pairs.push((
                                remainder_elements[remainder_position].value,
                                inference_elements[inference_position].value,
                            ));
                        }
                    }
                } else {
                    let source_slots = self.tuple_slots(&s_els);
                    let target_slots = self.tuple_slots(&t_els);
                    match tuple_position_pairs(&source_slots, true, &target_slots) {
                        Some(positions) => pairs.extend(positions),
                        None => {
                            results.push(RelationResult::NotAssignable);
                            return;
                        }
                    }
                }
                if pairs.is_empty() {
                    results.push(assignable(bindings));
                    return;
                }
                // Tuple assignability is covariant elementwise — mutable
                // tuples included — exactly as TypeScript relates tuples:
                // `[1, 1]` satisfies `[number, number]`, and a generic
                // element (`[T, T]`) binds through the forward deposit. A
                // reverse (`target-element ≤ source-element`) leg would
                // reject literal-element sources and defer inference
                // elements, so no element pair evaluates one. The positions
                // pair up by the checker's arity rules
                // ([`tuple_position_pairs`]).
                let mut forward: Vec<RelateWork> = Vec::with_capacity(pairs.len() + 1);
                for (source_element, target_element) in pairs.iter().copied() {
                    forward.push(RelateWork::Eval(source_element, target_element));
                }
                if pairs.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(pairs.len() as u32));
                }
                push_forward_work(work, forward);
                return;
            }
            // Tuple ≤ Array: the tuple's number index — the union of its
            // element types — relates to the array's element.
            (
                SemanticNodeData::Tuple {
                    elements: s_els,
                    readonly: s_ro,
                },
                SemanticNodeData::Array {
                    element: t_el,
                    readonly: t_ro,
                },
            ) => {
                let s_els = Arc::clone(s_els);
                let s_ro = *s_ro;
                let t_el = *t_el;
                let t_ro = *t_ro;
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                if s_els.is_empty() {
                    results.push(assignable(bindings));
                    return;
                }
                let mut forward: Vec<RelateWork> = Vec::with_capacity(s_els.len() + 1);
                for slot in self.tuple_slots(&s_els) {
                    // A variadic element's number index is its whole
                    // array-like value against the target array.
                    forward.push(match slot.kind {
                        TupleSlotKind::Variadic => RelateWork::Eval(slot.value, target),
                        _ => RelateWork::Eval(slot.type_argument, t_el),
                    });
                }
                if s_els.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(s_els.len() as u32));
                }
                push_forward_work(work, forward);
                return;
            }
            // Array ≤ Tuple: the array is one rest position, paired up by
            // the checker's tuple arity rules.
            (
                SemanticNodeData::Array {
                    element: s_el,
                    readonly: s_ro,
                },
                SemanticNodeData::Tuple {
                    elements: t_els,
                    readonly: t_ro,
                },
            ) => {
                let (s_el, s_ro, t_ro) = (*s_el, *s_ro, *t_ro);
                let t_els = Arc::clone(t_els);
                drop(source_data);
                drop(target_data);
                if !t_ro && s_ro {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                let source_slots = [TupleSlot {
                    kind: TupleSlotKind::Rest,
                    type_argument: s_el,
                    missing_removed: s_el,
                    value: source,
                }];
                let target_slots = self.tuple_slots(&t_els);
                let Some(pairs) = tuple_position_pairs(&source_slots, false, &target_slots) else {
                    results.push(RelationResult::NotAssignable);
                    return;
                };
                if pairs.is_empty() {
                    results.push(assignable(bindings));
                    return;
                }
                let mut forward: Vec<RelateWork> = Vec::with_capacity(pairs.len() + 1);
                for (source_element, target_element) in pairs.iter().copied() {
                    forward.push(RelateWork::Eval(source_element, target_element));
                }
                if pairs.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(pairs.len() as u32));
                }
                push_forward_work(work, forward);
                return;
            }
            _ => {}
        }

        // ── Direct signatures: relate through the SHARED kind-aware
        //    signature surface — same kind relates the parameter/return
        //    structure; a call signature NEVER satisfies a construct
        //    signature or vice versa. ───────────────────────────────────
        if let (
            SemanticNodeData::Signature {
                kind: s_kind,
                return_type: s_ret,
                predicate: s_predicate,
                is_abstract: s_abstract,
                ..
            },
            SemanticNodeData::Signature {
                kind: t_kind,
                return_type: t_ret,
                predicate: t_predicate,
                is_abstract: t_abstract,
                ..
            },
        ) = (&*source_data, &*target_data)
        {
            if s_kind != t_kind {
                drop(source_data);
                drop(target_data);
                results.push(RelationResult::NotAssignable);
                return;
            }
            // An abstract construct signature is not assignable to a
            // non-abstract one.
            if *s_abstract && !*t_abstract {
                drop(source_data);
                drop(target_data);
                results.push(RelationResult::NotAssignable);
                return;
            }
            let kind = *s_kind;
            let source_result = FunctionResult {
                return_type: *s_ret,
                predicate: *s_predicate,
            };
            let target_result = FunctionResult {
                return_type: *t_ret,
                predicate: *t_predicate,
            };
            drop(source_data);
            drop(target_data);
            results.push(self.relate_function(
                source,
                source_result,
                target,
                target_result,
                kind,
                false,
                bindings,
            ));
            return;
        }

        // ── Object structural (with heritage via SurfaceView) ──────────
        if let (SemanticNodeData::Object(s_surf), SemanticNodeData::Object(t_surf)) =
            (&*source_data, &*target_data)
        {
            let s_surf = s_surf.clone();
            let t_surf = t_surf.clone();
            drop(source_data);
            drop(target_data);
            if self.implicit_index_rejects(source, target) {
                results.push(RelationResult::NotAssignable);
                return;
            }
            results.push(self.relate_objects(&s_surf, &t_surf, bindings));
            return;
        }

        // ── Direct signature source vs Object target: every target
        //    signature bucket must be satisfied by a MATCHING-KIND source
        //    signature (the shared kind-aware signature surface — a bare
        //    signature exposes exactly its one bucket). ─────────────────
        if let (SemanticNodeData::Signature { kind, .. }, SemanticNodeData::Object(t_surf)) =
            (&*source_data, &*target_data)
        {
            let s_kind = *kind;
            let t_surf = t_surf.clone();
            drop(source_data);
            drop(target_data);
            results.push(self.relate_signature_source_to_object(source, s_kind, &t_surf, bindings));
            return;
        }

        // ── Object source vs direct signature target: the source's
        //    MATCHING-KIND signature group must satisfy the target
        //    signature (the mirror direction of the surface rule). ──────
        if let (SemanticNodeData::Object(s_surf), SemanticNodeData::Signature { kind, .. }) =
            (&*source_data, &*target_data)
        {
            let t_kind = *kind;
            let s_surf = s_surf.clone();
            drop(source_data);
            drop(target_data);
            results.push(self.relate_object_to_signature(&s_surf, t_kind, target, bindings));
            return;
        }

        // ── A non-nullable primitive against an EMPTY object type (`{}`):
        //    every such value has the empty apparent surface, so it
        //    relates in every relation (`string` is assignable to `{}`
        //    and below it in the strict subtype relation). ─────────────
        if let (
            SemanticNodeData::Primitive(_) | SemanticNodeData::Literal(_),
            SemanticNodeData::Object(t_surf),
        ) = (&*source_data, &*target_data)
        {
            if t_surf.closed().is_empty()
                && !matches!(
                    &*source_data,
                    SemanticNodeData::Primitive(
                        PrimitiveKind::Null
                            | PrimitiveKind::Undefined
                            | PrimitiveKind::Void
                            | PrimitiveKind::Unknown
                    )
                )
            {
                drop(source_data);
                drop(target_data);
                results.push(assignable(bindings));
                return;
            }
        }

        // ── An array, a tuple or a bare signature against an EMPTY object
        //    type (`{}`): every such value is an object, and the empty
        //    surface asks for no member, so it relates in every relation
        //    (`any[]` is assignable to `{}` and below it in the strict
        //    subtype relation). ─────────────────────────────────────────
        if let (
            SemanticNodeData::Array { .. }
            | SemanticNodeData::Tuple { .. }
            | SemanticNodeData::Signature { .. },
            SemanticNodeData::Object(t_surf),
        ) = (&*source_data, &*target_data)
        {
            if t_surf.closed().is_empty() {
                drop(source_data);
                drop(target_data);
                results.push(assignable(bindings));
                return;
            }
        }

        // ── A primitive, a literal, an array or a tuple against an object
        //    type relates through its apparent type
        //    (`structuredTypeRelatedTo` over `getApparentType`): the
        //    library's wrapper interface (`String`, `Number`, `Boolean`,
        //    `Array<T>`, …), read through the one global lookup. ─────────
        if matches!(&*target_data, SemanticNodeData::Object(_)) {
            if let Some((name, args)) = self.apparent_wrapper_of(source) {
                drop(source_data);
                drop(target_data);
                let Some(canonical) = self.relation_wrapper_canonical(target) else {
                    results.push(RelationResult::Unknown);
                    return;
                };
                let wrapper = match self.global_wrapper_surface(name, &args, canonical.as_ref()) {
                    super::apparent_type::GlobalWrapper::Surface(surface) if surface != source => {
                        Some(surface)
                    }
                    super::apparent_type::GlobalWrapper::Absent => None,
                    super::apparent_type::GlobalWrapper::Surface(_)
                    | super::apparent_type::GlobalWrapper::Unsettled => {
                        results.push(RelationResult::Unknown);
                        return;
                    }
                };
                // A tuple's own members — its literal `length` and its
                // positions — stand over the `Array` wrapper, and exist
                // whatever the library declares.
                let tuple = match self.graph().node_data(source).as_deref() {
                    Some(SemanticNodeData::Tuple { elements, readonly }) => {
                        Some(self.tuple_apparent_surface(wrapper, elements, *readonly))
                    }
                    _ => None,
                };
                match tuple.or(wrapper) {
                    Some(apparent) => work.push(same_pair(apparent, target)),
                    None => results.push(RelationResult::NotAssignable),
                }
                return;
            }
        }

        // `object` relates to an object type as its apparent type, the empty
        // object type, which infers no index signature.
        if matches!(
            (&*source_data, &*target_data),
            (
                SemanticNodeData::Primitive(PrimitiveKind::Object),
                SemanticNodeData::Object(_)
            )
        ) {
            drop(source_data);
            drop(target_data);
            if self.implicit_index_rejects(source, target) {
                results.push(RelationResult::NotAssignable);
            } else {
                work.push(same_pair(self.empty_object(), target));
            }
            return;
        }

        // Different concrete kinds → NotAssignable.
        results.push(RelationResult::NotAssignable);
    }

    // ──────────────────────────────────────────────────────────────────
    // Identity-carrier unwrap + the Object-vs-Record arm (unchanged
    // shapes; recursion re-enters the authority)
    // ──────────────────────────────────────────────────────────────────

    /// Instantiate a decl identity carrier into its concrete shape for
    /// relation dispatch through `execute(Instantiate{…})` — the shared
    /// dispatch, never a private instantiation path.
    pub(super) fn unwrap_identity_carrier_one_step(
        &self,
        id: SemanticNodeId,
    ) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let Some(data) = graph.node_data(id) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
            SemanticNodeData::Alias(inner) => return IdentityCarrierUnwrap::Concrete(*inner),
            SemanticNodeData::MergedDecl { contributors } => {
                return IdentityCarrierUnwrap::Concrete(
                    super::walk::reduce_merged_decl_with_graph(graph, contributors),
                );
            }
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => (
                DeclIdentity {
                    canonical_id: Arc::clone(canonical_id),
                    owner: *owner,
                    whole_hash: *whole_hash,
                    decl_name: Arc::clone(name),
                },
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            SemanticNodeData::DeclRef { identity } => (
                identity.clone(),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            ),
            SemanticNodeData::InstantiationRef { base, args } => (base.clone(), Arc::clone(args)),
            _ => return IdentityCarrierUnwrap::Concrete(id),
        };
        drop(data);
        let transit = ProjectionReductionContext::structural_transit();
        // The lib `Function` global's carrier is its own terminal
        // identity: the `__builtin__` base names no declaration the
        // Instantiate dispatch can serve, so the dispatch would MISS
        // non-cacheably and taint every enclosing build's warm admission
        // for a carrier the checker publishes as an ordinary type
        // (`typeof x === "function"` over `object` narrows to
        // `Function`). Relations decide it by identity / tag, never by a
        // body it does not have. Every other base keeps dispatching.
        if self.runtime_nominal_identity(&identity)
            == Some(crate::intrinsic_registry::RuntimeNominal::Function)
        {
            return IdentityCarrierUnwrap::Concrete(id);
        }
        let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
            self.type_slot_for(
                Arc::clone(&identity.canonical_id),
                identity.owner,
                Arc::clone(&identity.decl_name),
            ),
            args,
            self.instantiate_context_for(&identity.canonical_id, transit),
        ));
        // `execute_type_node` IS the shared read boundary: it counts the
        // dispatch intent and folds the build-local taint once, inside
        // `execute_read`. A second fold here would double-count the same
        // read into the parent frame.
        let unwrapped = match self.execute_type_node(key) {
            QueryResult::Value(SemanticQueryOutput {
                value: unwrapped, ..
            }) => unwrapped,
            _ => return IdentityCarrierUnwrap::Unresolvable,
        };
        if unwrapped == id {
            IdentityCarrierUnwrap::Unresolvable
        } else {
            IdentityCarrierUnwrap::Concrete(unwrapped)
        }
    }

    /// Whether a node carries the lib-declared global `Function` — the
    /// `"__builtin__"`-sentinel carrier the bare-name fast path interns
    /// for an UNSHADOWED global reference, never a userland declaration
    /// that happens to share the name.
    pub(super) fn is_global_function_carrier(&self, data: &SemanticNodeData) -> bool {
        let identity = match data {
            SemanticNodeData::DeclRef { identity } => identity,
            SemanticNodeData::InstantiationRef { base, .. } => base,
            _ => return false,
        };
        self.runtime_nominal_identity(identity)
            == Some(crate::intrinsic_registry::RuntimeNominal::Function)
    }

    /// The two relation judgements the lib `Function` global's TERMINAL
    /// carrier has to answer by tag, because it has no declaration body
    /// the `Instantiate` dispatch could expand into an object surface:
    ///
    /// * `Function` IS an object, so it is assignable to the `object`
    ///   non-primitive (and to another spelling of the same global);
    /// * every CALLABLE source inhabits the global `Function` surface —
    ///   a bare signature, or an object surface carrying a call or
    ///   construct signature.
    ///
    /// `None` for every other pair, which then takes the ordinary arms:
    /// the deferred gate's `Unknown` is the honest answer for a carrier
    /// whose structure is genuinely unavailable here.
    fn relate_global_function_carrier(
        &self,
        source_data: &SemanticNodeData,
        target_data: &SemanticNodeData,
        bindings: &[InferBinding],
    ) -> Option<RelationResult> {
        let source_is_function = self.is_global_function_carrier(source_data);
        let target_is_function = self.is_global_function_carrier(target_data);
        if source_is_function && target_is_function {
            return Some(assignable(bindings));
        }
        if source_is_function {
            return matches!(
                target_data,
                SemanticNodeData::Primitive(PrimitiveKind::Object)
            )
            .then(|| assignable(bindings));
        }
        if target_is_function {
            return match source_data {
                SemanticNodeData::Signature { .. } => Some(assignable(bindings)),
                SemanticNodeData::Object(surface) => (!surface.call_signatures.is_empty()
                    || !surface.construct_signatures.is_empty())
                .then(|| assignable(bindings)),
                _ => None,
            };
        }
        None
    }

    /// Fully unwrap an identity carrier for ordinary structural relation.
    pub(crate) fn unwrap_identity_carrier_for_relation(
        &self,
        id: SemanticNodeId,
    ) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let transit = ProjectionReductionContext::structural_transit();
        let mut current = id;
        let mut seen = FxHashSet::default();
        while seen.insert(current) {
            let Some(data) = graph.node_data(current) else {
                return IdentityCarrierUnwrap::Unresolvable;
            };
            let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
                SemanticNodeData::Alias(inner) => {
                    current = *inner;
                    continue;
                }
                // A class expression's instance relates through its surface.
                SemanticNodeData::ClassExpressionInstance { surface, .. } => {
                    let surface = *surface;
                    drop(data);
                    current = self
                        .class_expression_read_surface(current)
                        .unwrap_or(surface);
                    continue;
                }
                SemanticNodeData::MergedDecl { contributors } => {
                    let contributors = Arc::clone(contributors);
                    drop(data);
                    current = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
                    continue;
                }
                SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                    canonical_id,
                    owner,
                    name,
                    whole_hash,
                }) => (
                    DeclIdentity {
                        canonical_id: Arc::clone(canonical_id),
                        owner: *owner,
                        whole_hash: *whole_hash,
                        decl_name: Arc::clone(name),
                    },
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                ),
                // A lowering-time self-reference sentinel (`interface Num {
                // compareTo(other: Num): number }` — the `Num` inside its
                // own body) names its declaration but not its file; the
                // node's origin scope, recorded at intern time, supplies
                // it. Resolving through the shared `Instantiate` dispatch
                // makes the sentinel relate exactly like the `DeclRef` the
                // same reference lowers to from any OTHER file position; a
                // genuinely in-flight cycle re-enters the relation whose
                // identity is already open and closes coinductively. A
                // scope-less sentinel stays concrete (fail-closed Unknown
                // downstream, never a fabricated verdict).
                SemanticNodeData::Opaque(QueryError::RecursiveRef { name, args }) => {
                    let Some(crate::semantic_query::NodeScopeId::File {
                        canonical_id,
                        owner,
                        whole_hash,
                        ..
                    }) = graph.node_scope(current)
                    else {
                        return IdentityCarrierUnwrap::Concrete(current);
                    };
                    (
                        DeclIdentity {
                            canonical_id,
                            owner,
                            whole_hash,
                            decl_name: Arc::clone(name),
                        },
                        Arc::clone(args),
                    )
                }
                SemanticNodeData::DeclRef { identity } => (
                    identity.clone(),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                ),
                SemanticNodeData::InstantiationRef { base, args } => {
                    (base.clone(), Arc::clone(args))
                }
                // An indexed access over a type that is not generic is the
                // property type it reads (`Box['lit']` IS `2`, and an
                // optional `opt?: 3` reads `3 | undefined`): the checker
                // resolves it where it is written, so a relation compares
                // that type. One the deferred evaluator cannot read further
                // (`T['k']` over an open `T`) stays the operand it is.
                SemanticNodeData::IndexedAccess { .. } => {
                    drop(data);
                    let read = self
                        .evaluate_deferred_semantic_node_with_context(current, transit)
                        .into_active_query_build_node(self);
                    if read == current {
                        return IdentityCarrierUnwrap::Concrete(current);
                    }
                    current = read;
                    continue;
                }
                // `keyof` over a type whose keys settle is that key set
                // (`keyof Face` IS `"a" | "b"`), whichever carrier prints it.
                SemanticNodeData::KeyOf { base } => {
                    let base = *base;
                    drop(data);
                    match self.key_set_of(base) {
                        Some(keys) if keys != current => {
                            current = keys;
                            continue;
                        }
                        _ => return IdentityCarrierUnwrap::Concrete(current),
                    }
                }
                // A written intersection is the type the checker constructs
                // from it (`1 & (1 | 2)` IS `1`) when that differs from it.
                SemanticNodeData::Intersection(_) => {
                    drop(data);
                    match self.reduced_authored_relation_pair(current, current) {
                        Some((reduced, _)) if reduced != current => {
                            current = reduced;
                            continue;
                        }
                        _ => return IdentityCarrierUnwrap::Concrete(current),
                    }
                }
                // A homomorphic mapped type over a closed object is the
                // object its keys map to (`Partial<Face>` IS `{ a?: 1 }`):
                // the checker resolves its members, so a relation compares
                // them. Any other mapped type stays the operand it is.
                SemanticNodeData::Mapped { source, mapper } => {
                    let (source, mapper) = (*source, mapper.clone());
                    drop(data);
                    match self
                        .identity_mapped_object_for_relation(source, &mapper)
                        .or_else(|| self.index_key_mapped_object_for_relation(&mapper, transit))
                    {
                        Some(object) if object != current => {
                            current = object;
                            continue;
                        }
                        _ => return IdentityCarrierUnwrap::Concrete(current),
                    }
                }
                _ => return IdentityCarrierUnwrap::Concrete(current),
            };
            drop(data);
            // The lib `Function` global's carrier is its own terminal
            // identity for relations: the `__builtin__` base names no
            // declaration the Instantiate dispatch can serve, so the
            // dispatch would MISS non-cacheably and taint every
            // enclosing build's warm admission for a carrier the
            // checker publishes as an ordinary type (`typeof x ===
            // "function"` over `object` narrows to `Function`). Relations
            // decide it by identity / tag, never by a body it does not
            // have. Every OTHER non-file base (the remaining runtime
            // nominals, the global sentinel) keeps dispatching — its
            // Unresolvable verdict is the measured relation behavior
            // those carriers' consumers pin.
            if self.runtime_nominal_identity(&identity)
                == Some(crate::intrinsic_registry::RuntimeNominal::Function)
            {
                return IdentityCarrierUnwrap::Concrete(current);
            }
            let key = SemanticQueryKey::Instantiate(crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    Arc::clone(&identity.canonical_id),
                    identity.owner,
                    Arc::clone(&identity.decl_name),
                ),
                args,
                self.instantiate_context_for(&identity.canonical_id, transit),
            ));
            // `execute_type_node` IS the shared read boundary (dispatch
            // counters + the universal build-local taint fold); a bespoke
            // re-fold here would double-count the same read.
            let unwrapped = match self.execute_type_node(key) {
                QueryResult::Value(SemanticQueryOutput {
                    value: unwrapped, ..
                }) => {
                    // A nominal `typeof` carrier IS the type it denotes, so
                    // the deferred evaluator must not be handed it: that
                    // evaluator's job is to resolve a shell to its content,
                    // and this carrier's content is the widened `symbol`
                    // primitive, which is exactly the declaring identity the
                    // relation is about to read.
                    if self.node_is_nominal_typeof(unwrapped) {
                        unwrapped
                    } else {
                        self.evaluate_deferred_semantic_node_with_context(unwrapped, transit)
                            .into_active_query_build_node(self)
                    }
                }
                _ => return IdentityCarrierUnwrap::Unresolvable,
            };
            if unwrapped == current {
                return IdentityCarrierUnwrap::Unresolvable;
            }
            current = unwrapped;
        }
        IdentityCarrierUnwrap::Unresolvable
    }

    /// The object an identity mapped type over a closed object surface
    /// names — `Partial<Face>` IS `{ a?: 1 }`, `Required<{ a?: 1 }>` IS
    /// `{ a: 1 }` — for a relation to compare: each key of the mapper's
    /// key space reads the source member of that name, with the mapper's
    /// modifiers applied, exactly as the mapped build's identity arm
    /// produces it.
    ///
    /// `None` (the operand stays the carrier it is, undecided) for an open
    /// or computed mapper, a key remap, a source that is no plain object
    /// surface — a union distributes, an array or tuple maps elementwise,
    /// and signatures and index signatures map on their own — and a key
    /// without a source member. The object is interned for the relation
    /// only: no member is published, so no member edge is recorded.
    fn identity_mapped_object_for_relation(
        &self,
        source: SemanticNodeId,
        mapper: &crate::semantic_query::MapperKey,
    ) -> Option<SemanticNodeId> {
        if mapper.name_remap.is_some()
            || !matches!(mapper.kind, crate::semantic_query::MapperKind::Identity)
            || super::raise::mapped_type_is_open_or_unknown(self, source, mapper)
        {
            return None;
        }
        let IdentityCarrierUnwrap::Concrete(object) =
            self.unwrap_identity_carrier_for_relation(source)
        else {
            return None;
        };
        let source_members: Vec<crate::semantic_query::SurfaceMember> = {
            let data = self.graph().node_data(object)?;
            let SemanticNodeData::Object(view) = &*data else {
                return None;
            };
            if !view.call_signatures.is_empty()
                || !view.construct_signatures.is_empty()
                || !view.index_signatures.is_empty()
            {
                return None;
            }
            view.closed().complete_members().to_vec()
        };
        // A homomorphic key space (`[P in keyof T]`) is the source's public
        // member names, read off the surface just unwrapped; any other is
        // the shared key-domain enumerator's.
        let homomorphic = matches!(
            self.graph().node_data(mapper.key_space).as_deref(),
            Some(SemanticNodeData::KeyOf { base }) if *base == source
        );
        let keys: Vec<crate::semantic_query::PropertyKey> = if homomorphic {
            source_members
                .iter()
                .filter(|member| member.visibility == verter_type_expr::MemberVisibility::Public)
                .map(|member| member.key.cloned_known())
                .collect::<Option<Vec<_>>>()?
        } else {
            self.key_literals_from_keyspace_node(mapper.key_space)?
                .into_iter()
                .map(|key| key.key)
                .collect()
        };
        let mut produced: Vec<crate::semantic_query::SurfaceMember> =
            Vec::with_capacity(keys.len());
        for key in keys {
            if produced
                .iter()
                .any(|member| member.key.cloned_known().as_ref() == Some(&key))
            {
                continue;
            }
            let member = source_members
                .iter()
                .find(|member| member.key.cloned_known().as_ref() == Some(&key))?;
            produced.push(crate::semantic_query::SurfaceMember {
                key: crate::semantic_query::AuthoredPropertyKey::from_known(key.clone()),
                value: member.value,
                optional: match mapper.optionality {
                    crate::semantic_query::OptionalityMod::Add => true,
                    crate::semantic_query::OptionalityMod::Remove => false,
                    crate::semantic_query::OptionalityMod::Keep => member.optional,
                },
                readonly: match mapper.readonly {
                    crate::semantic_query::ReadonlyMod::Add => true,
                    crate::semantic_query::ReadonlyMod::Remove => false,
                    crate::semantic_query::ReadonlyMod::Keep => member.readonly,
                },
                method_kind: None,
                has_implementation_body: false,
                visibility: member.visibility,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                spans: member.spans,
                declaration_origin: member.declaration_origin.clone(),
            });
        }
        Some(
            self.graph()
                .intern_node(SemanticNodeData::Object(SurfaceView::from_members(
                    produced,
                    Some(mapper.key_space),
                ))),
        )
    }

    /// The object a mapped type over a settled key domain that holds an
    /// INDEX key resolves to (the checker's `resolveMappedTypeMembers`):
    /// `Record<string, V>` IS `{ [x: string]: V }`, and
    /// `{ [K in "a" | number]: V }` IS `{ a: V; [x: number]: V }`. Each
    /// constituent of the key domain contributes the member the checker
    /// adds for it: a string literal a property, `string` /
    /// `number` / `symbol` an index signature of that key, each valued by
    /// the template with the binder bound to that key and carrying the
    /// mapper's modifiers (a `?` modifier makes a property optional and an
    /// index signature's value `undefined`-able under `strictNullChecks`).
    ///
    /// `None` (the operand stays the carrier it is, undecided) for a key
    /// remap, a key domain that does not settle to such constituents (a
    /// numeric literal or a template key among them), one without an
    /// index key (an enumerable key domain is the mapped build's own), and
    /// a template the binder substitution cannot read.
    /// The object is interned for the relation only.
    fn index_key_mapped_object_for_relation(
        &self,
        mapper: &crate::semantic_query::MapperKey,
        transit: ProjectionReductionContext,
    ) -> Option<SemanticNodeId> {
        if mapper.name_remap.is_some() {
            return None;
        }
        let graph = self.graph();
        let key_domain = self
            .evaluate_deferred_semantic_node_with_context(mapper.key_space, transit)
            .into_active_query_build_node(self);
        let constituents: Vec<SemanticNodeId> = match graph.node_data(key_domain).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            Some(_) => vec![key_domain],
            None => return None,
        };
        let optional = matches!(
            mapper.optionality,
            crate::semantic_query::OptionalityMod::Add
        );
        let readonly = matches!(mapper.readonly, crate::semantic_query::ReadonlyMod::Add);
        // A `?` modifier makes an index signature's value `undefined`-able
        // under `strictNullChecks` (the checker's `addOptionality`).
        let optional_undefined = optional
            && self
                .dispatch_txn
                .borrow()
                .relation
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT)
                .strict_null_checks;
        let value_for = |key: SemanticNodeId| -> Option<SemanticNodeId> {
            let substituted =
                self.substitute_semantic_type_param(mapper.value_expr, mapper.parameter_node, key);
            let value = self
                .evaluate_deferred_semantic_node_with_context(substituted, transit)
                .into_active_query_build_node(self);
            (!matches!(
                graph.node_data(value).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ))
            .then_some(value)
        };
        let mut members: Vec<crate::semantic_query::SurfaceMember> = Vec::new();
        let mut index_signatures: Vec<crate::semantic_query::IndexSignature> = Vec::new();
        for key in constituents {
            let data = graph.node_data(key)?;
            match &*data {
                SemanticNodeData::Literal(LiteralValue::String(text)) => {
                    let property =
                        crate::semantic_query::PropertyKey::String(Arc::from(text.as_str()));
                    drop(data);
                    members.push(crate::semantic_query::SurfaceMember {
                        key: crate::semantic_query::AuthoredPropertyKey::from_known(property),
                        value: value_for(key)?,
                        optional,
                        readonly,
                        method_kind: None,
                        has_implementation_body: false,
                        visibility: verter_type_expr::MemberVisibility::Public,
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
                        merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
                        spans: verter_type_expr::MemberSpans::default(),
                        declaration_origin: None,
                    });
                }
                SemanticNodeData::Primitive(
                    PrimitiveKind::String | PrimitiveKind::Number | PrimitiveKind::Symbol,
                ) => {
                    drop(data);
                    let mut value = value_for(key)?;
                    if optional_undefined {
                        let undefined = graph
                            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                        value =
                            self.intern_normalized_union_or_intersection(&[value, undefined], true);
                    }
                    index_signatures.push(crate::semantic_query::IndexSignature {
                        key_type: key,
                        value_type: value,
                        readonly,
                        spans: verter_type_expr::IndexSignatureSpans::default(),
                        declaration_origin: None,
                    });
                }
                _ => return None,
            }
        }
        if index_signatures.is_empty() {
            return None;
        }
        let entries: Vec<crate::semantic_query::SurfaceEntry> = members
            .into_iter()
            .map(crate::semantic_query::SurfaceEntry::Member)
            .chain(
                index_signatures
                    .into_iter()
                    .map(crate::semantic_query::SurfaceEntry::IndexSignature),
            )
            .collect();
        Some(
            graph.intern_node(SemanticNodeData::Object(SurfaceView::from_entries(
                entries, None, true,
            ))),
        )
    }

    /// Source-side declaration identity carrier with Object body against
    /// a target-side Record-shaped Object.
    fn try_object_vs_record_relation(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let source_data = graph.node_data(source)?;
        let identity = match &*source_data {
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder {
                canonical_id,
                owner,
                name,
                whole_hash,
            }) => Some(DeclIdentity {
                canonical_id: Arc::clone(canonical_id),
                owner: *owner,
                whole_hash: *whole_hash,
                decl_name: Arc::clone(name),
            }),
            SemanticNodeData::DeclRef { identity } => Some(identity.clone()),
            SemanticNodeData::Object(_) => None,
            _ => return None,
        };
        drop(source_data);

        let target_record = self.record_target_shape(target)?;
        let transit = ProjectionReductionContext::structural_transit();
        let unwrapped = match identity {
            None => source,
            Some(identity) => match self.execute_type_node(SemanticQueryKey::Instantiate(
                crate::semantic_query::InstantiateKey::new(
                    self.type_slot_for(
                        Arc::clone(&identity.canonical_id),
                        identity.owner,
                        Arc::clone(&identity.decl_name),
                    ),
                    Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                    self.instantiate_context_for(&identity.canonical_id, transit),
                ),
            )) {
                QueryResult::Value(SemanticQueryOutput { value: id, .. }) => self
                    .evaluate_deferred_semantic_node_with_context(id, transit)
                    .into_active_query_build_node(self),
                _ => return Some(RelationResult::Unknown),
            },
        };
        let source_view = match graph.node_data(unwrapped).as_deref() {
            Some(SemanticNodeData::Object(view)) => view.clone(),
            _ => return None,
        };

        Some(match target_record {
            RecordTargetShape::LiteralKey(target_view) => {
                self.relate_objects(&source_view, &target_view, bindings)
            }
            RecordTargetShape::GenericKey {
                key_type,
                value_type,
            } => self.relate_object_as_record(&source_view, key_type, value_type, bindings),
        })
    }

    /// Returns `Some(RecordTargetShape)` when `target` normalises to a
    /// Record-shaped `Object(SurfaceView)`.
    fn record_target_shape(&self, target: SemanticNodeId) -> Option<RecordTargetShape> {
        let graph = self.graph();
        let oracle_demand = ProjectionReductionContext::structural_transit_with_mode(
            crate::semantic_query::ProjectionMode::Navigate,
        );
        let mut normalised = self
            .evaluate_deferred_semantic_node_with_context(target, oracle_demand)
            .into_active_query_build_node(self);
        if let Some(SemanticNodeData::InstantiationRef { base, args }) =
            graph.node_data(normalised).as_deref()
        {
            let owner_canonical = Arc::clone(&base.canonical_id);
            let slot = self.type_slot_for(
                Arc::clone(&base.canonical_id),
                base.owner,
                Arc::clone(&base.decl_name),
            );
            let args: Arc<[SemanticNodeId]> = Arc::from(
                args.iter()
                    .map(|arg| {
                        self.evaluate_deferred_semantic_node_with_context(*arg, oracle_demand)
                            .into_active_query_build_node(self)
                    })
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            );
            if let QueryResult::Value(SemanticQueryOutput { value: id, .. }) = self
                .execute_type_node(SemanticQueryKey::Instantiate(
                    crate::semantic_query::InstantiateKey::new(
                        slot,
                        args,
                        self.instantiate_context_for(&owner_canonical, oracle_demand),
                    ),
                ))
            {
                normalised = self
                    .evaluate_deferred_semantic_node_with_context(id, oracle_demand)
                    .into_active_query_build_node(self);
            }
        }
        if let Some(SemanticNodeData::Mapped { mapper, .. }) =
            graph.node_data(normalised).as_deref()
        {
            if mapper.name_remap.is_none()
                && matches!(
                    mapper.optionality,
                    crate::semantic_query::OptionalityMod::Keep
                )
                && !self.subtree_references_node(mapper.value_expr, mapper.parameter_node)
            {
                let key_space = mapper.key_space;
                let value_type = mapper.value_expr;
                let key_type = self
                    .evaluate_deferred_semantic_node_with_context(key_space, oracle_demand)
                    .into_active_query_build_node(self);
                return Some(RecordTargetShape::GenericKey {
                    key_type,
                    value_type,
                });
            }
        }
        let data = graph.node_data(normalised)?;
        match &*data {
            SemanticNodeData::Object(view)
                if view.call_signatures.is_empty() && view.construct_signatures.is_empty() =>
            {
                let closed = view.closed();
                let members = closed.complete_members();
                if members.is_empty() && view.index_signatures.len() == 1 {
                    let ix = &view.index_signatures[0];
                    Some(RecordTargetShape::GenericKey {
                        key_type: ix.key_type,
                        value_type: ix.value_type,
                    })
                } else if !members.is_empty() && view.index_signatures.is_empty() {
                    Some(RecordTargetShape::LiteralKey(view.clone()))
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Relate an Object surface against a Record<K, V> target.
    fn relate_object_as_record(
        &self,
        source_view: &SurfaceView,
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
        let key_data = match graph.node_data(key_type) {
            Some(d) => d,
            None => return RelationResult::Unknown,
        };
        let required_keys: Vec<Arc<str>> = match &*key_data {
            SemanticNodeData::Literal(LiteralValue::String(s)) => vec![Arc::from(s.as_str())],
            SemanticNodeData::Literal(LiteralValue::Number(n)) => {
                vec![Arc::from(super::build::js_number_to_string(*n).as_str())]
            }
            SemanticNodeData::Union(members) => {
                let members = members.members_arc();
                drop(key_data);
                let mut keys: Vec<Arc<str>> = Vec::with_capacity(members.len());
                for member in members.iter() {
                    match graph.node_data(*member).as_deref() {
                        Some(SemanticNodeData::Literal(LiteralValue::String(s))) => {
                            keys.push(Arc::from(s.as_str()));
                        }
                        Some(SemanticNodeData::Literal(LiteralValue::Number(n))) => {
                            keys.push(Arc::from(super::build::js_number_to_string(*n).as_str()));
                        }
                        _ => return RelationResult::Unknown,
                    }
                }
                keys
            }
            SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Number) => {
                drop(key_data);
                // The strict subtype relation infers no index signature for
                // a source that declares none, an object literal's own type
                // aside (the checker's `typeRelatedToIndexInfo`).
                let applicable_source_index = source_view
                    .index_signatures
                    .iter()
                    .any(|s_index| index_domains_overlap(graph, s_index.key_type, key_type));
                if self.strict_subtype_mode()
                    && !applicable_source_index
                    && !surface_is_object_literal(source_view)
                {
                    return RelationResult::NotAssignable;
                }
                let mut acc = RelationResult::Assignable {
                    bindings: Arc::from(Vec::new().into_boxed_slice()),
                };
                for s_index in source_view.index_signatures.iter() {
                    if !index_domains_overlap(graph, s_index.key_type, key_type) {
                        continue;
                    }
                    let r = self.relate_member(
                        s_index.value_type,
                        value_type,
                        bindings,
                        InferPosition::Covariant,
                    );
                    acc = result_and(acc, r);
                    if matches!(acc, RelationResult::NotAssignable) {
                        return RelationResult::NotAssignable;
                    }
                }
                for member in source_view.positive_members().iter() {
                    let r = self.relate_member(
                        member.value,
                        value_type,
                        bindings,
                        InferPosition::Covariant,
                    );
                    acc = result_and(acc, r);
                    if matches!(acc, RelationResult::NotAssignable) {
                        return RelationResult::NotAssignable;
                    }
                }
                return acc;
            }
            _ => return RelationResult::Unknown,
        };

        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for key in required_keys {
            let member = match source_view.project_string_key(key.as_ref()) {
                crate::semantic_query::SurfaceKeyProjection::Exact(member) => member,
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                    return RelationResult::NotAssignable;
                }
            };
            let r =
                self.relate_member(member.value, value_type, bindings, InferPosition::Covariant);
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        acc
    }

    // ──────────────────────────────────────────────────────────────────
    // Object / function structural predicates (the retired
    // `relation_predicates` recursion sites — now methods re-entering the
    // full-key authority)
    // ──────────────────────────────────────────────────────────────────

    /// The positions of a tuple in the checker's element vocabulary
    /// ([`TupleSlot`]): a rest element over an array is a rest position
    /// whose type argument is the array's element, any other rest element
    /// is variadic, and an optional element's type argument carries
    /// `undefined` under `strictNullChecks` as the checker's does.
    fn tuple_slots(&self, elements: &[crate::semantic_query::TupleElement]) -> Vec<TupleSlot> {
        let graph = self.graph();
        // The relation root's snapshot of the strict family, as every
        // other relation rule reads it.
        let strict = self
            .dispatch_txn
            .borrow()
            .relation
            .strict
            .unwrap_or(StrictFamilyConfig::TS_STRICT);
        let strict_null_checks = strict.strict_null_checks;
        let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
        elements
            .iter()
            .map(|element| {
                if element.rest {
                    let mut current = element.value;
                    // bounded-loop: at most 8 transparent Alias hops.
                    for _ in 0..8 {
                        match graph.node_data(current).as_deref() {
                            Some(SemanticNodeData::Alias(inner)) => current = *inner,
                            _ => break,
                        }
                    }
                    return match graph.node_data(current).as_deref() {
                        Some(SemanticNodeData::Array { element: inner, .. }) => TupleSlot {
                            kind: TupleSlotKind::Rest,
                            type_argument: *inner,
                            missing_removed: *inner,
                            value: element.value,
                        },
                        _ => TupleSlot {
                            kind: TupleSlotKind::Variadic,
                            type_argument: element.value,
                            missing_removed: element.value,
                            value: element.value,
                        },
                    };
                }
                if element.optional {
                    let type_argument = if strict_null_checks {
                        self.intern_normalized_union(
                            &[element.value, undefined],
                            crate::semantic_query::NullabilityPolicy::Strict,
                        )
                    } else {
                        element.value
                    };
                    let missing_removed = if strict.exact_optional_property_types {
                        element.value
                    } else {
                        type_argument
                    };
                    return TupleSlot {
                        kind: TupleSlotKind::Optional,
                        type_argument,
                        missing_removed,
                        value: element.value,
                    };
                }
                TupleSlot {
                    kind: TupleSlotKind::Required,
                    type_argument: element.value,
                    missing_removed: element.value,
                    value: element.value,
                }
            })
            .collect()
    }

    /// Relate two object `SurfaceView`s structurally. Every required
    /// target member must be satisfied by a matching source member (or an
    /// applicable source index signature).
    pub(super) fn relate_objects(
        &self,
        source: &SurfaceView,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let closed_target = target.closed();

        // The subtype relations admit into an object LITERAL's type no
        // source with a property the literal lacks, unless that property
        // is `undefined` (the checker's `propertiesRelatedTo`): nested
        // literals compare as the top-level ones do, so `{ o: { a: 1, b: 2
        // } }` is not below `{ o: { a: 1 } }`.
        if self.subtype_mode() && surface_is_object_literal(target) {
            let lists_unknown = source.positive_members().iter().any(|member| {
                let Some(key) = member.key.cloned_known() else {
                    return false;
                };
                matches!(
                    target.project_known_key(&key),
                    crate::semantic_query::SurfaceKeyProjection::AbsentProven
                ) && !matches!(
                    self.graph().node_data(member.value).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
                )
            });
            if lists_unknown {
                return RelationResult::NotAssignable;
            }
        }

        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        // An accessor is ONE property on either side — its getter's return,
        // else its setter's parameter — related once per key, never as the
        // accessor functions.
        let graph = self.graph();
        let mut accessor_keys: Vec<crate::semantic_query::PropertyKey> = Vec::new();
        for t_prop in closed_target.complete_members() {
            let Some(target_key) = t_prop.key.cloned_known() else {
                return RelationResult::Unknown;
            };
            let target_property;
            let t_prop = match t_prop.method_kind {
                Some(
                    verter_type_expr::ObjectMethodKind::Get
                    | verter_type_expr::ObjectMethodKind::Set,
                ) => {
                    if accessor_keys.contains(&target_key) {
                        continue;
                    }
                    accessor_keys.push(target_key.clone());
                    match target
                        .project_known_key_accessor(&target_key)
                        .and_then(|accessor| accessor.property_member(graph))
                    {
                        Some(property) => {
                            target_property = property;
                            &target_property
                        }
                        None => return RelationResult::Unknown,
                    }
                }
                _ => t_prop,
            };
            if let Some(accessor) = source.project_known_key_accessor(&target_key) {
                let prop_result = match accessor.property_member(graph) {
                    Some(source_member) => {
                        self.relate_property_pair(&source_member, t_prop, bindings)
                    }
                    None => RelationResult::Unknown,
                };
                acc = result_and(acc, prop_result);
                if matches!(acc, RelationResult::NotAssignable) {
                    return RelationResult::NotAssignable;
                }
                continue;
            }
            let prop_result = match source.project_known_key(&target_key) {
                crate::semantic_query::SurfaceKeyProjection::Exact(source_member) => {
                    self.relate_property_pair(source_member, t_prop, bindings)
                }
                // The subtype relations require every target property of a
                // source that is not an object literal, the optional ones
                // included, and no index signature stands in for it
                // (the checker's `requireOptionalProperties`): `{ x: string }`
                // is not a subtype of `{ x: string; y?: number }`.
                crate::semantic_query::SurfaceKeyProjection::AbsentProven
                    if self.subtype_mode()
                        && !(t_prop.optional && surface_is_object_literal(source)) =>
                {
                    RelationResult::NotAssignable
                }
                // A property the source does not declare: a required target
                // property is unmatched, whatever index signature the source
                // carries, and an optional one is satisfied without relating
                // any index signature to it (the checker's
                // `getUnmatchedProperty`: `{ [k: string]: string }` is not
                // assignable to `{ a: string }`, and `{ [k: string]: number
                // }` is assignable to `{ a?: string }`).
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                    if let Some(apparent) =
                        self.relate_apparent_function_member(source, &target_key, t_prop, bindings)
                    {
                        apparent
                    } else if let Some(apparent) =
                        self.relate_apparent_object_member(&target_key, t_prop, bindings)
                    {
                        apparent
                    } else if t_prop.optional {
                        assignable(bindings)
                    } else {
                        RelationResult::NotAssignable
                    }
                }
            };
            acc = result_and(acc, prop_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for t_index in target.index_signatures.iter() {
            let index_result = self.relate_target_index_signature(source, t_index, bindings);
            acc = result_and(acc, index_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for t_sig in target.call_signatures.iter() {
            let signature_result =
                self.relate_signature_alternatives(&source.call_signatures, *t_sig, bindings);
            acc = result_and(acc, signature_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        if !self.construct_visibilities_compatible(
            &source.construct_signatures,
            &target.construct_signatures,
        ) {
            return RelationResult::NotAssignable;
        }
        for t_sig in target.construct_signatures.iter() {
            let signature_result =
                self.relate_signature_alternatives(&source.construct_signatures, *t_sig, bindings);
            acc = result_and(acc, signature_result);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        acc
    }

    /// Resolve a member value that is a `RecursiveRef` control sentinel to
    /// its declaration surface through the SHARED dispatch
    /// (`execute(Instantiate)` on the `(declaration_origin, name)` slot —
    /// never a private resolution path). The unfolding cannot spiral: the
    /// resolved referent re-enters [`Self::execute_relate`], whose full
    /// identity is already in flight for a genuine cycle, so the reentry
    /// intercept turns the unfold into the coinductive back-edge
    /// (`Assumed`). An unresolvable referent keeps the sentinel (which
    /// stays `Unknown` — fail-closed, never a fabricated verdict).
    fn resolve_recursive_member_value(
        &self,
        value: SemanticNodeId,
        origin: Option<&Arc<str>>,
    ) -> SemanticNodeId {
        let graph = self.graph();
        let name = match graph.node_data(value).as_deref() {
            Some(SemanticNodeData::Opaque(QueryError::RecursiveRef { name, .. })) => {
                Arc::clone(name)
            }
            _ => return value,
        };
        let Some(origin) = origin else {
            return value;
        };
        let transit = ProjectionReductionContext::structural_transit();
        match self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    Arc::clone(origin),
                    verter_type_expr::TopLevelOwnerId::ordinary_file(),
                    name,
                ),
                Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
                self.instantiate_context_for(origin, transit),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput {
                value: resolved, ..
            }) => {
                let resolved = self
                    .evaluate_deferred_semantic_node_with_context(resolved, transit)
                    .into_active_query_build_node(self);
                match graph.node_data(resolved).as_deref() {
                    // A referent that failed to materialise keeps the
                    // sentinel (Unknown), never a half-resolved carrier.
                    Some(SemanticNodeData::Opaque(_)) | None => value,
                    _ => resolved,
                }
            }
            _ => value,
        }
    }

    pub(super) fn relate_property_pair(
        &self,
        source: &crate::semantic_query::SurfaceMember,
        target: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // A property `readonly` modifier is not part of assignability in
        // either direction: `{ readonly a: T }` and `{ a: T }` relate both
        // ways. The `readonly` gates that DO apply live on the array and
        // tuple arms (a readonly array / tuple is not assignable to a
        // mutable one) and on index signatures — distinct rules over
        // distinct carriers, not this member pair.
        //
        // Optional-to-required: a source member that may be ABSENT never
        // satisfies a required target member, whatever the value types and
        // under both `strictNullChecks` settings — only the comparable
        // relation skips the rule (the checker's `propertyRelatedTo`;
        // measured on 7.0.2: `{ a?: string }` is not assignable to
        // `{ a: string }`, `{ a: string | undefined }` or `{ a: any }`,
        // with `strictNullChecks` on or off). The strict subtype relation
        // also refuses a `readonly` member below a mutable one.
        if let Some(decided) = self.property_accessibility_relation(source, target) {
            return decided;
        }
        if !target.optional
            && source.optional
            && self.current_relation_kind() != RelationKind::Comparable
        {
            return RelationResult::NotAssignable;
        }
        if self.strict_subtype_mode() && source.readonly && !target.readonly {
            return RelationResult::NotAssignable;
        }
        // A `RecursiveRef` member value rebinds to its declaration surface
        // through the shared dispatch so a genuinely recursive type
        // re-enters the authority (the coinductive back-edge) instead of
        // dead-ending on the sentinel.
        let source_value =
            self.resolve_recursive_member_value(source.value, source.declaration_origin.as_ref());
        let target_value =
            self.resolve_recursive_member_value(target.value, target.declaration_origin.as_ref());
        // An optional target property's type includes `undefined` under
        // `strictNullChecks` (the checker's `addOptionality`), unless
        // `exactOptionalPropertyTypes` keeps it to the declared type:
        // `{ a: string | undefined }` fits `{ a?: string }`.
        let strict = self
            .dispatch_txn
            .borrow()
            .relation
            .strict
            .unwrap_or(StrictFamilyConfig::TS_STRICT);
        let target_value = if target.optional
            && strict.strict_null_checks
            && !strict.exact_optional_property_types
        {
            let undefined = self
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
            self.intern_normalized_union_or_intersection(&[target_value, undefined], true)
        } else {
            target_value
        };
        // A target METHOD relates its parameters bivariantly under
        // `strictFunctionTypes` too: the checker's `strictVariance` is
        // decided by the target signature's declaration kind, so a
        // function-typed property with a narrower parameter fits a method.
        if target.method_kind == Some(verter_type_expr::ObjectMethodKind::Method) {
            if let Some(result) =
                self.relate_method_signatures(source_value, target_value, bindings)
            {
                return result;
            }
        }
        self.relate_member(
            source_value,
            target_value,
            bindings,
            InferPosition::Covariant,
        )
    }

    /// Relate a member value to a target METHOD's value when both are one
    /// call signature: parameters bivariantly
    /// ([`Self::relate_function`]'s `method_target`). `None` for any other
    /// pair of values (an overload group, a carrier), which relates as any
    /// member does.
    fn relate_method_signatures(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let result_of = |node: SemanticNodeId| match graph.node_data(node).as_deref() {
            Some(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Call,
                return_type,
                predicate,
                ..
            }) => Some(FunctionResult {
                return_type: *return_type,
                predicate: *predicate,
            }),
            _ => None,
        };
        let source_result = result_of(source)?;
        let target_result = result_of(target)?;
        Some(self.relate_function(
            source,
            source_result,
            target,
            target_result,
            crate::semantic_query::SignatureKind::Call,
            true,
            bindings,
        ))
    }

    /// The accessibility half of the checker's `propertyRelatedTo`, decided
    /// before the property types: a `private` property on either side
    /// relates only to the same declaration; a `protected` target property
    /// only to a property declared in a class derived from the target
    /// property's declaring class (`isValidOverrideOf` — the same
    /// declaration included); a `protected` source property never to a
    /// public one. `None` when the pair passes and its types decide;
    /// `Unknown` when a declaration or a declaring class the rule needs is
    /// not read from the graph.
    fn property_accessibility_relation(
        &self,
        source: &crate::semantic_query::SurfaceMember,
        target: &crate::semantic_query::SurfaceMember,
    ) -> Option<RelationResult> {
        use verter_type_expr::MemberVisibility;
        let same_declaration = || -> Option<bool> {
            let (Some(source_span), Some(target_span)) =
                (source.spans.declaration, target.spans.declaration)
            else {
                return None;
            };
            let (Some(source_file), Some(target_file)) = (
                source.declaration_origin.as_deref(),
                target.declaration_origin.as_deref(),
            ) else {
                return None;
            };
            Some(source_file == target_file && source_span == target_span)
        };
        if source.visibility == MemberVisibility::Private
            || target.visibility == MemberVisibility::Private
        {
            return match same_declaration() {
                Some(true) => None,
                Some(false) => Some(RelationResult::NotAssignable),
                None => Some(RelationResult::Unknown),
            };
        }
        if target.visibility == MemberVisibility::Protected {
            if same_declaration() == Some(true) {
                return None;
            }
            let MemberOwner::Class(target_class) = self.member_owner(target) else {
                return Some(RelationResult::Unknown);
            };
            return match self.member_owner(source) {
                MemberOwner::NotAClass => Some(RelationResult::NotAssignable),
                MemberOwner::Undecided => Some(RelationResult::Unknown),
                MemberOwner::Class(source_class) => {
                    match self.class_derives_from(&source_class, &target_class) {
                        Some(true) => None,
                        Some(false) => Some(RelationResult::NotAssignable),
                        None => Some(RelationResult::Unknown),
                    }
                }
            };
        }
        if source.visibility == MemberVisibility::Protected {
            return Some(RelationResult::NotAssignable);
        }
        None
    }

    /// The accessibility of a property pair the comparable relation reads:
    /// comparability holds when either direction relates, so the pair
    /// proves the types disjoint (`NotAssignable`) only when
    /// [`Self::property_accessibility_relation`] refuses it both ways.
    /// `None` when a direction admits the pair and its types decide;
    /// `Unknown` when a direction is undecided and none admits it.
    fn comparable_property_accessibility(
        &self,
        a: &crate::semantic_query::SurfaceMember,
        b: &crate::semantic_query::SurfaceMember,
    ) -> Option<RelationResult> {
        let forward = self.property_accessibility_relation(a, b)?;
        let backward = self.property_accessibility_relation(b, a)?;
        Some(
            if matches!(forward, RelationResult::NotAssignable)
                && matches!(backward, RelationResult::NotAssignable)
            {
                RelationResult::NotAssignable
            } else {
                RelationResult::Unknown
            },
        )
    }

    /// The class that declares `member` — the class node whose body
    /// declares it directly, read from the file's syntactic class index —
    /// or no class when a file-scope interface or type alias declares it.
    /// A file-scope class is named by its declaration identity, which the
    /// class-heritage ancestry authority reads; any other class (a class
    /// expression, a class declared inside a body) by its node. A member
    /// whose declaration neither reads is undecided.
    fn member_owner(&self, member: &crate::semantic_query::SurfaceMember) -> MemberOwner {
        use verter_semantic::analysis::type_eval::TypeDeclKind;
        let (Some(span), Some(file)) =
            (member.spans.declaration, member.declaration_origin.as_ref())
        else {
            return MemberOwner::Undecided;
        };
        let Some(indexed) = self
            .ctx
            .ensure_indexed_ready_serve(file.as_ref())
            .map(|serve| serve.indexed)
        else {
            return MemberOwner::Undecided;
        };
        let decl_bodies = indexed.shallow_state.decl_bodies();
        let headers = decl_bodies.header_index();
        let classes = decl_bodies.function_program_index();
        if let Some(class) = classes.class_declaring_member(span) {
            // A file-scope class declaration is the outermost class inside
            // its header.
            let file_scope = (!class.expression && !classes.class_encloses(class.span))
                .then(|| {
                    headers.type_headers.iter().find(|(_, header)| {
                        header.kind == TypeDeclKind::Class
                            && header.span.start <= class.span.start
                            && class.span.end <= header.span.end
                    })
                })
                .flatten();
            return MemberOwner::Class(match file_scope {
                Some((key, _)) => ClassOwner::Declared(crate::semantic_query::DeclIdentity {
                    canonical_id: Arc::clone(file),
                    owner: key.owner,
                    whole_hash: indexed.whole_hash,
                    decl_name: Arc::clone(&key.name),
                }),
                None => ClassOwner::Syntactic {
                    file: Arc::clone(file),
                    span: class.span,
                    has_heritage: class.has_heritage,
                },
            });
        }
        let within_a_type = headers.type_headers.iter().any(|(_, header)| {
            header.kind != TypeDeclKind::Class
                && header.span.start <= span.start
                && span.end <= header.span.end
        });
        if within_a_type {
            MemberOwner::NotAClass
        } else {
            MemberOwner::Undecided
        }
    }

    /// Whether the class `source` derives from the class `target`, the
    /// same class included — the checker's `isValidOverrideOf` over the
    /// members' declaring classes. A file-scope class reads its transitive
    /// ancestry from the class-heritage ancestry authority, whose decided
    /// chain names file-scope classes only; a class with no `extends`
    /// clause derives from itself alone. `None` when the chain is not
    /// decided, or when a class other than a file-scope one has a
    /// heritage clause.
    ///
    /// The ancestry walk reads each declaration file through an accessor
    /// that records no fact of its own, so the files it observed join the
    /// relation build's self-roots here: an edit to an ancestor's file
    /// retracts the answer.
    fn class_derives_from(&self, source: &ClassOwner, target: &ClassOwner) -> Option<bool> {
        match (source, target) {
            (ClassOwner::Declared(source), ClassOwner::Declared(target)) => {
                if super::build::same_class_identity(source, target) {
                    return Some(true);
                }
                let ancestry = self.class_heritage_ancestry(source);
                self.deposit_operand_self_roots(&ancestry.observed);
                if ancestry
                    .ancestors
                    .iter()
                    .any(|ancestor| super::build::same_class_identity(ancestor, target))
                {
                    Some(true)
                } else {
                    ancestry.decided.then_some(false)
                }
            }
            (ClassOwner::Declared(source), ClassOwner::Syntactic { .. }) => {
                let ancestry = self.class_heritage_ancestry(source);
                self.deposit_operand_self_roots(&ancestry.observed);
                ancestry.decided.then_some(false)
            }
            (
                ClassOwner::Syntactic {
                    file,
                    span,
                    has_heritage,
                },
                target,
            ) => {
                if let ClassOwner::Syntactic {
                    file: target_file,
                    span: target_span,
                    ..
                } = target
                {
                    if file == target_file && span == target_span {
                        return Some(true);
                    }
                }
                (!has_heritage).then_some(false)
            }
        }
    }

    /// A property the source declares through its apparent `Function` type
    /// — an object with call or construct signatures reads the members the
    /// checker's `getPropertyOfType` adds after its own (`Function`'s
    /// `prototype: any`, `length`, `name`, …; `(new () => Foo) extends {
    /// prototype: Bar }` holds). `None` when the source has no signature or
    /// its apparent type does not declare the key.
    fn relate_apparent_function_member(
        &self,
        source: &SurfaceView,
        key: &crate::semantic_query::PropertyKey,
        target: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        if source.call_signatures.is_empty() && source.construct_signatures.is_empty() {
            return None;
        }
        let node = self
            .graph()
            .intern_node(SemanticNodeData::Object(source.clone()));
        let value = self.apparent_function_member_value(node, key)?;
        Some(self.relate_member(value, target.value, bindings, InferPosition::Covariant))
    }

    /// A property an object type does not declare, read off the global
    /// `Object` interface its apparent type carries — the members the
    /// checker's `getPropertyOfType` adds after an object type's own
    /// (`{}` is below `Object`: `toString`, `valueOf`, …). `None` when the
    /// project's `Object` does not declare the key or does not settle.
    fn relate_apparent_object_member(
        &self,
        key: &crate::semantic_query::PropertyKey,
        target: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let canonical = self
            .wrapper_demand_canonical()
            .or_else(|| target.declaration_origin.clone())?;
        let super::apparent_type::GlobalWrapper::Surface(surface) =
            self.global_wrapper_surface("Object", &[], canonical.as_ref())
        else {
            return None;
        };
        let value = match &*self.graph().node_data(surface)? {
            SemanticNodeData::Object(view) => match view.project_known_key(key) {
                crate::semantic_query::SurfaceKeyProjection::Exact(member) => member.value,
                crate::semantic_query::SurfaceKeyProjection::AbsentProven => return None,
            },
            _ => return None,
        };
        Some(self.relate_member(value, target.value, bindings, InferPosition::Covariant))
    }

    /// The type of the member `key` the apparent `Function` type of the
    /// callable `node` declares, `None` when it declares none or the
    /// apparent type does not settle.
    fn apparent_function_member_value(
        &self,
        node: SemanticNodeId,
        key: &crate::semantic_query::PropertyKey,
    ) -> Option<SemanticNodeId> {
        // A rootless callable (a function type written in a type position)
        // reads the apparent type of the project the relation is asked in.
        let apparent = match self.apparent_type_of(node) {
            Some(apparent) => apparent,
            None => {
                let demand = self.wrapper_demand_canonical()?;
                let _scope =
                    super::LexicalDemandScopeGuard::push(&self.lexical_demand_scope, demand);
                self.apparent_type_of(node)?
            }
        };
        let SemanticNodeData::Object(view) = &*self.graph().node_data(apparent)? else {
            return None;
        };
        match view.project_known_key(key) {
            crate::semantic_query::SurfaceKeyProjection::Exact(member) => Some(member.value),
            crate::semantic_query::SurfaceKeyProjection::AbsentProven => None,
        }
    }

    pub(super) fn relate_target_index_signature(
        &self,
        source: &SurfaceView,
        target_index: &crate::semantic_query::IndexSignature,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
        // The strict subtype relation infers no index signature for a
        // source that declares none, an object literal's own type aside
        // (the checker's `typeRelatedToIndexInfo`): `{ x: string }` is not
        // below `{ [k: string]: string }`.
        if self.strict_subtype_mode()
            && !surface_is_object_literal(source)
            && !source.index_signatures.iter().any(|s_index| {
                index_domains_overlap(graph, s_index.key_type, target_index.key_type)
            })
        {
            return RelationResult::NotAssignable;
        }
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for s_index in source.index_signatures.iter() {
            if !index_domains_overlap(graph, s_index.key_type, target_index.key_type) {
                continue;
            }
            let r = self.relate_member(
                s_index.value_type,
                target_index.value_type,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for prop in source.positive_members().iter() {
            let Some(property_key) = prop.key.cloned_known() else {
                return RelationResult::Unknown;
            };
            if !index_signature_applies_to_property(graph, target_index.key_type, &property_key) {
                continue;
            }
            let r = self.relate_member(
                prop.value,
                target_index.value_type,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        if source.has_known_index_signature() && source.index_signatures.is_empty() {
            RelationResult::Unknown
        } else {
            acc
        }
    }

    fn current_relation_kind(&self) -> crate::semantic_query::RelationKind {
        self.dispatch_txn
            .borrow()
            .reentry()
            .nearest_relate()
            .map(|(key, _)| key.relation)
            .unwrap_or(crate::semantic_query::RelationKind::Assignable)
    }

    fn subtype_mode(&self) -> bool {
        matches!(
            self.current_relation_kind(),
            crate::semantic_query::RelationKind::Subtype
                | crate::semantic_query::RelationKind::StrictSubtype
        )
    }

    /// Whether the relation being decided is the checker's STRICT subtype
    /// relation — the one its union subtype reduction asks
    /// (`isTypeStrictSubtypeOf`). Beyond the subtype rules it refuses
    /// `any` below `unknown`, a `readonly` property below a mutable one,
    /// and a signature taking more parameters than its target.
    fn strict_subtype_mode(&self) -> bool {
        self.current_relation_kind() == crate::semantic_query::RelationKind::StrictSubtype
    }

    /// Relate two [`SemanticNodeData::Signature`] shells. The parameter
    /// positions and the arity verdict are the checker's
    /// `compareSignaturesRelated` read through the ONE positional model
    /// ([`Self::signature_comparison_plan`]): a rest parameter supplies its
    /// element at every position past the fixed ones, and a target with a
    /// rest accepts any source arity. Under the strict subtype relation the
    /// arity is the checker's `StrictArity`: below a target without a rest,
    /// a source with a rest or with more parameters is never a subtype —
    /// `(s?: string) => number` is not below `() => number`. Parameter
    /// variance follows the key's policy (RI-10 behavioral branch):
    /// strictly contravariant under `strictFunctionTypes`, bivariant
    /// otherwise (either direction suffices per parameter pair); the
    /// return is covariant. A `method_target` — a target signature declared
    /// as a method — relates its parameters bivariantly whatever
    /// `strictFunctionTypes` says (the checker's `strictVariance` excludes
    /// method declarations). Subtype never uses the bivariant shortcut. A
    /// comparison whose positions do not settle is unknown, never a guess.
    ///
    /// A target carrying a TYPE predicate (`x is T` / `this is T`) relates
    /// predicates instead of returns, as TypeScript's
    /// `compareSignaturesRelated` does: a source without a predicate never
    /// satisfies it (a `boolean`-returning function is not a type guard),
    /// and a source predicate must be of the same kind about the same
    /// parameter with a related target type. Measured on 7.0.2:
    /// `((x: unknown) => boolean) extends ((x: unknown) => x is string)`
    /// is false, `((x: unknown) => x is string) extends ((x: unknown) =>
    /// x is string | number)` is true, a predicate about another
    /// parameter or an assertion source is false.
    pub(super) fn relate_function(
        &self,
        source: SemanticNodeId,
        source_result: FunctionResult,
        target: SemanticNodeId,
        target_result: FunctionResult,
        kind: crate::semantic_query::SignatureKind,
        method_target: bool,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let Ok(plan) =
            self.signature_comparison_plan(source, target, kind, self.strict_subtype_mode())
        else {
            return RelationResult::Unknown;
        };
        if let (Some(src_this), Some(tgt_this)) = (plan.source_receiver, plan.target_receiver) {
            let this_rel = self.relate_member(
                tgt_this,
                src_this,
                bindings,
                InferPosition::ContravariantParam,
            );
            if matches!(this_rel, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        // A source that demands more arguments than a rest-less target can
        // supply rejects unconditionally.
        if plan.source_has_more_parameters {
            return RelationResult::NotAssignable;
        }
        let bivariant = {
            let txn = self.dispatch_txn.borrow();
            let strict = txn.relation.strict.unwrap_or(StrictFamilyConfig::TS_STRICT);
            (method_target || !strict.strict_function_types) && !self.subtype_mode()
        };
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for (s_param, t_param) in plan.positions {
            // Contravariant: target param ≤ source param. Under the
            // bivariant regime either direction discharges the pair.
            let checkpoint = self.relation_session_checkpoint();
            let bindings_len = bindings.len();
            let contravariant = self.relate_member(
                t_param,
                s_param,
                bindings,
                InferPosition::ContravariantParam,
            );
            let pair = if bivariant && !matches!(contravariant, RelationResult::Assignable { .. }) {
                self.relation_session_rollback(&checkpoint);
                bindings.truncate(bindings_len);
                let fallback_checkpoint = self.relation_session_checkpoint();
                let fallback_bindings_len = bindings.len();
                let fallback =
                    self.relate_member(s_param, t_param, bindings, InferPosition::Covariant);
                if !matches!(fallback, RelationResult::Assignable { .. }) {
                    self.relation_session_rollback(&fallback_checkpoint);
                    bindings.truncate(fallback_bindings_len);
                }
                result_or(contravariant, fallback)
            } else {
                contravariant
            };
            acc = result_and(acc, pair);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        // The checker's `compareSignaturesRelated`: a target whose result is
        // exactly `void` or `any` accepts any source result, so neither the
        // source's return nor its type predicate is compared — `() => number`
        // is assignable to `() => void`, and a plain `boolean` function to an
        // assertion signature. The parameters above still are.
        if self.signature_result_accepts_any(target_result.return_type) {
            return acc;
        }
        if let Some(target_predicate) = target_result
            .predicate
            .filter(|predicate| !predicate.asserts)
        {
            let related = match source_result.predicate {
                Some(source_predicate)
                    if !source_predicate.asserts
                        && source_predicate.subject == target_predicate.subject =>
                {
                    match (source_predicate.ty, target_predicate.ty) {
                        (Some(source_ty), Some(target_ty)) => self.relate_member(
                            source_ty,
                            target_ty,
                            bindings,
                            InferPosition::Return,
                        ),
                        _ => RelationResult::NotAssignable,
                    }
                }
                _ => RelationResult::NotAssignable,
            };
            return result_and(acc, related);
        }
        // Covariant return.
        let r = self.relate_member(
            source_result.return_type,
            target_result.return_type,
            bindings,
            InferPosition::Return,
        );
        result_and(acc, r)
    }

    /// Whether a target signature's result is exactly the checker's `void`
    /// or `any` — the two results `compareSignaturesRelated` accepts any
    /// source result against. Transparent aliases and declaration carriers
    /// are followed (`type V = void` is `void` itself); a union such as
    /// `void | undefined` is not `void`, and a carrier that cannot be
    /// resolved is not assumed to be.
    fn signature_result_accepts_any(&self, result: SemanticNodeId) -> bool {
        /// Alias and declaration-carrier hops followed before giving up.
        const RESULT_CARRIER_HOPS: usize = 8;
        let mut current = result;
        // bounded-loop: RESULT_CARRIER_HOPS alias / declaration-carrier hops.
        for _ in 0..RESULT_CARRIER_HOPS {
            // The node read ends here, before any carrier resolution runs.
            let alias_target = match self.graph().node_data(current).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Void | PrimitiveKind::Any)) => {
                    return true;
                }
                Some(SemanticNodeData::Alias(inner)) => Some(*inner),
                Some(
                    SemanticNodeData::DeclRef { .. } | SemanticNodeData::InstantiationRef { .. },
                ) => None,
                _ => return false,
            };
            current = match alias_target {
                Some(inner) => inner,
                None => match self.unwrap_identity_carrier_one_step(current) {
                    IdentityCarrierUnwrap::Concrete(next) => next,
                    IdentityCarrierUnwrap::Unresolvable => return false,
                },
            };
        }
        false
    }

    /// Relate a function source against an object target carrying call
    /// signatures.
    /// A DIRECT signature source against an Object target: a bare signature
    /// declares no member of its own, so a required target member relates
    /// to the member its apparent `Function` type declares (`prototype:
    /// any`, `length`, …) or rejects, and every target signature bucket
    /// must be satisfied by the source's single matching-kind signature — a
    /// bucket of the OTHER kind is unmet.
    fn relate_signature_source_to_object(
        &self,
        source_sig: SemanticNodeId,
        source_kind: crate::semantic_query::SignatureKind,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        // A function type has no implicit index signature
        // (`isObjectTypeWithInferableIndex` excludes a type with call or
        // construct signatures): only an index signature of type `any`
        // takes it.
        if target.index_signatures.iter().any(|index| {
            !matches!(
                self.graph().node_data(index.value_type).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
            )
        }) {
            return RelationResult::NotAssignable;
        }
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for m in target.positive_members().iter() {
            if m.optional {
                continue;
            }
            let value = m
                .key
                .cloned_known()
                .and_then(|key| self.apparent_function_member_value(source_sig, &key));
            let Some(value) = value else {
                return RelationResult::NotAssignable;
            };
            let r = self.relate_member(value, m.value, bindings, InferPosition::Covariant);
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        for (bucket_kind, bucket) in [
            (
                crate::semantic_query::SignatureKind::Call,
                &target.call_signatures,
            ),
            (
                crate::semantic_query::SignatureKind::Construct,
                &target.construct_signatures,
            ),
        ] {
            for t_sig in bucket.iter() {
                if bucket_kind != source_kind {
                    return RelationResult::NotAssignable;
                }
                let r = self.relate_member(source_sig, *t_sig, bindings, InferPosition::Covariant);
                acc = result_and(acc, r);
                if matches!(acc, RelationResult::NotAssignable) {
                    return RelationResult::NotAssignable;
                }
            }
        }
        acc
    }

    /// The checker's `constructorVisibilitiesAreCompatible` over the FIRST
    /// construct signature of each side: a private target accepts every
    /// source, a protected target a public or protected one, and a public
    /// target only a public one. A side with no signature, or whose first
    /// signature has no declaration, is compatible.
    fn construct_visibilities_compatible(
        &self,
        source: &[SemanticNodeId],
        target: &[SemanticNodeId],
    ) -> bool {
        use verter_type_expr::MemberVisibility::{Private, Protected, Public};
        let (Some(source), Some(target)) = (source.first(), target.first()) else {
            return true;
        };
        match (
            self.construct_signature_visibility(*source),
            self.construct_signature_visibility(*target),
        ) {
            (Some(source), Some(target)) => match (source, target) {
                (_, Private) | (Public | Protected, Protected) | (Public, Public) => true,
                (Private, Protected) | (Protected | Private, Public) => false,
            },
            _ => true,
        }
    }

    /// Whether `signature` is an ABSTRACT construct signature (the
    /// checker's `SignatureFlags.Abstract`).
    pub(super) fn signature_is_abstract(&self, signature: SemanticNodeId) -> bool {
        matches!(
            self.graph().node_data(signature).as_deref(),
            Some(SemanticNodeData::Signature {
                is_abstract: true,
                ..
            })
        )
    }

    /// The accessibility of a construct signature's DECLARATION, `None`
    /// when it has none. A class's own construct signature (no authored
    /// return annotation, returning the class's instance) is its first
    /// constructor's; a class that declares no constructor carries its
    /// base's construct signatures, declaration included
    /// (`getDefaultConstructSignatures`), and a class with neither has a
    /// declaration-less default signature. Any other construct signature
    /// is a public declaration.
    pub(super) fn construct_signature_visibility(
        &self,
        signature: SemanticNodeId,
    ) -> Option<verter_type_expr::MemberVisibility> {
        let graph = self.graph();
        let instance = match graph.node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Construct,
                return_type_span: None,
                return_type,
                ..
            }) => *return_type,
            _ => return Some(verter_type_expr::MemberVisibility::Public),
        };
        let class = match graph.node_data(instance).as_deref() {
            Some(SemanticNodeData::ClassExpressionInstance { identity, .. }) => {
                return identity.constructor_visibility;
            }
            Some(SemanticNodeData::DeclRef { identity }) => identity.clone(),
            Some(SemanticNodeData::InstantiationRef { base, .. }) => base.clone(),
            _ => return Some(verter_type_expr::MemberVisibility::Public),
        };
        let mut current = (
            Arc::clone(&class.canonical_id),
            class.owner,
            Arc::clone(&class.decl_name),
        );
        let mut seen = rustc_hash::FxHashSet::default();
        while seen.insert(current.clone()) {
            let declared = self
                .ctx
                .ensure_indexed_ready_serve(current.0.as_ref())
                .and_then(|serve| {
                    serve
                        .indexed
                        .shallow_state
                        .decl_bodies()
                        .header_index()
                        .constructor_visibility
                        .get(&verter_type_expr::DeclBindingKey::new(
                            current.1,
                            current.2.as_ref(),
                        ))
                        .copied()
                });
            if declared.is_some() {
                return declared;
            }
            match self
                .class_heritage_bases(current.0.as_ref(), current.1, current.2.as_ref())
                .into_iter()
                .next()
            {
                Some((canonical, owner, name, _)) => current = (canonical, owner, name),
                None => break,
            }
        }
        None
    }

    /// An Object source against a DIRECT signature target: some signature
    /// in the source's MATCHING-KIND group must satisfy the target
    /// signature.
    fn relate_object_to_signature(
        &self,
        source: &SurfaceView,
        target_kind: crate::semantic_query::SignatureKind,
        target_sig: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let group = match target_kind {
            crate::semantic_query::SignatureKind::Call => &source.call_signatures,
            crate::semantic_query::SignatureKind::Construct => &source.construct_signatures,
        };
        if target_kind == crate::semantic_query::SignatureKind::Construct
            && !self.construct_visibilities_compatible(group, &[target_sig])
        {
            return RelationResult::NotAssignable;
        }
        let alternatives: Vec<_> = group
            .iter()
            .map(|source_signature| (*source_signature, target_sig))
            .collect();
        self.relate_pair_alternatives(&alternatives, bindings, InferPosition::Covariant)
    }
}

/// The pop result of a relation frame.
enum FramePop {
    /// A provisional member's caller-return step (never the published
    /// payload).
    Provisional(RelationStep),
    /// The SCC root's public close outcome.
    RootClose(RootClose),
}

/// Outcome of the identity-carrier unwrap performed before relation
/// dispatch.
pub(crate) enum IdentityCarrierUnwrap {
    Concrete(SemanticNodeId),
    Unresolvable,
}

/// A PROOF that two nodes have no common inhabitant, carrying the CHECKER'S
/// answer to the one question a consumer may not decide for itself: whether
/// an intersection of the two operands reduces to `never` or is kept.
///
/// The fields are private to this module, so a value of this type cannot be
/// constructed anywhere else in the crate — the disjointness proof is
/// mintable ONLY by the shared relation authority's `Comparable` reduction,
/// and a value of it in a consumer's hands therefore came from the
/// authority. The collapse class is minted WITH the proof by the same
/// authority: `tsc`'s intersection reduction answers `never` for a unit
/// discriminant (disjoint primitive/literal tags, distinct `unique symbol`
/// identities, a shared REQUIRED member whose two types are both unit types
/// and conflict) and KEEPS `A & B` for a conflict reachable only through
/// non-unit member values, at any depth. A consumer that collapsed every
/// disjoint pair would publish `never` where the checker publishes an
/// intersection — a wrong-complete warm value.
///
/// The confinement is a SUPPORTING constraint, not the enforcement. It is
/// deliberately honest about its limit: today's consumer decides by
/// ELIMINATION (`let ComparabilityVerdict::Disjoint(_) = comparable else`),
/// so a re-introduced private classifier would compute a bare `bool` and
/// branch on it without ever needing to construct this type. What actually
/// holds "flow owns no relation classifier" is BEHAVIOURAL: the narrow
/// populates the shared `Comparable` memo family, issues zero `Identity`
/// judgements, and publishes a value only the authority's own descent can
/// produce — a permissive private classifier would fail all three. No
/// `#[must_use]` here either: it would not fire on the `let ... else`
/// pattern the verdict is matched through and would read as enforcement
/// that is not there.
pub(crate) struct DisjointnessProof {
    collapse: IntersectionCollapse,
}

/// Whether the checker reduces an intersection of a provably disjoint pair
/// to `never`, or keeps `A & B`. Minted only inside the relation module,
/// alongside the proof it classifies.
pub(crate) enum IntersectionCollapse {
    /// The conflict is a checker collapse criterion: disjoint tags, distinct
    /// nominal identities, or a shared REQUIRED member whose values are both
    /// unit types and conflict.
    ReducesToNever,
    /// The disjointness proof is real, but the conflict is reachable only
    /// through member values that are not both unit types. `tsc` keeps the
    /// intersection; a consumer narrowing by it must too.
    Kept,
}

impl DisjointnessProof {
    fn new(collapse: IntersectionCollapse) -> Self {
        Self { collapse }
    }

    /// Whether the checker reduces an intersection of the proved-disjoint
    /// operands to `never`. `false` means the consumer must keep the
    /// intersection — never a license to widen or to guess a different value.
    pub(crate) fn checker_reduces_intersection_to_never(&self) -> bool {
        matches!(self.collapse, IntersectionCollapse::ReducesToNever)
    }
}

/// The three verdicts of [`RelationKind::Comparable`] as its consumers see
/// them.
pub(crate) enum ComparabilityVerdict {
    /// No proof of empty overlap exists — treat the two as comparable.
    Overlaps,
    /// The two provably have NO common inhabitant. Carries the authority's
    /// [`DisjointnessProof`]; what a consumer DOES with a disjointness proof
    /// (whether an intersection collapses to `never`) is the consumer's /
    /// the canonical algebra's decision, not this relation's.
    Disjoint(DisjointnessProof),
    /// Undecided — a subject the oracle could not read, a budget cap, or an
    /// open coinductive assumption. Never warm-admitted, never either answer.
    Undecided,
}

/// What the nominal (`unique symbol`) leaf contributed to one relation pair.
enum NominalLeaf {
    /// The nominal axis alone decides the pair.
    Decided(RelationResult),
    /// Exactly one side was nominal: it widened to its inhabited type and
    /// the pair is re-asked on the ordinary lattice.
    Retry(SemanticNodeId, SemanticNodeId),
}

enum ComparableSurface {
    Object(SurfaceView),
    NonObject,
    Unresolvable,
}

/// Canonical Record shapes the `Record<K, V>`-against-Object arm handles.
enum RecordTargetShape {
    LiteralKey(SurfaceView),
    GenericKey {
        key_type: SemanticNodeId,
        value_type: SemanticNodeId,
    },
}

/// Map a reducer verdict + optional session fixation onto the pending
/// record a popped frame carries.
fn pending_verdict_of(
    verdict: &RelationResult,
    budget_cap: &Option<RecursionOrBudgetCap>,
    session_bindings: &mut Option<Arc<[InferBinding]>>,
    bindings: Vec<InferBinding>,
) -> PendingVerdict {
    if let Some(cap) = budget_cap {
        return PendingVerdict::BudgetExceeded(*cap);
    }
    match verdict {
        RelationResult::Assignable { .. } => PendingVerdict::Assignable {
            bindings: session_bindings
                .take()
                .unwrap_or_else(|| Arc::from(bindings.into_boxed_slice())),
        },
        RelationResult::NotAssignable => PendingVerdict::NotAssignable,
        RelationResult::Unknown => PendingVerdict::Unknown,
    }
}

/// The caller-return step of a provisional pending verdict.
pub(super) fn relation_step_from_pending(pending: &PendingVerdict) -> RelationStep {
    match pending {
        PendingVerdict::Assignable { bindings } => RelationStep::Assignable {
            bindings: Arc::clone(bindings),
        },
        PendingVerdict::NotAssignable => RelationStep::NotAssignable,
        PendingVerdict::Unknown => RelationStep::Unknown,
        PendingVerdict::BudgetExceeded(cap) => RelationStep::BudgetExceeded(*cap),
    }
}

/// The caller-return step of a published (or warm) payload.
fn relation_step_from_payload(payload: &RelationPayload) -> RelationStep {
    match &payload.outcome {
        RelationOutcome::Assignable => RelationStep::Assignable {
            bindings: Arc::clone(&payload.bindings),
        },
        RelationOutcome::NotAssignable => RelationStep::NotAssignable,
        RelationOutcome::BudgetExceeded(kind) => {
            RelationStep::BudgetExceeded(RecursionOrBudgetCap {
                kind: *kind,
                limit: 0,
            })
        }
    }
}

/// Iterative worklist item for [`ProjectSemanticDispatch::decide_relation`].
#[derive(Debug, Clone)]
enum RelateWork {
    /// Expand the current frame's root pair locally; `true` when the
    /// frame relates one arm of an intersection target
    /// ([`RelationPolicy::intersection_target_arm`]).
    Expand(SemanticNodeId, SemanticNodeId, bool),
    /// Evaluate `(source, target)`.
    Eval(SemanticNodeId, SemanticNodeId),
    /// Evaluate one ARM of a union source (or of an enum literal's value).
    /// Every arm of the other two composite forms (a union target's
    /// alternatives, an intersection source's) already relates through the
    /// member authority, whose identity unwrap decides a declaration
    /// carrier; an arm pair naming one takes that authority too, and every
    /// other arm expands inline like [`Self::Eval`].
    Arm(SemanticNodeId, SemanticNodeId),
    /// Evaluate one arm of an intersection target like [`Self::Arm`], exempt
    /// from the weak-type check the whole intersection already passed.
    TargetArm(SemanticNodeId, SemanticNodeId),
    /// Pop `n` prior results, AND them, push one combined result.
    ReduceAnd(u32),
}

fn reduce_and_from_results(results: &mut Vec<RelationResult>, n: u32) -> RelationResult {
    let mut combined = RelationResult::Assignable {
        bindings: Arc::from(Vec::new().into_boxed_slice()),
    };
    // bounded-loop: drains `n` per-pair results owned by this reducer — fan-out of the originating distribution; total work bounded by `decide_relation` budget (graph-size × 10).
    for _ in 0..n {
        let r = results
            .pop()
            .expect("RelateWork::ReduceAnd: result-stack underflow");
        combined = result_and(combined, r);
    }
    combined
}

/// Build a forward-ordered sequence of `RelateWork` items such that after
/// `push_forward_work`, the first item pops first.
fn push_forward_work(work: &mut Vec<RelateWork>, forward: Vec<RelateWork>) {
    for item in forward.into_iter().rev() {
        work.push(item);
    }
}

/// The checker's element flag of one array or tuple position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TupleSlotKind {
    /// A required element.
    Required,
    /// An optional element (`T?`).
    Optional,
    /// A rest element over an array (`...T[]`) — and an array itself, as
    /// the one position an array source has.
    Rest,
    /// A variadic element over a generic (`...T`).
    Variadic,
}

/// One array or tuple position: its flag, its TYPE ARGUMENT (the element
/// type of a rest position; the type with `undefined` of an optional one
/// under `strictNullChecks`), and the authored value it came from.
#[derive(Debug, Clone, Copy)]
struct TupleSlot {
    kind: TupleSlotKind,
    type_argument: SemanticNodeId,
    /// The type argument with an optional element's implied `undefined`
    /// removed where `exactOptionalPropertyTypes` makes it the distinct
    /// missing type (the checker removes it from a target optional
    /// element, and from a source optional element facing one).
    missing_removed: SemanticNodeId,
    value: SemanticNodeId,
}

/// Pair the positions of an array or tuple SOURCE with the positions of a
/// tuple TARGET by the checker's tuple arity rules (`propertiesRelatedTo`
/// over a tuple target): a source without a rest element must supply the
/// target's required length, a target without a variable element must not
/// be shorter than the source's required length nor accept a source rest,
/// and each source position meets the target position counted from the
/// front, or — past the target's leading fixed elements — from the back.
/// A variadic position matches only a variable one, a required target
/// position only a required source one. `None` is a decided
/// `NotAssignable`; otherwise the `(source, target)` type pairs every one
/// of which must relate. `source_is_tuple` is false for an array source,
/// whose required length is zero.
fn tuple_position_pairs(
    source: &[TupleSlot],
    source_is_tuple: bool,
    target: &[TupleSlot],
) -> Option<Vec<(SemanticNodeId, SemanticNodeId)>> {
    let is_variable =
        |kind: TupleSlotKind| matches!(kind, TupleSlotKind::Rest | TupleSlotKind::Variadic);
    let min_length = |slots: &[TupleSlot]| {
        slots
            .iter()
            .filter(|slot| matches!(slot.kind, TupleSlotKind::Required | TupleSlotKind::Variadic))
            .count()
    };
    let source_arity = source.len();
    let target_arity = target.len();
    let source_has_rest = source.iter().any(|slot| slot.kind == TupleSlotKind::Rest);
    let target_has_variable = target.iter().any(|slot| is_variable(slot.kind));
    let source_min_length = if source_is_tuple {
        min_length(source)
    } else {
        0
    };
    let target_min_length = min_length(target);
    if !source_has_rest && source_arity < target_min_length {
        return None;
    }
    if !target_has_variable
        && (target_arity < source_min_length || source_has_rest || target_arity < source_arity)
    {
        return None;
    }
    let target_start = target
        .iter()
        .position(|slot| slot.kind == TupleSlotKind::Rest)
        .unwrap_or(target_arity);
    let target_end = target
        .iter()
        .rev()
        .position(|slot| slot.kind == TupleSlotKind::Rest)
        .unwrap_or(target_arity);
    let mut pairs = Vec::with_capacity(source_arity);
    for (position, source_slot) in source.iter().enumerate() {
        let from_end = source_arity - 1 - position;
        let target_position = if target_has_variable && position >= target_start {
            target_arity - 1 - from_end.min(target_end)
        } else {
            position
        };
        let target_slot = target[target_position];
        if target_slot.kind == TupleSlotKind::Variadic
            && source_slot.kind != TupleSlotKind::Variadic
        {
            return None;
        }
        if source_slot.kind == TupleSlotKind::Variadic && !is_variable(target_slot.kind) {
            return None;
        }
        if target_slot.kind == TupleSlotKind::Required
            && source_slot.kind != TupleSlotKind::Required
        {
            return None;
        }
        let target_type = if source_slot.kind == TupleSlotKind::Variadic
            && target_slot.kind == TupleSlotKind::Rest
        {
            target_slot.value
        } else {
            target_slot.missing_removed
        };
        let source_type = if target_slot.kind == TupleSlotKind::Optional {
            source_slot.missing_removed
        } else {
            source_slot.type_argument
        };
        pairs.push((source_type, target_type));
    }
    Some(pairs)
}

/// The verdict a template literal type's string kind alone decides for a
/// pair: `true` for a template below `string`, `false` for a template
/// against a primitive or literal of another kind (either direction),
/// `None` for every other pair.
/// Whether each of two object surfaces requires a property the other
/// proves absent. The checker's comparable relation, like assignability,
/// refuses a source that lacks a required target property
/// (`propertiesRelatedTo` reports it unmatched), and two types are
/// comparable when either direction relates: a pair missing a required
/// property both ways is comparable in neither (measured: `x: A | C`
/// over `interface A { kind: 'a'; a: 1 }` and `interface C { c: 3 }`
/// reads `C` inside `if (x === c)`). A surface with an index signature,
/// a call or construct signature (its apparent type declares more), or a
/// key that is not known keeps the pair to the member descent.
fn surfaces_require_members_the_other_lacks(a: &SurfaceView, b: &SurfaceView) -> bool {
    let closed = |view: &SurfaceView| {
        view.index_signatures.is_empty()
            && view.call_signatures.is_empty()
            && view.construct_signatures.is_empty()
            && view
                .positive_members()
                .iter()
                .all(|member| member.key.as_known().is_some())
    };
    let lacks_a_required_member = |source: &SurfaceView, target: &SurfaceView| {
        target.positive_members().iter().any(|member| {
            !member.optional
                && member.key.cloned_known().is_some_and(|key| {
                    matches!(
                        source.project_known_key(&key),
                        crate::semantic_query::SurfaceKeyProjection::AbsentProven
                    )
                })
        })
    };
    closed(a) && closed(b) && lacks_a_required_member(a, b) && lacks_a_required_member(b, a)
}

/// What declares a property, for the checker's protected-member rule
/// ([`ProjectSemanticDispatch::property_accessibility_relation`]).
enum MemberOwner {
    /// A class declares it directly.
    Class(ClassOwner),
    /// A file-scope interface or type alias declares it: no class does.
    NotAClass,
    /// Its declaration is not read from the graph.
    Undecided,
}

/// The class that declares a member directly.
enum ClassOwner {
    /// A file-scope class, by its declaration identity.
    Declared(crate::semantic_query::DeclIdentity),
    /// Any other class — a class expression, a class declared inside a
    /// body — by its node in the file's syntactic class index.
    Syntactic {
        file: Arc<str>,
        span: verter_span::Span,
        /// Whether the class has an `extends` clause.
        has_heritage: bool,
    },
}

fn template_literal_kind_verdict(
    source: &SemanticNodeData,
    target: &SemanticNodeData,
) -> Option<bool> {
    let other_kind = |data: &SemanticNodeData| {
        matches!(
            data,
            SemanticNodeData::Primitive(
                PrimitiveKind::Number
                    | PrimitiveKind::Boolean
                    | PrimitiveKind::BigInt
                    | PrimitiveKind::Symbol
                    | PrimitiveKind::Null
                    | PrimitiveKind::Undefined
                    | PrimitiveKind::Void
            ) | SemanticNodeData::Literal(
                LiteralValue::Number(_) | LiteralValue::Boolean(_) | LiteralValue::BigInt(_)
            )
        )
    };
    match (source, target) {
        (
            SemanticNodeData::TemplateLiteral { .. },
            SemanticNodeData::Primitive(PrimitiveKind::String),
        ) => Some(true),
        (SemanticNodeData::TemplateLiteral { .. }, other)
        | (other, SemanticNodeData::TemplateLiteral { .. })
            if other_kind(other) =>
        {
            Some(false)
        }
        _ => None,
    }
}

/// Whether a surface is an object literal's type: a member the literal
/// itself authored (a spread-carried one included) marks it, where a
/// declared object type's members are all non-literal.
fn surface_is_object_literal(surface: &SurfaceView) -> bool {
    surface
        .positive_members()
        .iter()
        .any(|member| member.excess_origin != verter_type_expr::ExcessPropertyOrigin::NonLiteral)
}

/// Whether a source index signature keyed by `source_key` applies to a target
/// index signature keyed by `target_key` (the checker's
/// `getApplicableIndexInfo`): a `string` key applies to `string` and `number`
/// keys, a `number` key to `number` keys only, and any other key as its
/// domain overlaps the target's.
fn index_key_applies(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    source_key: SemanticNodeId,
    target_key: SemanticNodeId,
) -> bool {
    match (
        graph.node_data(source_key).as_deref(),
        graph.node_data(target_key).as_deref(),
    ) {
        (
            Some(SemanticNodeData::Primitive(PrimitiveKind::String)),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Number)),
        )
        | (
            Some(SemanticNodeData::Primitive(PrimitiveKind::Number)),
            Some(SemanticNodeData::Primitive(PrimitiveKind::Number)),
        ) => true,
        (
            Some(SemanticNodeData::Primitive(_)),
            Some(SemanticNodeData::Primitive(PrimitiveKind::String | PrimitiveKind::Number)),
        ) => false,
        _ => source_key == target_key || index_domains_overlap(graph, source_key, target_key),
    }
}

/// Build and push the worklist fan-out for a distribution whose reducer
/// is AND-all, each pair evaluated as `arm` builds it.
fn distribute_and<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
    arm: fn(SemanticNodeId, SemanticNodeId) -> RelateWork,
    mut pairer: F,
) where
    F: FnMut(&SemanticNodeId) -> (SemanticNodeId, SemanticNodeId),
{
    let n = members.len();
    if n == 0 {
        results.push(RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        });
        return;
    }
    let mut forward: Vec<RelateWork> = Vec::with_capacity(n + 1);
    for m in members.iter() {
        let (s, t) = pairer(m);
        forward.push(arm(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceAnd(n as u32));
    }
    push_forward_work(work, forward);
}

/// Concrete root-kind tags for the overlap oracle. Mixed tags that cannot
/// share an inhabitant are disjoint; tags that TypeScript treats as
/// overlapping (array/function vs object, template vs string, `object`
/// vs a structural object) stay permissive.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ComparableRootKind {
    Primitive(PrimitiveKind),
    Literal,
    Nominal,
    Object,
    ArrayLike,
    Callable,
    Template,
}

fn comparable_root_kind(data: &SemanticNodeData) -> Option<ComparableRootKind> {
    match data {
        SemanticNodeData::Primitive(kind) => match kind {
            PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never => None,
            _ => Some(ComparableRootKind::Primitive(*kind)),
        },
        SemanticNodeData::Literal(_) | SemanticNodeData::EnumLiteral(_) => {
            Some(ComparableRootKind::Literal)
        }
        // An UNREDUCED operation has no comparable root shape yet — comparing
        // it structurally would compare the operation, not its value.
        SemanticNodeData::IntrinsicApplication { .. } => None,
        SemanticNodeData::TypeOfNominal(_) => Some(ComparableRootKind::Nominal),
        // A class instance is an object whatever its members.
        SemanticNodeData::Object(_)
        | SemanticNodeData::ObjectSpreadProgram(_)
        | SemanticNodeData::MergedDecl { .. }
        | SemanticNodeData::ClassExpressionInstance { .. } => Some(ComparableRootKind::Object),
        SemanticNodeData::Array { .. } | SemanticNodeData::Tuple { .. } => {
            Some(ComparableRootKind::ArrayLike)
        }
        SemanticNodeData::Signature { .. } | SemanticNodeData::DeferredCallable(_) => {
            Some(ComparableRootKind::Callable)
        }
        SemanticNodeData::TemplateLiteral { .. } => Some(ComparableRootKind::Template),
        SemanticNodeData::Union(_)
        | SemanticNodeData::Intersection(_)
        | SemanticNodeData::Alias(_)
        | SemanticNodeData::Opaque(_)
        | SemanticNodeData::KeyOf { .. }
        | SemanticNodeData::IndexedAccess { .. }
        | SemanticNodeData::Mapped { .. }
        | SemanticNodeData::TypeOf(_)
        | SemanticNodeData::TypeParam { .. }
        | SemanticNodeData::Infer { .. }
        | SemanticNodeData::InferRef { .. }
        | SemanticNodeData::Conditional { .. }
        | SemanticNodeData::DeclRef { .. }
        | SemanticNodeData::InstantiationRef { .. }
        | SemanticNodeData::BareRef(_)
        | SemanticNodeData::ImportType(_)
        | SemanticNodeData::RawFallback { .. }
        | SemanticNodeData::SyntheticBinding { .. } => None,
    }
}

fn comparable_root_kinds_disjoint(a: &SemanticNodeData, b: &SemanticNodeData) -> bool {
    let (Some(left), Some(right)) = (comparable_root_kind(a), comparable_root_kind(b)) else {
        return false;
    };
    if left == right {
        return false;
    }
    match (left, right) {
        (ComparableRootKind::Primitive(x), ComparableRootKind::Primitive(y)) => {
            // KNOWN DIVERGENCE, inherited from the shared tag oracle: the
            // checker's disjointness answer for `null` / `undefined`
            // participates depends on the strict-null-checks regime, and a
            // `Comparable` key never reads the strict snapshot the
            // assignability engine applies. The oracle answers with the
            // canonical tag algebra — the same answer in every regime — so
            // a loose-mode program where `null` overlaps everything still
            // gets the strict-regime verdict here.
            let widening_pair = matches!(
                (x, y),
                (PrimitiveKind::Undefined, PrimitiveKind::Void)
                    | (PrimitiveKind::Void, PrimitiveKind::Undefined)
            );
            x != y && !widening_pair
        }
        (ComparableRootKind::Literal, ComparableRootKind::Primitive(_))
        | (ComparableRootKind::Primitive(_), ComparableRootKind::Literal) => false,
        (ComparableRootKind::Nominal, ComparableRootKind::Primitive(PrimitiveKind::Symbol))
        | (ComparableRootKind::Primitive(PrimitiveKind::Symbol), ComparableRootKind::Nominal) => {
            false
        }
        (ComparableRootKind::Template, ComparableRootKind::Primitive(PrimitiveKind::String))
        | (ComparableRootKind::Primitive(PrimitiveKind::String), ComparableRootKind::Template)
        | (ComparableRootKind::Template, ComparableRootKind::Literal)
        | (ComparableRootKind::Literal, ComparableRootKind::Template) => false,
        (ComparableRootKind::Primitive(PrimitiveKind::Object), ComparableRootKind::Object)
        | (ComparableRootKind::Object, ComparableRootKind::Primitive(PrimitiveKind::Object))
        | (ComparableRootKind::ArrayLike, ComparableRootKind::Object)
        | (ComparableRootKind::Object, ComparableRootKind::ArrayLike)
        | (ComparableRootKind::Callable, ComparableRootKind::Object)
        | (ComparableRootKind::Object, ComparableRootKind::Callable)
        | (ComparableRootKind::ArrayLike, ComparableRootKind::Callable)
        | (ComparableRootKind::Callable, ComparableRootKind::ArrayLike)
        | (ComparableRootKind::ArrayLike, ComparableRootKind::Primitive(PrimitiveKind::Object))
        | (ComparableRootKind::Primitive(PrimitiveKind::Object), ComparableRootKind::ArrayLike)
        | (ComparableRootKind::Callable, ComparableRootKind::Primitive(PrimitiveKind::Object))
        | (ComparableRootKind::Primitive(PrimitiveKind::Object), ComparableRootKind::Callable) => {
            false
        }
        _ => true,
    }
}

/// Proven tag-level disjointness for the contravariant-candidate
/// intersection collapse — delegates to the canonical algebra's single
/// proven-disjoint authority ([`super::canonical_algebra::tag_level_disjoint`]),
/// so the relation engine and canonical intersection construction share one
/// implementation. Conservative `false` for every undecided shape (the
/// structural Intersection carrier is kept).
fn tag_level_disjoint(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    super::canonical_algebra::tag_level_disjoint(graph, a, b)
}

#[cfg(test)]
pub(crate) mod reverse_ownership_tests {
    use super::super::dispatch_txn::SessionId;
    use super::*;

    fn reverse_setup(param: SemanticNodeId) -> InferenceSessionSetup {
        InferenceSessionSetup::new(
            Arc::from(vec![InferenceInfoSetup::new(param, Arc::from("T"))].into_boxed_slice()),
            VariancePhase::Covariant,
            InferencePassKind::ReverseHomomorphicMapped,
            InferenceCandidatePriority::HomomorphicMapped,
            NoInferMask::empty(),
            ConstParamPolicy::NonConst,
            ContextualInferenceMode::None,
        )
    }

    fn reverse_state(param: SemanticNodeId) -> ReverseProjectionState {
        ReverseProjectionState::new(ReverseHomomorphicSpec {
            mapped_node: SemanticNodeId(301),
            base_infer: param,
            mapper_parameter: SemanticNodeId(302),
            template: SemanticNodeId(303),
            modifiers: ReverseMappedModifiers {
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
            },
        })
    }

    fn require_relation_result_signature<'dispatch>(
        _pass: fn(
            &ProjectSemanticDispatch<'dispatch>,
            SemanticNodeId,
            &ReverseHomomorphicSpec,
            &mut Vec<InferBinding>,
        ) -> RelationResult,
    ) {
    }

    fn classify_relation_result_exhaustively(result: RelationResult) {
        match result {
            RelationResult::Assignable { .. }
            | RelationResult::NotAssignable
            | RelationResult::Unknown => {}
        }
    }

    #[test]
    pub(crate) fn reverse_mapped_inference_is_relation_owned_in_session() {
        // This private function item is nameable only from the relation
        // authority's own module tree, and its sole output is the closed
        // reducer lattice rather than a standalone binding map.
        require_relation_result_signature(ProjectSemanticDispatch::relate_reverse_homomorphic);
        classify_relation_result_exhaustively(RelationResult::Unknown);

        let active_param = SemanticNodeId(304);
        let aggregate = SemanticNodeId(305);
        let fallback = SemanticNodeId(306);

        let mut inactive = InferenceSession::new(
            SessionId(1),
            reverse_setup(active_param),
            Some(reverse_state(active_param)),
        );
        assert!(
            !inactive.deposit_reverse_aggregate(
                SemanticNodeId(999),
                aggregate,
                InferenceCandidatePriority::HomomorphicMapped,
            ),
            "a reverse aggregate cannot bind outside the frozen session setup"
        );
        let inactive_bindings = inactive
            .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(fallback))
            .expect("collecting session stages");
        assert_eq!(
            inactive_bindings[0].bound, fallback,
            "a refused deposit must leave no independently publishable reverse result"
        );

        let mut active = InferenceSession::new(
            SessionId(2),
            reverse_setup(active_param),
            Some(reverse_state(active_param)),
        );
        assert!(active.deposit_reverse_aggregate(
            active_param,
            aggregate,
            InferenceCandidatePriority::HomomorphicMapped,
        ));
        let active_bindings = active
            .stage_fixation(|nodes, _| nodes.first().copied().unwrap_or(fallback))
            .expect("collecting session stages");
        assert_eq!(
            active_bindings[0].bound, aggregate,
            "the accepted aggregate reaches bindings only through session fixation"
        );
    }
}
