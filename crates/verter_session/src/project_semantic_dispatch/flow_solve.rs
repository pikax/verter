//! The private completeness-proof layer for flow-bearing semantic
//! operations. Hermetic: reachable only from test code, never from a
//! production entry point, and sharing the ONE graph authority (the
//! memoized `FlowGraphBundle`) and the ONE closed query registry
//! ([`SemanticQueryKeyTag`]) — no second graph, planner, or resolver.
//!
//! - The flow-operation contract registry projects the flow-contract
//!   columns over the closed query tags; an undeclared requirement is NEVER
//!   a wildcard fallthrough — it installs as a `Gap(FlowGap)` obligation
//!   that retains the offending requirement.
//! - [`FlowDemandPlan`] is the demand/completeness authority over one
//!   memoized graph bundle — NOT an alias of `ReturnSlicePlan` (graph
//!   reachability selection only, stored here as the structural selection).
//! - [`finalize_flow_solve`] is the sole proof-bearing finalizer and the
//!   ONLY minter of [`CompleteFlowResult`] (its constructor is private).

use std::sync::Arc;

use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{InputBasisId, ResultContractId};
use verter_semantic::analysis::flow::flow_graph::{FlowEdgeClass, FlowNodeKind};
use verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan;
use verter_semantic::analysis::flow::peeker::{
    FlowSliceBudget, FlowSliceBudgetExceeded, ReturnPathPeeker, SliceDemand,
};

use super::dispatch_txn::flow_obligation_state::{
    FlowObligationId, FlowObligationOrigin, FlowObligationSpec, ObligationState,
};
use super::dispatch_txn::ObligationRuntime;
use crate::cache_runtime::flow_slice_node::{FlowGraphBundle, FlowSliceFunctionKey};
use crate::semantic_query::{FlowGap, FlowReturnResult, SemanticQueryKey, SemanticQueryKeyTag};

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
/// expansion channel.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowExpansionRule { BindingSlotFacts, ReturnSiteFacts, SelectedEdgeFacts, CallSiteFacts }

/// Whether an operation is a demand root or a semantic suboperation.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowOperationRole { Root, SemanticSuboperation }

/// `PendingReducer` roots surface typed gaps until their reducer exists;
/// `Live` suboperations keep their own production admission rails.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowOperationStatus { EnabledHermetic, PendingReducer, Live }

/// The finalizer kind an operation's result passes through.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FlowFinalizerKind { CompletenessProof, TypedGapOnly, Suboperation }

/// The result-contract descriptor of one operation: how its result may be
/// admitted and which gaps it may surface as typed partials.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowResultContractDescriptor { pub finalizer: FlowFinalizerKind, pub accepted_gaps: &'static [FlowGap] }

/// One row of the flow-operation contract registry.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowOperationContract {
    pub tag: SemanticQueryKeyTag, pub role: FlowOperationRole, pub status: FlowOperationStatus,
    pub required_domains: &'static [FlowDomain], pub required_fact_families: &'static [FlowFactFamily],
    pub result: FlowResultContractDescriptor,
}

#[rustfmt::skip]
const fn row(tag: SemanticQueryKeyTag, role: R, status: S, domains: &'static [D], families: &'static [F], result: FlowResultContractDescriptor) -> FlowOperationContract {
    FlowOperationContract { tag, role, status, required_domains: domains, required_fact_families: families, result }
}

#[rustfmt::skip]
const fn desc(finalizer: K, accepted_gaps: &'static [FlowGap]) -> FlowResultContractDescriptor {
    FlowResultContractDescriptor { finalizer, accepted_gaps }
}

// The closed flow-operation contract registry: exactly the flow-bearing
// query tags. Lookup is total over `SemanticQueryKeyTag` and returns
// `Option` — there is no wildcard arm.
#[rustfmt::skip]
static FLOW_OPERATION_CONTRACTS: &[FlowOperationContract] = &[
    // The whole-function return producer: the one proof-enabled root.
    row(SemanticQueryKeyTag::FlowReturn, R::Root, S::EnabledHermetic,
        &[D::ReachingValue, D::ReachingType, D::Narrowing, D::Completion, D::ClosureCapture, D::Freshness, D::Effects, D::CallResolution, D::Relation],
        &[F::GraphEdge(FlowEdgeClass::ValueDef), F::GraphEdge(FlowEdgeClass::PathWrite), F::GraphEdge(FlowEdgeClass::EvalEffect), F::GraphEdge(FlowEdgeClass::ControlRegion),
          F::BindingSlot, F::ReturnSite, F::GuardPredicate, F::CallSite, F::ContextualTarget, F::Capture, F::SemanticRelation],
        desc(K::CompletenessProof, &[FlowGap::GuardNarrowing, FlowGap::NominalRelation, FlowGap::ClosureCapture, FlowGap::AbruptCompletion, FlowGap::UnmodeledExpression])),
    // Roots whose reducers do not exist yet: typed gaps only.
    row(SemanticQueryKeyTag::FlowNarrowingAt, R::Root, S::PendingReducer,
        &[D::ReachingValue, D::ReachingType, D::Narrowing, D::Relation],
        &[F::GraphEdge(FlowEdgeClass::ValueDef), F::GraphEdge(FlowEdgeClass::PathWrite), F::GraphEdge(FlowEdgeClass::ControlRegion), F::BindingSlot, F::GuardPredicate, F::SemanticRelation],
        desc(K::TypedGapOnly, &[])),
    row(SemanticQueryKeyTag::ContextualTypeAt, R::Root, S::PendingReducer,
        &[D::ReachingType, D::ContextualTyping, D::CallResolution, D::Relation],
        &[F::GraphEdge(FlowEdgeClass::ValueDef), F::BindingSlot, F::CallSite, F::ContextualTarget, F::SemanticRelation],
        desc(K::TypedGapOnly, &[])),
    // Live semantic suboperations.
    row(SemanticQueryKeyTag::ResolveCall, R::SemanticSuboperation, S::Live,
        &[D::CallResolution, D::Relation], &[F::CallSite, F::SemanticRelation], desc(K::Suboperation, &[])),
    row(SemanticQueryKeyTag::Relate, R::SemanticSuboperation, S::Live,
        &[D::Relation], &[F::SemanticRelation], desc(K::Suboperation, &[])),
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
        RK::Domain(domain) => c.required_domains.contains(domain),
        RK::FactFamily(family) => c.required_fact_families.contains(family),
    });
    if registered {
        Ok(())
    } else {
        Err(reject())
    }
}

/// Canonical descriptor backing [`flow_result_contract_id`].
struct ResultContractDescriptor<'a>(&'a FlowOperationContract);

#[rustfmt::skip]
impl CanonicalEncode for ResultContractDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.flow.result_contract.v1";
    fn encode_fields(&self, e: &mut CanonicalEncoder) { e.field_str(1, self.0.tag.name()); }
}

/// The deterministic result-contract identity of one registered operation.
pub fn flow_result_contract_id(contract: &FlowOperationContract) -> ResultContractId {
    ResultContractId::from_canonical(&ResultContractDescriptor(contract))
}

/// One flow demand: the content-pinned body, the full query identity
/// (function, demand, input, profile axes), the observation basis, the
/// result contract, the subject, and the policies.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub struct FlowDemandRequest {
    pub(crate) graph_body: FlowSliceFunctionKey,
    pub query: SemanticQueryKey, pub input_basis: InputBasisId, pub result_contract: ResultContractId,
    pub subject: FlowDemandSubject, pub resources: FlowResourcePolicy,
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
/// authored key text (empty = the whole return).
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowDemandSubject { pub projection_path: Arc<[Arc<str>]> }

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

/// The demand plan over one memoized graph bundle: the obligation and
/// completeness authority a solve installs, discharges, and finalizes
/// against. Not an alias of [`ReturnSlicePlan`] (graph reachability
/// selection only), stored here as the structural selection the obligations
/// expand from. `obligation_specs` is private: obligations enter a runtime
/// only through `install_flow_demand`.
#[rustfmt::skip]
#[derive(Debug, Clone)]
pub struct FlowDemandPlan {
    pub basis: FlowDemandBasis, pub subject: FlowDemandSubject,
    /// The structural selection (graph reachability result, planned once).
    pub structural_selection: ReturnSlicePlan,
    /// The contract-required domains, in domain-rank order.
    pub required_domains: Arc<[FlowDomain]>,
    /// The initial (contract-domain and caller-asserted) obligation ids.
    pub initial_obligations: Arc<[FlowObligationId]>,
    /// The expanded (structural-selection) obligation ids.
    pub expanded_obligations: Arc<[FlowObligationId]>,
    /// The deterministic work order over all obligations.
    pub work_order: Arc<[FlowObligationId]>,
    pub tie_break: FlowTieBreak, pub convergence: FlowConvergencePolicy, pub resources: FlowResourcePolicy,
    obligation_specs: Vec<FlowObligationSpec>,
}

impl FlowDemandPlan {
    /// The obligation specifications, in work order.
    pub(crate) fn obligation_specs(&self) -> &[FlowObligationSpec] {
        &self.obligation_specs
    }
}

/// Why a demand could not be planned: no registered contract, not a
/// proof-enabled root, the slice budget tripped, or the obligation budget
/// tripped.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowDemandPlanError {
    UnregisteredOperation, NotAnEnabledRoot,
    SliceBudget(FlowSliceBudgetExceeded),
    ObligationBudget { limit: u32, observed: u32 },
}

/// Build the demand plan of `request` over the memoized `bundle`. The
/// demand planner runs EXACTLY ONCE over the caller-supplied graph — this
/// function never builds or reacquires a graph — and obligations expand
/// only through the registered [`FlowExpansionRule`]s, in domain rank,
/// ascending node index, edge class and source ordinal order.
#[rustfmt::skip]
pub(crate) fn build_flow_demand_plan(
    request: FlowDemandRequest,
    bundle: &FlowGraphBundle,
) -> Result<FlowDemandPlan, FlowDemandPlanError> {
    let tag = request.query.tag();
    let contract = require_flow_operation_contract(tag).map_err(|_| FlowDemandPlanError::UnregisteredOperation)?;
    if contract.role != R::Root || contract.status != S::EnabledHermetic {
        return Err(FlowDemandPlanError::NotAnEnabledRoot);
    }
    let demand = SliceDemand::for_return_projection(&bundle.skeleton, &request.subject.projection_path);
    let structural_selection = ReturnPathPeeker::new(&bundle.graph)
        .plan(&demand, &request.resources.slice_budget)
        .map_err(FlowDemandPlanError::SliceBudget)?;
    let mut specs: Vec<FlowObligationSpec> = Vec::new();
    let mut push = |operation, requirement, origin| {
        let id = FlowObligationId(u32::try_from(specs.len()).unwrap_or(u32::MAX));
        specs.push(FlowObligationSpec { id, requirement: FlowRequirement { operation, requirement }, origin, binding: None });
        id
    };

    // Initial obligations: one per contract-required domain, domain-rank order.
    let mut domains: Vec<FlowDomain> = contract.required_domains.to_vec();
    domains.sort();
    let mut initial: Vec<FlowObligationId> = Vec::with_capacity(domains.len());
    for domain in domains.iter().copied() {
        initial.push(push(tag, RK::Domain(domain), FlowObligationOrigin::ContractDomain));
    }
    // Caller-asserted requirements beyond the contract. A duplicate of an
    // already-planned contract domain of this root collapses onto it; every
    // other requirement — registered or not — gets its own obligation (the
    // runtime installs undeclared ones directly in `Gap` state).
    for requirement in request.additional_requirements.iter() {
        let duplicate = matches!(&requirement.requirement, RK::Domain(domain) if domains.contains(domain));
        if requirement.operation == tag && duplicate { continue; }
        initial.push(push(requirement.operation, requirement.requirement.clone(), FlowObligationOrigin::Additional));
    }
    // Expanded obligations, through the registered rules only: node-kind
    // facts in ascending node-index order, then edge facts in (node, edge
    // class, source ordinal) order.
    let graph = &bundle.graph;
    let mut selected: Vec<_> = structural_selection.value_nodes.iter()
        .chain(structural_selection.effect_only_nodes.iter()).copied().collect();
    selected.sort_by_key(|node| node.index());
    let mut expanded: Vec<FlowObligationId> = Vec::new();
    for node in &selected {
        let (operation, family, rule) = match graph.node_kind(*node) {
            FlowNodeKind::Binding(_) => (tag, F::BindingSlot, E::BindingSlotFacts),
            FlowNodeKind::ReturnSite(_) => (tag, F::ReturnSite, E::ReturnSiteFacts),
            FlowNodeKind::ExprSite(site) if !bundle.skeleton.expr_site(site).calls.is_empty() =>
                (SemanticQueryKeyTag::ResolveCall, F::CallSite, E::CallSiteFacts),
            FlowNodeKind::ExprSite(_) | FlowNodeKind::Region(_) => continue,
        };
        expanded.push(push(operation, RK::FactFamily(family), FlowObligationOrigin::Expansion(rule)));
    }
    for node in &selected {
        let mut edges: Vec<_> = graph.out_edges(*node).iter()
            .filter(|edge| structural_selection.is_selected(edge.to)).collect();
        edges.sort_by_key(|edge| (edge.kind.class() as u8, edge.ordinal));
        for edge in edges {
            let family = RK::FactFamily(F::GraphEdge(edge.kind.class()));
            expanded.push(push(tag, family, FlowObligationOrigin::Expansion(E::SelectedEdgeFacts)));
        }
    }
    let observed = u32::try_from(specs.len()).unwrap_or(u32::MAX);
    if observed > request.resources.max_obligations {
        return Err(FlowDemandPlanError::ObligationBudget { limit: request.resources.max_obligations, observed });
    }
    let mut work_order = initial.clone();
    work_order.extend(expanded.iter().copied());
    let basis = FlowDemandBasis {
        graph_body: request.graph_body, query: request.query,
        input_basis: request.input_basis, result_contract: request.result_contract,
    };
    Ok(FlowDemandPlan {
        basis, subject: request.subject, structural_selection,
        required_domains: Arc::from(domains.into_boxed_slice()),
        initial_obligations: Arc::from(initial.into_boxed_slice()),
        expanded_obligations: Arc::from(expanded.into_boxed_slice()),
        work_order: Arc::from(work_order.into_boxed_slice()),
        tie_break: FlowTieBreak::DomainNodeEdgeSlot,
        convergence: FlowConvergencePolicy { max_iterations: 16 },
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

/// The convergence evidence a solve presents at finalization: the policy it
/// ran under (must equal the plan's), the iterations it took, and whether
/// the final iteration changed nothing.
#[rustfmt::skip]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FlowConvergenceEvidence { pub policy: FlowConvergencePolicy, pub iterations: u32, pub stable: bool }

/// Why a solve is not complete: no installed demand, a stale basis, an
/// unprovable operation, a foreign result contract, a non-exact obligation
/// set, an unfinished obligation, a typed gap, a failure, invalid evidence,
/// non-convergence, or a degraded value payload.
#[rustfmt::skip]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FlowPartialReason {
    NoDemandInstalled, StaleBasis, OperationNotProvable, ResultContractMismatch,
    ObligationSetMismatch, IncompleteObligations, Gap(FlowGap), Failed(FlowFailure),
    InvalidEvidence, NonConverged, DegradedValue,
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
/// demand discharged with validated evidence under the exact basis the
/// demand was planned against, with deterministic convergence. Fields are
/// private and the sole constructor is private to this module, so a proof
/// is unforgeable outside [`finalize_flow_solve`].
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

/// The sole proof-bearing finalizer. Compares the complete demand basis,
/// confirms the operation contract (registered, proof-enabled root, coherent
/// result contract), requires the exact obligation-ID/spec set with every
/// obligation `Discharged`, validates dependency and same-contract
/// suboperation evidence, validates deterministic convergence, and rejects
/// every gap, stale basis, cancellation, budget exhaustion, panic marker,
/// or internal failure.
#[rustfmt::skip]
pub fn finalize_flow_solve(
    runtime: &ObligationRuntime, plan: &FlowDemandPlan, value: FlowReturnResult,
    convergence: &FlowConvergenceEvidence,
) -> FlowSolveOutcome {
    let partial = |reason: FlowPartialReason| FlowSolveOutcome::Partial(PartialFlowResult { basis: plan.basis.clone(), reason });

    let Some(installed) = runtime.flow_basis() else { return partial(FlowPartialReason::NoDemandInstalled) };
    if installed != &plan.basis { return partial(FlowPartialReason::StaleBasis); }
    let Some(contract) = flow_operation_contract(plan.basis.query.tag()) else {
        return partial(FlowPartialReason::OperationNotProvable);
    };
    if contract.role != R::Root || contract.status != S::EnabledHermetic {
        return partial(FlowPartialReason::OperationNotProvable);
    }
    if plan.basis.result_contract != flow_result_contract_id(contract) {
        return partial(FlowPartialReason::ResultContractMismatch);
    }
    let records = runtime.flow_obligations();
    let specs = plan.obligation_specs();
    let exact_set = records.len() == specs.len() && records.iter().zip(specs.iter()).all(|(r, s)| r.spec == *s);
    if !exact_set { return partial(FlowPartialReason::ObligationSetMismatch); }
    for record in records {
        let ObligationState::Discharged(evidence) = &record.state else {
            return partial(match &record.state {
                ObligationState::Gap(gap) => FlowPartialReason::Gap(*gap),
                ObligationState::Failed(failure) => FlowPartialReason::Failed(*failure),
                ObligationState::Pending | ObligationState::Running => FlowPartialReason::IncompleteObligations,
                ObligationState::Discharged(_) => FlowPartialReason::InvalidEvidence,
            });
        };
        let basis_ok = evidence.input_basis == plan.basis.input_basis && evidence.result_contract == plan.basis.result_contract;
        let deps_ok = evidence.dependencies.iter().all(|dep| specs.iter().any(|spec| spec.id == *dep));
        let subs_ok = evidence.suboperations.iter().all(|sub| sub.result_contract == plan.basis.result_contract
            && flow_operation_contract(sub.operation).is_some_and(|c| c.role == R::SemanticSuboperation));
        if !basis_ok || !deps_ok || !subs_ok { return partial(FlowPartialReason::InvalidEvidence); }
    }
    if convergence.policy != plan.convergence || !convergence.stable || convergence.iterations > plan.convergence.max_iterations {
        return partial(FlowPartialReason::NonConverged);
    }
    if value.degradation().is_some() { return partial(FlowPartialReason::DegradedValue); }

    FlowSolveOutcome::Complete(CompleteFlowResult::new(CompleteFlowResultParts {
        basis: plan.basis.clone(), value, convergence: *convergence, discharged: Arc::clone(&plan.work_order),
    }))
}
