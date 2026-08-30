//! The completeness-proof layer for flow-bearing semantic operations.
//! Production-compiled but publicly unreachable: the demand planning and
//! finalization entry points have no production caller until the proof
//! admission is wired (the test surface reaches them through
//! `crate::for_tests`), while the registry identity IS production-live —
//! the `FlowReturnKey` constructor derives its result contract from it.
//! The layer shares the ONE graph authority — the store-minted
//! [`BoundFlowGraph`] seals each memoized `FlowGraphBundle` to the content
//! key it was built for — and the ONE closed query registry
//! ([`SemanticQueryKeyTag`]): no second graph, planner, or resolver.
//!
//! - The flow-operation contract registry projects the flow-contract
//!   columns over the closed query tags; an undeclared requirement is NEVER
//!   a wildcard fallthrough — it installs as a `Gap(FlowGap)` obligation
//!   that retains the offending requirement. The requirement universe is
//!   the CLOSED domain → fact-family mapping ([`FlowDomainClosure`]): each
//!   family's expansion rule, fixed-point requirement, and accepted typed
//!   gap come from the total [`flow_family_route`] mapping, and every
//!   required family gets exactly ONE family-coverage obligation — "proved
//!   empty" is discharged, "planner forgot the family" is unrepresentable.
//! - [`FlowDemandPlan`] is the demand/completeness authority over one
//!   store-bound graph — NOT an alias of `ReturnSlicePlan` (graph
//!   reachability selection only, stored here as the structural selection).
//!   It is SEALED: every field is private, immutable getters only, and it
//!   carries its own registry closure. The basis takes its body identity
//!   from the bound graph's key and the subject from the query's own
//!   demand axis; every obligation spec carries a closed semantic identity
//!   (demand root, family coverage, graph site, real binding slot, guard,
//!   call occurrence, dynamic relation, capture subject, or full edge)
//!   plus its exact evidence contract.
//! - The obligation runtime seals discharge evidence at mint time
//!   (validated against the specific spec, with dependency readiness —
//!   a dependent discharges only after its exact dependencies), observes
//!   convergence itself (only over a closed, fully discharged frontier),
//!   and mints the ONE sealed completion artifact (`Converged → Sealed`,
//!   one-shot: `AlreadySealed` on repetition, every post-seal transition
//!   refused); [`finalize_flow_solve`] consumes ONLY that artifact and is
//!   the ONLY minter of [`CompleteFlowResult`] (its constructor is
//!   private).

use std::sync::Arc;

use rustc_hash::FxHashMap;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_semantic::analysis::flow::flow_graph::{FlowEdgeClass, FlowNodeId, FlowNodeKind};
use verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan;
use verter_semantic::analysis::flow::peeker::{FlowSliceBudget, FlowSliceBudgetExceeded};
use verter_semantic::analysis::flow::{
    FunctionBodySkeleton, SkeletonBindingId, SkeletonBindingKind,
};
use verter_semantic::analysis::function_program::{
    FlowBindingIdentity, FunctionBindingKind, FunctionBindingRecord, FunctionProgramKey,
};

use super::dispatch_txn::flow_obligation_state::{
    FlowBindingBasis, FlowConvergenceEvidence, FlowDemandHandle, FlowObligationBasis,
    FlowObligationId, FlowObligationOrigin, FlowObligationSpec, ObligationState,
    SealedFlowCompletion,
};
use super::dispatch_txn::ObligationRuntime;
use crate::cache_runtime::flow_slice_node::{BoundFlowGraph, FlowSliceFunctionKey};
use crate::semantic_query::demand::Demand;
use crate::semantic_query::{
    FlowGap, FlowReturnResult, PathSegment, SemanticQueryKey, SemanticQueryKeyTag,
};

// Short aliases keep the closed registry table and the planner legible.
use self::FlowDomain as D;
use self::FlowExpansionRule as E;
use self::FlowFactFamily as F;
use self::FlowFinalizerKind as K;
use self::FlowOperationRole as R;
use self::FlowOperationStatus as S;
use self::FlowRequirementKind as RK;

/// One semantic domain a flow-bearing operation may require. Declaration
/// order IS the domain rank used for obligation ordering. `Coverage` is
/// deliberately declared by NO contract.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum FlowDomain {
    ReachingValue, ReachingType, Narrowing, Completion, ClosureCapture,
    Freshness, Effects, CallResolution, Relation, ContextualTyping, Coverage,
}

/// One fact family an operation may consume; `GraphEdge` reuses the shared
/// edge-class vocabulary.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlowFactFamily {
    GraphEdge(FlowEdgeClass), BindingSlot, ReturnSite, GuardPredicate,
    CallSite, ContextualTarget, Capture, SemanticRelation,
}

/// One requirement asserted against an operation: a domain or a fact family.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FlowRequirementKind { Domain(FlowDomain), FactFamily(FlowFactFamily) }

/// One requirement asserted against one operation's contract.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FlowRequirement { pub operation: SemanticQueryKeyTag, pub requirement: FlowRequirementKind }

/// The registered rules a structural selection expands through — the ONLY
/// expansion channel. Every fact family has exactly ONE registered rule
/// (see [`flow_family_route`]); the dispatcher is an exhaustive,
/// wildcard-free match over the family vocabulary.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowExpansionRule {
    BindingSlotFacts, ReturnSiteFacts, SelectedEdgeFacts, CallSiteFacts,
    GuardPredicateFacts, ContextualTargetFacts, CaptureFacts, SemanticRelationFacts,
}

/// Whether a family route's facts participate in the solve's fixed point
/// (re-observed to stability) or are a single structural enumeration that
/// is stable at plan time.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowFixedPointRequirement { FixedPoint, SinglePass }

/// The ONE closed route of a fact family: its registered expansion rule,
/// its fixed-point requirement, and the typed gap a required-but-unnameable
/// subject of this family installs. Total over the [`FlowFactFamily`]
/// vocabulary by construction (a wildcard-free `match`).
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowFamilyRoute {
    pub rule: FlowExpansionRule, pub fixed_point: FlowFixedPointRequirement, pub accepted_gap: FlowGap,
}

/// The closed route of `family` — TOTAL over the family vocabulary: every
/// variant (and every edge class of `GraphEdge`) resolves to exactly one
/// registered expansion rule, one fixed-point requirement, and one
/// accepted typed gap. There is no wildcard arm and no `Option`.
#[rustfmt::skip]
pub const fn flow_family_route(family: &FlowFactFamily) -> FlowFamilyRoute {
    match family {
        FlowFactFamily::GraphEdge(_) => FlowFamilyRoute { rule: FlowExpansionRule::SelectedEdgeFacts, fixed_point: FlowFixedPointRequirement::FixedPoint, accepted_gap: FlowGap::UnmodeledExpression },
        FlowFactFamily::BindingSlot => FlowFamilyRoute { rule: FlowExpansionRule::BindingSlotFacts, fixed_point: FlowFixedPointRequirement::FixedPoint, accepted_gap: FlowGap::UnmodeledExpression },
        FlowFactFamily::ReturnSite => FlowFamilyRoute { rule: FlowExpansionRule::ReturnSiteFacts, fixed_point: FlowFixedPointRequirement::SinglePass, accepted_gap: FlowGap::AbruptCompletion },
        FlowFactFamily::GuardPredicate => FlowFamilyRoute { rule: FlowExpansionRule::GuardPredicateFacts, fixed_point: FlowFixedPointRequirement::FixedPoint, accepted_gap: FlowGap::GuardNarrowing },
        FlowFactFamily::CallSite => FlowFamilyRoute { rule: FlowExpansionRule::CallSiteFacts, fixed_point: FlowFixedPointRequirement::SinglePass, accepted_gap: FlowGap::UnmodeledExpression },
        FlowFactFamily::ContextualTarget => FlowFamilyRoute { rule: FlowExpansionRule::ContextualTargetFacts, fixed_point: FlowFixedPointRequirement::SinglePass, accepted_gap: FlowGap::UnmodeledExpression },
        FlowFactFamily::Capture => FlowFamilyRoute { rule: FlowExpansionRule::CaptureFacts, fixed_point: FlowFixedPointRequirement::SinglePass, accepted_gap: FlowGap::ClosureCapture },
        FlowFactFamily::SemanticRelation => FlowFamilyRoute { rule: FlowExpansionRule::SemanticRelationFacts, fixed_point: FlowFixedPointRequirement::FixedPoint, accepted_gap: FlowGap::NominalRelation },
    }
}

/// One domain's closure in the closed registry universe: the fact families
/// the domain requires. Each family's expansion rule, fixed-point
/// requirement, and accepted typed gap come from the TOTAL
/// [`flow_family_route`] mapping — the registry can never declare a family
/// without a registered route, and two domains naming one family can never
/// disagree about its route.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDomainClosure { pub domain: FlowDomain, pub families: &'static [FlowFactFamily] }

/// Whether an operation is a demand root or a semantic suboperation.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowOperationRole { Root, SemanticSuboperation }

/// `PendingReducer` roots surface typed gaps until their reducer exists;
/// `Live` suboperations keep their own production admission rails.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowOperationStatus { Enabled, PendingReducer, Live }

/// The finalizer kind an operation's result passes through.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowFinalizerKind { CompletenessProof, TypedGapOnly, Suboperation }

/// The result-contract descriptor of one operation: how its result may be
/// admitted and which gaps it may surface as typed partials.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowResultContractDescriptor { pub finalizer: FlowFinalizerKind, pub accepted_gaps: &'static [FlowGap] }

/// One row of the flow-operation contract registry. The requirement
/// universe is the CLOSED domain→family mapping `closures` (never two
/// unrelated flat lists): the contract's required domains are the closure
/// domains in declaration order, and its required fact families are the
/// closures' families deduplicated in first-declaration order. Each
/// family's expansion rule, fixed-point requirement, and accepted typed
/// gap come from the total [`flow_family_route`] mapping.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowOperationContract {
    pub tag: SemanticQueryKeyTag, pub role: FlowOperationRole, pub status: FlowOperationStatus,
    pub closures: &'static [FlowDomainClosure],
    pub result: FlowResultContractDescriptor,
}

impl FlowOperationContract {
    /// The contract's required domains, in closure declaration order.
    pub fn required_domains(&self) -> impl Iterator<Item = FlowDomain> + '_ {
        self.closures.iter().map(|closure| closure.domain)
    }

    /// The contract's required fact families, deduplicated in
    /// first-declaration order across the closures.
    pub fn required_fact_families(&self) -> Vec<FlowFactFamily> {
        let mut families: Vec<FlowFactFamily> = Vec::new();
        for closure in self.closures {
            for family in closure.families {
                if !families.contains(family) {
                    families.push(family.clone());
                }
            }
        }
        families
    }
}

#[rustfmt::skip]
const fn row(tag: SemanticQueryKeyTag, role: R, status: S, closures: &'static [FlowDomainClosure], result: FlowResultContractDescriptor) -> FlowOperationContract {
    FlowOperationContract { tag, role, status, closures, result }
}

#[rustfmt::skip]
const fn desc(finalizer: K, accepted_gaps: &'static [FlowGap]) -> FlowResultContractDescriptor {
    FlowResultContractDescriptor { finalizer, accepted_gaps }
}

#[rustfmt::skip]
const fn closure(domain: D, families: &'static [F]) -> FlowDomainClosure {
    FlowDomainClosure { domain, families }
}

// Shared family slices of the closed domain→family mapping. A family
// appears in every domain that consumes it; the planner dedups in
// first-declaration order.
#[rustfmt::skip]
const REACHING_VALUE_FAMILIES: &[F] = &[F::GraphEdge(FlowEdgeClass::ValueDef), F::GraphEdge(FlowEdgeClass::PathWrite), F::BindingSlot];
#[rustfmt::skip]
const REACHING_TYPE_FAMILIES: &[F] = &[F::GraphEdge(FlowEdgeClass::ValueDef), F::BindingSlot];
#[rustfmt::skip]
const NARROWING_FAMILIES: &[F] = &[F::GuardPredicate, F::GraphEdge(FlowEdgeClass::ControlRegion)];
#[rustfmt::skip]
const COMPLETION_FAMILIES: &[F] = &[F::ReturnSite, F::GraphEdge(FlowEdgeClass::ControlRegion)];
#[rustfmt::skip]
const CLOSURE_CAPTURE_FAMILIES: &[F] = &[F::Capture];
#[rustfmt::skip]
const FRESHNESS_FAMILIES: &[F] = &[F::BindingSlot, F::GraphEdge(FlowEdgeClass::ValueDef)];
#[rustfmt::skip]
const EFFECTS_FAMILIES: &[F] = &[F::GraphEdge(FlowEdgeClass::EvalEffect), F::CallSite];
#[rustfmt::skip]
const CALL_RESOLUTION_FAMILIES: &[F] = &[F::CallSite, F::ContextualTarget];
#[rustfmt::skip]
const RELATION_FAMILIES: &[F] = &[F::SemanticRelation, F::ContextualTarget];
#[rustfmt::skip]
const CONTEXTUAL_TYPING_FAMILIES: &[F] = &[F::ContextualTarget];
#[rustfmt::skip]
const RELATION_ONLY_FAMILIES: &[F] = &[F::SemanticRelation];

// The closed flow-operation contract registry: exactly the flow-bearing
// query tags. Lookup is total over `SemanticQueryKeyTag` and returns
// `Option` — there is no wildcard arm.
#[rustfmt::skip]
static FLOW_OPERATION_CONTRACTS: &[FlowOperationContract] = &[
    // The whole-function return producer: the one proof-enabled root.
    row(SemanticQueryKeyTag::FlowReturn, R::Root, S::Enabled,
        &[
            closure(D::ReachingValue, REACHING_VALUE_FAMILIES),
            closure(D::ReachingType, REACHING_TYPE_FAMILIES),
            closure(D::Narrowing, NARROWING_FAMILIES),
            closure(D::Completion, COMPLETION_FAMILIES),
            closure(D::ClosureCapture, CLOSURE_CAPTURE_FAMILIES),
            closure(D::Freshness, FRESHNESS_FAMILIES),
            closure(D::Effects, EFFECTS_FAMILIES),
            closure(D::CallResolution, CALL_RESOLUTION_FAMILIES),
            closure(D::Relation, RELATION_FAMILIES),
        ],
        desc(K::CompletenessProof, &[FlowGap::GuardNarrowing, FlowGap::NominalRelation, FlowGap::ClosureCapture, FlowGap::AbruptCompletion, FlowGap::UnmodeledExpression])),
    // Roots whose reducers do not exist yet: typed gaps only.
    row(SemanticQueryKeyTag::FlowNarrowingAt, R::Root, S::PendingReducer,
        &[
            closure(D::ReachingValue, REACHING_VALUE_FAMILIES),
            closure(D::ReachingType, REACHING_TYPE_FAMILIES),
            closure(D::Narrowing, NARROWING_FAMILIES),
            closure(D::Relation, RELATION_ONLY_FAMILIES),
        ],
        desc(K::TypedGapOnly, &[])),
    row(SemanticQueryKeyTag::ContextualTypeAt, R::Root, S::PendingReducer,
        &[
            closure(D::ReachingType, REACHING_TYPE_FAMILIES),
            closure(D::ContextualTyping, CONTEXTUAL_TYPING_FAMILIES),
            closure(D::CallResolution, CALL_RESOLUTION_FAMILIES),
            closure(D::Relation, RELATION_ONLY_FAMILIES),
        ],
        desc(K::TypedGapOnly, &[])),
    // Live semantic suboperations.
    row(SemanticQueryKeyTag::ResolveCall, R::SemanticSuboperation, S::Live,
        &[closure(D::CallResolution, CALL_RESOLUTION_FAMILIES), closure(D::Relation, RELATION_ONLY_FAMILIES)],
        desc(K::Suboperation, &[])),
    row(SemanticQueryKeyTag::Relate, R::SemanticSuboperation, S::Live,
        &[closure(D::Relation, RELATION_ONLY_FAMILIES)],
        desc(K::Suboperation, &[])),
];

/// The registered contract of `tag`, when the tag is flow-bearing.
#[rustfmt::skip]
pub fn flow_operation_contract(tag: SemanticQueryKeyTag) -> Option<&'static FlowOperationContract> {
    FLOW_OPERATION_CONTRACTS.iter().find(|c| c.tag == tag)
}

/// The registered contract of `tag`, or the offending tag.
#[rustfmt::skip]
pub fn require_flow_operation_contract(tag: SemanticQueryKeyTag) -> Result<&'static FlowOperationContract, SemanticQueryKeyTag> {
    flow_operation_contract(tag).ok_or(tag)
}

/// Require that `operation`'s contract declares `requirement`; the `Err`
/// carries the full offending requirement (operation included) so a caller
/// records a typed gap without losing the private reason.
pub fn require_registered_flow_requirement(
    operation: SemanticQueryKeyTag,
    requirement: &FlowRequirementKind,
) -> Result<(), FlowRequirement> {
    let reject = || FlowRequirement {
        operation,
        requirement: requirement.clone(),
    };
    let registered = flow_operation_contract(operation).is_some_and(|c| match requirement {
        RK::Domain(domain) => c.closures.iter().any(|closure| closure.domain == *domain),
        RK::FactFamily(family) => c
            .closures
            .iter()
            .any(|closure| closure.families.contains(family)),
    });
    if registered {
        Ok(())
    } else {
        Err(reject())
    }
}

// ── Planner semantics shared with the result-contract identity ─────────

/// The registered expansion vocabulary the planner expands through, in
/// registration order — the ONLY expansion channel. Exactly one rule per
/// fact-family variant (see [`flow_family_route`]). The result-contract
/// identity encodes this exact list, so a change to the registered
/// expansion semantics changes every minted contract identity.
const REGISTERED_EXPANSION_RULES: &[FlowExpansionRule] = &[
    E::BindingSlotFacts,
    E::ReturnSiteFacts,
    E::SelectedEdgeFacts,
    E::CallSiteFacts,
    E::GuardPredicateFacts,
    E::ContextualTargetFacts,
    E::CaptureFacts,
    E::SemanticRelationFacts,
];

/// The fixed-point iteration cap every plan carries.
const FLOW_FIXED_POINT_MAX_ITERATIONS: u32 = 16;

/// The tie-break rule every plan's work order is built with.
const FLOW_WORK_ORDER_TIE_BREAK: FlowTieBreak = FlowTieBreak::DomainNodeEdgeSlot;

// Explicit stable discriminants for the canonical result-contract
// encoding. The numeric value of each variant is identity schema: adding,
// removing, or renumbering a variant requires bumping the domain tag.
#[rustfmt::skip]
const fn domain_discriminant(domain: FlowDomain) -> u32 {
    match domain {
        FlowDomain::ReachingValue => 1, FlowDomain::ReachingType => 2, FlowDomain::Narrowing => 3,
        FlowDomain::Completion => 4, FlowDomain::ClosureCapture => 5, FlowDomain::Freshness => 6,
        FlowDomain::Effects => 7, FlowDomain::CallResolution => 8, FlowDomain::Relation => 9,
        FlowDomain::ContextualTyping => 10, FlowDomain::Coverage => 11,
    }
}

#[rustfmt::skip]
const fn edge_class_discriminant(class: FlowEdgeClass) -> u32 {
    match class {
        FlowEdgeClass::ValueDef => 1, FlowEdgeClass::PathWrite => 2,
        FlowEdgeClass::EvalEffect => 3, FlowEdgeClass::ControlRegion => 4,
    }
}

/// One fact family as a discriminant pair: the family tag, then the
/// edge-class discriminant for `GraphEdge` (`0` otherwise).
#[rustfmt::skip]
const fn fact_family_discriminants(family: &FlowFactFamily) -> [u32; 2] {
    match family {
        FlowFactFamily::GraphEdge(class) => [1, edge_class_discriminant(*class)],
        FlowFactFamily::BindingSlot => [2, 0], FlowFactFamily::ReturnSite => [3, 0],
        FlowFactFamily::GuardPredicate => [4, 0], FlowFactFamily::CallSite => [5, 0],
        FlowFactFamily::ContextualTarget => [6, 0], FlowFactFamily::Capture => [7, 0],
        FlowFactFamily::SemanticRelation => [8, 0],
    }
}

#[rustfmt::skip]
const fn gap_discriminant(gap: FlowGap) -> u32 {
    match gap {
        FlowGap::GuardNarrowing => 1, FlowGap::NominalRelation => 2, FlowGap::ClosureCapture => 3,
        FlowGap::AbruptCompletion => 4, FlowGap::UnmodeledExpression => 5,
    }
}

#[rustfmt::skip]
const fn role_discriminant(role: FlowOperationRole) -> u32 {
    match role { FlowOperationRole::Root => 1, FlowOperationRole::SemanticSuboperation => 2 }
}

#[rustfmt::skip]
const fn status_discriminant(status: FlowOperationStatus) -> u32 {
    match status {
        FlowOperationStatus::Enabled => 1, FlowOperationStatus::PendingReducer => 2,
        FlowOperationStatus::Live => 3,
    }
}

#[rustfmt::skip]
const fn finalizer_discriminant(finalizer: FlowFinalizerKind) -> u32 {
    match finalizer {
        FlowFinalizerKind::CompletenessProof => 1, FlowFinalizerKind::TypedGapOnly => 2,
        FlowFinalizerKind::Suboperation => 3,
    }
}

#[rustfmt::skip]
const fn expansion_rule_discriminant(rule: FlowExpansionRule) -> u32 {
    match rule {
        FlowExpansionRule::BindingSlotFacts => 1, FlowExpansionRule::ReturnSiteFacts => 2,
        FlowExpansionRule::SelectedEdgeFacts => 3, FlowExpansionRule::CallSiteFacts => 4,
        FlowExpansionRule::GuardPredicateFacts => 5, FlowExpansionRule::ContextualTargetFacts => 6,
        FlowExpansionRule::CaptureFacts => 7, FlowExpansionRule::SemanticRelationFacts => 8,
    }
}

#[rustfmt::skip]
const fn fixed_point_discriminant(requirement: FlowFixedPointRequirement) -> u32 {
    match requirement {
        FlowFixedPointRequirement::FixedPoint => 1, FlowFixedPointRequirement::SinglePass => 2,
    }
}

#[rustfmt::skip]
const fn tie_break_discriminant(tie_break: FlowTieBreak) -> u32 {
    match tie_break { FlowTieBreak::DomainNodeEdgeSlot => 1 }
}

/// Append one field carrying an ORDERED list of discriminants: the count
/// (u64 LE) followed by each discriminant (u32 LE) in declaration order.
fn encode_ordered_discriminants(
    e: &mut CanonicalEncoder,
    tag: u16,
    discriminants: impl IntoIterator<Item = u32>,
) {
    let discriminants: Vec<u32> = discriminants.into_iter().collect();
    let mut payload = Vec::with_capacity(8 + 4 * discriminants.len());
    payload.extend_from_slice(&(discriminants.len() as u64).to_le_bytes());
    for discriminant in discriminants {
        payload.extend_from_slice(&discriminant.to_le_bytes());
    }
    e.field_bytes(tag, &payload);
}

/// Canonical descriptor backing [`flow_result_contract_id`]: the COMPLETE
/// closed contract — the tag, the role and status, the closed
/// domain→family mapping (each domain with its ordered families AND each
/// family's registered route: expansion rule, fixed-point requirement,
/// accepted typed gap), the registered expansion and fixed-point semantics
/// the planner runs under, the finalizer kind, and the accepted gaps —
/// under a versioned domain tag. Any change to the closed contract's
/// semantics changes the minted identity.
struct ResultContractDescriptor<'a>(&'a FlowOperationContract);

#[rustfmt::skip]
impl CanonicalEncode for ResultContractDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.flow.result_contract.v3";
    fn encode_fields(&self, e: &mut CanonicalEncoder) {
        let contract = self.0;
        e.field_str(1, contract.tag.name());
        e.field_u32(2, role_discriminant(contract.role));
        e.field_u32(3, status_discriminant(contract.status));
        // The closed mapping, nested: per closure, the domain discriminant
        // and its family count, then per family the family discriminant
        // pair plus its registered route (rule, fixed-point, gap).
        encode_ordered_discriminants(e, 4, contract.closures.iter().flat_map(|closure| {
            std::iter::once(domain_discriminant(closure.domain))
                .chain(std::iter::once(closure.families.len() as u32))
                .chain(closure.families.iter().flat_map(|family| {
                    let route = flow_family_route(family);
                    let [family_tag, class_tag] = fact_family_discriminants(family);
                    [family_tag, class_tag, expansion_rule_discriminant(route.rule), fixed_point_discriminant(route.fixed_point), gap_discriminant(route.accepted_gap)]
                }))
        }));
        encode_ordered_discriminants(e, 6, REGISTERED_EXPANSION_RULES.iter().map(|rule| expansion_rule_discriminant(*rule)));
        e.field_u32(7, tie_break_discriminant(FLOW_WORK_ORDER_TIE_BREAK));
        e.field_u32(8, FLOW_FIXED_POINT_MAX_ITERATIONS);
        e.field_u32(9, finalizer_discriminant(contract.result.finalizer));
        encode_ordered_discriminants(e, 10, contract.result.accepted_gaps.iter().map(|gap| gap_discriminant(*gap)));
    }
}

/// The deterministic result-contract identity of one registered operation.
pub fn flow_result_contract_id(contract: &FlowOperationContract) -> ResultContractId {
    ResultContractId::from_canonical(&ResultContractDescriptor(contract))
}

/// The result-contract identity of the `FlowReturn` operation, derived
/// ONLY here from the closed registry row (the registry is static, so
/// this is a deterministic constant of the registry revision). The single
/// production `FlowReturnKey` constructor folds it into every key — a
/// caller-selected contract is unrepresentable on a production request.
#[must_use]
pub fn flow_return_result_contract_id() -> ResultContractId {
    flow_result_contract_id(
        flow_operation_contract(SemanticQueryKeyTag::FlowReturn)
            .expect("FlowReturn is a registered flow operation"),
    )
}

/// One flow demand: the full query identity (function, demand, input,
/// profile axes), the observation basis, and the policies. The demand
/// carries NO graph axis, NO subject axis, and NO result-contract axis:
/// the store-minted [`BoundFlowGraph`] pins the body identity, the subject
/// derives EXHAUSTIVELY from the operation-specific query payload, and the
/// result contract IS the `FlowReturnKey`'s own key-derived contract —
/// never caller-selected.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub struct FlowDemandRequest {
    pub query: SemanticQueryKey, pub input_basis: InputBasisId,
    pub resources: FlowResourcePolicy,
    pub additional_requirements: Arc<[FlowRequirement]>,
}

/// The complete basis a flow solve is bound to: a replay under any other
/// body, query, observation basis, or result contract is stale.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDemandBasis {
    pub(crate) graph_body: FlowSliceFunctionKey,
    pub query: SemanticQueryKey, pub input_basis: InputBasisId, pub result_contract: ResultContractId,
}

/// The subject of one flow demand: the demanded return-projection path in
/// authored key text (empty = the whole return). DERIVED from the query's
/// demand axis by [`build_flow_demand_plan`] — never caller-supplied.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDemandSubject { pub projection_path: Arc<[Arc<str>]> }

/// The frame's binding-inventory authority the planner resolves each
/// binding obligation's cross-frame identity against: the
/// `FunctionProgramIndex` entry's FULL binding list. Its slot numbering
/// IS the [`FlowBindingIdentity::binding_slot`] domain — the planner zips
/// the skeleton's binding index against it in source order, and a binding
/// whose record does not correspond is planned as unmodelable (a typed
/// gap at install), never with a fabricated slot.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub struct FlowBindingInventory { pub bindings: Arc<[FunctionBindingRecord]> }

/// The tie-break rule the work order is built with: domain rank, then
/// ascending graph-node index, then edge class and source ordinal, then
/// stable binding slot.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum FlowTieBreak { #[default] DomainNodeEdgeSlot }

/// The fixed-point convergence policy of one solve: the maximum fixed-point
/// iterations before the solve is budget-exhausted.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowConvergencePolicy { pub max_iterations: u32 }

/// The resource policy a demand plans and solves under.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowResourcePolicy { pub slice_budget: FlowSliceBudget, pub max_obligations: u32 }

#[rustfmt::skip]
impl Default for FlowResourcePolicy {
    fn default() -> Self { Self { slice_budget: FlowSliceBudget::default(), max_obligations: 1024 } }
}

/// The demand plan over one store-bound graph: the obligation and
/// completeness authority a solve installs, discharges, and finalizes
/// against. Not an alias of [`ReturnSlicePlan`] (graph reachability
/// selection only), stored here as the structural selection the obligations
/// expand from. `obligation_specs` enters a runtime only through
/// `install_flow_demand`.
///
/// SEALED: every field is private and the sole constructor is
/// [`build_flow_demand_plan`]. Consumers get immutable views only — no
/// mutable slices, no setters, no `DerefMut`, no public struct literal,
/// and no caller-supplied work order. The plan carries its own registry
/// closure (the contract's exact domain→family mapping) and required
/// fact families, so a solve can never be re-keyed to a different
/// requirement universe after planning.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub struct FlowDemandPlan {
    basis: FlowDemandBasis, subject: FlowDemandSubject,
    /// The structural selection (graph reachability result, planned once).
    structural_selection: ReturnSlicePlan,
    /// The contract-required domains, in domain-rank order.
    required_domains: Arc<[FlowDomain]>,
    /// The contract's required fact families, deduplicated in
    /// first-declaration order across the closed domain→family mapping.
    required_fact_families: Arc<[FlowFactFamily]>,
    /// The registry closure the plan was built against: the contract's
    /// exact domain→family mapping snapshot.
    registry_closure: Arc<[FlowDomainClosure]>,
    /// The family-coverage obligation ids: exactly ONE per required fact
    /// family (the family's enumeration obligation — "proved empty" is
    /// discharged, never "planner forgot the family").
    coverage_obligations: Arc<[FlowObligationId]>,
    /// The initial (contract-domain and caller-asserted) obligation ids.
    initial_obligations: Arc<[FlowObligationId]>,
    /// The expanded (structural-selection) obligation ids.
    expanded_obligations: Arc<[FlowObligationId]>,
    /// The deterministic work order over all obligations.
    work_order: Arc<[FlowObligationId]>,
    tie_break: FlowTieBreak, convergence: FlowConvergencePolicy, resources: FlowResourcePolicy,
    obligation_specs: Vec<FlowObligationSpec>,
}

#[rustfmt::skip]
impl FlowDemandPlan {
    /// The exact basis the demand was planned against.
    pub fn basis(&self) -> &FlowDemandBasis { &self.basis }
    /// The demand subject derived from the query's own demand axis.
    pub fn subject(&self) -> &FlowDemandSubject { &self.subject }
    /// The structural selection the obligations expand from.
    pub fn structural_selection(&self) -> &ReturnSlicePlan { &self.structural_selection }
    /// The contract-required domains, in domain-rank order.
    pub fn required_domains(&self) -> &[FlowDomain] { &self.required_domains }
    /// The contract's required fact families, deduplicated in
    /// first-declaration order.
    pub fn required_fact_families(&self) -> &[FlowFactFamily] { &self.required_fact_families }
    /// The registry closure the plan was built against.
    pub fn registry_closure(&self) -> &[FlowDomainClosure] { &self.registry_closure }
    /// The family-coverage obligation ids: exactly one per required
    /// family, in family order.
    pub fn coverage_obligations(&self) -> &[FlowObligationId] { &self.coverage_obligations }
    /// The initial (contract-domain and caller-asserted) obligation ids.
    pub fn initial_obligations(&self) -> &[FlowObligationId] { &self.initial_obligations }
    /// The expanded (structural-selection) obligation ids.
    pub fn expanded_obligations(&self) -> &[FlowObligationId] { &self.expanded_obligations }
    /// The deterministic work order over all obligations.
    pub fn work_order(&self) -> &[FlowObligationId] { &self.work_order }
    /// The tie-break rule the work order was built with.
    pub fn tie_break(&self) -> FlowTieBreak { self.tie_break }
    /// The fixed-point convergence policy of this solve.
    pub fn convergence(&self) -> FlowConvergencePolicy { self.convergence }
    /// The resource policy this demand planned under.
    pub fn resources(&self) -> FlowResourcePolicy { self.resources }
    /// The obligation specifications, in work order.
    pub fn obligation_specs(&self) -> &[FlowObligationSpec] {
        &self.obligation_specs
    }
}

/// Why a demand could not be planned: no registered contract, not a
/// proof-enabled root, a demand the flow planner cannot represent, the
/// slice budget tripped, the obligation budget tripped, or the query does
/// not name the bound graph's function and parse environment.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowDemandPlanError {
    UnregisteredOperation, NotAnEnabledRoot, UnrepresentableDemand,
    SliceBudget(FlowSliceBudgetExceeded),
    ObligationBudget { limit: u32, observed: u32 },
    BasisKeyMismatch,
}

/// Derive the demand subject EXHAUSTIVELY from the operation-specific
/// query payload — the demand axis the key itself carries. The planner
/// represents exactly the identity demand lattice point plus an authored
/// named-member projection path; every other demand (widened signature,
/// facet, or policy axes; numeric / symbol / index path segments) is a
/// typed planning error, never a silent default subject.
pub fn derive_demand_subject(
    query: &SemanticQueryKey,
) -> Result<FlowDemandSubject, FlowDemandPlanError> {
    let SemanticQueryKey::FlowReturn(key) = query else {
        return Err(FlowDemandPlanError::NotAnEnabledRoot);
    };
    let point = &key.demand.point;
    let identity = Demand::identity();
    let projection = &point.projection;
    let representable = point.policy == identity.policy
        && projection.facets == identity.projection.facets
        && projection.member_demand == identity.projection.member_demand
        && projection.call_signatures == identity.projection.call_signatures
        && projection.construct_signatures == identity.projection.construct_signatures
        && projection.index_signatures == identity.projection.index_signatures
        && projection.display_needs == identity.projection.display_needs;
    if !representable {
        return Err(FlowDemandPlanError::UnrepresentableDemand);
    }
    let mut path = Vec::with_capacity(projection.path.len());
    for segment in projection.path.as_slice() {
        match segment {
            PathSegment::Member(verter_type_expr::PropertyKey::String(name)) => {
                path.push(Arc::clone(name));
            }
            PathSegment::Member(_) | PathSegment::Index(_) => {
                return Err(FlowDemandPlanError::UnrepresentableDemand);
            }
        }
    }
    Ok(FlowDemandSubject {
        projection_path: Arc::from(path.into_boxed_slice()),
    })
}

/// Resolve every skeleton binding's cross-frame identity ONCE, in skeleton
/// order, against the frame's binding inventory. The inventory records the
/// mappable, non-destructured subset of the skeleton's bindings in the
/// same source order, so the zip consumes one inventory slot per mappable
/// non-destructured skeleton binding; a record that does not correspond
/// (a foreign inventory) yields NO identity, and kinds the cross-frame
/// vocabulary cannot name yield NONE by construction. The planner turns
/// `None` into an unmodelable obligation (a typed gap at install), never
/// a fabricated slot.
fn resolve_binding_identities(
    skeleton: &FunctionBodySkeleton,
    inventory: &FlowBindingInventory,
    function: &FunctionProgramKey,
) -> Vec<Option<FlowBindingIdentity>> {
    let mut cursor = 0usize;
    skeleton
        .bindings
        .iter()
        .map(|binding| {
            let kind = match binding.kind {
                SkeletonBindingKind::Param => FunctionBindingKind::Param,
                SkeletonBindingKind::Const => FunctionBindingKind::Const,
                SkeletonBindingKind::Let => FunctionBindingKind::Let,
                SkeletonBindingKind::Var => FunctionBindingKind::Var,
                SkeletonBindingKind::NestedFunction => FunctionBindingKind::NestedFunction,
                SkeletonBindingKind::Class
                | SkeletonBindingKind::CatchParam
                | SkeletonBindingKind::Enum
                | SkeletonBindingKind::Namespace
                | SkeletonBindingKind::ImportEquals
                | SkeletonBindingKind::TypeAlias
                | SkeletonBindingKind::Interface => return None,
            };
            // A destructuring-pattern element is no whole-slot inventory
            // entry; it consumes no slot.
            if binding.destructured {
                return None;
            }
            let slot = cursor;
            cursor += 1;
            let record = inventory.bindings.get(slot)?;
            if record.name.as_ref() != skeleton.name(binding.name) || record.kind != kind {
                return None;
            }
            Some(FlowBindingIdentity {
                name: Arc::clone(&record.name),
                kind,
                defining_function: function.clone(),
                binding_slot: u32::try_from(slot).unwrap_or(u32::MAX),
            })
        })
        .collect()
}

/// The query↔graph coherence proof: planning rejects unless every
/// identity axis present in BOTH the query's `FlowReturnKey` and the
/// bound graph's `FlowSliceFunctionKey` matches — the canonical identity,
/// the owner, the merged-symbol name, the symbol space, the function
/// part, the overload ordinal, and the parse-environment hash. Without
/// this check a harness could supply graph A with a query naming function
/// B, discharge A's obligations, and seal a `Complete` the query never
/// addressed; the splice is a typed planning error, never a plan.
fn require_query_names_bound_graph(
    query: &SemanticQueryKey,
    bound: &FlowSliceFunctionKey,
) -> Result<(), FlowDemandPlanError> {
    let SemanticQueryKey::FlowReturn(key) = query else {
        return Err(FlowDemandPlanError::NotAnEnabledRoot);
    };
    let slot = &key.function.declaration_slot;
    let query_space = match slot.symbol_space {
        crate::semantic_query::SemanticSymbolSpace::Type => {
            verter_semantic::facts::SymbolSpace::Type
        }
        crate::semantic_query::SemanticSymbolSpace::Value => {
            verter_semantic::facts::SymbolSpace::Value
        }
        crate::semantic_query::SemanticSymbolSpace::Namespace => {
            verter_semantic::facts::SymbolSpace::Namespace
        }
    };
    let coherent = slot.defining_canonical.as_ref() == bound.canonical_id.as_ref()
        && slot.owner == bound.function.declaration.owner
        && slot.merged_symbol_name.as_ref() == bound.function.declaration.name.as_ref()
        && query_space == bound.function.declaration.space
        && key.function.function_part == bound.function.part
        && key.function.overload_ordinal == bound.function.overload_ordinal
        && key.context.parse_env_hash == bound.parse_env_hash;
    if coherent {
        Ok(())
    } else {
        Err(FlowDemandPlanError::BasisKeyMismatch)
    }
}

/// Build the demand plan of `request` over the store-minted `bound` graph,
/// assembling obligations from the ALREADY-PLANNED structural selection —
/// the one plan the hash node's cold compute produced and retained
/// (`PlannedFlowSlice`); this function never builds or reacquires a graph
/// and never re-plans. The
/// plan's body identity IS the bound graph's key, the subject derives from
/// the query's own demand axis, the result contract IS the key's
/// constructor-derived contract, and obligations expand only through the
/// registered [`FlowExpansionRule`]s, in domain rank, ascending node
/// index, edge class and source ordinal order. Every obligation spec
/// carries its closed semantic identity (populated from the skeleton/graph
/// and the frame's binding inventory) and its exact evidence contract.
/// Obligation insertion is budget-checked BEFORE each append: the first
/// excess returns the typed budget error and the remaining population is
/// never constructed or scanned.
#[rustfmt::skip]
pub(crate) fn build_flow_demand_plan(
    request: FlowDemandRequest,
    bound: &BoundFlowGraph,
    structural_selection: ReturnSlicePlan,
    inventory: &FlowBindingInventory,
) -> Result<FlowDemandPlan, FlowDemandPlanError> {
    let tag = request.query.tag();
    let contract = require_flow_operation_contract(tag).map_err(|_| FlowDemandPlanError::UnregisteredOperation)?;
    if contract.role != R::Root || contract.status != S::Enabled {
        return Err(FlowDemandPlanError::NotAnEnabledRoot);
    }
    let subject = derive_demand_subject(&request.query)?;
    require_query_names_bound_graph(&request.query, bound.key())?;
    let SemanticQueryKey::FlowReturn(key) = &request.query else {
        return Err(FlowDemandPlanError::NotAnEnabledRoot);
    };
    // The result contract is the KEY's — derived by the single production
    // key constructor from the closed registry row; the request carries no
    // caller-selected contract axis.
    let result_contract = key.result_contract.clone();
    let bundle = bound.bundle();
    let max_obligations = request.resources.max_obligations;

    let mut specs: Vec<FlowObligationSpec> = Vec::new();
    let mut push = |requirement: FlowRequirement, origin: FlowObligationOrigin, basis: FlowObligationBasis,
                    expected_dependencies: Arc<[FlowObligationId]>, expected_suboperations: Arc<[SemanticQueryKeyTag]>|
     -> Result<FlowObligationId, FlowDemandPlanError> {
        let next = specs.len() as u64 + 1;
        if next > u64::from(max_obligations) {
            return Err(FlowDemandPlanError::ObligationBudget {
                limit: max_obligations,
                observed: u32::try_from(next).unwrap_or(u32::MAX),
            });
        }
        let id = FlowObligationId(u32::try_from(specs.len()).unwrap_or(u32::MAX));
        specs.push(FlowObligationSpec::new(id, requirement, origin, basis, expected_dependencies, expected_suboperations));
        Ok(id)
    };

    // The closed requirement universe of this demand: the contract's
    // families deduplicated in first-declaration order across its domain
    // closures. Every family routes through the TOTAL `flow_family_route`
    // mapping — one registered expansion rule, one fixed-point
    // requirement, one accepted typed gap.
    let families: Vec<FlowFactFamily> = contract.required_fact_families();

    // Initial obligations: one per contract-required domain, domain-rank order.
    let mut domains: Vec<FlowDomain> = contract.required_domains().collect();
    domains.sort();
    // `additional_requirements` is unbounded caller input: count its
    // non-duplicate contribution BEFORE constructing any obligation for
    // it, so an oversized vector trips the budget without a single spec.
    // The counted base also includes the family-coverage obligations (one
    // per required family) — the registry-closure population is part of
    // the initial budget, planned before any concrete expansion.
    let additional_count = request.additional_requirements.iter()
        .filter(|requirement| !(requirement.operation == tag && matches!(&requirement.requirement, RK::Domain(domain) if domains.contains(domain))))
        .count() as u64;
    let counted_base = families.len() as u64 + domains.len() as u64 + additional_count;
    if counted_base > u64::from(max_obligations) {
        return Err(FlowDemandPlanError::ObligationBudget {
            limit: max_obligations,
            observed: u32::try_from(counted_base).unwrap_or(u32::MAX),
        });
    }

    // Family-coverage obligations: exactly ONE per required family, in
    // family order — the family's enumeration obligation. Even a family
    // with zero concrete instances gets its coverage obligation, so
    // "proved empty" is a discharged obligation, never a forgotten family.
    let mut coverage: Vec<FlowObligationId> = Vec::with_capacity(families.len());
    for family in &families {
        let route = flow_family_route(family);
        coverage.push(push(
            FlowRequirement { operation: tag, requirement: RK::FactFamily(family.clone()) },
            FlowObligationOrigin::Expansion(route.rule),
            FlowObligationBasis::FamilyCoverage { family: family.clone() },
            Arc::from([]), Arc::from([]),
        )?);
    }

    // Domain obligations: each domain depends on EXACTLY the coverage
    // obligations of the families its closure maps it to, so a generic
    // domain discharge can never bypass a missing family enumeration.
    let mut initial: Vec<FlowObligationId> = Vec::with_capacity(domains.len());
    for domain in domains.iter().copied() {
        let dependencies: Vec<FlowObligationId> = contract.closures.iter()
            .filter(|closure| closure.domain == domain)
            .flat_map(|closure| closure.families.iter())
            .map(|family| {
                let index = families.iter().position(|candidate| candidate == family)
                    .expect("every closure family is a required family");
                coverage[index]
            })
            .collect();
        let basis = FlowObligationBasis::DemandRoot { subject: subject.clone() };
        initial.push(push(FlowRequirement { operation: tag, requirement: RK::Domain(domain) }, FlowObligationOrigin::ContractDomain, basis, Arc::from(dependencies.into_boxed_slice()), Arc::from([]))?);
    }
    // Caller-asserted requirements beyond the contract. A duplicate of an
    // already-planned contract domain of this root collapses onto it; every
    // other requirement — registered or not — gets its own obligation (the
    // runtime installs undeclared ones directly in `Gap` state).
    for requirement in request.additional_requirements.iter() {
        let duplicate = matches!(&requirement.requirement, RK::Domain(domain) if domains.contains(domain));
        if requirement.operation == tag && duplicate { continue; }
        let basis = FlowObligationBasis::DemandRoot { subject: subject.clone() };
        initial.push(push(requirement.clone(), FlowObligationOrigin::Additional, basis, Arc::from([]), Arc::from([]))?);
    }

    // The cross-frame binding identities, resolved once against the
    // frame's binding inventory (the ONE slot-numbering authority).
    let identities = resolve_binding_identities(&bundle.skeleton, inventory, &bound.key().function);

    let graph = &bundle.graph;
    let mut selected: Vec<_> = structural_selection.value_nodes.iter()
        .chain(structural_selection.effect_only_nodes.iter()).copied().collect();
    selected.sort_by_key(|node| node.index());
    let mut expanded: Vec<FlowObligationId> = Vec::new();
    // The FIRST obligation planned for a node is its primary obligation —
    // the dependency anchor of the node's out-edge facts.
    let mut node_obligations: FxHashMap<FlowNodeId, FlowObligationId> = FxHashMap::default();
    let note_node_obligation = |node_obligations: &mut FxHashMap<FlowNodeId, FlowObligationId>, node: FlowNodeId, id: FlowObligationId| {
        node_obligations.entry(node).or_insert(id);
    };
    // The call occurrences planned by the `CallSite` route: the dynamic
    // `SemanticRelation` facts anchor on these registered expansion events.
    let mut call_obligations: Vec<(FlowNodeId, verter_semantic::analysis::flow::SkeletonExprSiteId, u32, FlowObligationId)> = Vec::new();

    // The exhaustive, wildcard-free expansion dispatcher: EVERY required
    // family is iterated through its registered route. Node-kind facts
    // expand first (in family declaration order, ascending node index
    // inside a family); selected-edge facts expand last because their
    // dependency contract anchors on the node obligations.
    for family in &families {
        match family {
            F::GraphEdge(_) => {} // edges expand last — they anchor on node obligations
            F::BindingSlot => {
                for node in &selected {
                    let FlowNodeKind::Binding(binding) = graph.node_kind(*node) else { continue };
                    let basis = match &identities[binding.index()] {
                        Some(identity) => FlowObligationBasis::Binding {
                            node: *node,
                            slot: FlowBindingBasis { binding, identity: identity.clone() },
                        },
                        None => FlowObligationBasis::UnmodeledBinding {
                            node: *node, binding, kind: bundle.skeleton.binding(binding).kind,
                        },
                    };
                    let id = push(FlowRequirement { operation: tag, requirement: RK::FactFamily(F::BindingSlot) }, FlowObligationOrigin::Expansion(E::BindingSlotFacts), basis, Arc::from([]), Arc::from([]))?;
                    note_node_obligation(&mut node_obligations, *node, id);
                    expanded.push(id);
                }
            }
            F::ReturnSite => {
                for node in &selected {
                    let kind @ FlowNodeKind::ReturnSite(_) = graph.node_kind(*node) else { continue };
                    let id = push(FlowRequirement { operation: tag, requirement: RK::FactFamily(F::ReturnSite) }, FlowObligationOrigin::Expansion(E::ReturnSiteFacts), FlowObligationBasis::Site { node: *node, kind }, Arc::from([]), Arc::from([]))?;
                    note_node_obligation(&mut node_obligations, *node, id);
                    expanded.push(id);
                }
            }
            F::GuardPredicate => {
                // Guards anchor on (region, control input): one obligation
                // per selected predicated region.
                for node in &selected {
                    let FlowNodeKind::Region(region) = graph.node_kind(*node) else { continue };
                    let Some(control_input) = bundle.skeleton.region(region).control_input else { continue };
                    let id = push(FlowRequirement { operation: tag, requirement: RK::FactFamily(F::GuardPredicate) }, FlowObligationOrigin::Expansion(E::GuardPredicateFacts), FlowObligationBasis::Guard { node: *node, region, control_input }, Arc::from([]), Arc::from([]))?;
                    note_node_obligation(&mut node_obligations, *node, id);
                    expanded.push(id);
                }
            }
            F::CallSite => {
                // Every concrete call occurrence gets its OWN identity:
                // one call obligation per (expression site, call ordinal),
                // never one per site.
                for node in &selected {
                    let FlowNodeKind::ExprSite(site) = graph.node_kind(*node) else { continue };
                    let calls = &bundle.skeleton.expr_site(site).calls;
                    for (call_ordinal, _call) in calls.iter().enumerate() {
                        let call_ordinal = u32::try_from(call_ordinal).unwrap_or(u32::MAX);
                        let id = push(
                            FlowRequirement { operation: SemanticQueryKeyTag::ResolveCall, requirement: RK::FactFamily(F::CallSite) },
                            FlowObligationOrigin::Expansion(E::CallSiteFacts),
                            FlowObligationBasis::CallSite { node: *node, site, call_ordinal },
                            Arc::from([]), Arc::from([SemanticQueryKeyTag::ResolveCall]),
                        )?;
                        note_node_obligation(&mut node_obligations, *node, id);
                        call_obligations.push((*node, site, call_ordinal, id));
                        expanded.push(id);
                    }
                }
            }
            F::ContextualTarget => {
                // One contextual target per selected expression site.
                for node in &selected {
                    let FlowNodeKind::ExprSite(site) = graph.node_kind(*node) else { continue };
                    let id = push(FlowRequirement { operation: tag, requirement: RK::FactFamily(F::ContextualTarget) }, FlowObligationOrigin::Expansion(E::ContextualTargetFacts), FlowObligationBasis::ContextualTarget { node: *node, site }, Arc::from([]), Arc::from([]))?;
                    note_node_obligation(&mut node_obligations, *node, id);
                    expanded.push(id);
                }
            }
            F::Capture => {
                // Nested function DECLARATIONS anchor on the nested
                // function's binding identity. The capture SET of a
                // nested body is beyond this skeleton's authority (nested
                // bodies carry no reads here), so each nested-function
                // subject installs as the family's accepted typed gap —
                // never an omission.
                for node in &selected {
                    let FlowNodeKind::Binding(binding) = graph.node_kind(*node) else { continue };
                    if bundle.skeleton.binding(binding).kind != SkeletonBindingKind::NestedFunction { continue; }
                    let id = push(
                        FlowRequirement { operation: tag, requirement: RK::FactFamily(F::Capture) },
                        FlowObligationOrigin::Expansion(E::CaptureFacts),
                        FlowObligationBasis::Capture { node: *node, binding, identity: identities[binding.index()].clone() },
                        Arc::from([]), Arc::from([]),
                    )?;
                    expanded.push(id);
                }
                // Closure EXPRESSIONS (arrow / function expression sites):
                // the skeleton authority records the exact captured names
                // on the closure's own site — one concrete capture
                // obligation per (closure site, captured binding),
                // resolved through the frame's lexical binding authority
                // and carrying the binding's real cross-frame identity. A
                // captured name the frame does not bind is a free/global
                // read, not a capture; a captured binding the cross-frame
                // inventory cannot name (a destructured parameter)
                // installs the family's accepted typed gap — never
                // silence.
                for node in &selected {
                    let FlowNodeKind::ExprSite(site) = graph.node_kind(*node) else { continue };
                    let site_record = bundle.skeleton.expr_site(site);
                    let mut seen: Vec<SkeletonBindingId> = Vec::new();
                    for name in site_record.captures.iter() {
                        for binding in bundle.skeleton.bindings_of_name_in_scope(*name, site_record.region) {
                            if seen.contains(&binding) { continue; }
                            seen.push(binding);
                            let (basis, dischargeable) = match &identities[binding.index()] {
                                Some(identity) => (
                                    FlowObligationBasis::CapturedBinding { node: *node, site, identity: identity.clone() },
                                    true,
                                ),
                                None => (
                                    FlowObligationBasis::Capture { node: *node, binding, identity: None },
                                    false,
                                ),
                            };
                            let id = push(
                                FlowRequirement { operation: tag, requirement: RK::FactFamily(F::Capture) },
                                FlowObligationOrigin::Expansion(E::CaptureFacts),
                                basis,
                                Arc::from([]), Arc::from([]),
                            )?;
                            // A concrete capture subject discharges, so it
                            // may anchor the node's out-edge facts; the
                            // gap path never anchors (a gap discharges
                            // nothing a dependent could wait on).
                            if dischargeable {
                                note_node_obligation(&mut node_obligations, *node, id);
                            }
                            expanded.push(id);
                        }
                    }
                }
            }
            F::SemanticRelation => {
                // Dynamic relations anchor on the registered call-expansion
                // events: one relation obligation per call occurrence,
                // depending on EXACTLY its call obligation.
                for (node, site, call_ordinal, call_id) in &call_obligations {
                    let id = push(
                        FlowRequirement { operation: tag, requirement: RK::FactFamily(F::SemanticRelation) },
                        FlowObligationOrigin::Expansion(E::SemanticRelationFacts),
                        FlowObligationBasis::SemanticRelation { node: *node, site: *site, call_ordinal: *call_ordinal },
                        Arc::from(vec![*call_id].into_boxed_slice()),
                        Arc::from([SemanticQueryKeyTag::Relate]),
                    )?;
                    expanded.push(id);
                }
            }
        }
    }
    // The selected-edge facts expand last, per required `GraphEdge` class
    // in family declaration order, then (node, source ordinal) order.
    for family in &families {
        let F::GraphEdge(class) = family else { continue };
        for node in &selected {
            let mut edges: Vec<_> = graph.out_edges(*node).iter()
                .filter(|edge| structural_selection.is_selected(edge.to) && edge.kind.class() == *class)
                .collect();
            edges.sort_by_key(|edge| edge.ordinal);
            for edge in edges {
                let basis = FlowObligationBasis::Edge { from: edge.from, to: edge.to, class: *class, ordinal: edge.ordinal };
                // The exact dependency contract of an edge-fact obligation:
                // the obligation planned for the edge's source node, when
                // that node carries one.
                let dependencies: Arc<[FlowObligationId]> = match node_obligations.get(&edge.from) {
                    Some(dependency) => Arc::from(vec![*dependency].into_boxed_slice()),
                    None => Arc::from([]),
                };
                let id = push(FlowRequirement { operation: tag, requirement: RK::FactFamily(F::GraphEdge(*class)) }, FlowObligationOrigin::Expansion(E::SelectedEdgeFacts), basis, dependencies, Arc::from([]))?;
                expanded.push(id);
            }
        }
    }
    let mut work_order = coverage.clone();
    work_order.extend(initial.iter().copied());
    work_order.extend(expanded.iter().copied());
    let basis = FlowDemandBasis {
        graph_body: bound.key().clone(), query: request.query,
        input_basis: request.input_basis, result_contract,
    };
    Ok(FlowDemandPlan {
        basis, subject, structural_selection,
        required_domains: Arc::from(domains.into_boxed_slice()),
        required_fact_families: Arc::from(families.into_boxed_slice()),
        registry_closure: Arc::from(contract.closures.to_vec().into_boxed_slice()),
        coverage_obligations: Arc::from(coverage.into_boxed_slice()),
        initial_obligations: Arc::from(initial.into_boxed_slice()),
        expanded_obligations: Arc::from(expanded.into_boxed_slice()),
        work_order: Arc::from(work_order.into_boxed_slice()),
        tie_break: FLOW_WORK_ORDER_TIE_BREAK,
        convergence: FlowConvergencePolicy { max_iterations: FLOW_FIXED_POINT_MAX_ITERATIONS },
        resources: request.resources, obligation_specs: specs,
    })
}

/// The class of one flow-solve failure: cancellation, budget exhaustion, a
/// stale basis, a panic marker, or an internal failure.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowFailureClass { Cancelled, BudgetExhausted, StaleBasis, Panic, Internal }

/// A typed flow-solve failure.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowFailure { pub class: FlowFailureClass }

/// Why a solve is not complete: no installed demand, a stale basis, an
/// unprovable operation, a foreign result contract, a non-exact obligation
/// set, an unfinished obligation, a typed gap, a failure, or
/// non-convergence.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowPartialReason {
    NoDemandInstalled, StaleBasis, OperationNotProvable, ResultContractMismatch,
    ObligationSetMismatch, IncompleteObligations, Gap(FlowGap), Failed(FlowFailure),
    NonConverged, DegradedValue,
}

/// A non-complete outcome: the basis plus the typed reason. NEVER a warm
/// candidate.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartialFlowResult { pub basis: FlowDemandBasis, pub reason: FlowPartialReason }

/// The parts a [`CompleteFlowResult`] is minted from. Private to this
/// module: only [`finalize_flow_solve`] assembles them.
#[rustfmt::skip]
#[derive(Debug)]
struct CompleteFlowResultParts {
    basis: FlowDemandBasis, value: FlowReturnResult,
    convergence: FlowConvergenceEvidence, discharged: Arc<[FlowObligationId]>,
}

/// The proof that a flow solve completed: every planned obligation of the
/// demand discharged with spec-validated evidence under the exact basis
/// the demand was planned against, with runtime-observed deterministic
/// convergence. Fields are private and the sole constructor is private to
/// this module, so a proof is unforgeable outside [`finalize_flow_solve`].
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompleteFlowResult {
    basis: FlowDemandBasis, value: FlowReturnResult,
    convergence: FlowConvergenceEvidence, discharged: Arc<[FlowObligationId]>,
}

impl CompleteFlowResult {
    /// The sole constructor — private to this module; only the finalizer
    /// may mint a proof.
    #[rustfmt::skip]
    fn new(parts: CompleteFlowResultParts) -> Self {
        Self { basis: parts.basis, value: parts.value, convergence: parts.convergence, discharged: parts.discharged }
    }

    /// The completed value payload.
    #[must_use]
    pub fn value(&self) -> &FlowReturnResult {
        &self.value
    }
}

/// The outcome of one flow solve.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub enum FlowSolveOutcome { Complete(CompleteFlowResult), Partial(PartialFlowResult) }

impl FlowSolveOutcome {
    /// The warm-admission candidate: `Some` ONLY for a proof-bearing
    /// complete result. No gap, failure, cancellation, stale basis, or
    /// partial replay is ever a warm candidate.
    #[must_use]
    pub fn warm_candidate(&self) -> Option<&CompleteFlowResult> {
        match self {
            Self::Complete(complete) => Some(complete),
            Self::Partial(_) => None,
        }
    }
}

/// The sole proof-bearing finalizer. CONSUMES the runtime-sealed
/// completion artifact — the artifact is one-shot and non-cloneable, so
/// finalization owns it outright — never a separate value or
/// caller-authored convergence evidence — and verifies the three-way
/// binding of artifact, runtime, and plan: the artifact's basis IS the
/// plan's basis IS the installed basis (the demand named by `handle`),
/// the operation contract is coherent
/// (registered, proof-enabled root, matching result contract), the
/// artifact's discharge proofs equal the plan's exact spec set and the
/// runtime's current records with every obligation `Discharged`, and the
/// runtime-observed convergence matches the plan's policy. Every gap,
/// stale basis, cancellation, budget exhaustion, panic marker, internal
/// failure, non-convergence, or degraded value is a typed partial.
#[rustfmt::skip]
pub fn finalize_flow_solve(
    runtime: &ObligationRuntime, handle: FlowDemandHandle, plan: &FlowDemandPlan, completion: SealedFlowCompletion,
) -> FlowSolveOutcome {
    let partial = |reason: FlowPartialReason| FlowSolveOutcome::Partial(PartialFlowResult { basis: plan.basis().clone(), reason });

    let Some(installed) = runtime.flow_basis(handle) else { return partial(FlowPartialReason::NoDemandInstalled) };
    if installed != plan.basis() { return partial(FlowPartialReason::StaleBasis); }
    if completion.basis() != plan.basis() { return partial(FlowPartialReason::StaleBasis); }
    let Some(contract) = flow_operation_contract(plan.basis().query.tag()) else {
        return partial(FlowPartialReason::OperationNotProvable);
    };
    if contract.role != R::Root || contract.status != S::Enabled {
        return partial(FlowPartialReason::OperationNotProvable);
    }
    if plan.basis().result_contract != flow_result_contract_id(contract) {
        return partial(FlowPartialReason::ResultContractMismatch);
    }
    let Some(records) = runtime.flow_obligations(handle) else { return partial(FlowPartialReason::NoDemandInstalled) };
    let proofs = completion.proofs();
    let specs = plan.obligation_specs();
    let exact_set = records.len() == specs.len() && proofs.len() == specs.len()
        && records.iter().zip(proofs.iter()).all(|(record, proof)| record == proof)
        && records.iter().zip(specs.iter()).all(|(record, spec)| record.spec == *spec);
    if !exact_set { return partial(FlowPartialReason::ObligationSetMismatch); }
    for record in records {
        match &record.state {
            ObligationState::Discharged(_) => {}
            ObligationState::Gap(gap) => return partial(FlowPartialReason::Gap(*gap)),
            ObligationState::Failed(failure) => return partial(FlowPartialReason::Failed(*failure)),
            ObligationState::Pending | ObligationState::Running => {
                return partial(FlowPartialReason::IncompleteObligations);
            }
        }
    }
    if completion.convergence().policy() != plan.convergence() {
        return partial(FlowPartialReason::NonConverged);
    }
    if completion.value().degradation().is_some() { return partial(FlowPartialReason::DegradedValue); }

    FlowSolveOutcome::Complete(CompleteFlowResult::new(CompleteFlowResultParts {
        basis: plan.basis().clone(), value: completion.value().clone(),
        convergence: *completion.convergence(), discharged: Arc::from(plan.work_order().to_vec().into_boxed_slice()),
    }))
}
