//! The demand-sliced `FlowReturn` authority.
//!
//! One `SemanticQueryKey::FlowReturn` producer through
//! [`ProjectSemanticDispatch`]: the demanded function's slice is planned
//! as graph reachability over the once-per-content-version
//! `FunctionFlowGraph`, hashed, lowered (`FlowSliceIR`), and evaluated
//! through the slice-gated owned content
//! ([`crate::flow_slice_content::SliceContent`]) on the shared tagged
//! obligation runtime — return sites, `if` reachability, bare return,
//! fallthrough, primitive widening, unions, parameters and simple local
//! reaching definitions, object returns (spread delegated to
//! `ProjectObjectSpread`), symbolic call returns (`ReturnType<typeof …>`
//! / `any` carriers), return-free loop transparency, and direct same-slot
//! recursion through coinductive holds. Content outside the demanded
//! slice never lowers and never evaluates.
//!
//! Admission is proof-gated: every cold frame installs its own flow
//! demand (`prepare_flow_return_demand`), the evaluation reports the
//! obligations it completed, the component close replays the
//! runtime-observed convergence, and the finalizer mints the sole
//! warm-admission proof (`CompleteFlowResult`). Only a proof-bearing
//! result admits into the family memo; a partial keeps its usable value
//! (ReturnOnly), and every no-value shape is a typed `FlowReturnFailure`
//! through `ReturnOnly` (never admitted, never `never`).

use std::sync::Arc;

use super::call_resolve::union_self_roots;
use super::dispatch_txn::flow_obligation_state::{
    FlowDemandCarrier, FlowEvaluationProvenance, ObservedFlowConvergence,
};
use super::dispatch_txn::{
    CompletedFlowReturnMember, FlowReturnPendingOutcome, FlowReturnPendingState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
};
use super::flow_return_callee::{
    CallValue, CalleeClause, CalleeClauseLookup, HeldCallee, ReturnOrigin, SignatureCall,
};
use super::flow_solve::{
    FlowPartialReason, FlowSolveOutcome, NoValueFlowResult, PartialFlowResult,
};
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::resolver_core::{FactVersionRef, ProgramAnalysisFactRef};
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnFailure, FlowReturnKey, FlowReturnResult, FlowReturnStep,
    FlowReturnUnsupported, PartialReasonSet, PrimitiveKind, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryValue,
};

/// The consumer outcome of one sealed function-return demand
/// ([`ProjectSemanticDispatch::execute_function_return_source`]).
///
/// The per-outcome admission fold this type once carried is retired: the
/// `FlowReturn` build's own output rails carry the partial classes now,
/// and the universal read funnel (`fold_cache_read_rails` at the shared
/// cold-build helper's return) propagates them into the enclosing
/// composition.
#[derive(Debug)]
pub(crate) enum FunctionReturnNode {
    /// A DECLARED return lowered through the memoized locator rail.
    Declared(crate::semantic_query::HotTypeRef),
    /// A body-derived return: the admitted whole-function result (the
    /// canonical, carrier-preserving return node plus the fallthrough bit).
    Flow(FlowReturnResult),
    /// A DECLARED locator whose raise missed — the enclosing composition
    /// records the interior failure at its own position.
    DeclaredMiss,
    /// A body-derived evaluation with NO VALUE AT ALL: the typed
    /// `FlowReturnFailure` through `ReturnOnly` (never admitted) — the
    /// enclosing composition marks partial / fails closed.
    ///
    /// NOT the arm a positionally-unmodelled sub-expression takes: that
    /// one is a degraded SUCCESS through [`Self::Flow`], carrying the
    /// typed unresolved marker at the position and every modelled sibling
    /// intact.
    NoValue(FlowReturnFailure),
    /// No recoverable return carrier (a bodiless overload or a synthesized
    /// signature) — the consumer's absent-position arm.
    Absent,
}

/// The partial class EVERY typed NO-VALUE [`FlowReturnFailure`] carries:
/// [`PartialReasonSet::FLOW_RETURN_NO_SURFACE`], the class contained by
/// the TSC lane ALONE.
///
/// One rule rather than a per-variant match, because the classification
/// axis is the CONSUMER, not the cause, and every no-value cause lands on
/// the same side of it. A consumer that splices the AUTHORED declaration
/// and lets an external checker type it (the Vue macro TSC projection) is
/// unaffected by ANY of them — the declaration rides verbatim whether the
/// substrate failed on a control surface, a missing body, a budget edge or
/// a torn view. A consumer that derives its output FROM the value (the
/// runtime `props: {...}` projection, `get_component_meta`) is broken by
/// all of them alike: there is no surface, so publishing around it emits
/// a member set missing every member this producer was asked for.
///
/// It is a DISTINCT bit from the degraded-success
/// [`PartialReasonSet::FLOW_RETURN_UNVERIFIED`] rather than a shared one,
/// and the distinction is load-bearing rather than descriptive. Sharing
/// one bit forces every consumer that contains the unverified class —
/// which it must, because that class's member set is complete by
/// definition — to contain this one too, and the only remaining defence
/// is a structural "is the assembled surface empty" check at the
/// projection. That check is per-SURFACE where the invariant is
/// per-CONTRIBUTION: one authored intersection arm, or one `interface …
/// extends` heritage clause, makes the surface non-empty and the
/// no-surface producer's members vanish with no diagnostic at all.
///
/// A per-variant match here would be a constant-returning stub. The
/// distinctions that DO matter are recorded elsewhere and survive: the
/// class-member inference rail records its own precise
/// `BUDGET_EXCEEDED` into the file-level aggregate (pinned by
/// `tsc_class_inference_budget_is_exact_partial_and_non_cacheable`), and
/// the typed `FlowReturnFailure` itself is what the flow-return consumers
/// branch on.
const NO_VALUE_REASON_CLASS: PartialReasonSet = PartialReasonSet::FLOW_RETURN_NO_SURFACE;

/// The partial class a DEGRADED SUCCESS's typed
/// [`FlowReturnDegradation`] carries.
///
/// The axis is the SHAPE OF THE EVIDENCE, not which consumer sees it.
/// Both classes are contained by both Vue macro codegen lanes; they
/// differ in what a value-reading consumer can still do with the result.
///
/// [`PartialReasonSet::FLOW_RETURN_UNINFERRED`] — POSITIONAL. The surface
/// is FAITHFUL: every modelled sibling is exact, and the one position the
/// substrate could not type carries the typed marker rather than a
/// fabricated `any`. An unmodelled position, an unresolved-value carrier,
/// an unrepresentable callee, a failed binding initializer — each of them
/// mints the marker AT the position, which is what lets a per-member
/// consumer degrade exactly that member and keep its siblings exact.
///
/// [`PartialReasonSet::FLOW_RETURN_UNVERIFIED`] — FRAME-WIDE. The member
/// set is complete but one member's TYPE may be WRONG: a write effect the
/// evaluator did not apply, a conditional `var` join it has no algebra
/// for, a declared union it could not reduce, a call on a non-callable
/// binding that evaluated to `any`. Nothing names WHICH member — the
/// unapplied-write reason is seeded from the lowered slice's effect list
/// before any member evaluates — so a value-reading consumer degrades
/// every member rather than a nameable one.
///
/// Per-member attribution is NOT "intersect each member value's slot
/// reads with the unapplied write's targets". That reading is FAIL-OPEN:
/// a member can depend on a written slot through an INTERMEDIATE local
/// whose own definition read it, while naming only the intermediate.
/// `function f(seed: string | number) { seed = "y"; const q = seed;
/// return { label: q } }` reads `q` alone, so the intersection is empty
/// and the member would publish the unnarrowed `string | number` warm
/// where the checker says `string`. The sound direction is the
/// COMPLEMENT — a position is exact only when it provably reads no frame
/// binding at all (an owner-scope leaf), and every other position takes
/// the positional marker. Adopting it changes both which typed
/// [`FlowReturnDegradation`] a written frame reports and when the
/// evaluator first observes it, so it belongs with the work that APPLIES
/// write effects rather than beside it.
fn degradation_reason_class(degradation: FlowReturnDegradation) -> PartialReasonSet {
    match degradation {
        FlowReturnDegradation::FlowGap(_) => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        FlowReturnDegradation::UnmodeledPosition
        | FlowReturnDegradation::UnresolvedValue
        | FlowReturnDegradation::UnrepresentableCallee
        | FlowReturnDegradation::FailedBindingInitializer => {
            PartialReasonSet::FLOW_RETURN_UNINFERRED
        }
        FlowReturnDegradation::NonCallableBinding
        | FlowReturnDegradation::UnappliedWriteEffect
        | FlowReturnDegradation::ConditionalVarDefinition
        | FlowReturnDegradation::UnreducedDeclaredUnion => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
    }
}

/// Veto-side fault injection for the flow admission guards' negative
/// legs. Each slot can only make the pipeline REFUSE more — strip the
/// finalizer's proof token off an otherwise-clean root build, or drop
/// one claim off an otherwise-complete discharge report — so an armed
/// slot can never mint, promote, or warm anything. The slots exist to
/// let a test present the adversarial inputs the production pipeline is
/// built never to produce (a proofless clean value at the memo gate; a
/// report leaving one planned obligation pending), proving the guards
/// discriminate rather than ride along.
///
/// The slots are PER-HOST (a [`FlowAdmissionFaultKnobs`] field on
/// `VerterHost`), never process-global: a test arming one on its own
/// host cannot perturb a concurrent unrelated test running on a
/// different host in the same process. Production reads each slot as a
/// relaxed atomic load inside `cfg(any(test, feature = "test-support"))`
/// blocks only — a shipped build compiles to no field and no load.
#[cfg(any(test, feature = "test-support"))]
pub(crate) mod flow_admission_fault_injection {
    use std::sync::atomic::{AtomicBool, Ordering};

    /// The per-host flow-admission fault slots. One instance per
    /// `VerterHost`; every slot defaults to disarmed.
    #[derive(Debug, Default)]
    pub(crate) struct FlowAdmissionFaultKnobs {
        /// When armed, `build_flow_return` removes the `flow_completion`
        /// proof token from its build output while leaving the boolean
        /// rails untouched — the exact shape the memo's flow-proof gate
        /// must refuse.
        pub(crate) strip_root_proof: AtomicBool,

        /// When armed, `finalize_flow_demand` applies the evaluator's
        /// discharge report with its LAST claim removed, leaving exactly
        /// one planned obligation pending at the seal.
        pub(crate) drop_last_discharge_claim: AtomicBool,

        /// When armed, `finalize_flow_demand` sees evaluation evidence
        /// whose provenance is NOT this evaluation's one-shot mint —
        /// modelling evidence that did not originate from
        /// `evaluate_flow_return`.
        pub(crate) foreign_evaluation_evidence: AtomicBool,

        /// When armed, `finalize_flow_demand` sees evaluation evidence
        /// whose provenance carries the carrier's OWN store/generation/
        /// runtime but a DIFFERENT demand ordinal — modelling a
        /// same-store, same-generation value or report produced by
        /// ANOTHER demand of the same runtime.
        pub(crate) foreign_demand_provenance: AtomicBool,

        /// When armed, `prepare_flow_return_demand` stamps the installed
        /// demand carrier with a provenance from another store/generation
        /// — modelling a demand handle and value pair minted elsewhere.
        pub(crate) stale_demand_carrier: AtomicBool,

        /// When armed, `finalize_flow_demand` sees convergence evidence
        /// with ZERO observed iterations — modelling a caller-fabricated
        /// claim the discharge driver never produced.
        pub(crate) unobserved_convergence: AtomicBool,

        /// When armed, `evaluate_flow_return` assembles its execution
        /// witness with the evaluator's recorded call evidence dropped —
        /// modelling an evaluation that claims call obligations whose
        /// work it never performed.
        pub(crate) suppress_call_evidence: AtomicBool,

        /// When armed, `evaluate_flow_return` assembles its execution
        /// witness with every recorded call marked relation-undecided —
        /// modelling a call whose consumed relation outcomes were never
        /// decided.
        pub(crate) undecided_relation_evidence: AtomicBool,

        /// When armed, `evaluate_flow_return` assembles its execution
        /// witness with the evaluator's walk ledger marked aborted —
        /// modelling an evaluation whose structural walk did not run to
        /// completion (an early exit leaving the ledger short).
        pub(crate) short_execution_ledger: AtomicBool,

        /// When set, the next MACHINERY-ROOT close of ANY domain (a
        /// relation, call, or flow root build) drains one extra flow
        /// member: this key's REAL prepared demand, popped provisionally
        /// beneath the open root with an evaluated value but NO discharge
        /// report, so its planned obligations stay pending and the
        /// component close finalizes it UNPROVEN — the torn mixed
        /// component the production pipeline is built never to leave
        /// behind. Consumed by the injection (one shot); see
        /// `ProjectSemanticDispatch::inject_unproven_flow_member_for_tests`.
        pub(crate) unproven_flow_member:
            std::sync::Mutex<Option<crate::semantic_query::FlowReturnKey>>,

        /// When armed (`Some`), every FLOW-root close records the
        /// canonicals of the component carrier it composed — the
        /// self-root union the root AND its batched members publish on —
        /// whether or not the component then admits. An observation seam,
        /// not a fault: a degraded component publishes no entry to read the
        /// carrier back from, and its composition must still be provable.
        /// See `ProjectSemanticDispatch::record_flow_root_carrier_for_tests`.
        pub(crate) flow_root_carrier_probe: std::sync::Mutex<Option<Vec<std::sync::Arc<str>>>>,
    }

    /// RAII arm/disarm for one slot of one host's knobs.
    // Constructed by the in-crate admission tests only; the lib-only
    // `test-support` build compiles the slots for the application sites
    // without constructing a guard.
    #[allow(dead_code)]
    pub(crate) struct Guard<'h>(&'h AtomicBool);

    #[allow(dead_code)]
    impl<'h> Guard<'h> {
        pub(crate) fn arm(slot: &'h AtomicBool) -> Self {
            slot.store(true, Ordering::Relaxed);
            Self(slot)
        }
    }

    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
        }
    }
}

/// The typed reason an installed demand is not provable: the first
/// non-discharged obligation explains the refusal — a typed gap, a
/// failure, or an obligation the evaluation never completed.
fn unproven_flow_reason(
    runtime: &super::dispatch_txn::ObligationRuntime,
    handle: super::dispatch_txn::flow_obligation_state::FlowDemandHandle,
) -> FlowPartialReason {
    use super::dispatch_txn::flow_obligation_state::ObligationState;
    runtime
        .flow_obligations(handle)
        .and_then(|records| {
            records.iter().find_map(|record| match &record.state {
                ObligationState::Gap(gap) => Some(FlowPartialReason::Gap(*gap)),
                ObligationState::Failed(failure) => Some(FlowPartialReason::Failed(*failure)),
                ObligationState::Pending | ObligationState::Running => {
                    Some(FlowPartialReason::IncompleteObligations)
                }
                ObligationState::Discharged(_) => None,
            })
        })
        .unwrap_or(FlowPartialReason::NoDemandInstalled)
}

/// The popped root's close outcome.
enum FlowRootClose {
    /// The root evaluated a value (possibly a DEGRADED success — the
    /// caller still receives it; only admission is refused). This is the
    /// PRE-PROOF value arm: completeness is claimed only by the
    /// finalizer's verdict riding inside.
    EvaluatedValue(Box<EvaluatedFlowRoot>),
    /// Typed NO-VALUE failure — `ReturnOnly`, never admitted.
    NoValue(FlowReturnFailure),
}

/// The root close's evaluated-value payload: the final value (post
/// component fixed point, literal widening, and per-key substitution), the
/// component's UNIONED self-roots (every drained member's file roots
/// across both domains), the materialised point set the root's compute
/// actually produced (§3.4), and the finalizer's verdict over the root's
/// own demand. `verdict` is `None` only when the root's demand could not
/// be planned at all — unproven either way, never warm without the token.
struct EvaluatedFlowRoot {
    result: FlowReturnResult,
    scc_self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    materialized: crate::semantic_query::demand::MaterializedSet,
    verdict: Option<super::flow_solve::FlowSolveOutcome>,
}

/// The shared demand-site of one flow demand — derived ONCE by
/// [`ProjectSemanticDispatch::flow_slice_demand_site`] and consumed by both
/// the demand preparation and the content evaluation.
struct FlowSliceDemandSite {
    /// The served indexed state of the function's own file (pinned).
    indexed: Arc<crate::project_type_store::IndexedReady>,
    /// The function's own file roots.
    self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The content-pinned function identity.
    slice_key_function: crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey,
    /// The hash-node key (function + demand identity).
    slice_key: crate::cache_runtime::flow_slice_node::FlowSliceHashKey,
    /// The demanded member for a member-projection demand.
    demanded_member: Option<Arc<str>>,
    /// The frame's binding inventory — the cross-frame binding authority
    /// the demand planner resolves slot identities against.
    inventory: super::flow_solve::FlowBindingInventory,
}

/// One flow frame's evaluation result, before the frame closes.
struct FlowEvaluationOutcome {
    /// The frame's decided outcome.
    outcome: FlowReturnPendingOutcome,
    /// The frame's own file roots.
    self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The coinductive hold targets the evaluation met.
    holds: Vec<HeldCallee>,
    /// The materialised point set the compute ACTUALLY produced (§3.4).
    materialized: crate::semantic_query::demand::MaterializedSet,
    /// Whether every one of the frame's OWN return contributors was a
    /// FRESH literal (and no bare-return / fallthrough arm joined) — the
    /// post-convergence literal-widening input.
    fresh_seed: bool,
    /// The evaluator's typed discharge report: which planned obligations
    /// of the frame's installed demand the evaluation actually completed.
    /// `Some` only on an evaluated-value outcome over an installed demand;
    /// applied centrally at the frame's SCC close through
    /// `ObligationRuntime::apply_flow_discharge_report` — never through
    /// scattered per-obligation calls.
    discharge: Option<super::dispatch_txn::flow_obligation_state::FlowDischargeReport>,
    /// The evaluation provenance minted at this evaluation's start: binds
    /// the value, the discharge report, and the convergence evidence of
    /// THIS evaluation to the serving store and request generation. The
    /// finalization driver refuses anything else as a typed partial.
    provenance: super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance,
}

/// The §3.4 materialised point set a FAILED frame evaluation records.
///
/// A hold-only [`FlowReturnFailure::EmptyCycle`] is the one failure the
/// component discharge RESURRECTS to `Complete`
/// (`discharge_flow_component_to_fixed_point` admits exactly `Complete`
/// and `EmptyCycle`): its value IS the join of its hold targets, and the
/// point that join serves is the frame's own demand point. The
/// resurrection copies only the outcome, so an empty set here would
/// publish an entry `cached_satisfies` (an `.any(...)` over the recorded
/// set) can never satisfy — a candidate holding a slot, a reverse-index
/// registration and a FIFO budget admission while being permanently
/// unreadable.
///
/// Every OTHER failure is a real no-value outcome that never publishes,
/// so it records nothing.
fn failure_materialized_set(
    failure: FlowReturnFailure,
    key: &FlowReturnKey,
) -> crate::semantic_query::demand::MaterializedSet {
    use crate::semantic_query::demand::{MaterializedPoint, MaterializedSet};
    if matches!(failure, FlowReturnFailure::EmptyCycle) {
        MaterializedSet::single(MaterializedPoint::new(key.demand.point.clone()))
    } else {
        MaterializedSet::empty()
    }
}

/// The frame-pop result.
enum FlowFramePop {
    /// Caller-return for a non-root pop (the provisional member).
    Provisional(FlowReturnStep),
    /// The root's close.
    RootClose(FlowRootClose),
}

impl<'a> ProjectSemanticDispatch<'a> {
    /// The full `FlowReturnContext` for a demand rooted at `canonical`:
    /// the live `P R T L J` env, the empty type-only substitution, and
    /// the empty policy. The ONE context derivation point — every
    /// `FlowReturnKey` construction routes through here.
    pub(crate) fn flow_return_context_for(
        &self,
        canonical: &str,
    ) -> crate::semantic_query::FlowReturnContext {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical);
        crate::semantic_query::FlowReturnContext {
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().0,
            type_substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
            policy: crate::semantic_query::FlowReturnPolicy {},
        }
    }

    /// The env-bearing function slot identity for one served function
    /// position — the slot derives through the ONE generalized
    /// slot-finalization choke point.
    pub(crate) fn flow_function_slot_for(
        &self,
        canonical: Arc<str>,
        owner: verter_type_expr::TopLevelOwnerId,
        name: Arc<str>,
        part: verter_type_expr::facts::FunctionPartIdentity,
        overload_ordinal: u32,
    ) -> crate::semantic_query::FlowFunctionSlotIdentity {
        crate::semantic_query::FlowFunctionSlotIdentity {
            declaration_slot: self.finalize_slot_seed(
                crate::semantic_query::DeclarationSlotSeed::new(
                    canonical,
                    owner,
                    name,
                    crate::semantic_query::SemanticSymbolSpace::Value,
                ),
            ),
            function_part: part,
            overload_ordinal,
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The sealed function-return consumer entry
    // ──────────────────────────────────────────────────────────────────

    /// The ONE `FlowReturnKey` construction: every body-derived
    /// function-return demand (signature composition incl. the `typeof`
    /// raise, function / arrow publication, `ReturnType<typeof f>`, class
    /// instance / static method composition, `tsc_projection`) builds the
    /// IDENTICAL key here — the env-bearing slot through
    /// [`Self::flow_function_slot_for`], the full `P R T L J` context
    /// through [`Self::flow_return_context_for`], no normalized type
    /// arguments (consumers instantiate downstream under their own mode).
    pub(crate) fn flow_return_key_for(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
    ) -> FlowReturnKey {
        // The canonical production point: whole return. The demand axis
        // is KEY DATA — a narrower demand is a distinct cache and
        // re-entry identity, never an implicit default.
        self.flow_return_key_with_demand(
            identity,
            crate::semantic_query::ReturnProjectionDemand::whole_return(),
        )
    }

    /// Demand-parameterised half of [`Self::flow_return_key_for`].
    /// Input axis stays the canonical empty point: no production
    /// contextual-input producer exists. A non-empty point is a
    /// distinct cache/re-entry identity. The result contract is derived
    /// HERE — the ONLY derivation point — from the closed flow-operation
    /// registry; no caller ever selects it.
    pub(crate) fn flow_return_key_with_demand(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
        demand: crate::semantic_query::ReturnProjectionDemand,
    ) -> FlowReturnKey {
        FlowReturnKey {
            function: self.flow_function_slot_for(
                Arc::clone(&identity.anchor.canonical_id),
                identity.anchor.owner,
                Arc::clone(&identity.anchor.symbol),
                identity.function_part.clone(),
                identity.overload_ordinal,
            ),
            normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
            context: self.flow_return_context_for(identity.anchor.canonical_id.as_ref()),
            demand,
            input: crate::semantic_query::FlowInputContext::empty(),
            result_contract: super::flow_solve::flow_return_result_contract_id(),
        }
    }

    /// The instantiation half of the ONE construction point: call
    /// resolution demands the callee's flow return under the call's
    /// FINAL ordered type-argument mapping and substitution — the same
    /// env-bearing slot and context derivation, with the demand and
    /// input axes at the canonical production point.
    pub(crate) fn flow_return_key_for_instantiation(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
        normalized_type_args: Arc<[SemanticNodeId]>,
        substitution: crate::semantic_query::CanonicalTypeSubstitution,
    ) -> FlowReturnKey {
        let mut key = self.flow_return_key_with_demand(
            identity,
            crate::semantic_query::ReturnProjectionDemand::whole_return(),
        );
        key.normalized_type_args = normalized_type_args;
        key.context.type_substitution = substitution;
        key
    }

    /// Full live context for an indexed semantic call at `canonical`:
    /// the same `P R T L J` env derivation [`Self::flow_return_context_for`]
    /// owns, with the empty type substitution.
    pub(crate) fn resolve_call_context_for(
        &self,
        canonical: &str,
    ) -> crate::semantic_query::ResolveCallContext {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes_for(canonical);
        crate::semantic_query::ResolveCallContext {
            parse_env_hash: env.parse_env_hash,
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: env.type_env_hash,
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().0,
            substitution: crate::semantic_query::CanonicalTypeSubstitution::empty(),
        }
    }

    /// Execute one indexed semantic call and return only a complete admitted
    /// selected or genuine-dynamic value node.
    pub(crate) fn execute_indexed_resolve_call(
        &self,
        key: crate::semantic_query::ResolveCallKey,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        self.execute_indexed_resolve_call_with_flow_hold(key, false)
            .0
    }

    /// The flow-aware half of [`Self::execute_indexed_resolve_call`]: when
    /// `hold_flow_result` is set, a complete or held call whose result a
    /// surrounding FlowReturn evaluation depends on is recorded as a
    /// tagged hold on the nearest active flow frame instead of serving a
    /// provisional value — the frame's SCC close joins the admitted
    /// result through its return equation.
    pub(crate) fn execute_indexed_resolve_call_with_flow_hold(
        &self,
        key: crate::semantic_query::ResolveCallKey,
        hold_flow_result: bool,
    ) -> (Option<crate::semantic_query::SemanticNodeId>, bool) {
        match self.execute_resolve_call(key.clone()) {
            super::call_resolve::ResolveCallStep::Complete(result) => {
                let return_type = super::return_equation::resolved_call_return_type(&result);
                let held_by_flow = hold_flow_result
                    && self
                        .dispatch_txn
                        .borrow_mut()
                        .reentry_mut()
                        .record_nearest_flow_hold(
                            super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key),
                        );
                if held_by_flow {
                    (None, true)
                } else {
                    (Some(return_type), false)
                }
            }
            super::call_resolve::ResolveCallStep::Hold(target) => {
                let held_by_flow = hold_flow_result
                    && self
                        .dispatch_txn
                        .borrow_mut()
                        .reentry_mut()
                        .record_nearest_flow_hold(
                            super::dispatch_txn::ReturnObligationIdentity::ResolveCall(*target),
                        );
                (None, held_by_flow)
            }
            super::call_resolve::ResolveCallStep::Degraded(_) => (None, false),
        }
    }

    /// Evaluate parsed indexed expression IR directly to a semantic node.
    /// Calls route only through `ResolveCall`; a non-result is a typed miss
    /// without a scanner/raw-expression fallback.
    pub(crate) fn evaluate_indexed_value_expression_node(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        expression: &verter_type_expr::IndexedValueExpression,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        self.evaluate_indexed_value_expression_node_inner(canonical, owner, expression, true)
    }

    pub(crate) fn evaluate_indexed_value_expression_node_inner(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        expression: &verter_type_expr::IndexedValueExpression,
        hold_flow_result: bool,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        use verter_type_expr::{IndexedValueCallKind, IndexedValueExpression};
        match expression {
            IndexedValueExpression::Value(value) => self.lower_type_expr_in_owner_scope_with_mode(
                canonical,
                owner,
                value,
                crate::semantic_query::ProjectionMode::Navigate,
            ),
            IndexedValueExpression::UnsupportedCall { .. } => {
                crate::request_context::mark_request_result_partial();
                None
            }
            IndexedValueExpression::Call(call) => {
                let callee = self.evaluate_indexed_value_expression_node_inner(
                    canonical,
                    owner,
                    &call.callee,
                    false,
                )?;
                let receiver = call.receiver.as_deref().and_then(|receiver| {
                    self.evaluate_indexed_value_expression_node_inner(
                        canonical, owner, receiver, false,
                    )
                });
                let mut args = Vec::with_capacity(call.args.len());
                for argument in call.args.iter() {
                    let ty = self.evaluate_indexed_value_expression_node_inner(
                        canonical,
                        owner,
                        &argument.expression,
                        false,
                    )?;
                    args.push(crate::semantic_query::CallArgKey::Eager {
                        ty,
                        spread: argument.spread,
                        context_sensitive: argument.context_sensitive,
                        literal_mode: match argument.literal_mode {
                            verter_type_expr::IndexedValueLiteralMode::Widened => {
                                crate::semantic_query::ArgumentLiteralMode::Widened
                            }
                            verter_type_expr::IndexedValueLiteralMode::Literal => {
                                crate::semantic_query::ArgumentLiteralMode::Literal
                            }
                        },
                    });
                }
                let explicit_type_args = call
                    .explicit_type_args
                    .iter()
                    .map(|argument| {
                        self.lower_type_expr_in_owner_scope_with_mode(
                            canonical,
                            owner,
                            argument,
                            crate::semantic_query::ProjectionMode::Navigate,
                        )
                    })
                    .collect::<Option<Vec<_>>>()?;
                let (result, held) = self.execute_indexed_resolve_call_with_flow_hold(
                    crate::semantic_query::ResolveCallKey {
                        point: crate::semantic_query::ProgramPointId {
                            canonical_id: Arc::from(canonical),
                            offset: call.point,
                        },
                        callee,
                        kind: match call.kind {
                            IndexedValueCallKind::Call => crate::semantic_query::CallKind::Call,
                            IndexedValueCallKind::Construct => {
                                crate::semantic_query::CallKind::Construct
                            }
                        },
                        receiver,
                        args: Arc::from(args.into_boxed_slice()),
                        explicit_type_args: Arc::from(explicit_type_args.into_boxed_slice()),
                        flow: crate::semantic_query::FlowNarrowingKey::empty(),
                        context: self.resolve_call_context_for(canonical),
                    },
                    hold_flow_result,
                );
                if result.is_none() && !held {
                    crate::request_context::mark_request_result_partial();
                }
                result
            }
        }
    }

    /// Consume an inferred declaration's indexed semantic expression source.
    pub(crate) fn execute_semantic_expression_source(
        &self,
        source: &verter_type_expr::facts::SemanticExpressionSource,
        owner: verter_type_expr::TopLevelOwnerId,
    ) -> Option<crate::semantic_query::SemanticNodeId> {
        match source {
            verter_type_expr::facts::SemanticExpressionSource::FunctionReturn(source) => {
                let canonical = match source {
                    verter_type_expr::facts::FunctionReturnSource::Declared(locator) => {
                        locator.slot().anchor.canonical_id.as_ref()
                    }
                    verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                        identity.anchor.canonical_id.as_ref()
                    }
                    verter_type_expr::facts::FunctionReturnSource::Absent => return None,
                };
                match self.execute_function_return_source(source, canonical) {
                    FunctionReturnNode::Declared(node) => Some(node.node()),
                    FunctionReturnNode::Flow(result) => Some(result.return_type()),
                    FunctionReturnNode::DeclaredMiss
                    | FunctionReturnNode::NoValue(_)
                    | FunctionReturnNode::Absent => None,
                }
            }
            verter_type_expr::facts::SemanticExpressionSource::ProgramExpression(point) => {
                let serve = self
                    .ctx
                    .ensure_indexed_ready_serve(point.canonical_id.as_ref())?;
                let indexed = serve.indexed;
                let memo = indexed.shallow_state.decl_bodies();
                let index = memo.function_program_index();
                let record = index.expression(point)?;
                match &record.source {
                    verter_semantic::analysis::function_program::ProgramExpressionSource::FunctionReturn(source) => {
                        match self.execute_function_return_source(source, point.canonical_id.as_ref()) {
                            FunctionReturnNode::Declared(node) => Some(node.node()),
                            FunctionReturnNode::Flow(result) => Some(result.return_type()),
                            FunctionReturnNode::DeclaredMiss
                            | FunctionReturnNode::NoValue(_)
                            | FunctionReturnNode::Absent => None,
                        }
                    }
                    verter_semantic::analysis::function_program::ProgramExpressionSource::UnsupportedCall => {
                        crate::request_context::mark_request_result_partial();
                        None
                    }
                    verter_semantic::analysis::function_program::ProgramExpressionSource::Value
                    | verter_semantic::analysis::function_program::ProgramExpressionSource::SemanticCall { .. } => {
                        let expression = memo.indexed_program_expression_ir(record)?;
                        self.evaluate_indexed_value_expression_node(
                            point.canonical_id.as_ref(),
                            owner,
                            expression.as_ref(),
                        )
                    }
                }
            }
        }
    }

    /// The ONE sealed function-return consumer entry: routes the fact's
    /// [`verter_type_expr::facts::FunctionReturnSource`] to its producer.
    /// `Declared` lowers through the memoized locator rail; `Flow`
    /// constructs and executes the [`FlowReturnKey`] through
    /// [`Self::flow_return_key_for`] (never the `None → miss_node` arm);
    /// `Absent` reports the absent carrier.
    pub(crate) fn execute_function_return_source(
        &self,
        source: &verter_type_expr::facts::FunctionReturnSource,
        scope_canonical_id: &str,
    ) -> FunctionReturnNode {
        match source {
            verter_type_expr::facts::FunctionReturnSource::Declared(locator) => {
                match self
                    .raise_body_slot(locator.slot(), scope_canonical_id)
                    .at_optional_boundary()
                {
                    Some(hot) => FunctionReturnNode::Declared(hot),
                    None => FunctionReturnNode::DeclaredMiss,
                }
            }
            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                // A DEGRADED SUCCESS stays usable — the consumer keeps
                // the value (interning a miss would be the opposite
                // collapse). The admission rails are NOT folded here:
                // the `FlowReturn` build's own output carries them (the
                // finalizer-outcome adapter in `build_flow_return`), and
                // the universal read funnel propagates them into this
                // enclosing composition when the read returns.
                match self.execute_flow_return(self.flow_return_key_for(identity)) {
                    FlowReturnStep::Complete(result) => FunctionReturnNode::Flow(result),
                    FlowReturnStep::NoValue(failure) => FunctionReturnNode::NoValue(failure),
                    // A hold surfacing at a consumer is a demand reentering
                    // its own in-flight component: undecided here, ReturnOnly.
                    // The hold produces NO memo read, so the universal read
                    // funnel cannot carry its rails: the consumption point
                    // marks the enclosing build itself — an answer composed
                    // around an undecided interior must not warm.
                    FlowReturnStep::Hold(_) => {
                        crate::request_context::mark_request_result_partial_from_read_with(
                            NO_VALUE_REASON_CLASS,
                        );
                        self.fold_into_top_build_local_taint_with(
                            true,
                            true,
                            NO_VALUE_REASON_CLASS,
                        );
                        FunctionReturnNode::NoValue(FlowReturnFailure::Unresolved)
                    }
                }
            }
            verter_type_expr::facts::FunctionReturnSource::Absent => FunctionReturnNode::Absent,
        }
    }

    /// The `ReturnType<typeof callee>` MEMBER-HOP admission: the
    /// path-precise projector demand rail. Given the argument node of a
    /// builtin `ReturnType` instantiation carrier and the pending walk
    /// segment, resolve the callee to its served function slot and — when
    /// the callee is a FUNCTION VALUE whose return is body-derived
    /// (`FunctionReturnSource::Flow`) — dispatch
    /// `SemanticQueryKey::FlowReturn` with the single-member
    /// `ReturnProjectionDemand`, returning the demanded member's node.
    ///
    /// `None` is the structured fall-through: a declared-return callee, a
    /// non-`typeof` argument, an overload group, a computed segment, or a
    /// degraded/held member evaluation all fall back to the generic
    /// `Instantiate` unwrap (the pre-existing whole-return route through
    /// this same dispatch) — never a fabricated member and never a
    /// second resolver.
    /// The typed `ReturnType<typeof callee>` CALLEE detection: `arg` is
    /// the bare `typeof callee` carrier (no member path) of a
    /// single-signature FUNCTION VALUE whose return is body-derived
    /// (`FunctionReturnSource::Flow`), resolved through the prepared
    /// value registry — identity only, no body lowering, no execution.
    /// `None` for a declared-return callee, an overload group, a dotted
    /// `typeof` path, or a non-`typeof` argument.
    pub(super) fn flow_return_callee_for_typeof_arg(
        &self,
        callee_arg: SemanticNodeId,
    ) -> Option<verter_type_expr::facts::FlowFunctionReturnIdentity> {
        let data = crate::project_semantic_dispatch::node_data_for(self.ctx, callee_arg)?;
        let (value_root, typeof_path) = data.typeof_head()?;
        if !typeof_path.is_empty() {
            return None;
        }
        let scope_canonical = Arc::clone(&value_root.scope.canonical_id);
        let scope_owner = value_root.scope.owner;
        let value_name = Arc::clone(&value_root.name);
        drop(data);
        let prepared = self.ctx.prepared_value_decl_return_only(
            scope_canonical.as_ref(),
            scope_owner,
            &value_name,
        )?;
        let [signature] = prepared.signatures.as_slice() else {
            return None;
        };
        let verter_type_expr::facts::FunctionReturnSource::Flow(identity) =
            &signature.return_source
        else {
            return None;
        };
        // Anchor fill mirrors the signature-composition consumers: the
        // extractor stamps the declaration name; canonical / owner come
        // from the serving scope.
        let mut identity = identity.clone();
        identity.anchor.canonical_id = scope_canonical;
        identity.anchor.owner = scope_owner;
        Some(identity)
    }

    /// Whether `node` is the builtin `ReturnType<typeof callee>`
    /// instantiation carrier over a body-derived (flow-return) callee —
    /// the shape whose MEMBER projection routes through the
    /// single-member `FlowReturn` demand instead of a whole-signature
    /// composition.
    pub(super) fn is_flow_return_type_member_base(&self, node: SemanticNodeId) -> bool {
        match crate::project_semantic_dispatch::node_data_for(self.ctx, node) {
            Some(data) => self.is_flow_return_type_member_base_data(&data),
            None => false,
        }
    }

    /// The node-data half of [`Self::is_flow_return_type_member_base`].
    /// Matches BOTH carrier stages: the resolved builtin
    /// `InstantiationRef` and the still-unresolved authored
    /// `BareRef("ReturnType", [arg])` (whose head the dispatch's
    /// carrier-subject normalization resolves shadowing-aware — a
    /// userland `ReturnType` shadow settles to its own declaration
    /// there and never enters the flow member rail).
    pub(super) fn is_flow_return_type_member_base_data(&self, data: &SemanticNodeData) -> bool {
        if let SemanticNodeData::InstantiationRef { base, args } = data {
            return base.canonical_id.as_ref() == "__builtin__"
                && base.decl_name.as_ref() == "ReturnType"
                && args.len() == 1
                && self.flow_return_callee_for_typeof_arg(args[0]).is_some();
        }
        if let Some((name, _scope)) = data.bare_ref_head() {
            let args = data.carrier_type_args();
            return name.as_ref() == "ReturnType"
                && args.len() == 1
                && self.flow_return_callee_for_typeof_arg(args[0]).is_some();
        }
        false
    }

    pub(super) fn flow_return_member_projection(
        &self,
        callee_arg: SemanticNodeId,
        segment: &crate::semantic_query::PathSegment,
    ) -> Option<SemanticNodeId> {
        // The demanded member must be a statically-named key.
        let member_name: Arc<str> = match segment {
            crate::semantic_query::PathSegment::Member(key) => Arc::from(key.as_string()?),
            crate::semantic_query::PathSegment::Index(crate::semantic_query::IndexKey::String(
                value,
            )) => Arc::clone(value),
            crate::semantic_query::PathSegment::Index(_) => return None,
        };
        let identity = self.flow_return_callee_for_typeof_arg(callee_arg)?;
        // The member demand rides the CANONICAL identity point plus the
        // authored member path — the only demand shape the proof planner
        // represents. (The evaluator reads only the path; the walker's
        // `Navigate` policy axes are not the flow demand's semantics.)
        let demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::clone(
                                &member_name,
                            )),
                        ),
                    ]);
                point
            },
        };
        let key = self.flow_return_key_with_demand(&identity, demand);
        // The member-demand dispatch is a PROBE with a structured
        // fall-through: a decline (`None`) hands the segment to the
        // generic `Instantiate` unwrap, which re-derives the answer AND
        // its own rails through its own reads. The probe's observations
        // therefore propagate only when its result is CONSUMED — a
        // declined probe's typed refusal (`UnmodeledDemandPoint` for a
        // spread-provisioned or absent member, a degraded success, an
        // in-flight hold) must not mark the enclosing build or the
        // request partial around a fall-through route that answers
        // cleanly. All three observation channels are scoped: the
        // build-local taint frame, the per-cold-compute completeness
        // scope, and the request-level partial sticky (deferred — the
        // sticky has no un-mark).
        let sticky_defer = crate::request_context::DeferredPartialStickyScope::enter();
        let completeness_scope = crate::request_context::ColdComputeCompletenessScope::enter();
        let observation =
            crate::project_semantic_dispatch::BuildLocalTaintGuard::push(&self.build_local_taint);
        let step = self.execute_flow_return(key);
        let observed = observation.finish();
        let completeness = crate::request_context::current_cold_compute_completeness();
        completeness_scope.discard();
        drop(sticky_defer);
        if matches!(&step, FlowReturnStep::Complete(result) if result.degradation().is_none()) {
            crate::request_context::fold_result_completeness(completeness);
            self.fold_into_top_build_local_taint_with(
                observed.result_is_partial,
                observed.cache_suppress,
                observed.partial_reasons,
            );
        }
        match step {
            FlowReturnStep::Complete(result) if result.degradation().is_none() => {
                // `ReturnType<…>` is a signature UTILITY, not a call: it
                // has no call site to be argument-free at, so every free
                // clause parameter instantiates at `unknown` and a
                // declared default never applies (`ReturnType<typeof
                // id>` over `id<T = number>(x: T)` is `{ … unknown … }`,
                // not `number`). That is precisely the policy the
                // WHOLE-return route applies through
                // `instantiate_free_signature_params_at_unknown`; this
                // route is the same utility over the same callee one
                // path segment longer, so it applies the same policy —
                // returning the flow return's raw member position would
                // publish the CALLEE's own binder as the consumer's
                // value, and the two routes would disagree about one
                // callee.
                //
                // The clause NAMES come from the shallow function-program
                // fact rather than from composing the callee's signature,
                // so the member demand stays as narrow as it was: a
                // whole-signature composition here would materialise
                // exactly the sibling members this rail exists to leave
                // cold.
                self.instantiate_callee_clause_at_unknown(&identity, result.return_type())
            }
            // Degraded success / typed failure / in-flight hold: the
            // generic unwrap route decides (it already owns these
            // shapes for every other consumer).
            _ => None,
        }
    }

    /// Instantiate the served callee's OWN type-parameter clause at
    /// `unknown` over a value taken from its body-derived return — the
    /// signature-UTILITY policy, applied without composing a signature.
    ///
    /// A clause the route could not READ is a MISS (`None`), never "the
    /// callee declares none". The two were the same value here — a
    /// failed read returned the callee's return UNTOUCHED, its own
    /// binders intact and warm-admissible — while the CALL-site route
    /// degraded on the identical miss. The clause reader is now shared
    /// (both take a `FunctionProgramEntry` witness) and both states are
    /// distinct, so the asymmetry has no spelling.
    fn instantiate_callee_clause_at_unknown(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
        node: SemanticNodeId,
    ) -> Option<SemanticNodeId> {
        let clause = self.served_callee_clause(identity)?;
        if clause.is_empty() {
            return Some(node);
        }
        // A body-derived return is evaluated with the callee's clause
        // BOUND, so its parameters spell as binders (and, for a
        // still-deferred head, as a bare name) — never as a resolved
        // same-named file-scope declaration, which would be a different
        // symbol.
        Some(self.instantiate_named_params_at_unknown(
            clause.param_names(),
            node,
            crate::semantic_query::ClauseSpelling::WithDeferredHeads,
        ))
    }

    /// The OWN clause of a served function position, read from the
    /// shallow per-file function-program index.
    ///
    /// `None` is a READ FAILURE (the file is not served at this version,
    /// or the position is not indexed) — never an empty clause. The
    /// clause itself is built by its owning module from the index entry,
    /// so this route cannot assemble one either.
    fn served_callee_clause(
        &self,
        identity: &verter_type_expr::facts::FlowFunctionReturnIdentity,
    ) -> Option<CalleeClause> {
        let canonical = identity.anchor.canonical_id.as_ref();
        let serve = self.ctx.ensure_indexed_ready_serve(canonical)?;
        let decl_bodies = serve.indexed.shallow_state.decl_bodies();
        let key = verter_semantic::analysis::function_program::FunctionProgramKey {
            declaration: verter_semantic::analysis::function_program::FunctionDeclarationRef {
                owner: identity.anchor.owner,
                name: Arc::clone(&identity.anchor.symbol),
                space: verter_semantic::facts::SymbolSpace::Value,
            },
            part: identity.function_part.clone(),
            overload_ordinal: identity.overload_ordinal,
        };
        let index = decl_bodies.function_program_index();
        let matched = index.get(&key)?;
        Some(CalleeClause::read_from_program_entry_at_unknown(matched))
    }

    /// The whole-function `FlowReturn` authority. Every whole-function
    /// return demand enters here with the full key:
    ///
    /// 1. **Reentry intercept** — the exact key is already in flight on
    ///    this transaction ⇒ record the scoped assumption edge (a
    ///    coinductive hold — neither a contributor nor a failure) and
    ///    return the `Hold` sentinel.
    /// 2. **Warm read** — a validated published `Complete` result
    ///    (carrier-validated, live-generation gated).
    /// 3. **Cold compute** — the machinery ROOT goes through the family
    ///    singleflight (`execute(FlowReturn)` → `build_flow_return`); a
    ///    nested flow evaluation computes INLINE on the transaction (its
    ///    publish is batched at its SCC's close and drained by the root).
    pub(crate) fn execute_flow_return(&self, key: FlowReturnKey) -> FlowReturnStep {
        // Per-request dispatch-mask trace, mirroring the cold-build choke
        // point: an INLINE flow evaluation (under an open relation or flow
        // frame) never funnels through `execute_via_cold_build_helper`, so
        // the family's participation is recorded here — idempotent per
        // tag, no-op without an installed `RequestContext`.
        if let Some(ctx) = crate::request_context::current_request_context() {
            ctx.record_dispatched_query_tag(crate::semantic_query::SemanticQueryKeyTag::FlowReturn);
        }
        // (1) Reentry intercept.
        {
            let identity = ObligationIdentity::FlowReturn(key.clone());
            let mut txn = self.dispatch_txn.borrow_mut();
            if let Some(idx) = txn.reentry().find(&identity) {
                txn.obligations.record_assumption(idx);
                drop(txn);
                // Cold-path flow-cycle sentinel: a re-entry only occurs
                // while the component is being cold-evaluated (a warm
                // hit never opens a frame to re-enter).
                crate::flow_return_audit::record_flow_cycle_reentry(
                    u32::try_from(idx).unwrap_or(u32::MAX),
                    &key.function.declaration_slot.merged_symbol_name,
                );
                return FlowReturnStep::Hold(Box::new(key));
            }
        }
        // (2) Warm read (carrier-validated, live-generation gated).
        if let Some(result) = self.graph().get_flow_return_result(self.ctx, &key) {
            return FlowReturnStep::Complete(result);
        }
        // (3) Cold compute. Root versus inline is decided by the generic
        // obligation transaction: any open frame — of any domain — makes
        // this evaluation inline.
        if self.dispatch_txn.borrow().obligations.decides_root() {
            self.execute_flow_return_root(key)
        } else {
            self.execute_flow_return_inline(key)
        }
    }

    /// The machinery ROOT path: the full family singleflight. After a
    /// published cold build, drain the SCC-closed member batch (relation
    /// and flow members) onto the root's carrier.
    fn execute_flow_return_root(&self, key: FlowReturnKey) -> FlowReturnStep {
        let read = self.execute_flow_return_cold_build(SemanticQueryKey::FlowReturn(Box::new(key)));
        match read.value {
            QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => {
                FlowReturnStep::Complete((*result).clone())
            }
            // A degraded evaluation surfaces `Error(Miss)` to the memo
            // (loud, never a fallback, never admitted); the TYPED failure
            // rides the transaction to this caller (`Unresolved` only when
            // the cold build never ran — a torn or refused read).
            _ => FlowReturnStep::NoValue(
                self.dispatch_txn
                    .borrow_mut()
                    .flow
                    .last_root_failure
                    .take()
                    .unwrap_or(FlowReturnFailure::Unresolved),
            ),
        }
    }

    /// THE publication-capturing `FlowReturn` executor — the SOLE way a
    /// [`SemanticQueryKey::FlowReturn`] reaches the family cold build.
    ///
    /// A flow SCC defers its non-root members: each one claims the
    /// ordinary family flight, computes inline on the root's
    /// transaction, and batches its publish for the machinery root to
    /// drain. That makes the ROOT the only place the batch can be
    /// released, so every entry into the family cold build must pass
    /// here — the typed producer entry (`execute_flow_return_root`) AND
    /// the generic `SemanticQueryApi` entries, which reach the family
    /// through the shared cold-build helper. A path that skips this
    /// executor leaves each member with a CLAIMED, uncompleted in-flight
    /// entry whose owner has already dropped: the next demand joins it,
    /// the wait graph reports a cycle against an inactive owner, and the
    /// caller gets a PERMANENT false [`QueryResult::Recursive`].
    ///
    /// Release is decided by the PUBLICATION, never by the shape of the
    /// returned value: a real
    /// [`PublishedMemoCandidate`](crate::semantic_query_memo::PublishedMemoCandidate)
    /// drains the batch onto that carrier (member fences preserved); its
    /// ABSENCE — `ReturnOnly`, a typed failure, cancellation, or a
    /// refused admission — aborts and retires the ENTIRE deferred batch
    /// without publishing anything, so `ReturnOnly` stays non-publishing
    /// and no torn or provisional member can warm.
    pub(super) fn execute_flow_return_cold_build(
        &self,
        key: SemanticQueryKey,
    ) -> crate::semantic_query::CacheRead<QueryResult<SemanticQueryValue>> {
        verter_debug_assert!(
            matches!(key, SemanticQueryKey::FlowReturn(_)),
            "the flow-return executor admits FlowReturn keys only"
        );
        let SemanticQueryKey::FlowReturn(root_key) = key.clone() else {
            unreachable!("the flow-return executor admits FlowReturn keys only")
        };
        let mut publication = None;
        let read = self.execute_via_cold_build_helper_capturing_publication(key, &mut publication);
        match publication {
            Some(publication) => self.flow_return_drain_completed_members(&root_key, &publication),
            None => self.relation_abort_completed_members(),
        }
        read
    }

    /// Drain the SCC-closed member batch onto the root's published
    /// carrier, fenced on the FLOW root's own published candidate.
    fn flow_return_drain_completed_members(
        &self,
        root_key: &FlowReturnKey,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
    ) {
        let (relation_members, flow_members, call_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            // The published component's members are warm in the store; the
            // transaction-local value channel's work is done.
            txn.flow.closed_values.clear();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
                std::mem::take(&mut txn.call.completed_members),
            )
        };
        self.publish_scc_member_batch(
            crate::semantic_query_memo::SccRootWitness::flow_return(
                root_key.clone(),
                carrier.admission_seq,
            ),
            carrier,
            relation_members,
            flow_members,
            call_members,
        );
    }

    /// A nested flow evaluation's INLINE cold compute: charge the
    /// connected-demand ledger for the frame open (the machinery root's
    /// charge covers only the root frame — a long DirectCall chain charges
    /// one unit per inline frame), then push a frame, run the evaluation,
    /// and close the frame through the SCC close. The publish is NEVER
    /// direct — it is batched at this frame's SCC close and drained by the
    /// machinery root onto the root's carrier.
    fn execute_flow_return_inline(&self, key: FlowReturnKey) -> FlowReturnStep {
        if self.charge_connected_work().is_err() {
            return FlowReturnStep::NoValue(FlowReturnFailure::Budget(
                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
            ));
        }
        let idx = self.flow_frame_open(&key);
        self.prepare_flow_return_demand(&key, idx);
        let evaluated = self.evaluate_flow_return(&key);
        self.flow_frame_close(idx, evaluated)
    }

    /// The family cold-build arm (the `execute(FlowReturn)` reducer).
    /// Runs the root frame and maps the close onto the admission boundary
    /// through the finalizer's verdict:
    ///
    /// - `Complete(proof)` ⇒ publish: the value is extracted from the
    ///   proof token, which rides the build output
    ///   (`QueryBuildOutput.flow_completion`) to the family memo's proof
    ///   gate — the ONLY warm-admission authority;
    /// - `Partial` (or no plannable demand) ⇒ the usable value RETURNS
    ///   through the success carrier with admission suppressed
    ///   (`ReturnOnly` — no memo entry, no fact signature, no
    ///   reverse-index metadata) and the partial rails set ONCE here;
    /// - `NoValue` ⇒ `Error(Miss)`, suppressed admission, the typed
    ///   failure riding the transaction's root-failure channel.
    ///
    /// The universal read funnel folds these rails into the enclosing
    /// composition's build when this read returns — there is no
    /// consumer-side fold.
    pub(super) fn build_flow_return(
        &self,
        key: &FlowReturnKey,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        verter_audit::attribute_scope!(FlowGraphBuild);
        let fence = self.project_generation_signature();
        let idx = self.flow_frame_open(key);
        self.prepare_flow_return_demand(key, idx);
        let evaluated = self.evaluate_flow_return(key);
        #[cfg(any(test, feature = "test-support"))]
        self.inject_unproven_flow_member_for_tests(idx);
        #[allow(unused_mut)]
        let mut built: QueryBuildOutput<SemanticQueryValue> = match self
            .flow_frame_close_root(idx, evaluated)
        {
            FlowRootClose::EvaluatedValue(root) => {
                let EvaluatedFlowRoot {
                    result,
                    scc_self_roots,
                    materialized,
                    verdict,
                } = *root;
                // The no-value verdict cannot arise on the
                // evaluated-value close arm (the evaluation failed →
                // the `NoValue` close below); fail closed regardless.
                if let Some(FlowSolveOutcome::NoValue(no_value)) = verdict {
                    self.dispatch_txn.borrow_mut().flow.last_root_failure = Some(no_value.failure);
                    let mut output: QueryBuildOutput<SemanticQueryValue> =
                        (QueryResult::Error(QueryError::Miss), fence).into();
                    output.cache_suppress = true;
                    output.result_is_partial = true;
                    output.partial_reasons = NO_VALUE_REASON_CLASS;
                    return output;
                }
                let proof = match &verdict {
                    Some(FlowSolveOutcome::Complete(proof)) => Some(proof.clone()),
                    _ => None,
                };
                // The published value is extracted from the proof token
                // when one exists; the raw evaluated value flows to the
                // caller either way (a degraded success stays usable).
                let published = match &proof {
                    Some(proof) => proof.value().clone(),
                    None => result,
                };
                let mut output: QueryBuildOutput<SemanticQueryValue> = QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::FlowReturn(Arc::new(published))),
                    fence,
                ))
                .with_observed_self_roots(scc_self_roots);
                // §3.4: the published entry's `satisfied_projection` is
                // the point set the compute ACTUALLY produced — recorded
                // by the evaluation, never the nominal request echoed at
                // publish time.
                output.satisfied_projection = materialized;
                match verdict {
                    Some(FlowSolveOutcome::Complete(proof)) => {
                        output.flow_completion = Some(proof);
                    }
                    // The finalizer-outcome adapter: translate the partial
                    // ONCE into the build's own rails (the displaced root
                    // channel). The value still flows to the caller; the
                    // memo refuses admission — the proof gate and the
                    // boolean rails now agree.
                    Some(FlowSolveOutcome::Partial(partial)) => {
                        output.cache_suppress = true;
                        output.result_is_partial = true;
                        output.partial_reasons = match partial.value.degradation() {
                            Some(degradation) => degradation_reason_class(degradation),
                            None => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
                        };
                    }
                    // The demand could not be planned at all (a typed
                    // planning refusal — an over-budget obligation set or
                    // an unrepresentable demand): unproven, ReturnOnly.
                    None => {
                        output.cache_suppress = true;
                        output.result_is_partial = true;
                        output.partial_reasons = PartialReasonSet::FLOW_RETURN_UNVERIFIED;
                    }
                    // Handled above.
                    Some(FlowSolveOutcome::NoValue(_)) => unreachable!(),
                }
                output
            }
            FlowRootClose::NoValue(failure) => {
                let mut output: QueryBuildOutput<SemanticQueryValue> =
                    (QueryResult::Error(QueryError::Miss), fence).into();
                // ReturnOnly: the failure flows to the caller through the
                // transaction's root-failure channel, the memo refuses
                // admission (no warm entry, no fact signature, no
                // reverse-index metadata), and the request marks partial —
                // the universal read funnel propagates both rails into the
                // enclosing build.
                self.dispatch_txn.borrow_mut().flow.last_root_failure = Some(failure);
                output.cache_suppress = true;
                output.result_is_partial = true;
                output.partial_reasons = NO_VALUE_REASON_CLASS;
                output
            }
        };
        #[cfg(any(test, feature = "test-support"))]
        if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .strip_root_proof
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            built.flow_completion = None;
        }
        built
    }

    // ──────────────────────────────────────────────────────────────────
    // Frames and the tagged SCC close
    // ──────────────────────────────────────────────────────────────────

    /// Push a flow-return frame for `key`, claiming the ordinary family
    /// flight for a non-root inline evaluation.
    fn flow_frame_open(&self, key: &FlowReturnKey) -> usize {
        let wants_inline_flight = !self.dispatch_txn.borrow().obligations.decides_root();
        let inline_flight = wants_inline_flight
            .then(|| self.graph().begin_inline_flow_return_flight(key))
            .flatten();
        let mut txn = self.dispatch_txn.borrow_mut();
        let watermark = txn.obligations.pending().pending_len();
        let idx = txn.reentry_mut().push_flow_return(key.clone(), watermark);
        if let Some(state) = txn
            .reentry_mut()
            .frame_mut_for_update(idx)
            .and_then(super::dispatch_txn::ObligationFrame::flow_return_mut)
        {
            state.inline_flight = inline_flight;
        }
        idx
    }

    /// Close an INLINE frame.
    fn flow_frame_close(&self, idx: usize, evaluated: FlowEvaluationOutcome) -> FlowReturnStep {
        match self.flow_frame_pop(idx, evaluated, false) {
            FlowFramePop::Provisional(step) => step,
            FlowFramePop::RootClose(close) => match close {
                FlowRootClose::EvaluatedValue(root) => FlowReturnStep::Complete(root.result),
                FlowRootClose::NoValue(failure) => FlowReturnStep::NoValue(failure),
            },
        }
    }

    /// Apply a frame key's type substitution to the frame's outgoing
    /// result — the instantiation transfer for a body-derived callee
    /// demanded under a call's final ordered mapping. Empty substitution
    /// (the canonical production key) is a no-op.
    pub(super) fn apply_frame_key_substitution(
        &self,
        key: &FlowReturnKey,
        result: FlowReturnResult,
    ) -> FlowReturnResult {
        let substitution = &key.context.type_substitution;
        if substitution.bindings().is_empty() {
            return result;
        }
        let substituted = self.substitute_canonical(result.return_type(), substitution);
        if substituted == result.return_type() {
            return result;
        }
        result.with_return_type(self.graph().as_ref(), substituted)
    }

    /// Close the machinery ROOT frame.
    /// Test seam: close one inline frame with a decided outcome and
    /// tagged holds. Flow identities are rejected by the callee gate's
    /// clause authority, so the seam accepts the tagged form and
    /// converts only the resolved-call arm — a flow hold minted without
    /// its clause would discharge untransferred.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)] // exercised by the return-equation close tests
    pub(super) fn flow_frame_close_for_tests(
        &self,
        idx: usize,
        outcome: FlowReturnPendingOutcome,
        holds: Vec<super::dispatch_txn::ReturnObligationIdentity>,
    ) -> FlowReturnStep {
        self.flow_frame_close_with_evidence_for_tests(idx, outcome, holds, None)
    }

    /// Test seam: [`Self::flow_frame_close_for_tests`] carrying the
    /// evaluation's discharge report as well — the proof-layer evidence a
    /// staged member finalizes against at the component close.
    #[cfg(any(test, feature = "test-support"))]
    #[allow(dead_code)] // exercised by the return-equation close tests
    pub(super) fn flow_frame_close_with_evidence_for_tests(
        &self,
        idx: usize,
        outcome: FlowReturnPendingOutcome,
        holds: Vec<super::dispatch_txn::ReturnObligationIdentity>,
        discharge: Option<super::dispatch_txn::flow_obligation_state::FlowDischargeReport>,
    ) -> FlowReturnStep {
        let holds = holds
            .into_iter()
            .filter_map(|identity| match identity {
                super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key) => {
                    Some(HeldCallee::call(key))
                }
                super::dispatch_txn::ReturnObligationIdentity::FlowReturn(_) => None,
            })
            .collect();
        // The staged close's evaluation provenance: the frame's installed
        // demand carrier's own token when one exists (the seam stages the
        // close of an evaluation that carrier served), else the bare
        // freshness mint, which can never match a real carrier.
        let provenance = {
            let txn = self.dispatch_txn.borrow();
            txn.reentry()
                .frame(idx)
                .and_then(|frame| match &frame.domain {
                    ObligationFrameDomain::FlowReturn(state) => state.flow_demand.clone(),
                    _ => None,
                })
                .map(|carrier| carrier.provenance)
                .unwrap_or_else(|| self.current_flow_evaluation_provenance())
        };
        self.flow_frame_close(
            idx,
            FlowEvaluationOutcome {
                outcome,
                self_roots: Vec::new(),
                holds,
                materialized: crate::semantic_query::demand::MaterializedSet::empty(),
                fresh_seed: false,
                discharge,
                provenance,
            },
        )
    }

    fn flow_frame_close_root(&self, idx: usize, evaluated: FlowEvaluationOutcome) -> FlowRootClose {
        match self.flow_frame_pop(idx, evaluated, true) {
            FlowFramePop::RootClose(close) => close,
            FlowFramePop::Provisional(_) => unreachable!(
                "the machinery root frame is always its SCC's root: the stack is \
                 empty below it, so no open assumption can target a deeper frame"
            ),
        }
    }

    /// Test-only: record the component carrier a FLOW-root close composed
    /// into the armed
    /// [`flow_admission_fault_injection::FlowAdmissionFaultKnobs::flow_root_carrier_probe`]
    /// slot (its canonicals, in composition order); a no-op while the
    /// slot is disarmed.
    #[cfg(any(test, feature = "test-support"))]
    fn record_flow_root_carrier_for_tests(
        &self,
        roots: &[crate::semantic_query_memo::ObservedGraphSelfRoot],
    ) {
        let mut probe = self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .flow_root_carrier_probe
            .lock()
            .expect("the flow-root carrier probe is never poisoned");
        if let Some(recorded) = probe.as_mut() {
            *recorded = roots
                .iter()
                .map(|(canonical, _)| Arc::clone(canonical))
                .collect();
        }
    }

    /// Test-only: leave ONE unproven provisional flow member on the
    /// pending ledger beneath the OPEN machinery-root frame `root_idx`,
    /// so the root's close drains it exactly as an organic torn component
    /// would. Fires once per set
    /// [`flow_admission_fault_injection::FlowAdmissionFaultKnobs::unproven_flow_member`]
    /// slot (the slot is consumed) and is a no-op while the slot is empty.
    ///
    /// The member takes the REAL provisional path — frame push, demand
    /// preparation over the key's actual function, an assumption edge to
    /// the root, and the non-root pop — with an evaluated value but NO
    /// discharge report: its planned obligations stay pending, so the
    /// component close finalizes it unproven. The pipeline is built never
    /// to produce this member on its own; the slot presents it so each
    /// root-close consumer can be proven to refuse warming around it.
    #[cfg(any(test, feature = "test-support"))]
    pub(super) fn inject_unproven_flow_member_for_tests(&self, root_idx: usize) {
        let key = self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .unproven_flow_member
            .lock()
            .expect("the unproven-member fault slot is never poisoned")
            .take();
        let Some(key) = key else {
            return;
        };
        let idx = {
            let mut txn = self.dispatch_txn.borrow_mut();
            let watermark = txn.obligations.pending().pending_len();
            txn.reentry_mut().push_flow_return(key.clone(), watermark)
        };
        self.prepare_flow_return_demand(&key, idx);
        let provenance = match self.flow_demand_carrier_of(&key) {
            Some(carrier) => carrier.provenance,
            None => self.current_flow_evaluation_provenance(),
        };
        // The member ASSUMES the root: that back-edge is what makes its
        // pop provisional (a deposit at or above the root's drain
        // watermark) instead of a root close of its own.
        self.dispatch_txn
            .borrow_mut()
            .obligations
            .record_assumption(root_idx);
        let number = self
            .graph()
            .intern_node(crate::semantic_query::SemanticNodeData::Primitive(
                crate::semantic_query::PrimitiveKind::Number,
            ));
        let value = crate::semantic_query::FlowReturnResult::new(self.graph(), number, false, None);
        let pop = self.flow_frame_pop(
            idx,
            FlowEvaluationOutcome {
                outcome: FlowReturnPendingOutcome::EvaluatedValue(value),
                self_roots: Vec::new(),
                holds: Vec::new(),
                materialized: crate::semantic_query::demand::MaterializedSet::empty(),
                fresh_seed: false,
                discharge: None,
                provenance,
            },
            false,
        );
        assert!(
            matches!(pop, FlowFramePop::Provisional(_)),
            "the injected member assumes the open root, so its pop is provisional"
        );
    }

    // ──────────────────────────────────────────────────────────────────
    // The flow-solve proof wiring
    // ──────────────────────────────────────────────────────────────────

    /// The dispatch's current evaluation FRESHNESS mint: the semantic
    /// store's instance identity, the live project generation, and THIS
    /// dispatch's obligation-runtime instance identity. The demand-unique
    /// axis is NOT knowable here — the mint carries the sentinel ordinal
    /// no installed demand ever bears, and only
    /// [`FlowEvaluationProvenance::freshness_axes`] of a minted token may
    /// be compared against it. Demand preparation derives the real token
    /// (freshness axes + the demand's ledger ordinal) from this mint; the
    /// finalization driver accepts evidence only when the evidence token
    /// IS the demand carrier's own token and the carrier's freshness axes
    /// equal a FRESH mint's: a value, report, or convergence claim from
    /// another demand, another runtime, another store, or a stale request
    /// generation is a typed partial, never a proof.
    pub(super) fn current_flow_evaluation_provenance(&self) -> FlowEvaluationProvenance {
        let store_identity = Arc::as_ptr(self.graph()) as *const () as usize as u64;
        let runtime_identity = self.dispatch_txn.borrow().obligations.instance_identity();
        FlowEvaluationProvenance::new(
            store_identity,
            self.ctx.project_type_store().project_generation(),
            runtime_identity,
            // The sentinel ordinal: no installed demand bears it (ledger
            // ordinals are small counting numbers), so a bare freshness
            // mint can never pose as one demand's evidence.
            u64::MAX,
        )
    }

    /// Prepare one cold flow demand's proof layer — IMMEDIATELY after the
    /// frame opens and before content evaluation (the root path
    /// [`Self::build_flow_return`] and the inline path
    /// [`Self::execute_flow_return_inline`] both call here): plan the
    /// demand ONCE over the store-minted bound graph and the retained
    /// structural selection — the same cold planning run the hash node
    /// retained, never a re-plan — and install the per-demand obligation
    /// set on the frame. A refusal at any step installs nothing: the
    /// evaluation still runs (values are the evaluator's), but no proof
    /// can mint and the close finalizes unproven (`ReturnOnly`).
    pub(super) fn prepare_flow_return_demand(&self, key: &FlowReturnKey, frame_idx: usize) {
        use super::flow_solve::{build_flow_demand_plan, FlowDemandRequest, FlowResourcePolicy};
        let Ok(site) = self.flow_slice_demand_site(key) else {
            return;
        };
        let flow_slice = self.ctx.project_type_store().flow_slice();
        // The retained structural plan of the ONE cold planning run (the
        // hash node's published outcome) — the demand planner assembles
        // obligations FROM it; it never invokes the slice planner again.
        let planned = match crate::cache_runtime::lookup(
            flow_slice.hash_node(),
            site.slice_key.clone(),
            self.ctx,
        ) {
            Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::Planned(planned)) => {
                planned
            }
            // An over-budget plan or a torn view installs no demand: the
            // evaluation's own hash-node lookup reaches the same outcome
            // and fails closed with the typed failure.
            _ => return,
        };
        let Some(bound) = flow_slice.bound_graph_for(&site.slice_key_function) else {
            return;
        };
        // The demand-unique axis of this demand's provenance: the ledger
        // ordinal the install below receives (installation appends, so the
        // peek IS the ordinal; the assertion after install pins it).
        let demand_ordinal = self.dispatch_txn.borrow().obligations.flow_demand_count() as u64;
        let provenance = {
            let freshness = self.current_flow_evaluation_provenance();
            let (store_identity, request_generation, runtime_identity) = freshness.freshness_axes();
            FlowEvaluationProvenance::new(
                store_identity,
                request_generation,
                runtime_identity,
                demand_ordinal,
            )
        };
        #[cfg(any(test, feature = "test-support"))]
        let provenance = if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .stale_demand_carrier
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(
                u64::MAX - 1,
                0,
                0,
                0,
            )
        } else {
            provenance
        };
        let request = FlowDemandRequest {
            query: SemanticQueryKey::FlowReturn(Box::new(key.clone())),
            // The in-flight observation identity of this demand: derived
            // from the SAME provenance mint the carrier and the evaluation
            // outcome bear — never a cache-candidate axis.
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources: FlowResourcePolicy::default(),
            additional_requirements: Arc::from([]),
        };
        let Ok(plan) = build_flow_demand_plan(request, &bound, &planned, &site.inventory) else {
            return;
        };
        flow_slice.note_demand_planned();
        let plan = Arc::new(plan);
        let mut txn = self.dispatch_txn.borrow_mut();
        let handle = txn.obligations.install_flow_demand(&plan);
        verter_debug_assert!(
            handle.slot_index() == demand_ordinal,
            "the peeked ledger ordinal must be the installed demand's slot: \
             the provenance binds to exactly this demand"
        );
        if let Some(state) = txn
            .reentry_mut()
            .frame_mut_for_update(frame_idx)
            .and_then(super::dispatch_txn::ObligationFrame::flow_return_mut)
        {
            state.flow_demand = Some(FlowDemandCarrier {
                handle,
                plan,
                provenance,
            });
        }
    }

    /// The open frame's installed demand carrier for `key`, when this
    /// frame was prepared.
    pub(super) fn flow_demand_carrier_of(&self, key: &FlowReturnKey) -> Option<FlowDemandCarrier> {
        let txn = self.dispatch_txn.borrow();
        let idx = txn
            .reentry()
            .find(&ObligationIdentity::FlowReturn(key.clone()))?;
        txn.reentry()
            .frame(idx)
            .and_then(|frame| match &frame.domain {
                ObligationFrameDomain::FlowReturn(state) => state.flow_demand.clone(),
                _ => None,
            })
    }

    /// The evaluator's typed discharge report for one evaluated frame:
    /// which planned obligations the evaluation ACTUALLY completed,
    /// claimed from the run's own execution witness — never from the
    /// plan's expectations. Built ONCE, at the end of the evaluation.
    ///
    /// Per-basis evidence sources:
    /// - STRUCTURAL bases (sites, bindings, guards, contextual targets,
    ///   captured bindings, edges) claim from the retained selection whose
    ///   lowered IR the evaluation executed — a node the executed
    ///   selection does not hold is never claimed;
    /// - the WHOLE-SLICE bases (family coverage, contract domains) claim
    ///   only when the executed selection IS the plan's retained selection
    ///   (the enumeration input and the executed content are one);
    /// - CALL bases claim only from a recorded decided call occurrence
    ///   (the evaluator's call-sink evidence — a value or a coinductive
    ///   hold, never an unmodelled or degraded outcome);
    /// - RELATION bases additionally require every relation outcome the
    ///   occurrence's resolution consumed to be DECIDED (`Unknown` /
    ///   `BudgetExceeded` are not evidence);
    /// - gap-installed bases (`UnmodeledBinding` / `Capture`) are never
    ///   claimed.
    ///
    /// An obligation without evaluator-produced evidence stays unclaimed,
    /// so the demand cannot seal and finalizes unproven (`ReturnOnly`).
    /// The runtime still re-validates every claim against the obligation's
    /// spec at application time.
    fn flow_evaluation_discharge_report(
        &self,
        carrier: &FlowDemandCarrier,
        witness: &FlowExecutionWitness<'_>,
    ) -> super::dispatch_txn::flow_obligation_state::FlowDischargeReport {
        use super::dispatch_txn::flow_obligation_state::{
            FlowDischargeEntry, FlowDischargeReport, FlowObligationBasis, FlowSuboperationEvidence,
            ObligationState,
        };
        let txn = self.dispatch_txn.borrow();
        let Some(records) = txn.obligations.flow_obligations(carrier.handle) else {
            return FlowDischargeReport::new(Vec::new());
        };
        // The whole-slice witness: the walk-completed selection the
        // evaluation executed is exactly the retained selection the
        // plan's obligations expanded from. A short/aborted walk yields
        // NO executed selection at all, and a torn view between the two
        // lookups executes a DIFFERENT slice — neither claims exhaustive
        // enumeration.
        let whole_selection_executed =
            witness.executed_selection == Some(carrier.plan.structural_selection());
        // The decided call evidence of one planned call occurrence:
        // `None` = the evaluation never evaluated it; `Some(decided)` =
        // it did, with `decided` the conjunction of every recorded
        // occurrence's relation-decided bit.
        let call_evidence_for = |site: verter_semantic::analysis::flow::SkeletonExprSiteId,
                                 call_ordinal: u32| {
            let call = witness
                .skeleton
                .expr_site(site)
                .calls
                .get(call_ordinal as usize)?;
            let mut relations_decided: Option<bool> = None;
            for evidence in witness.calls {
                let evaluated = verter_semantic::analysis::flow::FrameSpan::rebase(
                    witness.anchor,
                    evidence.span,
                );
                if evaluated == call.span {
                    let decided = relations_decided.get_or_insert(true);
                    *decided &= evidence.relations_decided;
                }
            }
            relations_decided
        };
        let entries = carrier
            .plan
            .obligation_specs()
            .iter()
            .filter(|spec| {
                records.iter().any(|record| {
                    record.spec.id() == spec.id()
                        && matches!(record.state, ObligationState::Pending)
                })
            })
            .filter(|spec| match spec.basis() {
                FlowObligationBasis::FamilyCoverage { .. }
                | FlowObligationBasis::DemandRoot { .. } => whole_selection_executed,
                FlowObligationBasis::Site { node, .. }
                | FlowObligationBasis::Binding { node, .. }
                | FlowObligationBasis::Guard { node, .. }
                | FlowObligationBasis::ContextualTarget { node, .. }
                | FlowObligationBasis::CapturedBinding { node, .. } => witness
                    .executed_selection
                    .is_some_and(|selection| selection.is_selected(*node)),
                FlowObligationBasis::Edge { from, to, .. } => {
                    witness.executed_selection.is_some_and(|selection| {
                        selection.is_selected(*from) && selection.is_selected(*to)
                    })
                }
                FlowObligationBasis::CallSite {
                    site, call_ordinal, ..
                } => call_evidence_for(*site, *call_ordinal).is_some(),
                FlowObligationBasis::SemanticRelation {
                    site, call_ordinal, ..
                } => call_evidence_for(*site, *call_ordinal) == Some(true),
                FlowObligationBasis::UnmodeledBinding { .. }
                | FlowObligationBasis::Capture { .. } => false,
            })
            .map(|spec| FlowDischargeEntry {
                obligation: spec.id(),
                dependencies: Arc::from(spec.expected_dependencies()),
                suboperations: spec
                    .expected_suboperations()
                    .iter()
                    .map(|operation| FlowSuboperationEvidence {
                        operation: *operation,
                        result_contract: carrier.plan.basis().result_contract.clone(),
                    })
                    .collect(),
            })
            .collect();
        FlowDischargeReport::new(entries)
    }

    /// The ONE finalization driver for one evaluated flow demand: the
    /// central application of the evaluator's typed discharge report (in
    /// the plan's deterministic work order), the replay of the
    /// runtime-observed component convergence into the demand's own
    /// observation log, the seal, and the finalizer. Runs ONLY after the
    /// component fixed point, the literal widening, and the per-key
    /// substitution — the proof covers exactly `result`.
    ///
    /// Returns `None` when the frame carries no installed demand (the
    /// demand could not be planned): unproven, never warm.
    pub(super) fn finalize_flow_demand(
        &self,
        carrier: Option<&FlowDemandCarrier>,
        discharge: Option<&super::dispatch_txn::flow_obligation_state::FlowDischargeReport>,
        convergence: &ObservedFlowConvergence,
        provenance: FlowEvaluationProvenance,
        result: &FlowReturnResult,
    ) -> Option<FlowSolveOutcome> {
        use super::dispatch_txn::flow_obligation_state::FlowSealError;
        let carrier = carrier?;
        #[cfg(any(test, feature = "test-support"))]
        let provenance = if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .foreign_evaluation_evidence
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(
                u64::MAX,
                u64::MAX,
                u64::MAX,
                u64::MAX,
            )
        } else if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .foreign_demand_provenance
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            // A same-store, same-generation, same-runtime token of ANOTHER
            // demand: the freshness axes pass; the demand ordinal betrays
            // it.
            let (store_identity, request_generation, runtime_identity) =
                carrier.provenance.freshness_axes();
            super::dispatch_txn::flow_obligation_state::FlowEvaluationProvenance::new(
                store_identity,
                request_generation,
                runtime_identity,
                carrier.provenance.demand_ordinal() ^ 1,
            )
        } else {
            provenance
        };
        #[cfg(any(test, feature = "test-support"))]
        let unobserved_convergence;
        #[cfg(any(test, feature = "test-support"))]
        let convergence = if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .unobserved_convergence
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            unobserved_convergence = ObservedFlowConvergence {
                iterations: 0,
                stable: true,
            };
            &unobserved_convergence
        } else {
            convergence
        };
        let plan = &carrier.plan;
        let basis = plan.basis().clone();
        let partial = |reason: FlowPartialReason| {
            Some(FlowSolveOutcome::Partial(PartialFlowResult {
                basis,
                reason,
                value: result.clone(),
            }))
        };
        // Provenance triangulation: the evaluation's evidence token must
        // BE the installed demand carrier's own token (the demand-unique
        // ordinal included), and the carrier's freshness axes must equal
        // the dispatch's CURRENT mint — a value, report, or convergence
        // claim from another demand, another store, or a stale request
        // generation is a typed partial, never a proof.
        let current = self.current_flow_evaluation_provenance();
        if provenance != carrier.provenance
            || carrier.provenance.freshness_axes() != current.freshness_axes()
        {
            return partial(FlowPartialReason::ForeignProvenance);
        }
        let mut txn = self.dispatch_txn.borrow_mut();
        let runtime = &mut txn.obligations;
        #[cfg(any(test, feature = "test-support"))]
        let dropped_claim_report;
        #[cfg(any(test, feature = "test-support"))]
        let discharge = match discharge {
            Some(report)
                if self
                    .ctx
                    .host_for_fact_tracer_install()
                    .flow_fault_injection
                    .drop_last_discharge_claim
                    .load(std::sync::atomic::Ordering::Relaxed)
                    && !report.entries().is_empty() =>
            {
                let entries = report.entries();
                dropped_claim_report =
                    super::dispatch_txn::flow_obligation_state::FlowDischargeReport::new(
                        entries[..entries.len() - 1].to_vec(),
                    );
                Some(&dropped_claim_report)
            }
            other => other,
        };
        // The central report application: deterministic, in the plan's
        // work order, every claim re-validated against its spec.
        if let Some(report) = discharge {
            if runtime
                .apply_flow_discharge_report(carrier.handle, plan, report)
                .is_err()
            {
                return partial(unproven_flow_reason(runtime, carrier.handle));
            }
        }
        // Replay the observed component convergence: the runtime counts
        // the iterations itself, so the sealed evidence comes from the
        // runtime's own log, never from the caller. The count itself must
        // come from the discharge driver's real observation — every close
        // routes its root through the fixed point, so a zero here is not
        // evidence at all: refuse it rather than fabricate an iteration.
        if convergence.iterations == 0 {
            return partial(FlowPartialReason::NonConverged);
        }
        for _ in 1..convergence.iterations {
            if runtime
                .observe_flow_iteration(carrier.handle, true)
                .is_err()
            {
                return partial(FlowPartialReason::NonConverged);
            }
        }
        if !convergence.stable {
            return partial(FlowPartialReason::NonConverged);
        }
        if let Err(error) = runtime.observe_flow_iteration(carrier.handle, false) {
            return partial(match error {
                super::dispatch_txn::flow_obligation_state::FlowTransitionError::ConvergenceBudget => {
                    FlowPartialReason::NonConverged
                }
                _ => unproven_flow_reason(runtime, carrier.handle),
            });
        }
        match runtime.seal_flow_completion(carrier.handle, result.clone()) {
            Ok(sealed) => Some(super::flow_solve::finalize_flow_solve(
                runtime,
                carrier.handle,
                plan,
                sealed,
            )),
            Err(error) => partial(match error {
                FlowSealError::DegradedValue => FlowPartialReason::DegradedValue,
                FlowSealError::NonConverged => FlowPartialReason::NonConverged,
                FlowSealError::UndischargedObligations => {
                    unproven_flow_reason(runtime, carrier.handle)
                }
                FlowSealError::NoDemandInstalled => FlowPartialReason::NoDemandInstalled,
                FlowSealError::AlreadySealed => {
                    FlowPartialReason::Failed(super::flow_solve::FlowFailure {
                        class: super::flow_solve::FlowFailureClass::Internal,
                    })
                }
            }),
        }
    }

    /// Record a typed failure on one installed demand: every still-open
    /// obligation transitions to `Failed` under the failure's class, and
    /// the demand's no-value verdict is produced (the typed failure plus
    /// the proof-state reason). Failure detection on the ledger — never
    /// an admission decision; a no-value outcome never warms.
    pub(super) fn fail_flow_demand(
        &self,
        carrier: Option<&FlowDemandCarrier>,
        failure: FlowReturnFailure,
    ) -> Option<FlowSolveOutcome> {
        use super::dispatch_txn::flow_obligation_state::ObligationState;
        let carrier = carrier?;
        let class = match failure {
            FlowReturnFailure::Budget(_) => super::flow_solve::FlowFailureClass::BudgetExhausted,
            _ => super::flow_solve::FlowFailureClass::Internal,
        };
        let record = super::flow_solve::FlowFailure { class };
        let open: Vec<_> = {
            let txn = self.dispatch_txn.borrow();
            txn.obligations
                .flow_obligations(carrier.handle)
                .into_iter()
                .flatten()
                .filter(|record| {
                    matches!(
                        record.state,
                        ObligationState::Pending | ObligationState::Running
                    )
                })
                .map(|record| record.spec.id())
                .collect()
        };
        let mut txn = self.dispatch_txn.borrow_mut();
        for id in open {
            let _ = txn
                .obligations
                .fail_flow_obligation(carrier.handle, id, record);
        }
        Some(FlowSolveOutcome::NoValue(NoValueFlowResult {
            basis: carrier.plan.basis().clone(),
            reason: FlowPartialReason::Failed(record),
            failure,
        }))
    }

    /// The shared flow frame-pop + tagged SCC close. On a non-root pop the
    /// member defers PROVISIONALLY to the tagged ledger and returns its
    /// caller-return step; on an SCC-root pop the whole tagged component
    /// closes atomically (the relation members discharge through the
    /// shared [`Self::relation_discharge_and_route`], the flow members'
    /// outcomes are final at pop).
    fn flow_frame_pop(
        &self,
        idx: usize,
        evaluated: FlowEvaluationOutcome,
        machinery_root: bool,
    ) -> FlowFramePop {
        let FlowEvaluationOutcome {
            outcome,
            self_roots,
            mut holds,
            materialized,
            fresh_seed,
            discharge,
            provenance,
        } = evaluated;
        let popped = self.dispatch_txn.borrow_mut().reentry_mut().pop();
        let self_cycle = popped.assumption_targets.contains(&idx);
        let pending_watermark = popped.pending_watermark;
        let budget_cap = popped.budget_cap;
        let root_key = popped
            .identity
            .as_flow_return()
            .expect("a flow code path pops a flow frame")
            .clone();
        let ObligationFrameDomain::FlowReturn(flow_state) = popped.domain else {
            unreachable!("a flow code path pops a flow frame");
        };
        let inline_flight = flow_state.inline_flight;
        // The frame's installed demand carrier (handle + plan +
        // provenance), installed by `prepare_flow_return_demand` at frame
        // open. It rides the deferral so the component close finalizes the
        // member against EXACTLY its own demand.
        let flow_demand = flow_state.flow_demand;
        // Tagged holds recorded against this frame by indexed call
        // evaluation while it was active ride into the close with the
        // evaluator's own holds — a resolved-call dependency joins the
        // fixed point as the call hold it is.
        holds.extend(
            flow_state
                .holds
                .iter()
                .filter_map(|identity| match identity {
                    super::dispatch_txn::ReturnObligationIdentity::ResolveCall(key) => {
                        Some(HeldCallee::call(key.clone()))
                    }
                    super::dispatch_txn::ReturnObligationIdentity::FlowReturn(_) => None,
                }),
        );
        // A budget edge on the frame poisons the whole component. The
        // outcome it replaces may already have observed a degradation —
        // carry it, so the budget failure does not launder it away.
        let outcome = if budget_cap.is_some() {
            FlowReturnPendingOutcome::NoValue {
                failure: FlowReturnFailure::Budget(
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                ),
                degradation: outcome.degradation(),
            }
        } else {
            outcome
        };
        let is_scc_root = match popped.min_open_target {
            None => true,
            Some(target) => target >= idx,
        };
        if !is_scc_root {
            // PROVISIONAL member: defer to the tagged ledger, propagate the
            // still-open lowlink to the parent, and return the caller-return
            // step. NEVER publishes here. The member's demand carrier and
            // its typed discharge report ride the deferral to the close.
            let step = match &outcome {
                FlowReturnPendingOutcome::EvaluatedValue(result) => FlowReturnStep::Complete(
                    self.apply_frame_key_substitution(&root_key, result.clone()),
                ),
                FlowReturnPendingOutcome::NoValue { failure, .. } => {
                    FlowReturnStep::NoValue(*failure)
                }
            };
            let mut txn = self.dispatch_txn.borrow_mut();
            txn.obligations.propagate_lowlink(popped.min_open_target);
            txn.obligations.pending_mut().deposit(PendingObligation {
                identity: ObligationIdentity::FlowReturn(root_key),
                domain: PendingObligationDomain::FlowReturn(FlowReturnPendingState {
                    outcome,
                    inline_flight,
                    holds,
                    self_roots,
                    materialized,
                    fresh_seed,
                    flow_demand,
                    discharge,
                    provenance,
                }),
            });
            return FlowFramePop::Provisional(step);
        }

        // ── SCC close at this root ──────────────────────────────────
        let mut relation_members = Vec::new();
        let mut flow_members = Vec::new();
        let mut call_members: Vec<(
            crate::semantic_query::ResolveCallKey,
            super::dispatch_txn::ResolveCallPendingState,
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
                    relation_members.push(super::relation::DrainedRelationMember {
                        key: key.clone(),
                        occurrence,
                        verdict: state.verdict,
                        session_delta: state.session_delta,
                        opened_session: state.opened_session,
                        inline_flight: state.inline_flight,
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
                PendingObligationDomain::FlowReturn(state) => {
                    let key = member
                        .identity
                        .as_flow_return()
                        .expect("flow-return pending member carries a flow identity")
                        .clone();
                    flow_members.push(super::relation::DrainedFlowReturnMember {
                        key,
                        outcome: state.outcome,
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
            }
        }
        let cyclic = !relation_members.is_empty()
            || !flow_members.is_empty()
            || !call_members.is_empty()
            || self_cycle;
        // The ONE discharge: every flow member and the root reach the
        // equation fixed point `result_i = seed_i ∪ (⋃ hold targets)`
        // together — an EmptyCycle with no discharged target stays
        // `ReturnOnly` and poisons the component.
        let mut outcome = outcome;
        // The mixed component (this root included, when it has holds or
        // drained members) discharges to ONE joint fixed point through
        // the shared close helper: a SELF-cycle converges to its seed,
        // a hold-only empty cycle resurrects on its targets' admitted
        // returns, and the call members' replay + return equation
        // iterates against the flow side's current values.
        let replay_substitution: super::dispatch_txn::ProvisionalSubstitution = relation_members
            .iter()
            .map(|member| {
                (
                    super::dispatch_txn::ObligationIdentity::Relate {
                        key: member.key.clone(),
                        occurrence: member.occurrence,
                    },
                    super::dispatch_txn::ProvisionalVerdict::Relate(
                        super::relation::relation_step_from_pending(&member.verdict),
                    ),
                )
            })
            .collect();
        // The root ALWAYS enters the fixed-point input — a hold-free solo
        // root included: its convergence evidence then comes from the real
        // discharge driver (one observed no-progress pass for a solo
        // entry), never from a caller-fabricated zero-iteration claim.
        let prefix_entries = vec![super::dispatch_txn::FlowDischargeEntry {
            key: root_key.clone(),
            outcome: outcome.clone(),
            holds: holds.clone(),
            fresh_seed,
        }];
        let (prefix_outcomes, call_results, convergence) = match self
            .discharge_mixed_component_to_fixed_point(
                prefix_entries,
                &mut flow_members,
                &mut call_members,
                &replay_substitution,
            ) {
            Ok(ok) => ok,
            Err(failure) => {
                self.flow_return_abort_inline_flight(inline_flight.as_ref());
                for member in &relation_members {
                    self.relation_abort_inline_flight(member.inline_flight.as_ref());
                }
                self.flow_return_abort_drained_flights(&flow_members);
                for (_, member) in &call_members {
                    self.resolve_call_abort_inline_flight(member.inline_flight.as_ref());
                    if let Some(session) = member.staged_session {
                        self.abandon_session(session);
                    }
                }
                // The component's OWN failure outranks the solver's: a
                // frame budget edge and a no-value flow outcome are the
                // poison the equation failure merely rides. Record the
                // failure on every installed demand of the component
                // (failure detection on the ledger — never an admission
                // decision).
                let failure = if budget_cap.is_some() {
                    FlowReturnFailure::Budget(
                        verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                    )
                } else if let FlowReturnPendingOutcome::NoValue { failure, .. } = outcome {
                    failure
                } else {
                    match failure {
                        crate::semantic_query::ResolveCallFailure::Budget => {
                            FlowReturnFailure::Budget(
                                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                            )
                        }
                        _ => FlowReturnFailure::Unresolved,
                    }
                };
                let _ = self.fail_flow_demand(flow_demand.as_ref(), failure);
                for member in &flow_members {
                    let _ = self.fail_flow_demand(member.flow_demand.as_ref(), failure);
                }
                return FlowFramePop::RootClose(FlowRootClose::NoValue(failure));
            }
        };
        if let Some(root_outcome) = prefix_outcomes.into_iter().next() {
            outcome = root_outcome;
        }
        // Component failure detection: a no-value flow outcome anywhere in
        // the component (the root included), or an undecided / budgeted
        // relation member, fails the WHOLE tagged component — nothing
        // publishes, every flight aborts, and every installed demand
        // records the failure on its ledger. This is failure detection,
        // not a completeness authority: warm admission is the finalizer's
        // alone, decided per member below.
        let component_failed = matches!(outcome, FlowReturnPendingOutcome::NoValue { .. })
            || flow_members
                .iter()
                .any(|member| matches!(member.outcome, FlowReturnPendingOutcome::NoValue { .. }))
            || relation_members.iter().any(|member| {
                matches!(
                    member.verdict,
                    super::dispatch_txn::PendingVerdict::Unknown
                        | super::dispatch_txn::PendingVerdict::BudgetExceeded(_)
                )
            });
        if component_failed {
            self.flow_return_abort_inline_flight(inline_flight.as_ref());
            for member in &relation_members {
                self.relation_abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            self.resolve_call_abort_drained_flights(&call_results);
            let failure = match outcome {
                FlowReturnPendingOutcome::NoValue { failure, .. } => failure,
                _ => {
                    if budget_cap.is_some() {
                        FlowReturnFailure::Budget(
                            verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                        )
                    } else {
                        FlowReturnFailure::Unresolved
                    }
                }
            };
            let _ = self.fail_flow_demand(flow_demand.as_ref(), failure);
            for member in &flow_members {
                let _ = self.fail_flow_demand(member.flow_demand.as_ref(), failure);
            }
            return FlowFramePop::RootClose(FlowRootClose::NoValue(failure));
        }
        // The published component's self-roots are the UNION of every
        // drained member's roots across ALL THREE domains (the root's own
        // file, every drained flow member's file, every drained CALL
        // member's observed roots — its callee, receiver, arguments and
        // explicit type arguments, which can live in files no flow member
        // touches — and every relation member's observed node roots), plus
        // the members already completed under this root that the drain
        // publishes on the same carrier: a cross-file edit invalidates the
        // whole component.
        let mut scc_self_roots = self_roots.clone();
        for member in &flow_members {
            union_self_roots(&mut scc_self_roots, &member.self_roots);
        }
        for (_, state, _) in &call_results {
            union_self_roots(&mut scc_self_roots, &state.self_roots);
        }
        if !relation_members.is_empty() {
            let mut nodes = Vec::with_capacity(relation_members.len() * 2);
            for member in &relation_members {
                nodes.push(member.key.source);
                nodes.push(member.key.target);
            }
            union_self_roots(
                &mut scc_self_roots,
                &self.observed_self_roots_from_nodes(nodes),
            );
        }
        {
            let txn = self.dispatch_txn.borrow();
            for member in &txn.flow.completed_members {
                union_self_roots(&mut scc_self_roots, &member.self_roots);
            }
            for member in &txn.call.completed_members {
                union_self_roots(&mut scc_self_roots, &member.self_roots);
            }
            let relation_nodes = txn
                .relation
                .completed_members
                .iter()
                .flat_map(|member| [member.key.source, member.key.target]);
            union_self_roots(
                &mut scc_self_roots,
                &self.observed_self_roots_from_nodes(relation_nodes),
            );
        }
        #[cfg(any(test, feature = "test-support"))]
        self.record_flow_root_carrier_for_tests(&scc_self_roots);
        // The drained members discharge through the shared coordinator
        // (no relation root — every relation member routes to the
        // completed batch; the flow members FINALIZE there — each enters
        // the batch only with its own proof — and the call members queue
        // beside them). A refused member batch poisons the ROOT's own
        // admission below: the root's fixed point consumed the members'
        // evaluated values, so its verdict may flow but must never warm.
        let mut member_batch_unproven = false;
        if !relation_members.is_empty() || !flow_members.is_empty() || !call_results.is_empty() {
            match self.relation_discharge_and_route(
                false,
                None,
                relation_members,
                flow_members,
                None,
                call_results,
                cyclic,
                &convergence,
            ) {
                Ok(outcome) => member_batch_unproven = outcome.flow_batch_unproven,
                Err(_) => {
                    self.flow_return_abort_inline_flight(inline_flight.as_ref());
                    let _ =
                        self.fail_flow_demand(flow_demand.as_ref(), FlowReturnFailure::Unresolved);
                    return FlowFramePop::RootClose(FlowRootClose::NoValue(
                        FlowReturnFailure::Unresolved,
                    ));
                }
            }
        }
        // The root's own outcome: the machinery root publishes through
        // the family singleflight; an inline root batch-publishes with
        // the SCC drain and the caller consumes the computed step.
        match outcome {
            FlowReturnPendingOutcome::EvaluatedValue(result) => {
                // The CALLER's mapping applies exactly here, where the
                // value leaves this frame: the component's internal fixed
                // point runs in this frame's own binders, and the admitted
                // value under an instantiation key is the instantiated
                // return — one transfer point for both publish channels.
                let result = self.apply_frame_key_substitution(&root_key, result);
                // FINALIZE the root's own demand — after the component
                // fixed point, the literal widening, and the per-key
                // substitution: the proof covers exactly this value. A
                // refused member batch withholds the root's own proof
                // outright: the fixed point consumed the unproven
                // members' values, so no verdict over this result may
                // admit (`ReturnOnly` — the value still flows).
                let verdict = if member_batch_unproven {
                    None
                } else {
                    self.finalize_flow_demand(
                        flow_demand.as_ref(),
                        discharge.as_ref(),
                        &convergence,
                        provenance,
                        &result,
                    )
                };
                if machinery_root {
                    // The machinery root publishes through the family
                    // singleflight, which owns the root's own admission —
                    // so it never claims an inline flight, and this arm
                    // has none to drop.
                    verter_debug_assert!(
                        inline_flight.is_none(),
                        "a machinery root publishes through the family singleflight \
                         and must never hold an inline flight to drop"
                    );
                    FlowFramePop::RootClose(FlowRootClose::EvaluatedValue(Box::new(
                        EvaluatedFlowRoot {
                            result,
                            scc_self_roots,
                            materialized,
                            verdict,
                        },
                    )))
                } else {
                    // The VALUE channel: the inline root's own evaluated
                    // value joins the closed-member overrides the shared
                    // return equation reads (proven or not)...
                    self.dispatch_txn
                        .borrow_mut()
                        .flow
                        .closed_values
                        .push((root_key.clone(), result.clone()));
                    // An inline root enters the batched publish ONLY with
                    // its own proof; an unproven outcome aborts its flight
                    // (the usable value still flows to the caller).
                    match &verdict {
                        Some(FlowSolveOutcome::Complete(proof)) => {
                            self.dispatch_txn.borrow_mut().flow.completed_members.push(
                                CompletedFlowReturnMember {
                                    key: root_key,
                                    result: proof.clone(),
                                    inline_flight,
                                    self_roots,
                                    materialized,
                                },
                            );
                        }
                        _ => {
                            self.flow_return_abort_inline_flight(inline_flight.as_ref());
                            // The inline path produces NO memo read, so the
                            // universal read funnel never sees this refusal:
                            // fold it into the ENCLOSING build's rails here
                            // (the same funnel primitives the consumer-side
                            // hold arm uses) — an answer composed around an
                            // unproven flow value must not warm.
                            let reasons = match &verdict {
                                Some(FlowSolveOutcome::Partial(partial)) => {
                                    match partial.value.degradation() {
                                        Some(degradation) => degradation_reason_class(degradation),
                                        None => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
                                    }
                                }
                                _ => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
                            };
                            self.fold_cache_read_rails(true, true, reasons);
                        }
                    }
                    FlowFramePop::Provisional(FlowReturnStep::Complete(result))
                }
            }
            FlowReturnPendingOutcome::NoValue { .. } => {
                unreachable!("a failed root fails the component above")
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
    /// Discharge one tagged flow component to its equation fixed point —
    /// the ONE discharge every close path (flow root, relation root) runs.
    /// Every entry's admitted result is the least fixed point of
    /// `result_i = seed_i ∪ (⋃ hold targets' results)`: a Complete
    /// outcome IS the member's concrete seed join; a hold-only EmptyCycle
    /// outcome has no seed. An entry whose hold targets cannot all
    /// discharge (a target outside the component, or a component with no
    /// concrete seed) stays degraded — the whole tagged component then
    /// refuses admission.
    pub(super) fn discharge_flow_component_to_fixed_point(
        &self,
        entries: &mut [super::dispatch_txn::FlowDischargeEntry],
        call_results: &rustc_hash::FxHashMap<crate::semantic_query::ResolveCallKey, SemanticNodeId>,
    ) -> ObservedFlowConvergence {
        let index: rustc_hash::FxHashMap<&FlowReturnKey, usize> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (&entry.key, i))
            .collect();
        let mut current: Vec<Option<FlowReturnResult>> = entries
            .iter()
            .map(|entry| match &entry.outcome {
                FlowReturnPendingOutcome::EvaluatedValue(result) => Some(result.clone()),
                // A failed member has no SEED of its own. Its observed
                // degradation is NOT lost with the seed — it is read back
                // from the entry's own outcome below, so a member the
                // discharge resurrects carries it into the fixed point.
                FlowReturnPendingOutcome::NoValue { .. } => None,
            })
            .collect();
        // The runtime-observed convergence of this discharge: the passes
        // the fixed point actually ran (including the final stable pass).
        let mut iterations: u32 = 0;
        loop {
            iterations = iterations.saturating_add(1);
            let mut progressed = false;
            for i in 0..entries.len() {
                let mut arms: Vec<SemanticNodeId> = Vec::new();
                // Degradation propagates through the join: a result built
                // from a degraded contributor is itself degraded
                // (first-observed reason wins, deterministic in entry /
                // hold order).
                // Seeded from the ENTRY's own outcome, not from
                // `current[i]`: a failed member has no `current[i]` seed,
                // yet its evaluation may well have observed a degradation
                // before it failed. Reading `current[i]` here would drop
                // exactly the degradation the resurrection path needs.
                let mut degradation = entries[i].outcome.degradation();
                if let Some(result) = &current[i] {
                    arms.push(result.return_type());
                }
                let mut ready = true;
                for hold in &entries[i].holds {
                    let held = match hold.flow_key() {
                        Some(key) => match index.get(key).and_then(|j| current[*j].as_ref()) {
                            Some(result) => {
                                if degradation.is_none() {
                                    degradation = result.degradation();
                                }
                                Some(result.return_type())
                            }
                            // A target outside this component, or one that
                            // has not discharged: undecided — the entry
                            // cannot move.
                            None => None,
                        },
                        // A resolved-call target joins from the close's
                        // current call results, already in the caller's
                        // terms — the same raw join the executor's own
                        // equation performs.
                        None => call_results
                            .get(match hold {
                                HeldCallee::Call { key } => key,
                                HeldCallee::Flow { .. } => unreachable!(),
                            })
                            .copied(),
                    };
                    match held {
                        Some(return_node) => {
                            // The SAME transfer the call arm performs, so
                            // it applies the SAME rule: a flow hold target
                            // is a CALLEE, and its admitted return is
                            // expressed in the CALLEE's binders. Joining
                            // `result.return_type()` raw here re-published
                            // exactly the binder the call arm had already
                            // instantiated away — the fixed point ran the
                            // transfer a second time, around the gate. The
                            // hold's own accessor is now the only way to
                            // reach a node from a target's result.
                            arms.push(hold.discharged(self, return_node).into_node());
                        }
                        None => {
                            ready = false;
                            break;
                        }
                    }
                }
                if !ready {
                    continue;
                }
                // Flatten one union level before joining: the fixed point
                // joins SETS of leaves — splicing a union arm's members
                // keeps the join canonical. A nested `Union{U, …}`
                // wrapper is fresh content on every pass, so the
                // iteration would never converge (and intern unbounded
                // ever-deeper unions).
                let graph = self.graph();
                let mut flat: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
                for arm in arms {
                    match graph.node_data(arm).as_deref() {
                        Some(SemanticNodeData::Union(members)) => flat.extend_from_slice(members),
                        _ => flat.push(arm),
                    }
                }
                let next = FlowReturnResult::new(
                    graph,
                    self.intern_normalized_union_or_intersection(&flat, true),
                    current[i]
                        .as_ref()
                        .is_some_and(|result| result.can_fall_through),
                    degradation,
                );
                if current[i].as_ref() != Some(&next) {
                    current[i] = Some(next);
                    progressed = true;
                }
            }
            if !progressed {
                break;
            }
        }
        // Post-convergence literal widening. tsc widens a fresh literal
        // return only when the function's AGGREGATE is a single type;
        // inside a recursive component the aggregate is only known once
        // the equation converges. `f = 0 ∪ f` collapses to the single
        // literal `0` and widens to `number`; `msa = "a" ∪ msb`,
        // `msb = 1 ∪ msa` converge to two arms and both stay pinned
        // (`"a" | 1`). Every contributing entry must be a FRESH seed —
        // one non-fresh contributor anywhere in the component (a `1 as
        // const`, an annotated binding, a bare return) pins the result.
        let component_is_fresh = entries.iter().all(|entry| entry.fresh_seed);
        for (entry, discharged) in entries.iter_mut().zip(current) {
            let Some(result) = discharged else {
                continue;
            };
            // ONLY a hold-only empty cycle is resurrectable. Its "failure"
            // is an artefact of evaluation order — it genuinely has no
            // seed of its own and its value IS the join of its hold
            // targets. Every OTHER failure kind is a real no-value
            // outcome, and stamping it `Complete` from its targets'
            // results would publish a value the member's own evaluation
            // never produced.
            if !matches!(
                entry.outcome,
                FlowReturnPendingOutcome::EvaluatedValue(_)
                    | FlowReturnPendingOutcome::NoValue {
                        failure: FlowReturnFailure::EmptyCycle,
                        ..
                    }
            ) {
                continue;
            }
            // The widened value is a NEW value, so its verdict is
            // re-derived rather than copied: `with_return_type` routes
            // back through the one constructor.
            let result = if component_is_fresh {
                result
                    .with_return_type(self.graph(), widen_literal_node(self, result.return_type()))
            } else {
                result
            };
            entry.outcome = FlowReturnPendingOutcome::EvaluatedValue(result);
        }
        // The loop above exits only on a no-progress pass: the component
        // reached its fixed point.
        ObservedFlowConvergence {
            iterations,
            stable: true,
        }
    }

    // The evaluator
    // ──────────────────────────────────────────────────────────────────

    /// The ONE binder environment for one type-parameter clause: the
    /// binders intern as `TypeParam` nodes in the file scope and shadow
    /// every outer same-name resolution. Shared by the root evaluation
    /// (parameters + body leaves) and every nested function value's
    /// signature; an empty clause with no `outer` carries an empty `env`,
    /// which reproduces the owner-scope lowering exactly.
    ///
    /// The environment COMPOSES in two directions.
    ///
    /// Outward, `outer` is the environment of the ENCLOSING frame — the
    /// class clause a member sits inside, or the frame a nested function
    /// value was authored in. A binder of an enclosing clause is in scope
    /// throughout everything it encloses, and the enclosed clause carries
    /// only its own names, so without the seed an enclosing `<T>` reads
    /// as a free name and binds an unrelated owner-scope `T`. The
    /// enclosed clause overwrites a same-named outer binder, which is
    /// exactly the shadowing rule.
    ///
    /// Inward, the clause binds its OWN siblings, so it interns in TWO
    /// passes: every binder is interned bare first, then the constraints
    /// and defaults lower under that environment. One pass in source
    /// order would be wrong — TypeScript accepts a FORWARD sibling
    /// reference in a constraint (`<U extends V, V>` type-checks and
    /// still constrains through `V`), so the visible inventory is the
    /// whole clause, never "the preceding siblings".
    ///
    /// The two passes are not a fixed point: a sibling reference in a
    /// constraint sees the sibling's BARE binder, so
    /// `<U extends V, V extends string>` gives `U` a constraint on `V`
    /// without `V`'s own constraint attached. That matches how a
    /// `TypeParam`'s constraint is treated everywhere else (declaration
    /// -local meaning, never re-substituted at a call site) and is the
    /// boundary of this scheme.
    ///
    /// Whether a BINDER or a same-named frame LOCAL wins is not decided
    /// here — it is a lexical question, settled by the content half's
    /// [`crate::flow_slice_content`] gate before an answer ever reaches
    /// this environment. TS2300 constrains only one frame
    /// (`function f<T>() { class T {} }`); across frames the two
    /// genuinely coexist and the nearest wins, in both directions.
    fn flow_binder_env(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        type_parameters: &[crate::flow_slice_content::SliceTypeParam],
        outer: Option<&FlowBinderEnv>,
    ) -> FlowBinderEnv {
        let graph = self.graph();
        let whole_hash = self
            .ctx
            .shallow_file_state(canonical)
            .map(|state| state.whole_hash)
            .unwrap_or_default();
        let scope = crate::semantic_query::NodeScopeId::File {
            canonical_id: Arc::from(canonical),
            owner,
            whole_hash,
            local_scope: None,
        };
        let scope_payload = self.ctx.prepared_decl_bundle(canonical).map(|bundle| {
            crate::resolver_core::bare_name_resolve::DeclarationScopePayload::from_bundle(
                &bundle, owner,
            )
        });
        let shadowing = crate::resolver_core::scope_shadowing::ScopeShadowing::from_scope_payload(
            scope_payload.as_ref(),
        );
        let name_resolution: rustc_hash::FxHashMap<
            std::sync::Arc<str>,
            verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
        > = rustc_hash::FxHashMap::default();
        // Seed from the ENCLOSING environment, then let this clause
        // shadow it.
        let mut env: rustc_hash::FxHashMap<String, SemanticNodeId> =
            outer.map(|outer| outer.env.clone()).unwrap_or_default();
        let intern_binder = |name: &Arc<str>,
                             constraint: Option<SemanticNodeId>,
                             default: Option<SemanticNodeId>| {
            graph.intern_node(SemanticNodeData::TypeParam {
                decl: crate::semantic_query::DeclIdentity::from_scope(&scope, Arc::clone(name)),
                param_index: 0,
                constraint,
                default,
                display_name: Arc::clone(name),
            })
        };
        // PASS 1 — every binder of this clause, bare. Sibling references
        // in a constraint / default resolve against these, in either
        // direction.
        for tp in type_parameters.iter() {
            env.insert(tp.name.to_string(), intern_binder(&tp.name, None, None));
        }
        // PASS 2 — the constraints and defaults, lowered under the
        // composed environment, then the final binders.
        let mut type_param_decls: Vec<crate::semantic_query::TypeParamDecl> =
            Vec::with_capacity(type_parameters.len());
        let mut finalized: Vec<(String, SemanticNodeId)> =
            Vec::with_capacity(type_parameters.len());
        for tp in type_parameters.iter() {
            let mut lower = |gated: &crate::flow_slice_content::GatedType| {
                let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                self.shallow_lower_type_expr_with_context(
                    gated.ty(),
                    &env,
                    &scope,
                    &name_resolution,
                    scope_payload.as_ref(),
                    &shadowing,
                    &mut substitutions,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                )
            };
            let constraint = tp.constraint.as_ref().map(&mut lower);
            let default = tp.default.as_ref().map(&mut lower);
            let display_name: Arc<str> = Arc::clone(&tp.name);
            let binder = intern_binder(&display_name, constraint, default);
            finalized.push((tp.name.to_string(), binder));
            type_param_decls.push(crate::semantic_query::TypeParamDecl {
                name: display_name,
                param: binder,
                constraint,
                default,
                // The slice-content clause does not model `<const T>`;
                // const-ness reaches declarations through the prepared
                // fact path, never through a body slice.
                is_const: false,
            });
        }
        env.extend(finalized);
        FlowBinderEnv {
            scope,
            scope_payload,
            shadowing,
            name_resolution,
            env,
            type_param_decls,
        }
    }

    /// The shared demand-site derivation of one flow demand: the demand
    /// gate, the served function entry, the content-pinned slice keys, the
    /// demanded member, and the frame's binding inventory. Derived ONCE
    /// here for both the demand preparation
    /// ([`Self::prepare_flow_return_demand`] — the proof planning) and the
    /// content evaluation ([`Self::evaluate_flow_return`]), so the proof
    /// layer and the evaluator can never disagree about which slice a
    /// demand addresses. The `Err` arm carries the typed failure the
    /// evaluation reports plus the self-roots observed so far.
    fn flow_slice_demand_site(
        &self,
        key: &FlowReturnKey,
    ) -> Result<
        FlowSliceDemandSite,
        (
            FlowReturnFailure,
            Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        ),
    > {
        // The evaluation models the whole-return point and the
        // single-named-member projection point, both at the empty input
        // point. Any other demand/input point fails CLOSED with a typed
        // no-value outcome — never a silently widened whole-return result,
        // never a sibling materialisation the narrower demand did not ask
        // for.
        if !key.input.is_empty() {
            return Err((FlowReturnFailure::UnmodeledDemandPoint, Vec::new()));
        }
        let demanded_member: Option<Arc<str>> = if key.demand.is_whole_return() {
            None
        } else {
            match flow_demanded_member_name(&key.demand) {
                Some(name) => Some(name),
                None => {
                    return Err((FlowReturnFailure::UnmodeledDemandPoint, Vec::new()));
                }
            }
        };
        let canonical = key.function.declaration_slot.defining_canonical.as_ref();
        let owner = key.function.declaration_slot.owner;
        let name = key.function.declaration_slot.merged_symbol_name.as_ref();
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            return Err((FlowReturnFailure::Missing, Vec::new()));
        };
        let indexed = serve.indexed;
        let self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot> =
            vec![(Arc::from(canonical), indexed.whole_hash)];
        let index = indexed.shallow_state.decl_bodies().function_program_index();
        // The KEYED lookup, not a scan: a function position is named by
        // its whole program key, and "the first entry that looks close
        // enough" is exactly the shape that hands over the wrong callee.
        let Some(entry) = index
            .value_function(
                owner,
                name,
                &key.function.function_part,
                key.function.overload_ordinal,
            )
            .map(|matched| matched.entry())
        else {
            return Err((FlowReturnFailure::Missing, self_roots));
        };
        // A body whose own bytes could not be read has no exact-content
        // axis, so no content-addressed key can be built for it: fail
        // closed rather than key on a constant every unreadable body
        // shares.
        let Some(flow_body_exact_hash) = entry.flow_body_exact_hash else {
            return Err((FlowReturnFailure::Unresolved, self_roots));
        };
        // The source axes come from the request-bound artifact identity
        // (the served `IndexedReady` through the canonical
        // `FileArtifactKey` identity) — never path reclassification, never
        // a constant. A serving artifact whose exact parse identity cannot
        // be recomputed fails closed: no content-addressed key may name a
        // source it cannot verify.
        let Some(source_key) = crate::file_artifact_store::FileArtifactKey::for_source_identity(
            Arc::from(canonical),
            indexed.whole_hash,
            indexed.raw_source.as_ref(),
            indexed.file_language.clone(),
            indexed.framework_parse.as_deref(),
            indexed.parse_env_hash,
        ) else {
            return Err((FlowReturnFailure::Unresolved, self_roots));
        };
        let slice_key_function = crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey {
            canonical_id: Arc::from(canonical),
            function: entry.key.clone(),
            flow_body_stable_hash: entry.flow_body_stable_hash,
            flow_body_exact_hash,
            parse_env_hash: key.context.parse_env_hash,
            parse_key: source_key.parse_key,
            file_language: source_key.file_language_id,
            build_toolchain_fingerprint:
                crate::build_toolchain_fingerprint::current_build_toolchain_fingerprint(),
        };
        let slice_key = crate::cache_runtime::flow_slice_node::FlowSliceHashKey {
            function: slice_key_function.clone(),
            demand: crate::cache_runtime::flow_slice_node::FlowSliceDemandIdentity {
                projection_path: match demanded_member.as_ref() {
                    Some(member) => Arc::from(vec![Arc::clone(member)].into_boxed_slice()),
                    None => Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
                },
            },
        };
        Ok(FlowSliceDemandSite {
            indexed,
            self_roots,
            slice_key_function,
            slice_key,
            demanded_member,
            inventory: super::flow_solve::FlowBindingInventory {
                bindings: Arc::clone(&entry.bindings),
            },
        })
    }

    /// Evaluate one demanded function through its flow IR. Reads the
    /// whole-body identity from the per-file `FunctionProgramIndex`
    /// (recording the `ProgramAnalysisFactRef::FlowBody` fact rail),
    /// plans + hashes the demand slice through the project-global
    /// flow-slice nodes (the budget outcome gates admission — an
    /// over-budget plan is a typed `Budget` failure, `ReturnOnly` at the
    /// memo), and evaluates the body, joining the return-site
    /// contributors with return widening and the fallthrough seed. The
    /// returned [`MaterializedSet`] is the point set this compute
    /// ACTUALLY produced (§3.4) — recorded here, at the one place the
    /// compute knows what it served.
    ///
    /// The demand site is derived ONCE ([`Self::flow_slice_demand_site`]),
    /// shared with the demand preparation, so the proof layer and the
    /// evaluator can never disagree about which slice a demand addresses.
    /// On an evaluated value the outcome carries the typed discharge
    /// report of the frame's installed demand.
    ///
    /// [`MaterializedSet`]: crate::semantic_query::demand::MaterializedSet
    fn evaluate_flow_return(&self, key: &FlowReturnKey) -> FlowEvaluationOutcome {
        verter_audit::attribute_scope!(FlowSliceCompute);
        use crate::semantic_query::demand::{MaterializedPoint, MaterializedSet};
        // The frame's installed demand carrier (installed by
        // `prepare_flow_return_demand` at frame open): the report at the
        // end of this evaluation claims against exactly this demand.
        let demand_carrier = self.flow_demand_carrier_of(key);
        // The evaluation provenance is bound HERE, at the evaluation's
        // start: the installed demand's OWN token when one is installed —
        // so the value, the discharge report, and the convergence evidence
        // of THIS evaluation carry exactly THIS demand's identity — and
        // otherwise the bare freshness mint (sentinel ordinal), which can
        // never match a real carrier at finalization.
        let provenance = demand_carrier
            .as_ref()
            .map(|carrier| carrier.provenance)
            .unwrap_or_else(|| self.current_flow_evaluation_provenance());
        // Every call site of this closure fails BEFORE the evaluator
        // runs, so no degradation has been observed yet: `None` is the
        // honest value, not a dropped one.
        let degraded =
            |failure: FlowReturnFailure,
             self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>| {
                FlowEvaluationOutcome {
                    outcome: FlowReturnPendingOutcome::NoValue {
                        failure,
                        degradation: None,
                    },
                    self_roots,
                    holds: Vec::new(),
                    materialized: MaterializedSet::empty(),
                    fresh_seed: false,
                    discharge: None,
                    provenance,
                }
            };
        // Cold-path start event: every cold whole-function evaluation
        // (root and nested inline frames) passes through here; the warm
        // family hit in `execute_flow_return` returns before any frame
        // opens, so a warm hit can never reach this emission.
        crate::flow_return_audit::record_flow_return_started(
            &key.function.declaration_slot.defining_canonical,
            &key.function.declaration_slot.merged_symbol_name,
        );
        // The evaluation models the whole-return point and the
        // single-named-member projection point (the `ReturnType<typeof
        // f>['b']` demand rail), both at the empty input point. Any
        // other demand/input point fails CLOSED with a typed no-value
        // outcome — never a silently widened whole-return result, never
        // a sibling materialisation the narrower demand did not ask for.
        // (The gate lives in the shared demand-site derivation.)
        let site = match self.flow_slice_demand_site(key) {
            Ok(site) => site,
            Err((failure, self_roots)) => return degraded(failure, self_roots),
        };
        let FlowSliceDemandSite {
            indexed,
            self_roots,
            slice_key_function,
            slice_key,
            demanded_member,
            inventory: _,
        } = site;
        let canonical = key.function.declaration_slot.defining_canonical.as_ref();
        let owner = key.function.declaration_slot.owner;
        let name = key.function.declaration_slot.merged_symbol_name.as_ref();
        let index = indexed.shallow_state.decl_bodies().function_program_index();
        // The KEYED lookup, not a scan: a function position is named by
        // its whole program key, and "the first entry that looks close
        // enough" is exactly the shape that hands over the wrong callee.
        // The demand site already proved this entry over this pinned
        // serve; the re-lookup is deterministic.
        let Some(entry) = index
            .value_function(
                owner,
                name,
                &key.function.function_part,
                key.function.overload_ordinal,
            )
            .map(|matched| matched.entry())
        else {
            return degraded(FlowReturnFailure::Missing, self_roots);
        };
        // The whole-body fact rail: the candidate roots on the indexed
        // whole-body hash (never re-lowered at validation).
        crate::resolver_core::resolver_context::observe_fan_out(FactVersionRef::ProgramAnalysis(
            ProgramAnalysisFactRef::FlowBody {
                function: key.function.program_analysis_ref(),
                flow_body_stable_hash: entry.flow_body_stable_hash,
            },
        ));
        // The demand-slice substrate: plan the demanded slice as graph
        // reachability over the once-per-content-version
        // `FunctionFlowGraph` and hash exactly the selection, through the
        // project-global content-addressed hash node. The whole-return
        // demand maps to the empty projection path. The outcome gates
        // admission: an over-budget plan is a typed `Budget` failure the
        // memo refuses (`ReturnOnly` — the fourth non-admission layer,
        // on top of the planner's typed refusal, the hash node's
        // `ReturnOnly`, and the unaddressable lowered store).
        let flow_slice = self.ctx.project_type_store().flow_slice();
        let (planned, lowered) =
            match crate::cache_runtime::lookup(flow_slice.hash_node(), slice_key.clone(), self.ctx)
            {
                None => {
                    // The skeleton source could not serve the pinned content
                    // version (a torn view between the served index and the
                    // retained snapshot): undecided, never a fabricated slice.
                    return degraded(FlowReturnFailure::Unresolved, self_roots);
                }
                Some(
                    crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::BudgetExceeded(
                        exceeded,
                    ),
                ) => {
                    tracing::debug!(
                        axis = ?exceeded.axis,
                        limit = exceeded.limit,
                        observed = exceeded.observed,
                        "flow-slice budget exceeded: typed Budget failure, ReturnOnly"
                    );
                    crate::flow_return_audit::record_flow_slice_budget_exceeded(&exceeded);
                    return degraded(
                        FlowReturnFailure::Budget(
                            verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                        ),
                        self_roots,
                    );
                }
                Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::Planned(
                    planned,
                )) => {
                    // Hash-then-lower: the minted slice identity keys the
                    // lowered-slice artifact (the key is unconstructible
                    // without it), and the lowered node lowers ONLY the
                    // retained plan. A lowered miss on the pinned content is
                    // a torn view — undecided, never a fabricated slice.
                    let lowered_key = crate::cache_runtime::flow_slice_node::FlowSliceLoweredKey {
                        hash_key: slice_key,
                        slice_hash: planned.hash(),
                    };
                    match crate::cache_runtime::lookup(
                        flow_slice.lowered_node(),
                        lowered_key,
                        self.ctx,
                    ) {
                        None => {
                            return degraded(FlowReturnFailure::Unresolved, self_roots);
                        }
                        Some(lowered) => (planned, lowered),
                    }
                }
            };
        // The member-projection demand evaluates ONLY the demanded member
        // of a structural object return; the slice's VALUE selection
        // below already keeps every unselected sibling cold.
        let member_filter: Option<MemberDemandFilter> =
            demanded_member.as_ref().map(|member| MemberDemandFilter {
                member: Arc::clone(member),
            });
        // The demand selection IS the lowered slice: only slice-selected
        // expression content and value-selected slots lower — an
        // unselected binding initializer, sibling member value, or
        // effect-position expression never lowers (no resolution, no
        // budget charge, no fact).
        let selection = crate::flow_slice_content::FlowSliceSelection::from_slice_ir(&lowered);
        // The content lowering resolves every identifier against the SAME
        // `FunctionBodySkeleton` the plan above resolved its lexical edges
        // against — one binding authority, one build per content version
        // (the graph store memoized it during planning).
        let Some(skeleton) = flow_slice.skeleton_for(&slice_key_function, self.ctx) else {
            return degraded(FlowReturnFailure::Unresolved, self_roots);
        };
        let Some(ir) = indexed.shallow_state.decl_bodies().flow_slice_content(
            entry,
            selection,
            Arc::clone(&skeleton),
        ) else {
            return degraded(FlowReturnFailure::Missing, self_roots);
        };
        // The frame anchor the skeleton's call footprint was rebased onto
        // (the function node's own start) — the witness pairs the
        // evaluator's absolute call spans with the skeleton's
        // frame-relative twins through it.
        let frame_anchor = entry.span.start;
        // A budget edge in one SELECTED leaf's expression lowering stops
        // the whole evaluation with the typed reason.
        if let Some(reason) = ir.budget_failure {
            return degraded(FlowReturnFailure::Budget(reason), self_roots);
        }
        // A member projection over a fall-through body would need the
        // `undefined` arm folded into the member access (a tsc error
        // shape) — beyond the modeled member point: fail closed.
        if member_filter.is_some() && ir.can_fall_through {
            return degraded(FlowReturnFailure::UnmodeledDemandPoint, self_roots);
        }
        // The writes the content half lowered as applicable assignments —
        // the scan below subtracts exactly these from the unapplied-write
        // degradation, by the span identity both halves inherit from the
        // skeleton.
        let applied_write_spans = {
            let mut spans = rustc_hash::FxHashSet::default();
            collect_assignment_spans(&ir.body, &mut spans);
            spans.extend(ir.inert_write_spans.iter().copied());
            spans
        };
        // Unapplied write effects fail CLOSED as a degraded success. The
        // slice contract (`FlowSliceIR.effects`) says the solver applies
        // write retypes / widenings before evaluating the value providers
        // that read the affected slots — that application is future
        // NARROW_SUBSTITUTION / VALUE_INFERENCE work on this same graph,
        // and THIS evaluator does not perform it (locals rebuild from
        // `Binding` statements only; parameters never update). A
        // whole-slot write targeting a parameter or a value-selected slot
        // can therefore change the demanded value's type (assignment
        // narrowing; object members evaluate left-to-right), so
        // evaluating past it may produce a WRONG type. Seed the typed
        // `UnappliedWriteEffect` degradation: the evaluation still
        // returns its usable value, but the result is a DEGRADED SUCCESS
        // — `ReturnOnly`, never warm-admitted. A projection-path write
        // (`x.a = v`) never retypes the binding itself and stays clean;
        // a write whose target slot is neither a parameter nor
        // value-selected cannot be observed by the demanded value.
        //
        // A whole-binding `=` write at STATEMENT position the content
        // half lowered as an assignment is APPLIED by the evaluator in
        // source order — it retypes the binding exactly where the
        // degradation feared — so it is subtracted here by span identity
        // (the same span the skeleton recorded for the effect), and the
        // scan keeps only the writes nobody applies: expression-position
        // writes, compound operators, and writes whose value the slice
        // did not select. The scan runs AFTER the content build for
        // exactly that subtraction.
        let unapplied_write_effect = {
            use verter_semantic::analysis::flow::flow_ir::{FlowEffect, FlowEffectTarget};
            let retypes_slot = |slot: &verter_semantic::analysis::flow::flow_ir::FlowSlot| {
                slot.value_selected
                    || slot.kind == verter_semantic::analysis::flow::SkeletonBindingKind::Param
            };
            lowered.effects.iter().any(|effect| {
                let FlowEffect::Write {
                    target, path, span, ..
                } = effect
                else {
                    return false;
                };
                if !path.is_empty() {
                    return false;
                }
                if applied_write_spans.contains(span) {
                    return false;
                }
                match target {
                    FlowEffectTarget::Slot(id) => retypes_slot(lowered.slot(*id)),
                    // A named root outside the slot table is unselected or
                    // shadow-ambiguous: degrade only when SOME slot of that
                    // name could be retyped (the ambiguous arm), never for
                    // a free / unselected name.
                    FlowEffectTarget::Named(name) => lowered
                        .slots
                        .iter()
                        .any(|slot| slot.name == *name && retypes_slot(slot)),
                    FlowEffectTarget::Opaque => false,
                }
            })
        };
        // The ONE binder environment: the function's OWN type parameters
        // are binders in scope for the parameter and body-leaf lowering (a
        // root `<T extends string>(x: T)` keeps the binder `T`, never the
        // file-scope alias); an empty clause reproduces the owner-scope
        // lowering exactly. Parameters lower through it.
        //
        // The ENCLOSING declaration's clause seeds it: a class member's
        // signature and body see `class C<T>`'s binders, which appear in
        // no clause of the member itself.
        let enclosing_binder_env = (!ir.enclosing_type_parameters.is_empty())
            .then(|| self.flow_binder_env(canonical, owner, &ir.enclosing_type_parameters, None));
        let binder_env = self.flow_binder_env(
            canonical,
            owner,
            &ir.type_parameters,
            enclosing_binder_env.as_ref(),
        );
        // The instantiation overlay: a flow key demanded under a call's
        // FINAL ordered type-argument mapping binds this frame's OWN
        // clause BY DECLARATION ORDER — the normalized args are the
        // applicability machinery's final mapping (inference, declared
        // defaults, and constraints already accounted), so the frame
        // evaluates with its parameters bound exactly as the call
        // instantiated them. The canonical production key carries no
        // args and keeps every binder.
        let binder_env = if key.normalized_type_args.is_empty() {
            binder_env
        } else {
            binder_env.with_instantiation(&ir.type_parameters, &key.normalized_type_args)
        };
        // THE root-identifier gate at the SIGNATURE entrances. Every
        // signature answer the content half minted carries the frame
        // names it references; if the owner scope answers one of them,
        // evaluating it would publish an unrelated module-scope (or
        // cross-file imported) symbol's type for a frame-owned binding —
        // cleanly and warm. The ROOT function's own signature is minted
        // ungated against the FRAME (its body-locals are not in scope
        // there), so the frame half of this gate only ever fires for a
        // nested signature reached through the same slice content; the
        // PARAMETER-LIST half fires in either arm, because a signature's
        // own parameters are not body-locals.
        //
        // The verdict is POSITIONAL, per signature slot: a shadowed
        // parameter annotation contributes the typed unresolved MARKER at
        // ITS ordinal and degrades the result, while every other parameter
        // — and the whole body that never reads the shadowed one — keeps
        // its modelled value. Collapsing the frame here discarded the
        // modelled positions for a fact about one of them.
        let mut signature_position_unmodeled = ir
            .type_parameters
            .iter()
            .flat_map(|tp| tp.constraint.iter().chain(tp.default.iter()))
            .any(|gated| signature_answer_is_frame_shadowed(self, &binder_env, gated));
        let mut params: Vec<SemanticNodeId> = Vec::with_capacity(ir.params.len());
        for param in ir.params.iter() {
            if signature_answer_is_frame_shadowed(self, &binder_env, &param.ty) {
                signature_position_unmodeled = true;
                params.push(super::flow_return_callee::unmodeled_position_marker(self));
                continue;
            }
            let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
            let node = self.shallow_lower_type_expr_with_context(
                param.ty.ty(),
                &binder_env.env,
                &binder_env.scope,
                &binder_env.name_resolution,
                binder_env.scope_payload.as_ref(),
                &binder_env.shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            );
            params.push(node);
        }
        let mut evaluator = FlowEvaluator {
            dispatch: self,
            self_slot: Some(key),
            canonical,
            owner,
            params: &params,
            param_names: &ir.params,
            binder_env: &binder_env,
            locals: rustc_hash::FxHashMap::default(),
            declared_locals: rustc_hash::FxHashMap::default(),
            var_locals: rustc_hash::FxHashMap::default(),
            var_declared_locals: rustc_hash::FxHashMap::default(),
            widening_locals: rustc_hash::FxHashSet::default(),
            var_widening_locals: rustc_hash::FxHashSet::default(),
            bare_return_seen: false,
            implicit_undefined_seen: false,
            member_filter,
            holds: Vec::new(),
            degradation: unapplied_write_effect
                .then_some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect)
                .or_else(|| {
                    signature_position_unmodeled
                        .then_some(crate::semantic_query::FlowReturnDegradation::UnmodeledPosition)
                }),
            pending_statement_gap: None,
            degraded_locals: rustc_hash::FxHashSet::default(),
            var_degraded_locals: rustc_hash::FxHashSet::default(),
            var_conditional_locals: rustc_hash::FxHashSet::default(),
            conditional_arm_nesting: 0,
            narrowings: Vec::new(),
            param_writes: rustc_hash::FxHashMap::default(),
            conditional_lexicals: rustc_hash::FxHashSet::default(),
            conditional_params: rustc_hash::FxHashSet::default(),
            inference_only_path: false,
            call_fresh_literal_returns: Vec::new(),
            break_exits: Vec::new(),
            return_edges: Vec::new(),
            throw_points: Vec::new(),
            collect_throw_points: false,
            scope_shadows: Vec::new(),
            call_evidence: Vec::new(),
            executed_walk: ExecutedSliceWalk::default(),
        };
        let holds;
        let degradation;
        let bare_return_seen;
        let implicit_undefined_seen;
        let mut call_evidence;
        let mut executed_walk;
        let (contributors, body_falls_through) = {
            evaluator.seed_hoisted_var_declarations(&ir.body);
            let (outcome, body_falls_through) = evaluator.eval_region(&ir.body);
            evaluator.promote_pending_statement_gap();
            holds = std::mem::take(&mut evaluator.holds);
            degradation = evaluator.degradation;
            bare_return_seen = evaluator.bare_return_seen;
            implicit_undefined_seen = evaluator.implicit_undefined_seen;
            call_evidence = std::mem::take(&mut evaluator.call_evidence);
            executed_walk = evaluator.executed_walk;
            (outcome, body_falls_through)
        };
        // A call the lowering DECIDED ABOVE (folded into a surviving
        // decided leaf, or sitting in a control position) never feeds the
        // demanded value its return, and no relation was consumed for it:
        // containment evidence, same ledger.
        call_evidence.extend(
            ir.decided_above_call_spans
                .iter()
                .map(|span| FlowCallEvidence {
                    span: *span,
                    relations_decided: true,
                }),
        );
        #[cfg(any(test, feature = "test-support"))]
        let call_evidence = if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .suppress_call_evidence
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            Vec::new()
        } else if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .undecided_relation_evidence
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            call_evidence
                .into_iter()
                .map(|evidence| FlowCallEvidence {
                    relations_decided: false,
                    ..evidence
                })
                .collect()
        } else {
            call_evidence
        };
        // Both failure exits carry the degradation the evaluation had
        // ALREADY observed, and both classify freshness identically: an
        // EMPTY cycle contributes NO seed of its own — it is
        // fresh-neutral, and vetoing the component's literal widening
        // from a seedless member would make the outcome depend on which
        // member was demanded first. Any other failure poisons the
        // component outright, so its bit never reaches a discharge.
        // A BUDGET edge is a FRAME condition — a resource limit over the
        // whole connected demand, never a fact about one sub-expression —
        // so it is read HERE, from the connected-demand ledger's sticky
        // trip state, rather than propagated out of a nested callee's
        // step. A callee that could not even open a frame answers its
        // CALLER at a POSITION, and the positional evaluators cannot
        // express a frame failure at all; without this read the budget
        // class would be laundered into `UnmodeledPosition` and the
        // request would attribute a resource edge as a semantic one.
        // A tripped budget also SHORTENS the walk ledger: positions past
        // the trip degraded under the resource edge, so the run must not
        // claim a completed structural walk.
        let contributors = match self.connected_demand_trip() {
            Some(_) => {
                executed_walk.aborted = true;
                Err(FlowReturnFailure::Budget(
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                ))
            }
            None => contributors,
        };
        #[cfg(any(test, feature = "test-support"))]
        if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .short_execution_ledger
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            executed_walk.aborted = true;
        }
        // The evaluation's execution witness: what THIS run actually did —
        // the walk-ledger-gated executed selection, the frame identity to
        // pair call spans through, and the per-occurrence call evidence.
        // The discharge-report producer claims obligations from it, never
        // from the plan's own expectations: a short or aborted walk
        // ledger yields NO executed selection.
        let witness = FlowExecutionWitness {
            executed_selection: executed_walk.completed_selection(planned.selection()),
            skeleton: &skeleton,
            anchor: frame_anchor,
            calls: &call_evidence,
        };
        let contributors = match contributors {
            Ok(contributors) => contributors,
            Err(failure) => {
                return FlowEvaluationOutcome {
                    outcome: FlowReturnPendingOutcome::NoValue {
                        failure,
                        degradation,
                    },
                    self_roots,
                    holds,
                    materialized: failure_materialized_set(failure, key),
                    fresh_seed: matches!(failure, FlowReturnFailure::EmptyCycle),
                    // A hold-only empty cycle DID evaluate its whole slice
                    // — the component discharge resurrects its value from
                    // the join — so its discharge report is real evidence.
                    // Every other failure truncated the evaluation: no
                    // report.
                    discharge: if matches!(failure, FlowReturnFailure::EmptyCycle) {
                        demand_carrier
                            .as_ref()
                            .map(|carrier| self.flow_evaluation_discharge_report(carrier, &witness))
                    } else {
                        None
                    },
                    provenance,
                };
            }
        };
        let (result, fresh_seed) = match self.join_flow_return_contributors(
            contributors,
            // The evaluator's own reachability refines the lowering's: a
            // switch the case tests make EXHAUSTIVE over the discriminant
            // has no no-matching-case path, which only the resolver can
            // see. The override only ever narrows downward.
            ir.can_fall_through && body_falls_through,
            bare_return_seen,
            implicit_undefined_seen,
            &holds,
            degradation,
        ) {
            Ok(joined) => joined,
            Err(failure) => {
                return FlowEvaluationOutcome {
                    outcome: FlowReturnPendingOutcome::NoValue {
                        failure,
                        degradation,
                    },
                    self_roots,
                    holds,
                    materialized: failure_materialized_set(failure, key),
                    fresh_seed: matches!(failure, FlowReturnFailure::EmptyCycle),
                    // A hold-only empty cycle DID evaluate its whole slice
                    // — the component discharge resurrects its value from
                    // the join — so its discharge report is real evidence.
                    // Every other failure truncated the evaluation: no
                    // report.
                    discharge: if matches!(failure, FlowReturnFailure::EmptyCycle) {
                        demand_carrier
                            .as_ref()
                            .map(|carrier| self.flow_evaluation_discharge_report(carrier, &witness))
                    } else {
                        None
                    },
                    provenance,
                };
            }
        };
        // §3.4: record the point this compute ACTUALLY materialised — the
        // whole-return point it just evaluated (the demand gate above
        // proves it is the only point this evaluation serves). Recorded by
        // the compute, never re-derived from the nominal key at publish.
        let materialized =
            MaterializedSet::single(MaterializedPoint::new(key.demand.point.clone()));
        // The typed discharge report of this evaluation: which planned
        // obligations of the frame's installed demand the evaluation
        // actually completed. Built ONCE here, at the successful end —
        // the sole evidence producer of the proof layer.
        let discharge = demand_carrier
            .as_ref()
            .map(|carrier| self.flow_evaluation_discharge_report(carrier, &witness));
        FlowEvaluationOutcome {
            outcome: FlowReturnPendingOutcome::EvaluatedValue(result),
            self_roots,
            holds,
            materialized,
            fresh_seed,
            discharge,
            provenance,
        }
    }

    /// The union arms of `node`, when it interned as a union — the
    /// `getAssignmentReducedType` gate (a NON-union declared type
    /// supplies its binding verbatim).
    fn union_arms_of(&self, node: SemanticNodeId) -> Option<Vec<SemanticNodeId>> {
        match self.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => Some(members.to_vec()),
            _ => None,
        }
    }

    /// Join one function's return-site contributors with the fallthrough
    /// seed: a fall-through body adds `undefined` (or `void` when it has no
    /// return at all); a body that terminates with NO contribution and no
    /// hold (a throw-only body) is `never`; a HOLD-only body with no
    /// fallthrough is the empty recursive cycle — a typed failure, never
    /// `never`.
    fn join_flow_return_contributors(
        &self,
        contributors: Vec<FlowContribution>,
        can_fall_through: bool,
        bare_return_seen: bool,
        implicit_undefined_seen: bool,
        holds: &[HeldCallee],
        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    ) -> Result<(FlowReturnResult, bool), FlowReturnFailure> {
        let graph = self.graph();
        // Literal widening is a SINGLE-contributor rule (tsc aggregates
        // the return-expression types with `pushIfUnique`, then widens
        // only when the aggregate is one type): `return 1` is `number`,
        // `if (c) return 1; return 1` deduplicates to one and is
        // `number`, but `if (c) return 1; return 0` is `0 | 1` and
        // `if (c) return 1;` is `1 | undefined`. Deduplicate FIRST — the
        // graph interns identical literals to one node id — then widen a
        // lone FRESH literal.
        let mut arms: Vec<SemanticNodeId> = Vec::with_capacity(contributors.len());
        let mut inference_only: Vec<bool> = Vec::with_capacity(contributors.len());
        let mut all_fresh = true;
        for contribution in contributors {
            // Fold freshness over EVERY contributor, including the ones
            // deduplication drops. `1` and `1 as const` intern to the SAME
            // node — that is precisely why the second dedupes — but only
            // the first is FRESH. Folding after the `continue` would make
            // the aggregate's freshness depend on which contributor
            // happened to come first and publish `number` for
            // `if (c) return 1; return 1 as const` while publishing `1`
            // for its reverse (tsgo 7.0.0-dev.20260526.1: `1` for both).
            //
            // Freshness deliberately does NOT enter the dedup identity:
            // these two arms ARE the same type, and separating them would
            // emit `1 | 1`.
            all_fresh &= contribution.fresh_literal;
            if let Some(index) = arms.iter().position(|node| *node == contribution.node) {
                inference_only[index] &= contribution.inference_only;
                continue;
            }
            arms.push(contribution.node);
            inference_only.push(contribution.inference_only);
        }
        // A recursive HOLD counts as a contributor: the SCC close joins
        // its discharged return into this result, so the join is not a
        // lone contributor. Excluding holds would make widening depend on
        // whether the callee happened to be in flight — i.e. on demand
        // ORDER — and publish two different values for the same key
        // (`msa` / `msb` in a mutual cycle).
        // A FRESH seed is one whose every contributor is a fresh literal
        // and which joins no bare-return / fallthrough arm. When the
        // evaluation carries HOLDS the widening decision is deferred: the
        // component's aggregate is only known once the equation fixed
        // point converges (`f = 0 ∪ f` collapses to the single literal
        // `0` and widens; `msa = "a" ∪ msb`, `msb = 1 ∪ msa` converge to
        // two arms and stay pinned). Deferring is also what makes the
        // decision demand-ORDER-independent — the fixed point is
        // computed once per component, not per entry order.
        let fresh_seed =
            all_fresh && !bare_return_seen && !implicit_undefined_seen && !can_fall_through;
        if fresh_seed && arms.len() == 1 && holds.is_empty() {
            arms[0] = widen_literal_node(self, arms[0]);
        }
        // Bare-return-as-void (BL12): a body whose only return
        // contributions are bare `return;` statements models as `void`
        // regardless of fallthrough — tsc's rule for expressionless
        // returns (a bare-only body is also the concrete `void` seed of
        // a recursive component). Alongside VALUE returns, a bare
        // return contributes `undefined` (`if (c) return 1; return;`
        // is `1 | undefined`).
        if bare_return_seen {
            if arms.is_empty() {
                let return_type =
                    graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void));
                return Ok((
                    FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
                    false,
                ));
            }
            arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)));
            inference_only.push(false);
        }
        if implicit_undefined_seen {
            arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)));
            inference_only.push(false);
        }
        if can_fall_through {
            if arms.is_empty() {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)));
                inference_only.push(false);
            } else {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)));
                inference_only.push(false);
            }
        } else if arms.is_empty() {
            if holds.is_empty() {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)));
                inference_only.push(false);
            } else {
                return Err(FlowReturnFailure::EmptyCycle);
            }
        }
        // A suffix reached only through the checker's return-inference view
        // of an overridden break is not a second runtime path. Drop that
        // synthetic contributor when an ordinary authored return already
        // covers it; keep incomparable suffixes, which are precisely why the
        // inference-only edge exists. Ordinary return contributors retain
        // their established graph shape — this is not generic union
        // dominance or assignment-time constituent selection.
        if inference_only.iter().any(|flag| *flag) {
            arms = arms
                .iter()
                .enumerate()
                .filter_map(|(index, candidate)| {
                    let covered = inference_only[index]
                        && arms.iter().enumerate().any(|(other_index, other)| {
                            !inference_only[other_index]
                                && matches!(
                                    self.execute_relate_pair(*candidate, *other),
                                    super::dispatch_txn::RelationStep::Assignable { .. }
                                )
                                && matches!(
                                    self.execute_relate_pair(*other, *candidate),
                                    super::dispatch_txn::RelationStep::NotAssignable
                                )
                        });
                    (!covered).then_some(*candidate)
                })
                .collect();
        }
        let return_type = self.intern_normalized_union_or_intersection(&arms, true);
        Ok((
            FlowReturnResult::new(graph, return_type, can_fall_through, degradation),
            fresh_seed,
        ))
    }
}

/// The single-named-member projection filter of one flow evaluation —
/// the demand-sliced `ReturnType<typeof f>['b']` point. Carries the
/// demanded member name; the slice's own value selection already keeps
/// unselected bindings and sibling member values out of the lowered
/// content.
struct MemberDemandFilter {
    /// The demanded member name.
    member: Arc<str>,
}

/// The single supported narrow projection point: a one-segment path of a
/// statically-named member (`['b']`). Returns the member name, or `None`
/// for any other non-whole-return point (fail closed at the caller).
/// Collect the spans of every assignment statement in a region tree —
/// the writes the evaluator APPLIES, which the unapplied-write
/// degradation subtracts by span identity.
fn collect_assignment_spans(
    region: &crate::flow_slice_content::SliceRegion,
    out: &mut rustc_hash::FxHashSet<verter_semantic::analysis::flow::FrameSpan>,
) {
    for statement in region.statements.iter() {
        match statement {
            crate::flow_slice_content::SliceStatement::Assignment { span, .. } => {
                out.insert(*span);
            }
            crate::flow_slice_content::SliceStatement::If {
                consequent,
                alternate,
                ..
            } => {
                collect_assignment_spans(consequent, out);
                if let Some(alternate) = alternate {
                    collect_assignment_spans(alternate, out);
                }
            }
            crate::flow_slice_content::SliceStatement::Block(block) => {
                collect_assignment_spans(block, out);
            }
            crate::flow_slice_content::SliceStatement::Switch { cases, .. } => {
                for case in cases.iter() {
                    collect_assignment_spans(&case.region, out);
                }
            }
            crate::flow_slice_content::SliceStatement::Try {
                block,
                catch,
                finally,
                ..
            } => {
                collect_assignment_spans(block, out);
                if let Some(catch) = catch {
                    collect_assignment_spans(&catch.region, out);
                }
                if let Some(finally) = finally {
                    collect_assignment_spans(finally, out);
                }
            }
            crate::flow_slice_content::SliceStatement::Labeled { body, .. } => {
                collect_assignment_spans(body, out);
            }
            crate::flow_slice_content::SliceStatement::Return { .. }
            | crate::flow_slice_content::SliceStatement::Gap(_)
            | crate::flow_slice_content::SliceStatement::Binding { .. }
            | crate::flow_slice_content::SliceStatement::Assertion { .. }
            | crate::flow_slice_content::SliceStatement::Break { .. }
            | crate::flow_slice_content::SliceStatement::Throw
            | crate::flow_slice_content::SliceStatement::ThrowPoint
            | crate::flow_slice_content::SliceStatement::TransparentLoop
            | crate::flow_slice_content::SliceStatement::Unsupported(_) => {}
        }
    }
}

fn flow_demanded_member_name(
    demand: &crate::semantic_query::ReturnProjectionDemand,
) -> Option<Arc<str>> {
    let path = demand.point.projection.path.as_slice();
    let [segment] = path else {
        return None;
    };
    match segment {
        crate::semantic_query::PathSegment::Member(key) => key.as_string().map(Arc::<str>::from),
        crate::semantic_query::PathSegment::Index(crate::semantic_query::IndexKey::String(
            value,
        )) => Some(Arc::clone(value)),
        crate::semantic_query::PathSegment::Index(_) => None,
    }
}

/// Widen one FRESH literal node to its primitive (tsc's
/// widening-literal-type rule). Every non-literal node passes through
/// unchanged.
fn widen_literal_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    let graph = dispatch.graph();
    let widened = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(literal)) => match literal {
            crate::semantic_query::LiteralValue::String(_) => PrimitiveKind::String,
            crate::semantic_query::LiteralValue::Number(_) => PrimitiveKind::Number,
            crate::semantic_query::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
            crate::semantic_query::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
        },
        _ => return node,
    };
    graph.intern_node(SemanticNodeData::Primitive(widened))
}

/// The `TypeExpr` a guard literal denotes — the ONE literal lowering the
/// `===` guard narrow and the switch case-test narrow share, so the two
/// spellings of `x === "a"` can never disagree about the literal's type.
/// `None` when the numeric spelling does not parse (`1_000` separators).
fn guard_literal_type_expr(
    literal: &crate::flow_slice_content::SliceGuardLiteral,
) -> Option<verter_type_expr::TypeExpr> {
    use crate::flow_slice_content::SliceGuardLiteral;
    Some(match literal {
        SliceGuardLiteral::String(value) => {
            verter_type_expr::TypeExpr::string_literal(value.as_ref())
        }
        SliceGuardLiteral::Number(text) => {
            verter_type_expr::TypeExpr::number_literal(text.parse::<f64>().ok()?)
        }
        SliceGuardLiteral::Boolean(value) => verter_type_expr::TypeExpr::boolean_literal(*value),
        SliceGuardLiteral::Null => {
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Null)
        }
        SliceGuardLiteral::Undefined => {
            verter_type_expr::TypeExpr::Primitive(verter_type_expr::PrimitiveName::Undefined)
        }
    })
}

/// The binder environment for one function's OWN type parameters — see
/// [`ProjectSemanticDispatch::flow_binder_env`]. Carried by the evaluator
/// so parameter and body-leaf lowering resolve the function's binders
/// instead of any outer same-name declaration.
impl FlowBinderEnv {
    /// This frame's clause instantiated by declaration order: every
    /// binder name maps to the call's final argument at its ordinal.
    /// An ordinal the mapping does not cover keeps its binder — the
    /// applicability machinery's mapping is total for the calls that
    /// mint it, and a partial one must not invent a binding.
    fn with_instantiation(
        mut self,
        type_parameters: &[crate::flow_slice_content::SliceTypeParam],
        normalized_type_args: &[SemanticNodeId],
    ) -> Self {
        for (ordinal, param) in type_parameters.iter().enumerate() {
            let Some(arg) = normalized_type_args.get(ordinal) else {
                continue;
            };
            self.env.insert(param.name.to_string(), *arg);
        }
        self
    }
}

struct FlowBinderEnv {
    /// The file scope the binders declare into.
    scope: crate::semantic_query::NodeScopeId,
    /// The file's declaration scope payload (bare-name resolution).
    scope_payload: Option<crate::resolver_core::bare_name_resolve::DeclarationScopePayload>,
    /// The scope's shadow set (derived from the payload).
    shadowing: crate::resolver_core::scope_shadowing::ScopeShadowing,
    /// The (empty) explicit name-resolution overlay.
    name_resolution: rustc_hash::FxHashMap<
        Arc<str>,
        verter_semantic::analysis::type_solver::host::ResolvedRootIdentity,
    >,
    /// Binder name → interned `TypeParam` node.
    env: rustc_hash::FxHashMap<String, SemanticNodeId>,
    /// The binder declarations, for a composed signature's generic clause.
    type_param_decls: Vec<crate::semantic_query::TypeParamDecl>,
}

/// Whether the file OWNER SCOPE answers `name` in the name meaning it was
/// referenced in — THE root-identifier gate's owner-side probe, shared by
/// every consumer of a
/// [`crate::flow_slice_content::GatedType`]'s shadow list.
///
/// Asked through the ONE shared lowering the answer itself would take
/// (`typeof name` for a value reference, a bare `name` reference for a
/// type or namespace one), so the verdict is exactly "would the answer
/// bind something here". A typed MISS means the owner scope answers
/// nothing, so nothing can be mis-bound.
///
/// The two type-space meanings share one probe by construction: the HEAD
/// of `N.B` is the same scope lookup as a bare `N`, and it is the FRAME
/// side — which local declarations shadow the reference — that the
/// meanings differ on.
fn owner_scope_answers_frame_name(
    dispatch: &ProjectSemanticDispatch<'_>,
    binder_env: &FlowBinderEnv,
    name: &crate::flow_slice_content::FrameShadowedName,
) -> bool {
    let probe = match name {
        crate::flow_slice_content::FrameShadowedName::Value(name) => {
            verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
                path: vec![name.as_ref().to_string()],
                type_args: Vec::new(),
            })
        }
        crate::flow_slice_content::FrameShadowedName::Type(name)
        | crate::flow_slice_content::FrameShadowedName::Namespace(name) => {
            verter_type_expr::TypeExpr::Ref {
                name: Arc::clone(name),
                type_arguments: Arc::from(Vec::new().into_boxed_slice()),
            }
        }
    };
    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
    let node = dispatch.shallow_lower_type_expr_with_context(
        &probe,
        &binder_env.env,
        &binder_env.scope,
        &binder_env.name_resolution,
        binder_env.scope_payload.as_ref(),
        &binder_env.shadowing,
        &mut substitutions,
        crate::semantic_query::ProjectionReductionContext::structural_transit(),
    );
    !matches!(
        dispatch.graph().node_data(node).as_deref(),
        Some(SemanticNodeData::Opaque(_))
    )
}

/// Whether ANY frame-owned name a signature answer references is
/// answered by the owner scope — the fail-closed test at every
/// [`crate::flow_slice_content::GatedType`] consumption point.
fn signature_answer_is_frame_shadowed(
    dispatch: &ProjectSemanticDispatch<'_>,
    binder_env: &FlowBinderEnv,
    gated: &crate::flow_slice_content::GatedType,
) -> bool {
    gated
        .shadowed()
        .iter()
        .any(|name| owner_scope_answers_frame_name(dispatch, binder_env, name))
}

/// One branch-local narrowing verdict. `Impossible` is distinct from
/// `Unchanged`: a conjunction whose later fact removes its last survivor is a
/// dead alternative, not an alternative that contributes the earlier overlay
/// to an enclosing disjunction.
enum GuardNarrowing {
    Unchanged,
    Narrowed(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    ),
    Impossible,
}

/// One union arm's verdict against a runtime guard test. `NoMatch` is
/// PROVED non-inhabitance of the tested edge — never "unrecognized". An
/// arm the graph cannot classify (`any`, `unknown`, a memberless `{}`
/// surface, an unresolved carrier, an undecided relation) is
/// `Unclassified`: the checker still narrows such an arm, so it stays
/// possible on BOTH edges of the test and the narrow records the typed
/// guard gap instead of silently deciding either reading. This is what
/// keeps [`GuardNarrowing::Impossible`] a positive proof: a branch goes
/// dead only when every arm is proved off its edge.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArmGuardClass {
    Match,
    NoMatch,
    Unclassified,
}

/// One union arm's key-presence verdict for a `"key" in subject` test.
/// The `in` guard needs one more state than [`ArmGuardClass`] because an
/// OPTIONAL member decides the two edges differently: the arm provably
/// stays on the NEGATED edge exactly as declared (a value of the arm's
/// type may lack the key), while on the POSITIVE edge retention is a
/// superset of the checker's key-present refinement and must carry the
/// typed guard gap. `Always`/`Never` are per-edge PROOFS (a required
/// member / a proven-absent key on a closed surface); `Unknown` proves
/// nothing and keeps the arm on both edges with the gap recorded.
#[derive(Clone, Copy, PartialEq, Eq)]
enum InArmPresence {
    Always,
    Never,
    Optional,
    Unknown,
}

/// Whether the evaluator genuinely CONSUMED a type-predicate fact, and
/// whether every relation outcome that consumption asked was decided —
/// the verdict the predicate call's guard-application evidence records
/// from. `NotConsumed` (a frame-shadowed target, an unmodelled subject)
/// deposits no evidence at all; `Undecided` deposits the call span with
/// its relation obligation left unclaimed.
enum PredicateNarrowConsumption {
    NotConsumed,
    Decided,
    Undecided,
}

#[derive(Default)]
struct NodeDisjointness {
    provably_disjoint: bool,
    nominal_identity_missing: bool,
}

fn slice_expr_is_exact_subject_read(
    expr: &crate::flow_slice_content::SliceExpr,
    subject: &crate::flow_slice_content::SliceNarrowSubject,
) -> bool {
    if !subject.path.is_empty() {
        return false;
    }
    match (expr, &subject.root) {
        (
            crate::flow_slice_content::SliceExpr::Param { ordinal },
            crate::flow_slice_content::SliceNarrowRoot::Param(subject_ordinal),
        ) => ordinal == subject_ordinal,
        (
            crate::flow_slice_content::SliceExpr::Local { name, .. },
            crate::flow_slice_content::SliceNarrowRoot::Local(subject_name),
        ) => name == subject_name,
        _ => false,
    }
}

fn guard_has_subject_matching(
    guard: &crate::flow_slice_content::SliceGuard,
    predicate: &impl Fn(&crate::flow_slice_content::SliceNarrowSubject) -> bool,
) -> bool {
    use crate::flow_slice_content::SliceGuard;
    match guard {
        SliceGuard::None => false,
        SliceGuard::Typeof { subject, .. }
        | SliceGuard::Truthy { subject, .. }
        | SliceGuard::EqLiteral { subject, .. }
        | SliceGuard::Instanceof { subject, .. }
        | SliceGuard::TypePredicate { subject, .. } => predicate(subject),
        SliceGuard::In { subject, .. } => predicate(subject),
        SliceGuard::And(parts) | SliceGuard::Or(parts) => parts
            .iter()
            .any(|part| guard_has_subject_matching(part, predicate)),
    }
}

fn slice_expr_is_exact_guard_subject_read(
    expr: &crate::flow_slice_content::SliceExpr,
    guard: &crate::flow_slice_content::SliceGuard,
) -> bool {
    guard_has_subject_matching(guard, &|subject| {
        slice_expr_is_exact_subject_read(expr, subject)
    })
}

fn slice_region_has_non_subject_return(
    region: &crate::flow_slice_content::SliceRegion,
    is_exact_subject_read: &impl Fn(&crate::flow_slice_content::SliceExpr) -> bool,
) -> bool {
    slice_statements_have_non_subject_return(region.statements.iter(), is_exact_subject_read)
}

fn slice_statements_have_non_subject_return<'a>(
    statements: impl Iterator<Item = &'a crate::flow_slice_content::SliceStatement>,
    is_exact_subject_read: &impl Fn(&crate::flow_slice_content::SliceExpr) -> bool,
) -> bool {
    use crate::flow_slice_content::SliceStatement;
    statements.into_iter().any(|statement| match statement {
        SliceStatement::Return { argument, .. } => argument
            .as_ref()
            .is_none_or(|expr| !is_exact_subject_read(expr)),
        SliceStatement::If {
            consequent,
            alternate,
            ..
        } => {
            slice_region_has_non_subject_return(consequent, is_exact_subject_read)
                || alternate.as_deref().is_some_and(|alternate| {
                    slice_region_has_non_subject_return(alternate, is_exact_subject_read)
                })
        }
        SliceStatement::Block(region) => {
            slice_region_has_non_subject_return(region, is_exact_subject_read)
        }
        SliceStatement::Labeled { body, .. } => {
            slice_region_has_non_subject_return(body, is_exact_subject_read)
        }
        SliceStatement::Switch { cases, .. } => cases
            .iter()
            .any(|case| slice_region_has_non_subject_return(&case.region, is_exact_subject_read)),
        SliceStatement::Try {
            block,
            catch,
            finally,
            ..
        } => {
            slice_region_has_non_subject_return(block, is_exact_subject_read)
                || catch.as_deref().is_some_and(|catch| {
                    slice_region_has_non_subject_return(&catch.region, is_exact_subject_read)
                })
                || finally.as_deref().is_some_and(|finally| {
                    slice_region_has_non_subject_return(finally, is_exact_subject_read)
                })
        }
        SliceStatement::Gap(_)
        | SliceStatement::Assignment { .. }
        | SliceStatement::Assertion { .. }
        | SliceStatement::Break { .. }
        | SliceStatement::Throw
        | SliceStatement::ThrowPoint
        | SliceStatement::Binding { .. }
        | SliceStatement::TransparentLoop
        | SliceStatement::Unsupported(_) => false,
    })
}

/// The per-frame evaluator state.
struct FlowEvaluator<'d, 'b> {
    dispatch: &'d ProjectSemanticDispatch<'d>,
    /// The flow slot THIS frame evaluates — the identity a same-slot
    /// recursive call holds on.
    ///
    /// `None` inside a NESTED function value: a nested body has no flow
    /// slot of its own, so there is no identity for a self-call to hold,
    /// and holding the enclosing frame's key would name the wrong
    /// function. The `Option` is what makes that mistake unexpressible.
    self_slot: Option<&'b FlowReturnKey>,
    canonical: &'d str,
    owner: verter_type_expr::TopLevelOwnerId,
    params: &'b [SemanticNodeId],
    /// The frame's formal parameters in the SAME order as `params` —
    /// their names are the closure-capture key: a nested function value
    /// reads a captured enclosing parameter BY NAME (its own `params`
    /// array indexes its own signature).
    param_names: &'b [crate::flow_slice_content::SliceParam],
    /// The function's OWN binder environment (parameters + body leaves
    /// lower under it).
    binder_env: &'b FlowBinderEnv,
    /// The LEXICAL (block-scoped) local layer: `const` / `let` reaching
    /// definitions. Block / `if`-arm evaluation saves and restores this
    /// layer (and its widening / degraded membership); the
    /// function-scoped `var` layer below survives those restores.
    locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    /// Authored declared types for the lexical layer. Reaching values change
    /// at assignments; these nodes do not, and are the authority used to
    /// assignment-reduce a later write.
    declared_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    /// The FUNCTION-scoped local layer: `var`-kind reaching definitions.
    /// `var` hoists to function scope, so block / `if` restores never
    /// touch this layer; a lexical same-name binding shadows it only
    /// while its block scope is live (reads consult `locals` first).
    var_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    /// Function-scoped twin of `declared_locals`.
    var_declared_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    /// Locals bound to a WIDENING literal (`const b = 1` — unannotated,
    /// no const assertion). Reads of these widen to the literal's
    /// primitive at return-object member positions and at the return
    /// join (tsc's widening-literal-type rule); `as const` / annotated
    /// literals never enter this set and stay pinned.
    widening_locals: rustc_hash::FxHashSet<String>,
    /// The `var`-layer widening membership (same rule, function scope).
    var_widening_locals: rustc_hash::FxHashSet<String>,
    /// Whether a bare `return;` was evaluated. A body whose ONLY return
    /// contributions are bare returns models as `void` (BL12);
    /// alongside value returns a bare return contributes `undefined`.
    bare_return_seen: bool,
    /// Whether an abrupt `finally` replaced a pending break authored in its
    /// try/catch clauses. The checker retains that exit as an implicit
    /// `undefined` contributor even though the runtime edge is overridden.
    implicit_undefined_seen: bool,
    /// The member-projection demand filter, when this evaluation serves
    /// a single-named-member `ReturnProjectionDemand` (`ReturnType<typeof
    /// f>['b']`). Return sites evaluate ONLY the demanded member of a
    /// structural object return (siblings never evaluate), and bindings
    /// outside the lowered slice's value-selected slot set never
    /// evaluate. `None` = the whole-return point.
    member_filter: Option<MemberDemandFilter>,
    /// The coinductive hold targets this evaluation met (in-flight direct
    /// callees and direct self-calls) — the SCC close discharges an
    /// empty-cycle outcome on its targets' admitted returns.
    holds: Vec<HeldCallee>,
    /// The first typed degradation this evaluation observed (a
    /// modeled-`any` substitution for a value it could not model). Rides
    /// the SUCCESS carrier; a degraded result is `ReturnOnly`.
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    /// The first statement-level gap observed in source order. A statement
    /// gap is a fallback diagnosis; a concrete degradation found during the
    /// evaluation takes precedence. Expression gaps remain immediate because
    /// they identify the unmodelled value position itself.
    pending_statement_gap: Option<crate::semantic_query::FlowGap>,
    /// Names bound to `any` because their initializer FAILED with a
    /// typed flow failure. Observing such a binding is the
    /// `FailedBindingInitializer` degradation; an unobserved failed
    /// binding degrades nothing.
    degraded_locals: rustc_hash::FxHashSet<String>,
    /// The `var`-layer failed-initializer membership (same rule,
    /// function scope).
    var_degraded_locals: rustc_hash::FxHashSet<String>,
    /// The `var`-layer CONDITIONAL-definition membership: names whose
    /// surviving reaching definition was recorded inside a conditional
    /// arm and never folded by the branch join (`if`-arm writes to
    /// bindings that predate the `if` ARE folded — the join clears the
    /// flag; a binding DECLARED inside an arm keeps it). Observing a
    /// still-flagged binding fails closed: its value is one arm's, not
    /// the join of every arm (and of the never-assigned path).
    var_conditional_locals: rustc_hash::FxHashSet<String>,
    /// How many `if` arms enclose the statement being evaluated. A plain
    /// block NEVER increments it — a block executes unconditionally, so a
    /// `var` it declares has exactly one reaching definition.
    conditional_arm_nesting: u32,
    /// The narrowing overlay: a scoped stack of positional substitutions
    /// a guard establishes (`(root, path) → narrowed node`). A read of a
    /// narrowed reference resolves through the NEWEST matching entry;
    /// arm evaluation records the stack length on entry and truncates on
    /// exit, so a narrow can never leak out of the arm it was established
    /// in, and a write deletes every fact about the value it replaced.
    narrowings: Vec<(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    )>,
    /// Whole-slot writes to formal parameters, by ordinal. Parameters
    /// live in the shared `params` slice (the signature's own array), so
    /// an applied write rides this map instead — same reaching-definition
    /// rule as the local layers, separate only because the storage is.
    param_writes: rustc_hash::FxHashMap<u32, SemanticNodeId>,
    /// Lexical bindings whose surviving value is one control-flow path's,
    /// not the join of every path that reaches the read (today: written
    /// inside a `try` clause and observed in the `catch` / `finally` /
    /// post-statement state, where the throw can precede the write).
    /// Observing one records the conditional-definition degradation —
    /// the same fail-closed contract as the `var`-layer membership, which
    /// the lexical layer otherwise cannot express.
    conditional_lexicals: rustc_hash::FxHashSet<String>,
    /// The parameter-ordinal twin of `conditional_lexicals`.
    conditional_params: rustc_hash::FxHashSet<u32>,
    /// Whether the current path exists only for return inference after an
    /// abrupt `finally` replaced the pending break at runtime.
    inference_only_path: bool,
    /// Return nodes a COMPLETED call in this frame marked
    /// `fresh_literal_return` (a generic callee whose naked-binder return
    /// fixed to a fresh-preserved literal). The call executor records the
    /// provenance; the flow frame's freshness widening happens at the
    /// return join, so a `return f(…)` whose call closed fresh
    /// contributes WITH the freshness bit — a value position (an object
    /// member, a binding initializer) keeps the literal, exactly as the
    /// return equation's own flow/call domain split decides for held
    /// calls.
    call_fresh_literal_returns: Vec<SemanticNodeId>,
    /// The `break` exits captured as `break` statements evaluate, in
    /// source order: the target (anonymous = the innermost switch) and
    /// the COMPLETE layer state at the break point. The absorbing
    /// construct (the switch, the labeled statement the name targets)
    /// drains exactly its own exits and joins them as the edge past it —
    /// the state AT the break, never the end state of the region the
    /// break happens to sit in. A `break` no modelled construct absorbs
    /// is a typed jump failure at lowering, so no exit can outlive its
    /// target.
    break_exits: Vec<FlowBreakExit>,
    /// Complete states at evaluated return edges. A crossing `finally`
    /// starts from every pending return path, and lexical scope closes replay
    /// over these snapshots exactly as over break and throw edges.
    return_edges: Vec<FlowLayerState>,
    /// The throw-point snapshots collected while a `try` block (or a
    /// `catch` clause whose statement has a `finally`) evaluates: the
    /// complete state at each call / `throw`, which is where the checker
    /// enters the `catch` / `finally` from. Collected only while
    /// `collect_throw_points` is on, so a call outside a `try` pays
    /// nothing.
    throw_points: Vec<FlowLayerState>,
    /// Whether throw-point snapshots are collected. On for the try block
    /// (a following `catch` or `finally` is entered from every throw
    /// point) and for the catch clause when a `finally` follows it.
    collect_throw_points: bool,
    /// The lexical declarations nested block scopes made, in declaration
    /// order: what each name bound BEFORE its scope (or that the name was
    /// fresh there). Closing a scope restores / drops exactly these —
    /// never the writes the scope made to bindings that PREDATE it, which
    /// is the reaching-definitions rule a wholesale layer restore cannot
    /// express.
    scope_shadows: Vec<ScopeShadow>,
    /// The call occurrences this evaluation ACTUALLY evaluated to a
    /// decided value or coinductive hold, recorded at the one call sink
    /// (`eval_call`) — the evaluator-origin evidence the discharge-report
    /// producer claims call and relation obligations from. A call the
    /// evaluation never performed, or one it could only degrade, records
    /// nothing, so its obligations stay unclaimed and the demand
    /// finalizes unproven.
    call_evidence: Vec<FlowCallEvidence>,
    /// The structural walk ledger of THIS run, recorded at the walk
    /// sites (`eval_region`'s recording shell and its statement loop) —
    /// see [`ExecutedSliceWalk`]. The execution witness yields an
    /// executed selection only from a complete, unaborted ledger.
    executed_walk: ExecutedSliceWalk,
}

/// The evaluator-recorded structural execution ledger of one run: how
/// many regions of the slice content the walk entered and completed, how
/// many statements it executed, and whether any abortive exit truncated
/// it (an unsupported construct, a member-demand mismatch, a budget
/// trip). Recorded exclusively at the evaluator's own walk sites, never
/// assigned from the plan. The witness claims the plan's retained
/// selection as EXECUTED only when the ledger is complete: the content
/// the walk ran over is derived from exactly that selection (the lowered
/// artifact is content-addressed to the plan's slice hash), so a
/// complete walk over it is a walk of the selection — and a short
/// ledger claims nothing.
#[derive(Debug, Default, Clone, Copy)]
struct ExecutedSliceWalk {
    /// Region walks entered.
    regions_entered: u32,
    /// Region walks that ran to their own control-flow end.
    regions_completed: u32,
    /// Statements the walk actually executed.
    statements_executed: u32,
    /// An abortive exit truncated the walk.
    aborted: bool,
}

impl ExecutedSliceWalk {
    /// The plan's retained selection, yielded as EXECUTED only when this
    /// ledger records a complete, unaborted walk (every entered region
    /// ran to its own end, and the root region was entered at all). A
    /// short or aborted ledger yields nothing — the discharge producer
    /// then claims no structural obligation.
    fn completed_selection<'p>(
        &self,
        selection: &'p verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan,
    ) -> Option<&'p verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan> {
        (!self.aborted
            && self.regions_entered == self.regions_completed
            && self.regions_entered > 0)
            .then_some(selection)
    }

    /// Fold a NESTED function body's walk into this ledger: the nested
    /// content is part of the same slice selection, so its regions,
    /// statements, and aborts belong to the enclosing run's walk.
    fn absorb(&mut self, nested: ExecutedSliceWalk) {
        self.regions_entered = self.regions_entered.saturating_add(nested.regions_entered);
        self.regions_completed = self
            .regions_completed
            .saturating_add(nested.regions_completed);
        self.statements_executed = self
            .statements_executed
            .saturating_add(nested.statements_executed);
        self.aborted |= nested.aborted;
    }
}

/// One evaluated call occurrence's evidence: the authored call
/// expression's span (the identity the skeleton's call footprint shares)
/// and whether every relation outcome the call's resolution consumed was
/// DECIDED (`Unknown` / `BudgetExceeded` are not evidence).
#[derive(Clone, Copy)]
struct FlowCallEvidence {
    span: verter_span::Span,
    relations_decided: bool,
}

/// One evaluation run's execution witness — what the run ACTUALLY did:
/// the selection whose derived content the walk EXECUTED TO COMPLETION
/// (`None` when the evaluator's own walk ledger is short or aborted —
/// no structural obligation is then claimable), the frame identity its
/// call evidence pairs against the skeleton footprint through (`anchor`
/// is the function node's own start — the same ingress the skeleton's
/// `FrameSpan`s were rebased with), and the call-sink evidence ledger.
/// The discharge-report producer consumes it once; the plan's own
/// expectations never substitute for it — `executed_selection` is
/// yielded by [`ExecutedSliceWalk::completed_selection`], never assigned
/// from the plan unconditionally.
struct FlowExecutionWitness<'w> {
    executed_selection: Option<&'w verter_semantic::analysis::flow::flow_ir::ReturnSlicePlan>,
    skeleton: &'w verter_semantic::analysis::flow::FunctionBodySkeleton,
    anchor: u32,
    calls: &'w [FlowCallEvidence],
}

/// One return-site contribution: the evaluated node plus whether it came
/// from a FRESH literal source (a bare literal return argument, or a read
/// of a widening-literal `const`). tsc widens a fresh literal return only
/// when the deduplicated contributor set has exactly ONE member, so the
/// freshness bit must survive to the join.
#[derive(Clone, Copy)]
struct FlowContribution {
    /// The evaluated contributor node.
    node: SemanticNodeId,
    /// The contributor is a fresh (widening) literal source.
    fresh_literal: bool,
    /// The contributor is reached only through an overridden break's
    /// return-inference suffix edge.
    inference_only: bool,
}

/// One captured `break` exit: the target the lowering resolved
/// (`None` = the innermost switch) and the complete layer state at the
/// break point.
#[derive(Clone)]
struct FlowBreakExit {
    target: Option<Arc<str>>,
    state: FlowLayerState,
}

/// What one block scope's lexical declaration shadowed: the outer
/// binding's value and memberships at scope entry (`None` = the name was
/// fresh in the scope).
struct ScopeShadow {
    name: String,
    prior: Option<(SemanticNodeId, bool, bool, bool)>,
    prior_declared: Option<SemanticNodeId>,
}

/// One point-in-time snapshot of the evaluator's binding layers — the
/// reaching-definitions state a multi-path construct (`switch` dispatch
/// and fall-through, `try` / `catch` / `finally`) joins over. The
/// narrowing overlay rides the state too: a guard or assertion fact lives
/// on the path that established it, so a join INTERSECTS the overlay (a
/// narrow holds past a join only when every joined path carries it) and a
/// clause start restores the entering overlay — a narrow established in a
/// `try` block or a sibling `case` can never leak across the boundary.
#[derive(Clone)]
struct FlowLayerState {
    locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    declared_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    degraded_locals: rustc_hash::FxHashSet<String>,
    widening_locals: rustc_hash::FxHashSet<String>,
    var_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    var_declared_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
    var_degraded_locals: rustc_hash::FxHashSet<String>,
    var_widening_locals: rustc_hash::FxHashSet<String>,
    var_conditional_locals: rustc_hash::FxHashSet<String>,
    param_writes: rustc_hash::FxHashMap<u32, SemanticNodeId>,
    narrowings: Vec<(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    )>,
    /// Lexical-layer bindings whose surviving value is one path's, not the
    /// join of every path that reaches the read (a `try`-internal write
    /// observed in the `catch` / `finally` / after the statement). The
    /// lexical twin of the `var`-layer conditional membership — observing
    /// one records the same conditional-definition degradation.
    conditional_lexicals: rustc_hash::FxHashSet<String>,
    /// The parameter-write twin of `conditional_lexicals`, by ordinal.
    conditional_params: rustc_hash::FxHashSet<u32>,
    /// Whether this snapshot exists only for the return-inference suffix
    /// of a break that an abrupt `finally` replaced at runtime.
    inference_only_path: bool,
}

/// The outcome of evaluating ONE POSITION — a sub-expression, or one call
/// in a sub-expression.
///
/// THREE outcomes, and deliberately NO error variant. The recurring defect
/// class of this substrate was a POSITIONAL condition carried as a
/// frame-level `Err`: with `Result<_, FlowReturnFailure>` as the
/// positional evaluators' return type, whole-frame propagation is what `?`
/// does by default and localisation is the thing each site has to
/// remember. Deleting the *reasons* left the *path*.
///
/// This type deletes the path. Inside
/// [`FlowEvaluator::eval_expr`] / [`FlowEvaluator::eval_call`] a
/// [`FlowReturnFailure`] is UNSPELLABLE, not merely unspelled: there is no
/// variant to construct one into, and `?` over a
/// `Result<_, FlowReturnFailure>` does not typecheck against a
/// non-[`std::ops::Try`] return type, so a nested evaluator's frame-level
/// failure has to be answered — as a value, a hold, or an unmodelled
/// position — before it can be returned.
///
/// A frame still fails, for the reasons that are genuinely ABOUT the whole
/// frame: an unmodelled CONTROL surface ([`FlowReturnUnsupported`]), a
/// missing body, a budget, an empty cycle, a torn view, and an unmodelled
/// DEMAND point. Every one of those is produced OUTSIDE these two
/// functions.
enum Positional<T> {
    /// A modelled value.
    Value(T),
    /// A coinductive HOLD — a recursive back-edge whose value the SCC
    /// fixed point supplies. Neither a contributor nor a failure.
    Hold,
    /// This POSITION has no modelled value. The enclosing structure still
    /// does: the consumer mints the typed marker here and records the
    /// positional degradation, so every modelled sibling survives and the
    /// whole result is a DEGRADED SUCCESS — usable, `ReturnOnly`, never
    /// warm.
    Unmodeled,
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Promote the first statement-level gap only when evaluation found no
    /// concrete degradation.
    fn promote_pending_statement_gap(&mut self) {
        if self.degradation.is_none() {
            self.degradation = self
                .pending_statement_gap
                .map(crate::semantic_query::FlowReturnDegradation::FlowGap);
        }
    }

    /// Record a typed degradation (first-observed reason wins,
    /// deterministic in source order).
    /// Contribute the typed unresolved MARKER at a position whose
    /// resolver is a named DOWNSTREAM block, and record the positional
    /// degradation.
    ///
    /// THE disposition of positional non-modelling. The class is stated
    /// over the CLASS, not over calls: an unmodelled CALL form and a
    /// frame-local binding the flow content does not model take the SAME
    /// arm, because the fact about them is the same fact — this POSITION
    /// has no modelled value, and the enclosing structure still does.
    ///
    /// A fabricated `any` is forbidden here (indistinguishable from an
    /// authored one at every downstream gate) and so is discarding the
    /// composite (an object literal with one unmodelled member HAS a
    /// value). The result is a DEGRADED SUCCESS: usable, `ReturnOnly`,
    /// never warm.
    fn unmodeled_position(&mut self) -> SemanticNodeId {
        self.record_degradation(crate::semantic_query::FlowReturnDegradation::UnmodeledPosition);
        super::flow_return_callee::unmodeled_position_marker(self.dispatch)
    }

    /// The [`CallValue`] twin of [`Self::unmodeled_position`].
    fn unmodeled_call_position(&mut self) -> CallValue {
        self.record_degradation(crate::semantic_query::FlowReturnDegradation::UnmodeledPosition);
        CallValue::unmodeled_position(self.dispatch)
    }

    /// Settle one positional expression outcome into this frame's node: a
    /// value passes through, a HOLD is `None` (the caller's own
    /// coinductive arm), and an unmodelled position becomes the typed
    /// marker plus the recorded degradation.
    fn settle(&mut self, outcome: Positional<SemanticNodeId>) -> Option<SemanticNodeId> {
        match outcome {
            Positional::Value(node) => Some(node),
            Positional::Hold => None,
            Positional::Unmodeled => Some(self.unmodeled_position()),
        }
    }

    /// Settle one positional sub-expression that must yield a node —
    /// an object-literal member value, a union arm — where a HOLD cannot
    /// be represented.
    ///
    /// A hold is a promise the SCC fixed point will union the hold
    /// TARGET's whole admitted return into this entry's result. Inside a
    /// composite that promise is false: the callee's return is not this
    /// object's value, it is one member of it. So the sub-expression's
    /// hold is dropped — leaving it registered would union the callee's
    /// return into the composite. `holds_before` is the frame's hold count
    /// taken immediately before the sub-expression, so a sibling's hold is
    /// never disturbed.
    ///
    /// The truncation is UNCONDITIONAL over the outcome, because the
    /// direct-call site registers a hold on the VALUE arm too: a callee
    /// that popped as a PROVISIONAL member of this component leaves an
    /// obligation edge even though it also handed back a usable value.
    /// Truncating only the `Hold` arm left that one registered, and the
    /// fixed point then unioned the callee's whole return into the
    /// composite — the exact outcome the paragraph above forbids
    /// (`t3a(){return {m:t3b(true)}}` / `t3b(c){if(c)return t3a();return
    /// 1}` published `1 | { m: 1 }`, and a bare `1` is not in `t3a`'s
    /// range for any input). The obligation itself is unaffected: it lives
    /// on the transaction's pending set, and the component's fixed point
    /// still iterates every member.
    fn settle_composite_part(
        &mut self,
        outcome: Positional<SemanticNodeId>,
        holds_before: usize,
    ) -> SemanticNodeId {
        self.holds.truncate(holds_before);
        match outcome {
            Positional::Value(node) => node,
            Positional::Hold | Positional::Unmodeled => self.unmodeled_position(),
        }
    }

    /// Evaluate one STRUCTURAL object literal — the entries in authored
    /// order, where construction order is meaning.
    ///
    /// A literal with no spread interns the object surface directly. A
    /// literal WITH one is a CONSTRUCTION PROGRAM, so it interns the
    /// shared [`SemanticNodeData::ObjectSpreadProgram`] carrier and the
    /// one object-spread projection owns merging it — never a second
    /// merge written here. That is the same carrier a spread-bearing
    /// object type from any other producer lowers to, so every downstream
    /// consumer already reduces it.
    fn eval_object_literal(
        &mut self,
        entries: &[crate::flow_slice_content::SliceObjectEntry],
        assignment_fresh: bool,
    ) -> Positional<SemanticNodeId> {
        let mut surface_members = Vec::with_capacity(entries.len());
        let mut effects: Vec<crate::semantic_query::ObjectConstructionEffect> = Vec::new();
        let mut spread_seen = false;
        for entry in entries.iter() {
            let member = match entry {
                crate::flow_slice_content::SliceObjectEntry::Spread { source } => {
                    // A spread SOURCE this frame cannot evaluate is not a
                    // fact about ONE member — it is a fact about the
                    // surface's KEY SET, and an object surface has no way
                    // to say "these keys, plus an unknown number of
                    // others". Publishing the literal's own properties
                    // alone would declare a member set that is missing
                    // keys the authored value has, which is the `props:
                    // {}` defect at a smaller scale. So the LITERAL is
                    // the unmodelled position.
                    //
                    // A HOLD is the same verdict for the same reason a
                    // member value's hold is dropped
                    // (`settle_composite_part`): the callee's return is
                    // not this object's value. It cannot become a marker
                    // member either, so it fails the literal closed.
                    let holds_before = self.holds.len();
                    let outcome = self.eval_expr(source);
                    self.holds.truncate(holds_before);
                    let Positional::Value(operand) = outcome else {
                        return Positional::Unmodeled;
                    };
                    spread_seen = true;
                    effects.extend(surface_members.drain(..).map(
                        |member: crate::semantic_query::SurfaceMember| {
                            super::object_spread_program_lowering::direct_effect_from_member(
                                &member,
                            )
                        },
                    ));
                    effects.push(crate::semantic_query::ObjectConstructionEffect::Spread(
                        operand,
                    ));
                    continue;
                }
                crate::flow_slice_content::SliceObjectEntry::Member(member) => member,
            };
            // Each member value evaluates as a flow expression (parameter
            // / local references substitute); a hold nested in a member
            // value cannot be a plain skip — the whole evaluation is
            // undecided (recursive object construction is beyond the
            // direct same-slot hold the return sites model).
            let holds_before = self.holds.len();
            let member_value = if assignment_fresh {
                member.assignment_value.as_ref().unwrap_or(&member.value)
            } else {
                &member.value
            };
            let outcome = self.eval_expr(member_value);
            let value = self.settle_composite_part(outcome, holds_before);
            // Selective object widening (BL02-class): a member read of a
            // WIDENING-literal local widens to its primitive at the
            // mutable member position (`const b = 1; return { b }`
            // publishes `b: number`), while `as const` / annotated
            // literal locals stay pinned. Direct literal members already
            // widened (or stayed pinned under a const assertion) at IR
            // lowering.
            let value = self.widen_if_widening_local_read(member_value, value);
            // A non-static key is its own evaluated position. It names
            // the member only if it settles to a LITERAL; anything else
            // leaves the surface's key SET unknown, which an object
            // surface cannot express — the same fail-closed verdict a
            // spread source this frame cannot evaluate takes, and for the
            // same reason.
            let Some(key) = self.eval_object_member_key(&member.key) else {
                return Positional::Unmodeled;
            };
            surface_members.push(crate::semantic_query::SurfaceMember {
                key,
                value,
                optional: false,
                readonly: member.readonly,
                method_kind: member.method_kind,
                has_implementation_body: member.method_kind.is_some(),
                visibility: verter_type_expr::MemberVisibility::Public,
                excess_origin: verter_type_expr::ExcessPropertyOrigin::FreshOwn,
                spans: member.spans,
                declaration_origin: Some(Arc::from(self.canonical)),
                declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::default(),
                merge_role: crate::semantic_query::MergeRoleStamp::default(),
            });
        }
        if spread_seen {
            effects.extend(surface_members.drain(..).map(|member| {
                super::object_spread_program_lowering::direct_effect_from_member(&member)
            }));
            return Positional::Value(self.dispatch.graph().intern_node_with_scope(
                SemanticNodeData::ObjectSpreadProgram(crate::semantic_query::ObjectSpreadProgram {
                    effects: Arc::from(effects.into_boxed_slice()),
                }),
                self.binder_env.scope.clone(),
            ));
        }
        Positional::Value(self.dispatch.graph().intern_node(SemanticNodeData::Object(
            crate::semantic_query::surface_view! {
                members: Arc::from(surface_members.into_boxed_slice()),
                call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                keyspace: None,
                has_index_signature: false,
            },
        )))
    }

    /// Evaluate a right-hand side under assignment context. Object literals
    /// retain their pre-property-widening member values solely for declared
    /// union selection; every other expression uses its ordinary flow value.
    fn eval_assignment_expr(
        &mut self,
        expression: &crate::flow_slice_content::SliceExpr,
    ) -> Positional<SemanticNodeId> {
        match expression {
            crate::flow_slice_content::SliceExpr::Object { entries } => {
                self.eval_object_literal(entries, true)
            }
            other => self.eval_expr(other),
        }
    }

    /// The property key one structurally lowered member names, or `None`
    /// when a non-static key does not settle to a literal.
    ///
    /// The literal requirement is the whole contract. A computed key
    /// whose value is a string or number literal names exactly one
    /// property, so the surface's key set stays known; a key whose value
    /// is anything else (a `string`-typed binding, an unresolved read)
    /// makes the literal provision an UNKNOWN key, and an object surface
    /// has no way to say "these keys, plus one more I cannot name".
    /// Publishing the modelled siblings alone would declare a member set
    /// missing a key the authored value has.
    fn eval_object_member_key(
        &mut self,
        key: &crate::flow_slice_content::SliceObjectKey,
    ) -> Option<crate::semantic_query::AuthoredPropertyKey> {
        let (expression, authored) = match key {
            crate::flow_slice_content::SliceObjectKey::Static(name) => {
                return Some(crate::semantic_query::AuthoredPropertyKey::string(
                    name.as_ref(),
                ))
            }
            crate::flow_slice_content::SliceObjectKey::Computed { value, authored } => {
                (value.as_ref(), authored)
            }
        };
        // A hold inside a KEY is not this object's value any more than a
        // hold inside a spread source is: drop it and read the outcome.
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(expression);
        self.holds.truncate(holds_before);
        let Positional::Value(node) = outcome else {
            return None;
        };
        match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::String(value))) => {
                Some(crate::semantic_query::AuthoredPropertyKey::string(
                    value.as_str(),
                ))
            }
            // The canonical numeric-key spelling is the SHARED authority's
            // (`{ 1: x }` is `1`, `{ 1.5: x }` is `"1.5"`, `{ 0x10: x }`
            // is `16`) — the same one the authored-key lowering uses, so
            // a numeric key never has two spellings.
            Some(SemanticNodeData::Literal(crate::semantic_query::LiteralValue::Number(value))) => {
                Some(crate::semantic_query::AuthoredPropertyKey::from_known(
                    crate::semantic_query::PropertyKey::from_js_number(*value),
                ))
            }
            // A SYMBOL-valued key is the one nameable form the value
            // channel cannot carry: a `unique symbol` names exactly one
            // nominal property, and the evaluator flattens its value to
            // the bare `symbol` primitive, losing the identity that IS
            // the name. So the AUTHORED key names it — the same carrier
            // the whole-literal leaf answer produced, resolved by the
            // same downstream reader.
            //
            // A NON-unique `symbol` key genuinely provisions an index
            // signature rather than one property, and is over-named here.
            // That is not a new divergence: it is exactly what the leaf
            // answer this replaces already did, and telling the two apart
            // needs the symbol's uniqueness on the value channel, which
            // is the same missing fact.
            Some(SemanticNodeData::Primitive(PrimitiveKind::Symbol)) => {
                match authored.cloned_known() {
                    Some(known) => Some(crate::semantic_query::AuthoredPropertyKey::from_known(
                        known,
                    )),
                    None => match authored {
                        verter_type_expr::AuthoredPropertyKey::Computed(ty) => {
                            Some(crate::semantic_query::AuthoredPropertyKey::Computed(
                                self.lower_key_type(ty),
                            ))
                        }
                        _ => None,
                    },
                }
            }
            // Anything else — an OPEN `string` / `number` key, an
            // unresolved read — leaves the surface's key SET unknown.
            _ => None,
        }
    }

    fn record_degradation(&mut self, degradation: crate::semantic_query::FlowReturnDegradation) {
        self.degradation.get_or_insert(degradation);
    }

    /// Bind one evaluated declarator into its SCOPE LAYER: a `var`-kind
    /// binding is function-scoped (the layer block / `if` restores never
    /// touch — `var` hoists, so `{ var y = 1 } return y` keeps `y`);
    /// `const` / `let` stay in the lexical layer. `degraded` records a
    /// failed initializer (the modeled `any`), `widening` the
    /// widening-literal membership — both ride the SAME layer as the
    /// value, so a block restore can never split a binding from its own
    /// flags. A function-scoped binding recorded under non-zero
    /// conditional-arm nesting additionally enters the
    /// conditional-definition set; an unconditional rebind of the same
    /// name clears it.
    fn bind_local(
        &mut self,
        name: &str,
        kind: crate::flow_slice_content::SliceBindingKind,
        node: SemanticNodeId,
        widening: bool,
        degraded: bool,
    ) {
        let function_scoped = kind == crate::flow_slice_content::SliceBindingKind::Var;
        if function_scoped {
            if self.conditional_arm_nesting > 0 {
                self.var_conditional_locals.insert(name.to_string());
            } else {
                self.var_conditional_locals.remove(name);
            }
        }
        let (locals, widening_set, degraded_set) = if function_scoped {
            (
                &mut self.var_locals,
                &mut self.var_widening_locals,
                &mut self.var_degraded_locals,
            )
        } else {
            (
                &mut self.locals,
                &mut self.widening_locals,
                &mut self.degraded_locals,
            )
        };
        if degraded {
            degraded_set.insert(name.to_string());
        } else {
            degraded_set.remove(name);
        }
        if widening {
            widening_set.insert(name.to_string());
        } else {
            widening_set.remove(name);
        }
        locals.insert(name.to_string(), node);
        // A (re)binding replaces the binding's value: every narrow fact a
        // guard established about the OLD value — at the root or under
        // any member path — dies with it.
        let root = crate::flow_slice_content::SliceNarrowRoot::Local(Arc::from(name));
        self.narrowings.retain(|(subject, _)| subject.root != root);
    }

    /// Record the authored declared type of a declaration separately from its
    /// reaching value. A later assignment is reduced against this stable type,
    /// just as an annotated initializer is. An unannotated lexical declaration
    /// clears an outer declaration's metadata while it shadows that binding;
    /// `close_lexical_scope` restores it at the boundary.
    fn set_declared_local(
        &mut self,
        name: &str,
        kind: crate::flow_slice_content::SliceBindingKind,
        declared: Option<SemanticNodeId>,
    ) {
        if kind == crate::flow_slice_content::SliceBindingKind::Var {
            if let Some(node) = declared {
                self.var_declared_locals.insert(name.to_owned(), node);
            }
        } else if let Some(node) = declared {
            self.declared_locals.insert(name.to_owned(), node);
        } else {
            self.declared_locals.remove(name);
        }
    }

    /// Apply the checker's assignment typing rule to a preserved RHS literal.
    /// Annotated unions select their comparable declared constituents;
    /// annotated non-unions retain the declared type; an unannotated target
    /// gets the ordinary widening-literal value.
    fn assignment_node_for_target(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
        value: SemanticNodeId,
    ) -> SemanticNodeId {
        if let crate::flow_slice_content::SliceNarrowRoot::Local(name) = &target.root {
            if !self.locals.contains_key(name.as_ref())
                && !self.var_locals.contains_key(name.as_ref())
            {
                self.seed_destructured_param_element(name.as_ref());
            }
        }
        let declared = match &target.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                self.params.get(*ordinal as usize).copied()
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                if self.locals.contains_key(name.as_ref()) {
                    self.declared_locals.get(name.as_ref()).copied()
                } else {
                    self.var_declared_locals.get(name.as_ref()).copied()
                }
            }
        };
        let Some(declared) = declared else {
            let widened = widen_literal_node(self.dispatch, value);
            // Reuse an equivalent reaching-definition arm when one already
            // exists. Primitive nodes can originate in distinct lowered
            // arenas; joining two ids that both spell `number` would otherwise
            // manufacture `number | number` instead of deduplicating the
            // assignment path.
            let current = match &target.root {
                crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => self
                    .param_writes
                    .get(ordinal)
                    .copied()
                    .or_else(|| self.params.get(*ordinal as usize).copied()),
                crate::flow_slice_content::SliceNarrowRoot::Local(name) => self
                    .locals
                    .get(name.as_ref())
                    .copied()
                    .or_else(|| self.var_locals.get(name.as_ref()).copied()),
            };
            if let Some(current) = current {
                let widened_data = self.dispatch.graph().node_data(widened);
                if let Some(existing) = self
                    .union_arms_or_self(current)
                    .into_iter()
                    .find(|node| self.dispatch.graph().node_data(*node) == widened_data)
                {
                    return existing;
                }
            }
            return widened;
        };
        match self.dispatch.union_arms_of(declared) {
            Some(arms) => self.assignment_reduced_union(declared, &arms, value),
            None => declared,
        }
    }

    /// Whether a write target is governed by a declared union. The check
    /// performs the same lazy destructured-parameter bootstrap as assignment
    /// application so RHS context and target authority cannot diverge.
    fn target_has_declared_union(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
    ) -> bool {
        if let crate::flow_slice_content::SliceNarrowRoot::Local(name) = &target.root {
            if !self.locals.contains_key(name.as_ref())
                && !self.var_locals.contains_key(name.as_ref())
            {
                self.seed_destructured_param_element(name.as_ref());
            }
        }
        let declared = match &target.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                self.params.get(*ordinal as usize).copied()
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                if self.locals.contains_key(name.as_ref()) {
                    self.declared_locals.get(name.as_ref()).copied()
                } else {
                    self.var_declared_locals.get(name.as_ref()).copied()
                }
            }
        };
        declared.is_some_and(|node| self.dispatch.union_arms_of(node).is_some())
    }

    /// Read the newest narrow fact for exactly `subject`, if a guard
    /// established one.
    fn narrowed_read(
        &self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
    ) -> Option<SemanticNodeId> {
        self.narrowings
            .iter()
            .rev()
            .find(|(candidate, _)| candidate == subject)
            .map(|(_, node)| *node)
    }

    /// Apply one evaluated write to its target: a parameter write rides
    /// `param_writes` (the shared `params` slice is the signature's own),
    /// a local write rebinds in the layer the binding already lives in.
    /// The write RETYPES the binding, so every narrow fact about the
    /// value it replaces dies first — including the one the enclosing
    /// guard just established (`if (typeof v === "string") { v = … }`
    /// reads the WRITTEN value after the statement, not the narrow).
    fn apply_write(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
        node: SemanticNodeId,
        degraded: bool,
    ) {
        // A failed RHS carries the explicit unmodelled-position marker. It
        // cannot select a declared constituent; preserving the marker keeps
        // the positional failure visible to every downstream consumer.
        let node = if degraded {
            node
        } else {
            self.assignment_node_for_target(target, node)
        };
        match &target.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                let ordinal = *ordinal;
                self.narrowings
                    .retain(|(subject, _)| subject.root != target.root);
                self.param_writes.insert(ordinal, node);
                if degraded {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnmodeledPosition,
                    );
                }
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                let kind = if self.locals.contains_key(name.as_ref()) {
                    crate::flow_slice_content::SliceBindingKind::Let
                } else {
                    crate::flow_slice_content::SliceBindingKind::Var
                };
                // `bind_local` itself clears the narrow facts about the
                // replaced value (the invalidation cannot be forgotten at
                // one of the two write sites).
                self.bind_local(name, kind, node, false, degraded);
            }
        }
    }

    /// Join the reaching definitions two conditional arms wrote, into the
    /// RESTORED layers: a binding an arm rebound holds, after the `if`,
    /// the union of its arm value and the value it had on the paths that
    /// never took that arm (with no `else`, the fall-through path is one
    /// of them). An arm whose path TERMINATED (`consequent_falls` /
    /// `alternate_falls` false — a return, throw, or absorbed break)
    /// contributes the pre-`if` value at every binding: its writes left
    /// with its path and never reach the join. Bindings an arm DECLARED
    /// stay out of the join — the layer restore already scoped them,
    /// except the hoisted `var`s, which survive by construction and keep
    /// the conditional-definition membership.
    ///
    /// Rebinding at nesting 0 through [`Self::bind_local`] is what clears
    /// the conditional-definition flag for a joined name: the join IS
    /// both arms' values folded, so observing it afterwards is no longer
    /// the one-arm answer the flag exists to fail closed on.
    #[allow(clippy::too_many_arguments)]
    fn join_arm_writes(
        &mut self,
        consequent_locals: &rustc_hash::FxHashMap<String, SemanticNodeId>,
        consequent_falls: bool,
        alternate_locals: Option<&rustc_hash::FxHashMap<String, SemanticNodeId>>,
        alternate_falls: bool,
        consequent_var: &rustc_hash::FxHashMap<String, SemanticNodeId>,
        alternate_var: Option<&rustc_hash::FxHashMap<String, SemanticNodeId>>,
        saved_var_conditional: &rustc_hash::FxHashSet<String>,
        consequent_param_writes: &rustc_hash::FxHashMap<u32, SemanticNodeId>,
        alternate_param_writes: Option<&rustc_hash::FxHashMap<u32, SemanticNodeId>>,
    ) {
        // The pre-`if` layers are the LIVE ones again (the caller
        // restored them). Each SURVIVING arm contributes its end value
        // (or the pre-`if` value when it never wrote the binding); a
        // TERMINATED arm contributes NOTHING — with an explicit `else`,
        // every path past the `if` took the surviving arm. A missing
        // `else` is the implicit alternate: it always survives, with the
        // pre-`if` value.
        for (name, before) in self.locals.clone().iter() {
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(consequent_locals.get(name).copied().unwrap_or(*before));
            }
            match alternate_locals {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.get(name).copied().unwrap_or(*before));
                }
                None => contributors.push(*before),
                _ => {}
            }
            if contributors.is_empty() || contributors.iter().all(|node| node == before) {
                continue;
            }
            let joined = self
                .dispatch
                .intern_normalized_union_or_intersection(&contributors, true);
            self.bind_local(
                name,
                crate::flow_slice_content::SliceBindingKind::Let,
                joined,
                false,
                false,
            );
        }
        for (name, before) in self.var_locals.clone().iter() {
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(consequent_var.get(name).copied().unwrap_or(*before));
            }
            match alternate_var {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.get(name).copied().unwrap_or(*before));
                }
                None => contributors.push(*before),
                _ => {}
            }
            // An arm WROTE the binding when its value moved OR the write
            // raised the conditional-definition flag during an arm (an
            // unchanged value still went through the write) — on an arm
            // whose path SURVIVES the `if`. A terminated arm's writes
            // never reach the join, so its flag-raising is folded the
            // same way. The join folds both, so the flag must not survive
            // either way.
            let written_in_arm = (consequent_falls || alternate_falls)
                && !saved_var_conditional.contains(name)
                && self.var_conditional_locals.contains(name);
            if contributors.is_empty()
                || (contributors.iter().all(|node| node == before) && !written_in_arm)
            {
                continue;
            }
            let joined = self
                .dispatch
                .intern_normalized_union_or_intersection(&contributors, true);
            self.bind_local(
                name,
                crate::flow_slice_content::SliceBindingKind::Var,
                joined,
                false,
                false,
            );
        }
        // Hoisted `var`s DECLARED inside an arm: the layer restore scopes
        // them away, but `var` hoisting means the binding itself survives
        // the `if` — even when the arm's path terminates (hoisting is
        // static). Merge them back with the conditional-definition flag
        // INTACT — on the paths that never took the arm the binding has
        // no reaching definition, which is exactly what the flag fails
        // closed on at a read.
        let mut declared_in_arm: Vec<String> = consequent_var
            .keys()
            .chain(alternate_var.into_iter().flat_map(|locals| locals.keys()))
            .filter(|name| !self.var_locals.contains_key(*name))
            .cloned()
            .collect();
        declared_in_arm.sort();
        declared_in_arm.dedup();
        for name in declared_in_arm {
            let consequent_node = consequent_var.get(&name).copied();
            let alternate_node = alternate_var.and_then(|locals| locals.get(&name).copied());
            let node = match (consequent_node, alternate_node) {
                (Some(consequent), Some(alternate)) => self
                    .dispatch
                    .intern_normalized_union_or_intersection(&[consequent, alternate], true),
                (Some(consequent), None) => consequent,
                (None, Some(alternate)) => alternate,
                (None, None) => continue,
            };
            self.var_locals.insert(name.clone(), node);
            self.var_conditional_locals.insert(name);
        }
        let mut param_ordinals: Vec<u32> = consequent_param_writes.keys().copied().collect();
        if let Some(alternate) = alternate_param_writes {
            param_ordinals.extend(alternate.keys().copied());
        }
        param_ordinals.sort_unstable();
        param_ordinals.dedup();
        for ordinal in param_ordinals {
            let before = self.param_writes.get(&ordinal).copied();
            let fallback = before.or_else(|| self.params.get(ordinal as usize).copied());
            let Some(fallback) = fallback else {
                continue;
            };
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(
                    consequent_param_writes
                        .get(&ordinal)
                        .copied()
                        .unwrap_or(fallback),
                );
            }
            match alternate_param_writes {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.get(&ordinal).copied().unwrap_or(fallback));
                }
                None => contributors.push(fallback),
                _ => {}
            }
            if contributors.is_empty() || contributors.iter().all(|node| *node == fallback) {
                continue;
            }
            let joined = self
                .dispatch
                .intern_normalized_union_or_intersection(&contributors, true);
            self.param_writes.insert(ordinal, joined);
        }
    }

    /// Snapshot the live binding layers.
    fn layer_state(&self) -> FlowLayerState {
        FlowLayerState {
            locals: self.locals.clone(),
            declared_locals: self.declared_locals.clone(),
            degraded_locals: self.degraded_locals.clone(),
            widening_locals: self.widening_locals.clone(),
            var_locals: self.var_locals.clone(),
            var_declared_locals: self.var_declared_locals.clone(),
            var_degraded_locals: self.var_degraded_locals.clone(),
            var_widening_locals: self.var_widening_locals.clone(),
            var_conditional_locals: self.var_conditional_locals.clone(),
            param_writes: self.param_writes.clone(),
            narrowings: self.narrowings.clone(),
            conditional_lexicals: self.conditional_lexicals.clone(),
            conditional_params: self.conditional_params.clone(),
            inference_only_path: self.inference_only_path,
        }
    }

    /// Restore a snapshot into the live layers.
    fn restore_layer_state(&mut self, state: FlowLayerState) {
        self.locals = state.locals;
        self.declared_locals = state.declared_locals;
        self.degraded_locals = state.degraded_locals;
        self.widening_locals = state.widening_locals;
        self.var_locals = state.var_locals;
        self.var_declared_locals = state.var_declared_locals;
        self.var_degraded_locals = state.var_degraded_locals;
        self.var_widening_locals = state.var_widening_locals;
        self.var_conditional_locals = state.var_conditional_locals;
        self.param_writes = state.param_writes;
        self.narrowings = state.narrowings;
        self.conditional_lexicals = state.conditional_lexicals;
        self.conditional_params = state.conditional_params;
        self.inference_only_path = state.inference_only_path;
    }

    /// Close ONE block scope over `state` (a snapshot, or the live layers
    /// through a clone): bindings the scope DECLARED are dropped, a
    /// same-name redeclaration restores the shadowed outer binding, and a
    /// write to a binding that PREDATES the scope survives untouched.
    /// That is the reaching-definitions rule a wholesale layer restore
    /// cannot express — restoring the entry layer throws the scope's
    /// writes away with its declarations.
    ///
    /// `shadows` is exactly the scope's own declaration records, sliced
    /// off the evaluator's `scope_shadows` at the scope's boundaries, so
    /// a scope can never drop (or keep) another scope's binding.
    fn close_lexical_scope(state: &mut FlowLayerState, shadows: &[ScopeShadow]) {
        for shadow in shadows.iter().rev() {
            if let Some(node) = shadow.prior_declared {
                state.declared_locals.insert(shadow.name.clone(), node);
            } else {
                state.declared_locals.remove(&shadow.name);
            }
            match &shadow.prior {
                Some((node, widening, degraded, conditional)) => {
                    state.locals.insert(shadow.name.clone(), *node);
                    if *widening {
                        state.widening_locals.insert(shadow.name.clone());
                    } else {
                        state.widening_locals.remove(&shadow.name);
                    }
                    if *degraded {
                        state.degraded_locals.insert(shadow.name.clone());
                    } else {
                        state.degraded_locals.remove(&shadow.name);
                    }
                    if *conditional {
                        state.conditional_lexicals.insert(shadow.name.clone());
                    } else {
                        state.conditional_lexicals.remove(&shadow.name);
                    }
                }
                None => {
                    state.locals.remove(&shadow.name);
                    state.widening_locals.remove(&shadow.name);
                    state.degraded_locals.remove(&shadow.name);
                    state.conditional_lexicals.remove(&shadow.name);
                }
            }
        }
    }

    /// Record one lexical declaration's shadow (the binding it replaces
    /// for the extent of its block scope, when an outer scope bound the
    /// same name). Every lexical `Binding` evaluation records BEFORE it
    /// binds, so the scope close above never has to guess which names the
    /// scope declared.
    fn record_scope_shadow(&mut self, name: &str) {
        let prior = self.locals.get(name).map(|node| {
            (
                *node,
                self.widening_locals.contains(name),
                self.degraded_locals.contains(name),
                self.conditional_lexicals.contains(name),
            )
        });
        self.scope_shadows.push(ScopeShadow {
            name: name.to_owned(),
            prior,
            prior_declared: self.declared_locals.get(name).copied(),
        });
    }

    /// Drain the break exits recorded since `base` that target
    /// `target` (`None` = the switch's anonymous break); exits naming any
    /// other target stay pending for their own construct.
    fn drain_break_exits(&mut self, base: usize, target: Option<&Arc<str>>) -> Vec<FlowLayerState> {
        let drained: Vec<FlowBreakExit> = self.break_exits.split_off(base);
        let (mine, rest): (Vec<FlowBreakExit>, Vec<FlowBreakExit>) = drained
            .into_iter()
            .partition(|exit| exit.target.as_ref() == target);
        self.break_exits.extend(rest);
        mine.into_iter().map(|exit| exit.state).collect()
    }

    /// Split off one block scope's declaration records and replay the close
    /// over every pending abrupt edge the scope's evaluation captured. A
    /// break, return, or throw edge rides its point-in-time bindings across
    /// the boundary, so declarations cannot leak and shadowed outer bindings
    /// are restored before any later join observes the state. The caller
    /// applies the returned records to its fall-through end state too.
    fn split_scope_shadows_close_exits(
        &mut self,
        shadow_base: usize,
        break_base: usize,
        return_base: usize,
        throw_base: usize,
    ) -> Vec<ScopeShadow> {
        let shadows: Vec<ScopeShadow> = self.scope_shadows.split_off(shadow_base);
        for exit in &mut self.break_exits[break_base..] {
            Self::close_lexical_scope(&mut exit.state, &shadows);
        }
        for state in &mut self.return_edges[return_base..] {
            Self::close_lexical_scope(state, &shadows);
        }
        for state in &mut self.throw_points[throw_base..] {
            Self::close_lexical_scope(state, &shadows);
        }
        shadows
    }

    /// Capture the complete state at a return edge. A crossing `finally`
    /// executes from this state; the returned value remains a separate flow
    /// contribution.
    fn capture_return_edge(&mut self) {
        let mut state = self.layer_state();
        self.complete_param_writes(&mut state);
        self.return_edges.push(state);
    }

    /// Snapshot the complete state at a throw point (a call or a
    /// `throw`): a `catch` / `finally` clause is entered from every one
    /// of these, not only from the try's entry.
    fn capture_throw_point(&mut self) {
        if !self.collect_throw_points {
            return;
        }
        let mut state = self.layer_state();
        self.complete_param_writes(&mut state);
        self.throw_points.push(state);
    }

    /// Complete a snapshot's parameter-write layer with the signature's
    /// own parameter nodes, so a pointwise join over snapshots reads "no
    /// write on this path" as the parameter's declared value instead of
    /// dropping the ordinal. Reads are unchanged — every consumer already
    /// falls back to `params[ordinal]` for a missing write.
    fn complete_param_writes(&self, state: &mut FlowLayerState) {
        for ordinal in 0..self.params.len() {
            let ordinal = ordinal as u32;
            if let std::collections::hash_map::Entry::Vacant(entry) =
                state.param_writes.entry(ordinal)
            {
                if let Some(node) = self.params.get(ordinal as usize) {
                    entry.insert(*node);
                }
            }
        }
    }

    /// The pointwise join of two reaching-definition states: a binding
    /// both states carry holds the normalized union of its two values; a
    /// binding only one carries keeps that value. Membership flags union
    /// (a binding degraded / widening / conditional on ANY reaching path
    /// keeps the flag) — the honest direction for a joined answer. The
    /// narrowing overlay INTERSECTS instead: a guard fact survives the
    /// join only when BOTH paths established it (the checker's own rule —
    /// a narrowing holds past a merge point only when it holds on every
    /// incoming edge).
    fn join_layer_states(&self, a: &FlowLayerState, b: &FlowLayerState) -> FlowLayerState {
        let join_values =
            |from: &rustc_hash::FxHashMap<String, SemanticNodeId>,
             into: &mut rustc_hash::FxHashMap<String, SemanticNodeId>| {
                for (name, node) in from.iter() {
                    match into.entry(name.clone()) {
                        std::collections::hash_map::Entry::Occupied(mut entry) => {
                            let existing = *entry.get();
                            if existing != *node {
                                entry.insert(
                                    self.dispatch.intern_normalized_union_or_intersection(
                                        &[existing, *node],
                                        true,
                                    ),
                                );
                            }
                        }
                        std::collections::hash_map::Entry::Vacant(entry) => {
                            entry.insert(*node);
                        }
                    }
                }
            };
        let mut joined = a.clone();
        join_values(&b.locals, &mut joined.locals);
        join_values(&b.declared_locals, &mut joined.declared_locals);
        join_values(&b.var_locals, &mut joined.var_locals);
        join_values(&b.var_declared_locals, &mut joined.var_declared_locals);
        joined
            .degraded_locals
            .extend(b.degraded_locals.iter().cloned());
        joined
            .widening_locals
            .extend(b.widening_locals.iter().cloned());
        joined
            .var_degraded_locals
            .extend(b.var_degraded_locals.iter().cloned());
        joined
            .var_widening_locals
            .extend(b.var_widening_locals.iter().cloned());
        joined
            .var_conditional_locals
            .extend(b.var_conditional_locals.iter().cloned());
        joined
            .conditional_lexicals
            .extend(b.conditional_lexicals.iter().cloned());
        joined
            .conditional_params
            .extend(b.conditional_params.iter().copied());
        for (ordinal, node) in b.param_writes.iter() {
            match joined.param_writes.entry(*ordinal) {
                std::collections::hash_map::Entry::Occupied(mut entry) => {
                    let existing = *entry.get();
                    if existing != *node {
                        entry.insert(
                            self.dispatch
                                .intern_normalized_union_or_intersection(&[existing, *node], true),
                        );
                    }
                }
                std::collections::hash_map::Entry::Vacant(entry) => {
                    entry.insert(*node);
                }
            }
        }
        // The narrowing overlay intersects: keep exactly the facts the
        // OTHER state also carries (a clone of `a`'s entries preserves the
        // newest-first read order).
        joined.narrowings = a
            .narrowings
            .iter()
            .filter(|fact| b.narrowings.contains(fact))
            .cloned()
            .collect();
        // A joined path is inference-only only when every incoming edge is.
        // One ordinary runtime edge makes the merged continuation ordinary.
        joined.inference_only_path = a.inference_only_path && b.inference_only_path;
        joined
    }

    /// The bindings whose value moved between two states — a write (or a
    /// write-bearing join) on one path that the other path never saw.
    /// Read at a clause boundary where an abrupt exit could have preceded
    /// the write: observing such a binding must fail closed.
    fn written_between(
        &self,
        before: &FlowLayerState,
        after: &FlowLayerState,
    ) -> (Vec<String>, Vec<String>, Vec<u32>) {
        let moved = |past: &rustc_hash::FxHashMap<String, SemanticNodeId>,
                     present: &rustc_hash::FxHashMap<String, SemanticNodeId>| {
            present
                .iter()
                .filter(|(name, node)| past.get(*name).copied() != Some(**node))
                .map(|(name, _)| name.clone())
                .collect::<Vec<String>>()
        };
        let params = after
            .param_writes
            .iter()
            .filter(|(ordinal, node)| before.param_writes.get(*ordinal).copied() != Some(**node))
            .map(|(ordinal, _)| *ordinal)
            .collect::<Vec<u32>>();
        (
            moved(&before.locals, &after.locals),
            moved(&before.var_locals, &after.var_locals),
            params,
        )
    }

    /// Flag a set of try-internal writes on a clause-entry state: a read
    /// of any of them in the clause fails closed (the throw can precede
    /// the write, so the value is one path's, not the join's).
    fn flag_clause_writes(
        &self,
        state: &mut FlowLayerState,
        written: &(Vec<String>, Vec<String>, Vec<u32>),
    ) {
        state.conditional_lexicals.extend(written.0.iter().cloned());
        state
            .var_conditional_locals
            .extend(written.1.iter().cloned());
        state.conditional_params.extend(written.2.iter().copied());
    }

    /// Flag the `var` bindings a fall-through edge carries that the
    /// dispatch edge (the state at the construct's entry) never defined:
    /// their value has no reaching definition on the dispatch path, which
    /// is exactly what the conditional-definition membership fails closed
    /// on at a read.
    fn flag_fallthrough_only_vars(&self, start: &mut FlowLayerState, entry: &FlowLayerState) {
        let fallthrough_only: Vec<String> = start
            .var_locals
            .keys()
            .filter(|name| !entry.var_locals.contains_key(*name))
            .cloned()
            .collect();
        start.var_conditional_locals.extend(fallthrough_only);
    }
    /// Flag the `var` bindings of a joined state that some normal-exit
    /// path never defined: their surviving value has no reaching
    /// definition on that path, which is exactly what the
    /// conditional-definition membership fails closed on at a read.
    fn flag_conditionally_defined_vars(
        &self,
        joined: &mut FlowLayerState,
        exit_states: &[FlowLayerState],
    ) {
        let conditionally_defined: Vec<String> = joined
            .var_locals
            .keys()
            .filter(|name| {
                exit_states
                    .iter()
                    .any(|state| !state.var_locals.contains_key(*name))
            })
            .cloned()
            .collect();
        joined.var_conditional_locals.extend(conditionally_defined);
    }

    /// Evaluate one clause region of a `try` statement in its own block
    /// scope, starting from `start`: lexical bindings declared inside do
    /// not escape (the catch parameter included); function-scoped `var`
    /// and parameter writes do — and so do WRITES to bindings that
    /// predate the clause (the clause runs on the paths that reach it, so
    /// its writes to outer bindings are reaching definitions past it).
    /// Returns the clause's return contributions, the end-of-clause state
    /// (parameter writes completed, block scope closed), and the writes
    /// the clause performed — computed BEFORE the block-scope close, so a
    /// write to an OUTER lexical binding is seen too. The live layers are
    /// the closed end state when the call returns.
    ///
    /// `collect_throws` turns throw-point collection on for the clause:
    /// on for the try block (a following `catch` / `finally` is entered
    /// from every throw point) and for the catch clause when a `finally`
    /// follows it. The snapshots the clause collects are scope-closed
    /// with it, so a clause-internal declaration never leaks into a
    /// clause-entry join.
    #[allow(clippy::type_complexity)]
    fn eval_try_clause(
        &mut self,
        start: &FlowLayerState,
        region: &crate::flow_slice_content::SliceRegion,
        catch_param: Option<&Arc<str>>,
        collect_throws: bool,
    ) -> Result<
        (
            Vec<FlowContribution>,
            FlowLayerState,
            (Vec<String>, Vec<String>, Vec<u32>),
        ),
        FlowReturnFailure,
    > {
        self.restore_layer_state(start.clone());
        let shadow_base = self.scope_shadows.len();
        let throw_base = self.throw_points.len();
        let break_base = self.break_exits.len();
        let return_base = self.return_edges.len();
        let saved_collect = self.collect_throw_points;
        self.collect_throw_points = collect_throws;
        if let Some(param) = catch_param {
            self.record_scope_shadow(param.as_ref());
            // The catch parameter is `unknown` under the checker's strict
            // default (`useUnknownInCatchVariables`): bound so a read
            // resolves to the honest primitive instead of a free-name miss.
            let unknown = self
                .dispatch
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Unknown));
            self.bind_local(
                param.as_ref(),
                crate::flow_slice_content::SliceBindingKind::Const,
                unknown,
                false,
                false,
            );
        }
        let (result, _) = self.eval_region(region);
        self.collect_throw_points = saved_collect;
        let contributions = result?;
        let written = self.written_between(start, &self.layer_state());
        let mut end = self.layer_state();
        self.complete_param_writes(&mut end);
        // The scope close replays on every state the clause's evaluation
        // produced: the end state, the throw points, and the pending
        // break exits that crossed the clause's scope.
        let shadows =
            self.split_scope_shadows_close_exits(shadow_base, break_base, return_base, throw_base);
        Self::close_lexical_scope(&mut end, &shadows);
        self.restore_layer_state(end.clone());
        Ok((contributions, end, written))
    }

    /// READ one local across the two scope layers — the ONLY way to take
    /// a local's bound node. The lexical layer shadows the function-scoped
    /// `var` layer while its block is live, and the read FOLDS the
    /// binding's LAYER-scoped membership flags into this evaluation's
    /// degradation channel as it goes (a failed initializer, a
    /// conditionally-defined `var`).
    ///
    /// The flags are recorded HERE, not returned, so "take the node
    /// without folding the flags" is not expressible at any call site:
    /// every observation of a degraded binding degrades the result, by
    /// construction rather than by per-site discipline.
    fn read_local(&mut self, name: &str) -> Option<SemanticNodeId> {
        // A destructured object-pattern parameter element binds its
        // annotation member LAZILY, on first read: an element the demand
        // never reads stays cold (no projection, no degradation), and an
        // element whose member cannot be projected binds the typed
        // unmodelled-position marker with the failed-initializer
        // membership — exactly like an unmodelled declarator initializer,
        // so observing it degrades and never observing it is free.
        if !self.locals.contains_key(name) && !self.var_locals.contains_key(name) {
            self.seed_destructured_param_element(name);
        }
        // The lexical layer's conditional flag comes from the
        // try-clause-write membership (a binding whose surviving value is
        // one path's, not the join's); a block-scoped conditional binding
        // otherwise never escapes its arm.
        let (node, degraded, conditional) = if let Some(node) = self.locals.get(name) {
            (
                *node,
                self.degraded_locals.contains(name),
                self.conditional_lexicals.contains(name),
            )
        } else {
            let node = self
                .var_locals
                .get(name)
                .copied()
                .or_else(|| self.var_declared_locals.get(name).copied())?;
            (
                node,
                self.var_degraded_locals.contains(name),
                self.var_conditional_locals.contains(name),
            )
        };
        if degraded {
            // Observing a binding whose initializer FAILED is the
            // `FailedBindingInitializer` degradation: the value is a
            // modeled `any`, not the initializer's real type. An
            // unobserved failed binding degrades nothing.
            self.record_degradation(
                crate::semantic_query::FlowReturnDegradation::FailedBindingInitializer,
            );
        }
        if conditional {
            // Observing a function-scoped binding whose surviving
            // reaching definition was recorded inside a conditional arm
            // is the `ConditionalVarDefinition` degradation: the value is
            // the last-evaluated arm's, not the join of every arm (and of
            // the never-assigned path).
            self.record_degradation(
                crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
            );
        }
        Some(node)
    }

    /// Whether one union arm IS `undefined` — by REDUCTION through the
    /// sole relation authority, never by spelling: an aliased
    /// `undefined` (`type U = undefined`) answers true identically to the
    /// direct spelling. `any` / `unknown` are mutually assignable with
    /// everything, so they are excluded up front.
    fn arm_reduces_to_undefined(&self, arm: SemanticNodeId) -> bool {
        match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined)) => true,
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown)) => false,
            _ => {
                let undefined = self
                    .dispatch
                    .graph()
                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
                self.assignable(arm, undefined) == Some(true)
                    && self.assignable(undefined, arm) == Some(true)
            }
        }
    }

    /// Project one destructured object-pattern element from its annotated
    /// parameter node, applying the element's optional/default rule.
    fn destructured_param_element_node(
        &mut self,
        base: SemanticNodeId,
        key: &Arc<str>,
        has_default: bool,
    ) -> Option<SemanticNodeId> {
        let data = self.dispatch.graph().node_data(base);
        let view = match data.as_deref() {
            Some(SemanticNodeData::Object(view)) => view,
            _ => return None,
        };
        let crate::semantic_query::SurfaceKeyProjection::Exact(member) = view.project_known_key(
            &crate::semantic_query::PropertyKey::identifier(Arc::clone(key)),
        ) else {
            return None;
        };
        let value = member.value;
        if has_default {
            // A default removes the member's `undefined` arm — the authored
            // one included. A member whose type is only `undefined` keeps it:
            // the default initializer's own type is not modelled.
            return Some(match self.dispatch.graph().node_data(value).as_deref() {
                Some(SemanticNodeData::Union(arms)) => {
                    let kept: Vec<SemanticNodeId> = arms
                        .iter()
                        .copied()
                        .filter(|arm| !self.arm_reduces_to_undefined(*arm))
                        .collect();
                    if kept.is_empty() || kept.len() == arms.len() {
                        value
                    } else {
                        self.dispatch
                            .intern_normalized_union_or_intersection(&kept, true)
                    }
                }
                _ => value,
            });
        }
        if member.optional {
            let undefined = self
                .dispatch
                .graph()
                .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
            return Some(
                self.dispatch
                    .intern_normalized_union_or_intersection(&[value, undefined], true),
            );
        }
        Some(value)
    }

    /// Seed one destructured object-pattern parameter element into the
    /// lexical layer on its first read. A projection miss binds the typed
    /// unmodelled-position marker with failed-initializer membership.
    fn seed_destructured_param_element(&mut self, name: &str) {
        let Some((ordinal, key, has_default)) =
            self.param_names
                .iter()
                .enumerate()
                .find_map(|(ordinal, param)| {
                    param
                        .destructured
                        .iter()
                        .find(|element| element.name.as_ref() == name)
                        .map(|element| (ordinal, Arc::clone(&element.key), element.has_default))
                })
        else {
            return;
        };
        let projected = self
            .params
            .get(ordinal)
            .copied()
            .and_then(|base| self.destructured_param_element_node(base, &key, has_default));
        let (node, degraded) = projected
            .map(|node| (node, false))
            .unwrap_or_else(|| (self.unmodeled_position(), true));
        self.set_declared_local(
            name,
            crate::flow_slice_content::SliceBindingKind::Const,
            Some(node),
        );
        self.bind_local(
            name,
            crate::flow_slice_content::SliceBindingKind::Const,
            node,
            false,
            degraded,
        );
    }

    /// Project a frame-rooted `typeof` path leaf: `x.a.b` whose root `x`
    /// is one of THIS frame's bindings (a parameter by name, or a reaching
    /// local) resolves its root through the frame's substitution, then
    /// walks the tail segments through the ONE shared path projection —
    /// the same `ProjectPath { mode: Navigate }` walk the owner-scope
    /// lowering applies to a free root's tail
    /// (`lower.rs`'s `TypeExpr::TypeOf` arm).
    ///
    /// `None` when the leaf is not a frame-rooted member path (the caller
    /// falls back to the leaf's own lowering, whose typed miss carrier
    /// stays the honest answer). A projection miss after a resolved root
    /// takes the same fallback: the position degrades exactly as the
    /// owner-scope walk's miss does, never a fabricated member.
    fn eval_frame_rooted_typeof_path(
        &mut self,
        ty: &verter_type_expr::TypeExpr,
    ) -> Option<Positional<SemanticNodeId>> {
        self.frame_rooted_typeof_path_node(ty)
            .map(Positional::Value)
    }

    /// The node half of [`Self::eval_frame_rooted_typeof_path`]: resolve
    /// the path's root through THIS frame (a parameter by name, else the
    /// two-layer local read) and walk the tail through the one shared
    /// `ProjectPath { mode: Navigate }` projection. `None` when the leaf
    /// is not a frame-rooted member path or the projection misses.
    fn frame_rooted_typeof_path_node(
        &mut self,
        ty: &verter_type_expr::TypeExpr,
    ) -> Option<SemanticNodeId> {
        let verter_type_expr::TypeExpr::TypeOf(value_ref) = ty else {
            return None;
        };
        if value_ref.path.len() < 2 || !value_ref.type_args.is_empty() {
            return None;
        }
        let head = value_ref.path[0].as_str();
        // A parameter is addressed BY NAME here (the leaf carries no
        // ordinal); a local resolves through the same two-layer read every
        // other local reference takes, flags folded by construction.
        let param_ordinal = self
            .param_names
            .iter()
            .position(|param| param.name.as_deref() == Some(head))
            .map(|ordinal| ordinal as u32);
        // THE narrowing overlay: a narrowed reference — at this exact
        // path, or at any PREFIX of it (a discriminant narrowed the root)
        // — substitutes the narrow's node and projects the remaining
        // segments from it, so a guarded member read can never see the
        // pre-narrow union.
        let overlay_root = param_ordinal
            .map(crate::flow_slice_content::SliceNarrowRoot::Param)
            .or_else(|| {
                (self.locals.contains_key(head) || self.var_locals.contains_key(head))
                    .then(|| crate::flow_slice_content::SliceNarrowRoot::Local(Arc::from(head)))
            });
        if let Some(root) = overlay_root {
            let segments: Vec<Arc<str>> = value_ref.path[1..]
                .iter()
                .map(|segment| Arc::from(segment.as_str()))
                .collect();
            for prefix_len in (0..=segments.len()).rev() {
                let subject = crate::flow_slice_content::SliceNarrowSubject {
                    root: root.clone(),
                    path: Arc::from(segments[..prefix_len].to_vec().into_boxed_slice()),
                };
                if let Some(base) = self.narrowed_read(&subject) {
                    return self.project_segments_navigate(base, &segments[prefix_len..]);
                }
            }
        }
        let root = if let Some(ordinal) = param_ordinal {
            // The same conditional-write observation the direct parameter
            // read folds: a `typeof p.…` leaf rooted at a try-written
            // parameter degrades instead of projecting one path's value.
            if self.conditional_params.contains(&ordinal) {
                self.record_degradation(
                    crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
                );
            }
            self.param_writes
                .get(&ordinal)
                .copied()
                .or_else(|| self.params.get(ordinal as usize).copied())
        } else {
            self.read_local(head)
        }?;
        let path: Arc<[crate::semantic_query::PathSegment]> = Arc::from(
            value_ref.path[1..]
                .iter()
                .map(|segment| {
                    crate::semantic_query::PathSegment::Member(
                        crate::semantic_query::PropertyKey::identifier(Arc::from(segment.as_str())),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        // The member walk may widen a ROOTLESS callable (a function-typed
        // parameter's signature) to its ambient apparent surface; that
        // widening scopes its registry lookup by the lexical demand site,
        // which rides this guard (see `apparent_type.rs`).
        let _demand_scope = super::LexicalDemandScopeGuard::push(
            &self.dispatch.lexical_demand_scope,
            Arc::from(self.canonical),
        );
        match self.dispatch.execute_type_node(
            crate::semantic_query::SemanticQueryKey::ProjectPath {
                base: root,
                path,
                context: crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            },
        ) {
            crate::semantic_query::QueryResult::Value(output) => Some(output.value),
            _ => None,
        }
    }

    /// tsc's `getAssignmentReducedType`: the union of the DECLARED
    /// constituents the initializer is comparable to. The survivors are
    /// DECLARED constituents — never the initializer's own type — so
    /// `let x: string | number = "s"` is `string` (not `"s"`) and
    /// `let x: 1 | 2 = 1` is `1`.
    ///
    /// Comparability is judged by the crate's SOLE relation authority
    /// (`execute_relate_pair`); an undecided constituent or an empty
    /// survivor set fails closed onto the whole declared union with the
    /// typed `UnreducedDeclaredUnion` degradation — never a guess. Generic
    /// strict-subtype dominance is not a checker constituent-selection rule:
    /// required-property presence does not discard an overlapping optional
    /// constituent. Only the exact-own-key and discriminant evidence below
    /// can select among comparable object arms.
    fn assignment_reduced_union(
        &mut self,
        declared: SemanticNodeId,
        arms: &[SemanticNodeId],
        init: SemanticNodeId,
    ) -> SemanticNodeId {
        let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        for arm in arms {
            match self.dispatch.execute_relate_pair(init, *arm) {
                super::dispatch_txn::RelationStep::Assignable { .. } => survivors.push(*arm),
                super::dispatch_txn::RelationStep::NotAssignable => {}
                super::dispatch_txn::RelationStep::Unknown
                | super::dispatch_txn::RelationStep::BudgetExceeded(_)
                | super::dispatch_txn::RelationStep::Assumed(_) => {
                    survivors.clear();
                    break;
                }
            }
        }
        if survivors.is_empty() {
            self.record_degradation(
                crate::semantic_query::FlowReturnDegradation::UnreducedDeclaredUnion,
            );
            return declared;
        }
        // A fresh, non-spread object carries exact own-key evidence that
        // assignability alone cannot recover when optional members make two
        // declared arms mutually assignable. Prefer the comparable arms whose
        // known member set has the greatest overlap with the literal's exact
        // surface. Spread programs deliberately do not enter this rule: their
        // final key set is construction-dependent, so the declared union stays
        // intact unless the exact-own-key/discriminant rules above select it.
        if survivors.len() > 1 {
            if let Some(SemanticNodeData::Object(init_view)) =
                self.dispatch.graph().node_data(init).as_deref()
            {
                let scores: Vec<(SemanticNodeId, usize)> = survivors
                    .iter()
                    .copied()
                    .map(|arm| {
                        let score = self
                            .dispatch
                            .resolve_typeinfo_surface_view(
                                arm,
                                crate::semantic_query::ProjectionReductionContext::structural_transit(),
                            )
                            .map(|candidate| {
                                candidate
                                    .positive_members()
                                    .iter()
                                    .filter(|member| {
                                        member.key.cloned_known().is_some_and(|key| {
                                            matches!(
                                                init_view.project_known_key(&key),
                                                crate::semantic_query::SurfaceKeyProjection::Exact(_)
                                            )
                                        })
                                    })
                                    .count()
                            })
                            .unwrap_or(0);
                        (arm, score)
                    })
                    .collect();
                let max_score = scores.iter().map(|(_, score)| *score).max().unwrap_or(0);
                if scores.iter().any(|(_, score)| *score < max_score) {
                    survivors.retain(|arm| {
                        scores
                            .iter()
                            .any(|(candidate, score)| candidate == arm && *score == max_score)
                    });
                }
            }
        }
        self.dispatch
            .intern_normalized_union_or_intersection(&survivors, true)
    }

    // ── Guard narrowing ─────────────────────────────────────────────

    /// The union arms of `node`, or `node` itself when it is not a
    /// union — the iteration domain every narrow filters.
    fn union_arms_or_self(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.to_vec(),
            _ => vec![node],
        }
    }

    /// The CURRENT node of a narrowable reference: the binding's reaching
    /// definition (parameters consult applied writes, locals the scoped
    /// layers), then the static member path walked through the one shared
    /// path projection. `None` when the reference cannot be resolved —
    /// the guard then establishes nothing, exactly the pre-guard
    /// behaviour for that reference.
    fn subject_current_node(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
    ) -> Option<SemanticNodeId> {
        // A narrow on the ROOT is visible from a member-path fact too:
        // `u.kind === "a"` narrows `u`, and a later `typeof u.v` reads
        // the narrowed root before projecting.
        let root_subject = crate::flow_slice_content::SliceNarrowSubject {
            root: subject.root.clone(),
            path: Arc::from(Vec::new().into_boxed_slice()),
        };
        let root = if let Some(node) = self.narrowed_read(&root_subject) {
            node
        } else {
            match &subject.root {
                crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => self
                    .param_writes
                    .get(ordinal)
                    .copied()
                    .or_else(|| self.params.get(*ordinal as usize).copied())?,
                crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                    self.read_local(name.as_ref())?
                }
            }
        };
        if subject.path.is_empty() {
            return Some(root);
        }
        self.project_segments_navigate(root, &subject.path)
    }

    /// Walk a static member path from `base` through the ONE shared
    /// `ProjectPath { mode: Navigate }` walk — the same authority the
    /// frame-rooted `typeof` leaf uses, so a guard's subject resolution
    /// and a member read can never disagree about what a path projects
    /// to. `None` on any projection miss (the narrow then does not
    /// establish).
    fn project_segments_navigate(
        &mut self,
        base: SemanticNodeId,
        segments: &[Arc<str>],
    ) -> Option<SemanticNodeId> {
        if segments.is_empty() {
            return Some(base);
        }
        let path: Arc<[crate::semantic_query::PathSegment]> = Arc::from(
            segments
                .iter()
                .map(|segment| {
                    crate::semantic_query::PathSegment::Member(
                        crate::semantic_query::PropertyKey::identifier(Arc::from(segment.as_ref())),
                    )
                })
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        // The member walk may widen a ROOTLESS callable (a function-typed
        // parameter's signature) to its ambient apparent surface; that
        // widening scopes its registry lookup by the lexical demand site,
        // which rides this guard (see `apparent_type.rs`).
        let _demand_scope = super::LexicalDemandScopeGuard::push(
            &self.dispatch.lexical_demand_scope,
            Arc::from(self.canonical),
        );
        match self.dispatch.execute_type_node(
            crate::semantic_query::SemanticQueryKey::ProjectPath {
                base,
                path,
                context: crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Navigate,
                ),
            },
        ) {
            crate::semantic_query::QueryResult::Value(output) => Some(output.value),
            _ => None,
        }
    }

    /// `a` is assignable to `b`, through the crate's SOLE relation
    /// authority. `None` when the relation is UNDECIDED — the caller
    /// treats an undecided fact as "the narrow cannot establish", never
    /// as either answer.
    fn assignable(&self, source: SemanticNodeId, target: SemanticNodeId) -> Option<bool> {
        match self.dispatch.execute_relate_pair(source, target) {
            super::dispatch_txn::RelationStep::Assignable { .. } => Some(true),
            super::dispatch_txn::RelationStep::NotAssignable => Some(false),
            super::dispatch_txn::RelationStep::Unknown
            | super::dispatch_txn::RelationStep::BudgetExceeded(_)
            | super::dispatch_txn::RelationStep::Assumed(_) => None,
        }
    }

    /// Whether two nodes have a provably empty intersection. The authority is
    /// deliberately conservative: concrete primitive/literal tag conflicts,
    /// or two structural surfaces with the same required member carrying
    /// conflicting concrete tags. Different object key sets can overlap and
    /// therefore are never declared disjoint here.
    fn nodes_provably_disjoint(
        &self,
        left: SemanticNodeId,
        right: SemanticNodeId,
    ) -> NodeDisjointness {
        fn tag_disjoint(left: &SemanticNodeData, right: &SemanticNodeData) -> NodeDisjointness {
            fn literal_base(value: &crate::semantic_query::LiteralValue) -> PrimitiveKind {
                match value {
                    crate::semantic_query::LiteralValue::String(_) => PrimitiveKind::String,
                    crate::semantic_query::LiteralValue::Number(_) => PrimitiveKind::Number,
                    crate::semantic_query::LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
                    crate::semantic_query::LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
                }
            }
            fn concrete(kind: PrimitiveKind) -> bool {
                !matches!(
                    kind,
                    PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never
                )
            }
            let nominal_identity_missing = matches!(
                (left, right),
                (SemanticNodeData::Primitive(PrimitiveKind::Symbol), _)
                    | (_, SemanticNodeData::Primitive(PrimitiveKind::Symbol))
            );
            let provably_disjoint = match (left, right) {
                (SemanticNodeData::Primitive(a), SemanticNodeData::Primitive(b)) => {
                    let widening_pair = matches!(
                        (*a, *b),
                        (PrimitiveKind::Undefined, PrimitiveKind::Void)
                            | (PrimitiveKind::Void, PrimitiveKind::Undefined)
                    );
                    concrete(*a) && concrete(*b) && a != b && !widening_pair
                }
                (SemanticNodeData::Literal(a), SemanticNodeData::Literal(b)) => a != b,
                (SemanticNodeData::Literal(literal), SemanticNodeData::Primitive(primitive))
                | (SemanticNodeData::Primitive(primitive), SemanticNodeData::Literal(literal)) => {
                    concrete(*primitive) && literal_base(literal) != *primitive
                }
                _ => false,
            };
            NodeDisjointness {
                provably_disjoint,
                nominal_identity_missing,
            }
        }

        let graph = self.dispatch.graph();
        if let (Some(left_data), Some(right_data)) = (graph.node_data(left), graph.node_data(right))
        {
            let relation = tag_disjoint(&left_data, &right_data);
            if relation.provably_disjoint || relation.nominal_identity_missing {
                return relation;
            }
        }
        let context = crate::semantic_query::ProjectionReductionContext::structural_transit();
        let (Some(left_view), Some(right_view)) = (
            self.dispatch.resolve_typeinfo_surface_view(left, context),
            self.dispatch.resolve_typeinfo_surface_view(right, context),
        ) else {
            return NodeDisjointness::default();
        };
        let mut relation = NodeDisjointness::default();
        for left_member in left_view.positive_members() {
            if left_member.optional {
                continue;
            }
            let Some(key) = left_member.key.cloned_known() else {
                continue;
            };
            let crate::semantic_query::SurfaceKeyProjection::Exact(right_member) =
                right_view.project_known_key(&key)
            else {
                continue;
            };
            if right_member.optional {
                continue;
            }
            let member_relation = match (
                graph.node_data(left_member.value),
                graph.node_data(right_member.value),
            ) {
                (Some(left_data), Some(right_data)) => tag_disjoint(&left_data, &right_data),
                _ => NodeDisjointness::default(),
            };
            relation.nominal_identity_missing |= member_relation.nominal_identity_missing;
            if member_relation.provably_disjoint {
                relation.provably_disjoint = true;
                break;
            }
        }
        relation
    }

    /// Apply a guard's facts for one branch (`positive` = the branch the
    /// test's positive reading guards), pushing the established narrows
    /// onto the overlay. The CALLER owns the scope: it records the
    /// overlay length before and truncates after the arm — application
    /// never pops anything itself.
    fn apply_guard_scoped(
        &mut self,
        guard: &crate::flow_slice_content::SliceGuard,
        positive: bool,
    ) -> bool {
        use crate::flow_slice_content::SliceGuard;
        let fact = match guard {
            SliceGuard::None => return true,
            SliceGuard::Typeof {
                subject,
                kind,
                negated,
            } => self.narrow_typeof(subject, *kind, *negated == positive),
            SliceGuard::Truthy { subject, negated } => {
                self.narrow_truthy(subject, *negated == positive)
            }
            SliceGuard::EqLiteral {
                subject,
                literal,
                negated,
            } => self.narrow_eq_literal(subject, literal, *negated == positive),
            SliceGuard::Instanceof {
                subject,
                ctor,
                negated,
            } => self.narrow_instanceof(subject, ctor, *negated == positive),
            SliceGuard::In {
                key,
                subject,
                negated,
            } => self.narrow_in(key, subject, *negated == positive),
            SliceGuard::TypePredicate {
                subject,
                target,
                negated,
                call,
            } => {
                // A predicate call in a control test is NEVER decided
                // above the call — its result selects the narrowing. The
                // guard application here is the one place its result is
                // actually consumed, so this is where its call evidence
                // is recorded: a genuinely consumed fact deposits the
                // call span with the narrowing computation's decidedness
                // (an undecided relation leaves the relation obligation
                // unclaimed), and a fact the evaluator could not consume
                // at all (shadowed target, unmodelled subject) deposits
                // nothing — the demand stays unproven.
                let (fact, consumption) = self.narrow_to_predicate_target_consuming(
                    subject,
                    target,
                    *negated == positive,
                );
                match consumption {
                    PredicateNarrowConsumption::NotConsumed => {}
                    PredicateNarrowConsumption::Decided => {
                        self.call_evidence.push(FlowCallEvidence {
                            span: *call,
                            relations_decided: true,
                        });
                    }
                    PredicateNarrowConsumption::Undecided => {
                        self.call_evidence.push(FlowCallEvidence {
                            span: *call,
                            relations_decided: false,
                        });
                    }
                }
                fact
            }
            // A conjunction applies every fact at once; its NEGATION is
            // the disjunction of the negated facts (De Morgan — the same
            // symmetry the lowering's `!` uses).
            SliceGuard::And(parts) => {
                if positive {
                    let base = self.narrowings.len();
                    for part in parts.iter() {
                        if !self.apply_guard_scoped(part, true) {
                            self.narrowings.truncate(base);
                            return false;
                        }
                    }
                    return true;
                } else {
                    return self.apply_guard_union(parts, false);
                }
            }
            SliceGuard::Or(parts) => {
                if positive {
                    return self.apply_guard_union(parts, true);
                } else {
                    let base = self.narrowings.len();
                    for part in parts.iter() {
                        if !self.apply_guard_scoped(part, false) {
                            self.narrowings.truncate(base);
                            return false;
                        }
                    }
                    return true;
                }
            }
        };
        match fact {
            GuardNarrowing::Unchanged => true,
            GuardNarrowing::Narrowed(subject, node) => {
                self.narrowings.push((subject, node));
                true
            }
            GuardNarrowing::Impossible => false,
        }
    }

    /// The union-of-facts reading of a disjunction: each disjunct's
    /// narrow is computed against the SAME starting overlay, and a
    /// subject narrowed by every disjunct lands the union of the
    /// per-disjunct narrows. A subject some disjunct leaves unnarrowed
    /// contributes its ORIGINAL type to that union — which is the
    /// original type, so no narrow is established for it.
    fn apply_guard_union(
        &mut self,
        parts: &[crate::flow_slice_content::SliceGuard],
        positive: bool,
    ) -> bool {
        let mut alternatives: Vec<
            Vec<(
                crate::flow_slice_content::SliceNarrowSubject,
                SemanticNodeId,
            )>,
        > = Vec::with_capacity(parts.len());
        for part in parts.iter() {
            let base = self.narrowings.len();
            let possible = self.apply_guard_scoped(part, positive);
            let applied = self.narrowings.split_off(base);
            if !possible {
                continue;
            }
            let mut final_overlay = Vec::with_capacity(applied.len());
            for (subject, node) in applied {
                match final_overlay
                    .iter_mut()
                    .find(|(candidate, _)| *candidate == subject)
                {
                    Some((_, current)) => *current = node,
                    None => final_overlay.push((subject, node)),
                }
            }
            alternatives.push(final_overlay);
        }
        if alternatives.is_empty() {
            return false;
        }
        let mut subjects: Vec<crate::flow_slice_content::SliceNarrowSubject> = Vec::new();
        for alternative in &alternatives {
            for (subject, _) in alternative {
                if !subjects.contains(subject) {
                    subjects.push(subject.clone());
                }
            }
        }
        for subject in subjects {
            let narrowed_in_all = alternatives.iter().all(|alternative| {
                alternative
                    .iter()
                    .any(|(candidate, _)| *candidate == subject)
            });
            if !narrowed_in_all {
                continue;
            }
            let nodes: Vec<SemanticNodeId> = alternatives
                .iter()
                .flat_map(|alternative| {
                    alternative
                        .iter()
                        .filter(|(candidate, _)| *candidate == subject)
                        .map(|(_, node)| *node)
                })
                .collect();
            let node = self
                .dispatch
                .intern_normalized_union_or_intersection(&nodes, true);
            self.narrowings.push((subject, node));
        }
        true
    }

    /// Filter `subject`'s arms by a per-arm predicate, joining the
    /// survivors back into the narrow's node. An empty survivor set is an
    /// impossible branch, distinct from an unchanged/undecidable fact, so a
    /// disjunction can omit the dead alternative without retaining an
    /// intermediate conjunction overlay.
    fn narrow_arms_by(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        entry_subject: &crate::flow_slice_content::SliceNarrowSubject,
        mut keep: impl FnMut(&mut Self, SemanticNodeId) -> Option<bool>,
    ) -> GuardNarrowing {
        let Some(current) = self.subject_current_node(subject) else {
            return GuardNarrowing::Unchanged;
        };
        let arms = self.union_arms_or_self(current);
        let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        for arm in &arms {
            match keep(self, *arm) {
                Some(true) => survivors.push(*arm),
                Some(false) => {}
                None => return GuardNarrowing::Unchanged,
            }
        }
        if survivors.is_empty() {
            return GuardNarrowing::Impossible;
        }
        if survivors.len() == arms.len() {
            return GuardNarrowing::Unchanged;
        }
        let node = self
            .dispatch
            .intern_normalized_union_or_intersection(&survivors, true);
        GuardNarrowing::Narrowed(entry_subject.clone(), node)
    }

    /// `typeof subject === "kind"`: keep the arms whose runtime type the
    /// comparison names; negation drops them. An arm the graph cannot
    /// classify stays on BOTH edges and degrades the result — the
    /// checker narrows `unknown` / `any` under a `typeof` test, so
    /// dropping such an arm would fabricate a dead branch and silently
    /// lose that branch's return contributor.
    fn narrow_typeof(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        kind: crate::flow_slice_content::SliceTypeofKind,
        negated: bool,
    ) -> GuardNarrowing {
        let mut unclassified = false;
        let fact = self.narrow_arms_by(subject, subject, |this, arm| {
            Some(match this.arm_typeof_class(arm, kind) {
                ArmGuardClass::Match => !negated,
                ArmGuardClass::NoMatch => negated,
                ArmGuardClass::Unclassified => {
                    unclassified = true;
                    true
                }
            })
        });
        if unclassified {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        fact
    }

    /// One union arm's verdict against the runtime type a `typeof`
    /// comparison names. A primitive is its own kind (`null` is the
    /// operator's `"object"` quirk); a literal its primitive's; objects,
    /// arrays, tuples and spread programs are `"object"`, signatures
    /// `"function"`; `never` is uninhabited, off both edges. Anything
    /// the graph cannot place under exactly one runtime kind — `any`,
    /// `unknown`, a memberless `{}` surface (primitives inhabit it), an
    /// unresolved carrier — is `Unclassified`: `NoMatch` means PROVED
    /// non-inhabitance of the tested edge, never "unrecognized".
    fn arm_typeof_class(
        &self,
        arm: SemanticNodeId,
        kind: crate::flow_slice_content::SliceTypeofKind,
    ) -> ArmGuardClass {
        use crate::flow_slice_content::SliceTypeofKind;
        let classified = match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(primitive)) => match primitive {
                PrimitiveKind::String => Some(SliceTypeofKind::String),
                PrimitiveKind::Number => Some(SliceTypeofKind::Number),
                PrimitiveKind::BigInt => Some(SliceTypeofKind::BigInt),
                PrimitiveKind::Boolean => Some(SliceTypeofKind::Boolean),
                PrimitiveKind::Symbol => Some(SliceTypeofKind::Symbol),
                PrimitiveKind::Undefined => Some(SliceTypeofKind::Undefined),
                PrimitiveKind::Null | PrimitiveKind::Object => Some(SliceTypeofKind::Object),
                PrimitiveKind::Never => return ArmGuardClass::NoMatch,
                _ => None,
            },
            Some(SemanticNodeData::Literal(literal)) => match literal {
                crate::semantic_query::LiteralValue::String(_) => Some(SliceTypeofKind::String),
                crate::semantic_query::LiteralValue::Number(_) => Some(SliceTypeofKind::Number),
                crate::semantic_query::LiteralValue::BigInt(_) => Some(SliceTypeofKind::BigInt),
                crate::semantic_query::LiteralValue::Boolean(_) => Some(SliceTypeofKind::Boolean),
            },
            Some(SemanticNodeData::Object(surface)) => {
                if surface.closed().is_empty() {
                    None
                } else {
                    Some(SliceTypeofKind::Object)
                }
            }
            Some(SemanticNodeData::ObjectSpreadProgram(_))
            | Some(SemanticNodeData::Array { .. })
            | Some(SemanticNodeData::Tuple { .. }) => Some(SliceTypeofKind::Object),
            Some(SemanticNodeData::Signature { .. }) => Some(SliceTypeofKind::Function),
            _ => None,
        };
        match classified {
            Some(observed) if observed == kind => ArmGuardClass::Match,
            Some(_) => ArmGuardClass::NoMatch,
            None => ArmGuardClass::Unclassified,
        }
    }

    /// A bare truthiness test keeps every arm that CAN take the requested
    /// edge. Broad primitives such as `boolean`, `string`, and `number` can
    /// be either truthy or falsy; treating "not definitely falsy" as
    /// "definitely truthy" would incorrectly make their negative edge dead.
    fn narrow_truthy(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        negated: bool,
    ) -> GuardNarrowing {
        self.narrow_arms_by(subject, subject, |this, arm| {
            Some(if negated {
                this.arm_can_be_falsy(arm)
            } else {
                this.arm_can_be_truthy(arm)
            })
        })
    }

    /// Whether one union arm has a truthy runtime inhabitant.
    fn arm_can_be_truthy(&self, arm: SemanticNodeId) -> bool {
        match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Undefined | PrimitiveKind::Null | PrimitiveKind::Void,
            )) => false,
            Some(SemanticNodeData::Primitive(PrimitiveKind::Never)) => false,
            Some(SemanticNodeData::Literal(literal)) => match literal {
                crate::semantic_query::LiteralValue::Boolean(value) => *value,
                crate::semantic_query::LiteralValue::String(value) => !value.is_empty(),
                crate::semantic_query::LiteralValue::Number(value) => *value != 0.0,
                crate::semantic_query::LiteralValue::BigInt(value) => {
                    !value.trim_start_matches('-').chars().all(|c| c == '0')
                }
            },
            _ => true,
        }
    }

    /// Whether one union arm has a falsy runtime inhabitant. Only broad
    /// scalar primitives and the concrete falsy values qualify; object and
    /// callable surfaces are always truthy, while unresolved forms stay
    /// conservative and keep both edges.
    fn arm_can_be_falsy(&self, arm: SemanticNodeId) -> bool {
        match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(primitive)) => matches!(
                primitive,
                PrimitiveKind::Any
                    | PrimitiveKind::Unknown
                    | PrimitiveKind::Undefined
                    | PrimitiveKind::Null
                    | PrimitiveKind::Void
                    | PrimitiveKind::String
                    | PrimitiveKind::Number
                    | PrimitiveKind::BigInt
                    | PrimitiveKind::Boolean
            ),
            Some(SemanticNodeData::Literal(literal)) => match literal {
                crate::semantic_query::LiteralValue::Boolean(value) => !value,
                crate::semantic_query::LiteralValue::String(value) => value.is_empty(),
                crate::semantic_query::LiteralValue::Number(value) => *value == 0.0,
                crate::semantic_query::LiteralValue::BigInt(value) => {
                    value.trim_start_matches('-').chars().all(|c| c == '0')
                }
            },
            Some(
                SemanticNodeData::Object(_)
                | SemanticNodeData::ObjectSpreadProgram(_)
                | SemanticNodeData::Array { .. }
                | SemanticNodeData::Tuple { .. }
                | SemanticNodeData::Signature { .. },
            ) => false,
            Some(SemanticNodeData::Union(_)) | Some(SemanticNodeData::Intersection(_)) => true,
            Some(_) | None => true,
        }
    }

    /// `subject === literal`. An EMPTY subject path filters the binding's
    /// own arms by overlap with the literal (either assignability
    /// direction — the two spellings of "the same literal"); when no arm
    /// filters (the subject's whole type is a BROAD arm the literal only
    /// narrows, `x === "a"` over `x: string`) the positive reading narrows
    /// the subject to the literal itself — the checker's own rule for a
    /// literal strictly narrower than the declared type. A non-empty path
    /// is a DISCRIMINANT, filtering the ROOT's arms by whether the literal
    /// is assignable to the arm's member type at the path.
    fn narrow_eq_literal(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        literal: &crate::flow_slice_content::SliceGuardLiteral,
        negated: bool,
    ) -> GuardNarrowing {
        let Some(literal_ty) = guard_literal_type_expr(literal) else {
            return GuardNarrowing::Unchanged;
        };
        let literal_node = self.lower_body_type(&literal_ty);
        if subject.path.is_empty() {
            let narrowed = self.narrow_arms_by(subject, subject, |this, arm| {
                if matches!(
                    this.dispatch.graph().node_data(arm).as_deref(),
                    Some(SemanticNodeData::Primitive(
                        PrimitiveKind::Any | PrimitiveKind::Unknown
                    ))
                ) {
                    return None;
                }
                let forward = this.assignable(arm, literal_node)?;
                let backward = this.assignable(literal_node, arm)?;
                // The positive edge keeps overlapping arms. The negative
                // edge drops only an arm wholly covered by the literal: a
                // broad `string` can exclude `"a"` and remain `string`, so
                // the unrepresentable exclusion leaves that arm unchanged.
                Some(if negated {
                    !forward
                } else {
                    forward || backward
                })
            });
            if matches!(narrowed, GuardNarrowing::Unchanged) && !negated {
                // No arm was filtered. The literal can still be a STRICT
                // subtype of the subject's whole type — then the literal
                // IS the narrow (`x: string` guarded by `=== "a"` reads
                // `"a"` on the positive edge). A mutually-assignable
                // subject (`any`) establishes nothing.
                let Some(current) = self.subject_current_node(subject) else {
                    return GuardNarrowing::Unchanged;
                };
                if self.assignable(literal_node, current) == Some(true)
                    && self.assignable(current, literal_node) != Some(true)
                {
                    return GuardNarrowing::Narrowed(subject.clone(), literal_node);
                }
            }
            narrowed
        } else {
            // The discriminant narrows the ROOT, so the fact lands at the
            // root subject: a later read of ANY member of the binding
            // resolves against the surviving arms.
            let root_subject = crate::flow_slice_content::SliceNarrowSubject {
                root: subject.root.clone(),
                path: Arc::from(Vec::new().into_boxed_slice()),
            };
            let path = subject.path.clone();
            self.narrow_arms_by(&root_subject, &root_subject, |this, arm| {
                let member = this.project_segments_navigate(arm, &path)?;
                if negated {
                    // Excluding one literal removes a root arm only when
                    // the projected member is wholly that literal. A named
                    // alias can project a broad discriminant union without
                    // exposing its root constituents; `"a"` fits
                    // `"a" | "b"`, but its negative edge remains possible.
                    Some(!this.assignable(member, literal_node)?)
                } else {
                    Some(this.assignable(literal_node, member)?)
                }
            })
        }
    }

    /// Bake a narrow verdict into a state SNAPSHOT's reaching-definition
    /// layer, so the binding reads the narrowed node on that edge. Used
    /// where two differently-narrowed edges JOIN: the overlay intersection
    /// would erase both facts (they differ), while the
    /// reaching-definition join unions the two narrowed values — the
    /// checker's own rule for a fall-through-joined switch case start.
    fn bake_narrow_into_state(
        state: &mut FlowLayerState,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        node: SemanticNodeId,
    ) {
        match &subject.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                state.param_writes.insert(*ordinal, node);
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                if let Some(slot) = state.locals.get_mut(name.as_ref()) {
                    *slot = node;
                } else if let Some(slot) = state.var_locals.get_mut(name.as_ref()) {
                    *slot = node;
                }
            }
        }
    }

    /// The switch discriminant's arms that NO case test covers: the
    /// remainder the default clause's dispatch edge narrows to, and —
    /// when empty with no default authored — the proof the
    /// no-matching-case path is dead (the ONE exhaustiveness verdict).
    /// `None` when anything is undecidable (a projection miss, an
    /// undecided relation): the caller then narrows nothing and keeps the
    /// no-match path live.
    ///
    /// "Covers" is MUTUAL assignability between the arm and the test
    /// literal (projected through the discriminant path for a member
    /// discriminant): a broad arm a literal merely fits (`string` under
    /// `case "a":`) is NOT covered — the checker's default edge keeps it.
    /// Returns the surviving arms and the arm count, so the caller can
    /// tell "no narrow established" (survivors == arms) from a real one.
    fn switch_discriminant_remainder(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        tests: &[crate::flow_slice_content::SliceGuardLiteral],
    ) -> Option<(Vec<SemanticNodeId>, usize)> {
        // The narrow lands at the ROOT (a member-path discriminant
        // narrows the binding itself), so the arms are the root's. The
        // probe reads the live layers WITHOUT folding a membership flag:
        // asking about coverage is not an observation of the binding's
        // value, so it must not degrade one.
        let root_subject = crate::flow_slice_content::SliceNarrowSubject {
            root: subject.root.clone(),
            path: Arc::from(Vec::new().into_boxed_slice()),
        };
        let root = if let Some(node) = self.narrowed_read(&root_subject) {
            node
        } else {
            match &subject.root {
                crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => self
                    .param_writes
                    .get(ordinal)
                    .copied()
                    .or_else(|| self.params.get(*ordinal as usize).copied())?,
                crate::flow_slice_content::SliceNarrowRoot::Local(name) => self
                    .locals
                    .get(name.as_ref())
                    .or_else(|| self.var_locals.get(name.as_ref()))
                    .copied()?,
            }
        };
        let arms = self.union_arms_or_self(root);
        // `boolean` decomposes into its two literal arms for coverage —
        // the checker's own reading of `case true:` / `case false:` over
        // a boolean discriminant.
        let arms: Vec<SemanticNodeId> = arms
            .into_iter()
            .flat_map(|arm| {
                if matches!(
                    self.dispatch.graph().node_data(arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Boolean))
                ) {
                    let graph = self.dispatch.graph();
                    vec![
                        graph.intern_node(SemanticNodeData::Literal(
                            crate::semantic_query::LiteralValue::Boolean(true),
                        )),
                        graph.intern_node(SemanticNodeData::Literal(
                            crate::semantic_query::LiteralValue::Boolean(false),
                        )),
                    ]
                } else {
                    vec![arm]
                }
            })
            .collect();
        // A test that cannot lower makes coverage undecidable — dropping
        // it could manufacture an empty remainder (a false exhaustiveness
        // verdict), so the whole probe declines.
        let mut test_nodes: Vec<SemanticNodeId> = Vec::with_capacity(tests.len());
        for test in tests {
            let ty = guard_literal_type_expr(test)?;
            test_nodes.push(self.lower_body_type(&ty));
        }
        let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        'arms: for arm in &arms {
            for test in &test_nodes {
                let covered = if subject.path.is_empty() {
                    self.assignable(*arm, *test)? && self.assignable(*test, *arm)?
                } else {
                    let member = self.project_segments_navigate(*arm, &subject.path)?;
                    self.assignable(*test, member)? && self.assignable(member, *test)?
                };
                if covered {
                    continue 'arms;
                }
            }
            survivors.push(*arm);
        }
        let total = arms.len();
        Some((survivors, total))
    }

    /// `subject instanceof Ctor`: keep the arms assignable to the
    /// constructor's instance type (resolved as a bare type reference in
    /// owner scope — the same lowering any authored annotation of that
    /// name takes). The lowering mints this fact only for a constructor
    /// name it proved to be the module's single same-file `class`
    /// declaration left free by the frame, which is exactly when that
    /// type reference IS the compared value's instance type; every other
    /// right-hand side reaches the evaluator as a typed gap, never as a
    /// fact over the wrong binding.
    fn narrow_instanceof(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        ctor: &Arc<str>,
        negated: bool,
    ) -> GuardNarrowing {
        let ctor_ty = verter_type_expr::TypeExpr::Ref {
            name: Arc::clone(ctor),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        };
        let Some(instance) = self.dispatch.lower_type_expr_in_owner_scope_with_context(
            self.canonical,
            self.owner,
            &ctor_ty,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        ) else {
            return GuardNarrowing::Unchanged;
        };
        let mut unclassified = false;
        let fact = self.narrow_arms_by(subject, subject, |this, arm| {
            Some(match this.arm_instanceof_class(arm, instance) {
                ArmGuardClass::Match => !negated,
                ArmGuardClass::NoMatch => negated,
                ArmGuardClass::Unclassified => {
                    unclassified = true;
                    true
                }
            })
        });
        if unclassified {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        fact
    }

    /// One union arm's verdict against a constructor's instance type.
    /// Assignability decides only CLASSIFIED arms: for a top-shaped arm
    /// (`any`, `unknown`, a memberless `{}` surface), an Opaque carrier,
    /// or an undecided relation, "not assignable" does not prove the arm
    /// cannot inhabit the `instanceof` edge — the checker narrows those
    /// arms to the instance type instead of killing the branch.
    fn arm_instanceof_class(&self, arm: SemanticNodeId, instance: SemanticNodeId) -> ArmGuardClass {
        match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown))
            | Some(SemanticNodeData::Opaque(_))
            | None => return ArmGuardClass::Unclassified,
            Some(SemanticNodeData::Object(surface)) if surface.closed().is_empty() => {
                return ArmGuardClass::Unclassified;
            }
            _ => {}
        }
        match self.assignable(arm, instance) {
            Some(true) => ArmGuardClass::Match,
            Some(false) => ArmGuardClass::NoMatch,
            None => ArmGuardClass::Unclassified,
        }
    }

    /// `"key" in subject`: keep the arms proved to carry the member;
    /// negation keeps the ones proved not to. Proof is per edge and
    /// per arm ([`InArmPresence`]): a closed surface's REQUIRED member
    /// proves the arm off the negated edge, a closed surface with the
    /// key ABSENT proves it off the positive edge, an OPTIONAL member
    /// proves neither drop — its arm is retained exactly on the
    /// negated edge (a value may lack the key) and retained as a
    /// degraded superset on the positive edge (the checker refines the
    /// key present there). An arm whose key set the graph cannot
    /// decide — a type parameter, an index-signature surface, an
    /// unresolvable carrier — stays possible on BOTH edges and records
    /// the typed guard gap: the checker narrows such an arm, so
    /// dropping it would fabricate a dead edge and silently lose that
    /// edge's return contributor.
    fn narrow_in(
        &mut self,
        key: &Arc<str>,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        negated: bool,
    ) -> GuardNarrowing {
        let mut gapped = false;
        let fact = self.narrow_arms_by(subject, subject, |this, arm| {
            Some(match this.arm_in_presence(arm, key) {
                InArmPresence::Always => !negated,
                InArmPresence::Never => negated,
                InArmPresence::Optional => {
                    if !negated {
                        gapped = true;
                    }
                    true
                }
                InArmPresence::Unknown => {
                    gapped = true;
                    true
                }
            })
        });
        if gapped {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        fact
    }

    /// One union arm's key-presence verdict for `"key" in subject`,
    /// decided from the arm's OWN closed surface: identity carriers
    /// unwrap through the shared instantiate dispatch, then only a
    /// closed object surface (no index signature) answers — a required
    /// member is `Always`, a proven-absent key `Never`, an optional
    /// member `Optional`. Every other shape — a type parameter, an
    /// open index-signature surface, a primitive, an intersection, an
    /// unresolvable carrier — is `Unknown`: nothing about the runtime
    /// key set is proved, so neither edge of the test may drop the arm.
    fn arm_in_presence(&mut self, arm: SemanticNodeId, key: &str) -> InArmPresence {
        let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(arm) {
            super::relation::IdentityCarrierUnwrap::Concrete(node) => node,
            super::relation::IdentityCarrierUnwrap::Unresolvable => return InArmPresence::Unknown,
        };
        match self.dispatch.graph().node_data(concrete).as_deref() {
            Some(SemanticNodeData::Object(surface)) => {
                if surface.closed().has_index_signature() || !surface.index_signatures.is_empty() {
                    return InArmPresence::Unknown;
                }
                match surface.project_string_key(key) {
                    crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
                        if member.optional {
                            InArmPresence::Optional
                        } else {
                            InArmPresence::Always
                        }
                    }
                    crate::semantic_query::SurfaceKeyProjection::AbsentProven => {
                        InArmPresence::Never
                    }
                }
            }
            _ => InArmPresence::Unknown,
        }
    }

    /// A user-defined predicate's narrow (`x is T` or `asserts x is T`):
    /// keep the subject's arms assignable to the predicate's target type.
    /// When NO arm survives but the target is itself assignable to the
    /// subject's type, the target IS the narrow (the checker's own rule
    /// for a predicate whose target is a strict subtype of the declared
    /// type). The target lowers like a declarator annotation — a
    /// frame-shadowed answer establishes nothing.
    ///
    /// The assertion-statement caller: the guard twin consumes the
    /// consumption verdict too and records the predicate call's
    /// evidence from it.
    fn narrow_to_predicate_target(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        target: &crate::flow_slice_content::GatedType,
        negated: bool,
    ) -> GuardNarrowing {
        self.narrow_to_predicate_target_consuming(subject, target, negated)
            .0
    }

    /// [`Self::narrow_to_predicate_target`] carrying the CONSUMPTION
    /// verdict — whether the evaluator genuinely consumed the predicate
    /// fact, and whether every relation outcome that consumption asked
    /// was decided. This is what the guard twin's call evidence is
    /// recorded from: a fact the evaluator could not consume at all (a
    /// frame-shadowed target, an unmodelled subject) is
    /// [`PredicateNarrowConsumption::NotConsumed`] — no evidence; a
    /// consumed fact whose relation oracle answered `None` anywhere is
    /// [`PredicateNarrowConsumption::Undecided`] — evidence with the
    /// relation obligation left unclaimed.
    fn narrow_to_predicate_target_consuming(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        target: &crate::flow_slice_content::GatedType,
        negated: bool,
    ) -> (GuardNarrowing, PredicateNarrowConsumption) {
        use PredicateNarrowConsumption as Consumption;
        if target
            .shadowed()
            .iter()
            .any(|name| self.owner_scope_answers_name(name))
        {
            return (GuardNarrowing::Unchanged, Consumption::NotConsumed);
        }
        let target_node = self.lower_body_type(target.ty());
        if self.subject_current_node(subject).is_none() {
            return (GuardNarrowing::Unchanged, Consumption::NotConsumed);
        }
        if !negated {
            let current = self
                .subject_current_node(subject)
                .expect("the subject answered just above");
            let arms = self.union_arms_or_self(current);
            let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
            for arm in &arms {
                match self.assignable(*arm, target_node) {
                    Some(true) => survivors.push(*arm),
                    Some(false) => {}
                    None => return (GuardNarrowing::Unchanged, Consumption::Undecided),
                }
            }
            if !survivors.is_empty() {
                if survivors.len() == arms.len() {
                    return (GuardNarrowing::Unchanged, Consumption::Decided);
                }
                let node = self
                    .dispatch
                    .intern_normalized_union_or_intersection(&survivors, true);
                return (
                    GuardNarrowing::Narrowed(subject.clone(), node),
                    Consumption::Decided,
                );
            }
            let reverse = self.assignable(target_node, current);
            if reverse == Some(true) {
                return (
                    GuardNarrowing::Narrowed(subject.clone(), target_node),
                    Consumption::Decided,
                );
            }
            let relation = self.nodes_provably_disjoint(current, target_node);
            if relation.nominal_identity_missing {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    crate::semantic_query::FlowGap::NominalRelation,
                ));
            }
            // An undecided REVERSE relation leaves the choice between the
            // target narrow and the intersection fallback unproven: the
            // value keeps the checker's intersection rule, the relation
            // obligation stays unclaimed.
            let consumption = if reverse.is_none() {
                Consumption::Undecided
            } else {
                Consumption::Decided
            };
            if !relation.provably_disjoint {
                let intersection = self
                    .dispatch
                    .intern_normalized_union_or_intersection(&[current, target_node], false);
                return (
                    GuardNarrowing::Narrowed(subject.clone(), intersection),
                    consumption,
                );
            }
            return (GuardNarrowing::Impossible, consumption);
        }
        let mut undecided = false;
        let fact = self.narrow_arms_by(subject, subject, |this, arm| {
            match this.assignable(arm, target_node) {
                Some(kept) => Some(kept != negated),
                None => {
                    undecided = true;
                    None
                }
            }
        });
        let consumption = if undecided {
            Consumption::Undecided
        } else {
            Consumption::Decided
        };
        (fact, consumption)
    }

    /// Whether one local carries a WIDENING literal, in the layer that
    /// currently answers reads of `name`. A pure predicate — it folds no
    /// degradation, because asking about widening is not an observation
    /// of the binding's value.
    fn widening_of(&self, name: &str) -> bool {
        if self.locals.contains_key(name) {
            return self.widening_locals.contains(name);
        }
        self.var_locals.contains_key(name) && self.var_widening_locals.contains(name)
    }

    /// Evaluate ONE return site under the member-projection demand: the
    /// argument must be a structural object literal carrying the
    /// demanded member statically — ONLY that member's value evaluates
    /// (with the same member-position widening the whole-return object
    /// path applies); sibling entries never evaluate. Any other return
    /// shape — a bare return, a non-object value, a missing member — is
    /// beyond the modeled member point: fail closed with the typed
    /// `UnmodeledDemandPoint`, never a silently widened whole-return
    /// evaluation and never a fabricated `undefined` member.
    fn eval_member_projected_return(
        &mut self,
        argument: Option<&crate::flow_slice_content::SliceExpr>,
    ) -> Result<Option<SemanticNodeId>, FlowReturnFailure> {
        let member_name = match self.member_filter.as_ref() {
            Some(filter) => Arc::clone(&filter.member),
            None => return Err(FlowReturnFailure::UnmodeledDemandPoint),
        };
        let Some(crate::flow_slice_content::SliceExpr::Object { entries }) = argument else {
            return Err(FlowReturnFailure::UnmodeledDemandPoint);
        };
        // Last write wins for duplicate keys (JS object-literal
        // semantics): take the LAST entry provisioning the demanded key.
        // A SPREAD provisions an unknown key set, so the last one is a
        // hard stop — anything before it may have been overridden and the
        // demanded key may originate in it. That is beyond the modeled
        // member point: fail closed rather than answer from a member the
        // spread might replace.
        let mut member = None;
        for entry in entries.iter().rev() {
            match entry {
                crate::flow_slice_content::SliceObjectEntry::Spread { .. } => break,
                crate::flow_slice_content::SliceObjectEntry::Member(candidate) => {
                    // A COMPUTED key may name the demanded member and may
                    // not, and deciding that needs its value. A later
                    // entry that MIGHT provision the key is the same hard
                    // stop a later spread is: anything before it may have
                    // been overridden.
                    let Some(name) = candidate.key.static_name() else {
                        break;
                    };
                    if name == member_name.as_ref() {
                        member = Some(candidate);
                        break;
                    }
                }
            }
        }
        let Some(member) = member else {
            return Err(FlowReturnFailure::UnmodeledDemandPoint);
        };
        let outcome = self.eval_expr(&member.value);
        match self.settle(outcome) {
            Some(node) => Ok(Some(self.widen_if_widening_local_read(&member.value, node))),
            // A hold inside the demanded member is the same coinductive
            // hold the whole-return object path reports.
            None => Ok(None),
        }
    }

    /// Whether `expr` is a read of a WIDENING-literal local (`const b =
    /// 1` — unannotated, no const assertion). `as const` / annotated
    /// literals, parameters, and non-local reads are never widening.
    fn reads_widening_literal_local(&self, expr: &crate::flow_slice_content::SliceExpr) -> bool {
        let crate::flow_slice_content::SliceExpr::Local { name, .. } = expr else {
            return false;
        };
        self.widening_of(name.as_ref())
    }

    /// Widen `node` to its literal's primitive when `expr` is a read of a
    /// WIDENING-literal local and the evaluated node IS that literal.
    /// Every other shape passes through unchanged.
    fn widen_if_widening_local_read(
        &self,
        expr: &crate::flow_slice_content::SliceExpr,
        node: SemanticNodeId,
    ) -> SemanticNodeId {
        if !self.reads_widening_literal_local(expr) {
            return node;
        }
        widen_literal_node(self.dispatch, node)
    }

    /// Lower one body-position `TypeExpr` (a fully lowered expression
    /// leaf or a declarator's authored annotation) under the function's
    /// OWN binder environment — a body type referencing a root binder
    /// keeps the binder, never an outer same-name resolution.
    /// Lower an AUTHORED computed-key type CARRIER-PRESERVINGLY.
    ///
    /// A computed key's nominal identity lives in the carrier
    /// (`typeof ob12Key`), and the structural-transit lowering
    /// [`Self::lower_body_type`] uses reduces it to the bare `symbol`
    /// primitive — which names no property. `Navigate` keeps the carrier
    /// for the downstream key reader to resolve, which is exactly what
    /// the whole-literal leaf answer handed it.
    fn lower_key_type(&self, ty: &verter_type_expr::TypeExpr) -> SemanticNodeId {
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        self.dispatch.shallow_lower_type_expr_with_context(
            ty,
            &self.binder_env.env,
            &self.binder_env.scope,
            &self.binder_env.name_resolution,
            self.binder_env.scope_payload.as_ref(),
            &self.binder_env.shadowing,
            &mut substitutions,
            crate::semantic_query::ProjectionReductionContext::structural_transit_with_mode(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        )
    }

    fn lower_body_type(&self, ty: &verter_type_expr::TypeExpr) -> SemanticNodeId {
        let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
        self.dispatch.shallow_lower_type_expr_with_context(
            ty,
            &self.binder_env.env,
            &self.binder_env.scope,
            &self.binder_env.name_resolution,
            self.binder_env.scope_payload.as_ref(),
            &self.binder_env.shadowing,
            &mut substitutions,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
    }

    /// Whether the file OWNER SCOPE answers `name` in the name space it
    /// was referenced in — the root-identifier gate's owner-side probe.
    ///
    /// Asked through the ONE shared lowering the leaf itself would take
    /// (`typeof name` for a value reference, a bare `name` reference for
    /// a type one), so the gate's verdict is exactly "would the leaf bind
    /// something here". A typed MISS means the owner scope answers
    /// nothing, so nothing can be mis-bound.
    fn owner_scope_answers_name(
        &self,
        name: &crate::flow_slice_content::FrameShadowedName,
    ) -> bool {
        owner_scope_answers_frame_name(self.dispatch, self.binder_env, name)
    }

    /// Evaluate one region, returning its contributor nodes and whether
    /// the region falls through (mirrors the IR's reachability — this
    /// recomputes nothing, it only evaluates contributors).
    ///
    /// The recording shell of the structural walk ledger: region entry,
    /// completion, and every abortive `Err` exit are recorded HERE — at
    /// the walk itself, never reconstructed from the plan — so the
    /// execution witness can only claim a selection whose derived
    /// content this run actually walked to completion.
    fn eval_region(
        &mut self,
        region: &crate::flow_slice_content::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        self.executed_walk.regions_entered = self.executed_walk.regions_entered.saturating_add(1);
        let outcome = self.eval_region_statements(region);
        match &outcome.0 {
            Ok(_) => {
                self.executed_walk.regions_completed =
                    self.executed_walk.regions_completed.saturating_add(1);
            }
            Err(_) => self.executed_walk.aborted = true,
        }
        outcome
    }

    /// The statement walk of [`Self::eval_region`] (the recording shell
    /// wraps every entry and exit).
    fn eval_region_statements(
        &mut self,
        region: &crate::flow_slice_content::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        let mut contributors: Vec<FlowContribution> = Vec::new();
        // The evaluator's own path liveness, refined past the lowering's
        // reachability where only the resolver can see a path die (an
        // exhaustive switch's no-matching-case path). A terminal statement
        // ends the path: statements after it are unreachable and never
        // evaluate. The returned fall-through is the lowering's flag
        // ANDed with this — the override only ever narrows downward.
        let mut path_alive = true;
        for (statement_index, statement) in region.statements.iter().enumerate() {
            if !path_alive {
                break;
            }
            self.executed_walk.statements_executed =
                self.executed_walk.statements_executed.saturating_add(1);
            match statement {
                crate::flow_slice_content::SliceStatement::Gap(gap) => {
                    self.pending_statement_gap.get_or_insert(*gap);
                }
                crate::flow_slice_content::SliceStatement::Return {
                    argument,
                    widening_literal,
                } => {
                    path_alive = false;
                    if self.member_filter.is_some() {
                        // Member-projection demand: evaluate ONLY the
                        // demanded member of a structural object return.
                        match self.eval_member_projected_return(argument.as_ref()) {
                            Ok(Some(node)) => contributors.push(FlowContribution {
                                node,
                                fresh_literal: false,
                                inference_only: self.inference_only_path,
                            }),
                            Ok(None) => {}
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        }
                        self.capture_return_edge();
                        continue;
                    }
                    match argument {
                        Some(expr) => {
                            // A FRESH literal contribution is a bare literal
                            // return argument or a read of a
                            // widening-literal `const`. The join decides
                            // whether it widens: tsc widens only a lone
                            // contributor (`return 1` is `number`, but
                            // `if (c) return 1; return 0` is `0 | 1`).
                            let mut fresh_literal =
                                *widening_literal || self.reads_widening_literal_local(expr);
                            // A `return f(…)` whose callee pops as a
                            // PROVISIONAL member of this component is
                            // fresh-NEUTRAL: its value is re-derived by the
                            // equation fixed point, so the component's own
                            // freshness (not this arm) decides. Treating it
                            // as non-fresh would make widening depend on
                            // whether the callee was already in flight —
                            // i.e. on demand ORDER.
                            let holds_before = self.holds.len();
                            let outcome = self.eval_expr(expr);
                            if let Some(node) = self.settle(outcome) {
                                fresh_literal |= self.holds.len() > holds_before;
                                // A COMPLETED call that closed fresh feeds
                                // the same join: its executor-marked
                                // fresh-literal return is a fresh source
                                // here, exactly as a bare literal argument
                                // is.
                                fresh_literal |= self.call_fresh_literal_returns.contains(&node);
                                contributors.push(FlowContribution {
                                    node,
                                    fresh_literal,
                                    inference_only: self.inference_only_path,
                                });
                            }
                        }
                        None => {
                            // Bare `return;` — recorded, never a direct
                            // `undefined` contributor: a bare-only body
                            // joins to `void` (BL12).
                            self.bare_return_seen = true;
                        }
                    }
                    self.capture_return_edge();
                }
                crate::flow_slice_content::SliceStatement::If {
                    guard,
                    consequent,
                    alternate,
                } => {
                    // Bindings are block-scoped: each `if` arm evaluates
                    // under its own local scope, and the consequent reads
                    // the test's POSITIVE narrow, the alternate its
                    // NEGATED one. Both overlays are arm-scoped — a narrow
                    // never leaks out of the arm it was established in.
                    //
                    // A WHOLE-BINDING WRITE inside an arm does escape —
                    // through the branch JOIN, never the raw arm value:
                    // after the `if`, a rebound binding holds the union of
                    // its arm value and the value it had on the paths that
                    // never took that arm (tsc's own join of reaching
                    // definitions). An arm whose path TERMINATES (return /
                    // throw / break) does not reach the join at all: its
                    // writes leave with it, and the SURVIVING edge carries
                    // the other reading's guard facts — the negated
                    // reading when the consequent terminated, the positive
                    // one when the alternate did (the checker's own rule
                    // for `if (guard) exit; …`). The lexical layer
                    // restores; the function-scoped `var` layer (and
                    // parameter writes) join by the same rule.
                    let saved = self.locals.clone();
                    let saved_declared = self.declared_locals.clone();
                    let saved_degraded = self.degraded_locals.clone();
                    let saved_widening = self.widening_locals.clone();
                    let saved_var = self.var_locals.clone();
                    let saved_var_conditional = self.var_conditional_locals.clone();
                    let saved_param_writes = self.param_writes.clone();
                    let narrow_len = self.narrowings.len();
                    let shadow_base = self.scope_shadows.len();
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    self.conditional_arm_nesting += 1;
                    let consequent_possible = self.apply_guard_scoped(guard, true);
                    if !consequent_possible
                        && slice_region_has_non_subject_return(consequent, &|expr| {
                            slice_expr_is_exact_guard_subject_read(expr, guard)
                        })
                    {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                    let (consequent_result, consequent_falls) = if consequent_possible {
                        self.eval_region(consequent)
                    } else {
                        (Ok(Vec::new()), false)
                    };
                    self.narrowings.truncate(narrow_len);
                    // Close the arm's lexical scope BEFORE snapshotting its
                    // contribution to the post-if join, and replay the same
                    // close on every abrupt edge that crossed the arm.
                    let shadows = self.split_scope_shadows_close_exits(
                        shadow_base,
                        break_base,
                        return_base,
                        throw_base,
                    );
                    let mut consequent_state = self.layer_state();
                    Self::close_lexical_scope(&mut consequent_state, &shadows);
                    let consequent_locals = consequent_state.locals;
                    let consequent_var = consequent_state.var_locals;
                    let consequent_param_writes = consequent_state.param_writes;
                    self.locals = saved.clone();
                    self.declared_locals = saved_declared.clone();
                    self.degraded_locals = saved_degraded.clone();
                    self.widening_locals = saved_widening.clone();
                    self.var_locals = saved_var.clone();
                    self.param_writes = saved_param_writes.clone();
                    let consequent_contributors = match consequent_result {
                        Ok(contributors) => contributors,
                        Err(failure) => {
                            self.conditional_arm_nesting -= 1;
                            return (Err(failure), region.can_fall_through);
                        }
                    };
                    contributors.extend(consequent_contributors);
                    let mut implicit_alternate_falls = true;
                    let alternate_layers = if let Some(alternate) = alternate {
                        let shadow_base = self.scope_shadows.len();
                        let break_base = self.break_exits.len();
                        let return_base = self.return_edges.len();
                        let throw_base = self.throw_points.len();
                        let alternate_possible = self.apply_guard_scoped(guard, false);
                        if !alternate_possible
                            && slice_region_has_non_subject_return(alternate, &|expr| {
                                slice_expr_is_exact_guard_subject_read(expr, guard)
                            })
                        {
                            self.record_degradation(FlowReturnDegradation::FlowGap(
                                crate::semantic_query::FlowGap::GuardNarrowing,
                            ));
                        }
                        let (alternate_result, alternate_falls) = if alternate_possible {
                            self.eval_region(alternate)
                        } else {
                            (Ok(Vec::new()), false)
                        };
                        self.narrowings.truncate(narrow_len);
                        let shadows = self.split_scope_shadows_close_exits(
                            shadow_base,
                            break_base,
                            return_base,
                            throw_base,
                        );
                        let mut alternate_state = self.layer_state();
                        Self::close_lexical_scope(&mut alternate_state, &shadows);
                        let alternate_locals = alternate_state.locals;
                        let alternate_var = alternate_state.var_locals;
                        let alternate_param_writes = alternate_state.param_writes;
                        self.locals = saved.clone();
                        self.declared_locals = saved_declared.clone();
                        self.degraded_locals = saved_degraded.clone();
                        self.widening_locals = saved_widening.clone();
                        self.var_locals = saved_var.clone();
                        self.param_writes = saved_param_writes.clone();
                        let alternate_contributors = match alternate_result {
                            Ok(contributors) => contributors,
                            Err(failure) => {
                                self.conditional_arm_nesting -= 1;
                                return (Err(failure), region.can_fall_through);
                            }
                        };
                        contributors.extend(alternate_contributors);
                        Some((
                            alternate_locals,
                            alternate_var,
                            alternate_param_writes,
                            alternate_falls,
                        ))
                    } else {
                        implicit_alternate_falls = self.apply_guard_scoped(guard, false);
                        self.narrowings.truncate(narrow_len);
                        None
                    };
                    self.conditional_arm_nesting -= 1;
                    self.locals = saved;
                    self.declared_locals = saved_declared;
                    self.degraded_locals = saved_degraded;
                    self.widening_locals = saved_widening;
                    self.var_locals = saved_var;
                    self.param_writes = saved_param_writes;
                    let (alternate_locals, alternate_var, alternate_param_writes, alternate_falls) =
                        match &alternate_layers {
                            Some((locals, var, param_writes, falls)) => {
                                (Some(locals), Some(var), Some(param_writes), *falls)
                            }
                            // No `else`: the implicit alternate reaches past
                            // the `if` only when the guard's negated edge is
                            // possible under the current overlay.
                            None => (None, None, None, implicit_alternate_falls),
                        };
                    self.join_arm_writes(
                        &consequent_locals,
                        consequent_falls,
                        alternate_locals,
                        alternate_falls,
                        &consequent_var,
                        alternate_var,
                        &saved_var_conditional,
                        &consequent_param_writes,
                        alternate_param_writes,
                    );
                    // The surviving edge's facts. Exactly one arm
                    // terminating means every path past the `if` took the
                    // OTHER reading of the test — apply its facts to the
                    // rest of the region, exactly where an arm-scoped
                    // truncation does not erase them. (Both arms reaching
                    // establishes nothing; both terminating makes the rest
                    // of the region unreachable.)
                    let surviving_edge_possible = if !consequent_falls && alternate_falls {
                        self.apply_guard_scoped(guard, false)
                    } else if consequent_falls && !alternate_falls {
                        self.apply_guard_scoped(guard, true)
                    } else {
                        true
                    };
                    path_alive = (consequent_falls || alternate_falls) && surviving_edge_possible;
                    if !surviving_edge_possible
                        && slice_statements_have_non_subject_return(
                            region.statements.iter().skip(statement_index + 1),
                            &|expr| slice_expr_is_exact_guard_subject_read(expr, guard),
                        )
                    {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                }
                crate::flow_slice_content::SliceStatement::Switch {
                    discriminant,
                    cases,
                    has_default,
                } => {
                    // tsc's switch flow: a case clause is entered by the
                    // dispatch edge (the state at the switch) AND, for
                    // every clause after the first, by the previous
                    // clause's fall-through edge — so each clause starts
                    // from the JOIN of those two states. Each component
                    // carries its OWN reading of the discriminant, baked
                    // into the reaching-definition layer before the join:
                    // the dispatch edge into a clause tested positive for
                    // the clause's test (the default clause's edge is the
                    // discriminant minus every test), and the fall-through
                    // edge out of a clause carries that clause's narrow
                    // with it — so a fall-through-joined start unions the
                    // chain's tests, exactly the checker's flow. (The
                    // narrowing OVERLAY cannot carry this: the join
                    // intersects it, and the two edges' facts differ.)
                    // The state past the switch joins every path that
                    // leaves it normally: the state AT each `break`
                    // (never the end state of the clause the break sits
                    // in — a write after the break is not on the break's
                    // edge), falling off the last clause, and the
                    // no-matching-case path when no `default` exists AND
                    // the tests do not cover the discriminant's every
                    // arm. The clauses share ONE block scope, exactly as
                    // the authored switch body does.
                    let mut entry = self.layer_state();
                    self.complete_param_writes(&mut entry);
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    let shadow_base = self.scope_shadows.len();
                    // The remainder the DEFAULT edge subtracts is built
                    // from the CARRIED relations only. An unrecognized
                    // clause contributes nothing to it, which leaves the
                    // remainder a SUPERSET of the true default set — the
                    // sound direction — and never lets that clause's own
                    // values disappear from another clause's edge.
                    let tests: Vec<crate::flow_slice_content::SliceGuardLiteral> = cases
                        .iter()
                        .filter_map(|case| match &case.test {
                            crate::flow_slice_content::SliceSwitchTest::Literal(literal) => {
                                Some(literal.clone())
                            }
                            crate::flow_slice_content::SliceSwitchTest::Default
                            | crate::flow_slice_content::SliceSwitchTest::Unmodeled => None,
                        })
                        .collect();
                    // Exhaustiveness is a resolver question: the lowering
                    // knows only `has_default`, so the no-matching-case
                    // path dies here, where the discriminant's arms and
                    // the tests can be related.
                    let covered = !has_default
                        && discriminant.as_ref().is_some_and(|subject| {
                            matches!(
                                self.switch_discriminant_remainder(subject, &tests),
                                Some((remainder, _)) if remainder.is_empty()
                            )
                        });
                    let mut chain_end: Option<FlowLayerState> = None;
                    let mut last_end: Option<FlowLayerState> = None;
                    let mut last_falls = false;
                    for case in cases.iter() {
                        // The dispatch component of this clause's start.
                        let mut dispatch = entry.clone();
                        let mut dead_dispatch = false;
                        if let Some(subject) = discriminant {
                            self.restore_layer_state(entry.clone());
                            match &case.test {
                                // An unrecognized relation: the clause is
                                // reachable for discriminant values this
                                // half cannot enumerate, so its dispatch
                                // edge carries NO narrow. It must never
                                // take the DEFAULT edge — the remainder is
                                // not this clause's reaching set, and
                                // baking it in would publish a type the
                                // clause was never proven to see.
                                crate::flow_slice_content::SliceSwitchTest::Unmodeled => {}
                                // The dispatch edge: the discriminant IS
                                // this test.
                                crate::flow_slice_content::SliceSwitchTest::Literal(test) => {
                                    match self.narrow_eq_literal(subject, test, false) {
                                        GuardNarrowing::Narrowed(fact_subject, node) => {
                                            Self::bake_narrow_into_state(
                                                &mut dispatch,
                                                &fact_subject,
                                                node,
                                            );
                                        }
                                        GuardNarrowing::Impossible => {
                                            dead_dispatch = true;
                                        }
                                        GuardNarrowing::Unchanged => {}
                                    }
                                }
                                // The default clause's dispatch edge: the
                                // discriminant minus every carried test.
                                crate::flow_slice_content::SliceSwitchTest::Default => {
                                    if let Some((remainder, total)) =
                                        self.switch_discriminant_remainder(subject, &tests)
                                    {
                                        if remainder.is_empty() {
                                            // Every arm is covered: the
                                            // clause is DEAD on this edge
                                            // — it contributes nothing and
                                            // falls through nowhere.
                                            dead_dispatch = true;
                                        } else if remainder.len() < total {
                                            let node = self
                                                .dispatch
                                                .intern_normalized_union_or_intersection(
                                                    &remainder, true,
                                                );
                                            let root_subject =
                                                crate::flow_slice_content::SliceNarrowSubject {
                                                    root: subject.root.clone(),
                                                    path: Arc::from(Vec::new().into_boxed_slice()),
                                                };
                                            Self::bake_narrow_into_state(
                                                &mut dispatch,
                                                &root_subject,
                                                node,
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        let start = match (dead_dispatch, &chain_end) {
                            // The dispatch edge is dead, but a preceding
                            // clause may still fall through into this body.
                            // Exhaustiveness kills only the dispatch
                            // component, never that live chain edge.
                            (true, Some(end)) => end.clone(),
                            (true, None) => {
                                if let Some(subject) = discriminant {
                                    if slice_region_has_non_subject_return(&case.region, &|expr| {
                                        slice_expr_is_exact_subject_read(expr, subject)
                                    }) {
                                        self.record_degradation(FlowReturnDegradation::FlowGap(
                                            crate::semantic_query::FlowGap::GuardNarrowing,
                                        ));
                                    }
                                }
                                last_end = None;
                                last_falls = false;
                                continue;
                            }
                            (false, None) => dispatch,
                            (false, Some(end)) => {
                                let mut start = self.join_layer_states(&dispatch, end);
                                // A `var` the fall-through edge first
                                // defines has no reaching definition on the
                                // dispatch edge: flag it so a read fails
                                // closed instead of publishing the
                                // fall-through arm's value clean.
                                self.flag_fallthrough_only_vars(&mut start, &entry);
                                start
                            }
                        };
                        self.restore_layer_state(start);
                        let (case_result, _) = self.eval_region(&case.region);
                        match case_result {
                            Ok(case_contributors) => contributors.extend(case_contributors),
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        }
                        let end = self.layer_state();
                        // Only a clause whose path FALLS THROUGH passes its
                        // end state to the next clause's start: a `break` /
                        // `return` / `throw` exits the switch, and joining
                        // that state into the next case would publish the
                        // exited path's writes where the checker has the
                        // dispatch edge's values.
                        chain_end = case.region.can_fall_through.then_some(end.clone());
                        last_falls = case.region.can_fall_through;
                        last_end = Some(end);
                    }
                    let mut exit_states: Vec<FlowLayerState> = Vec::new();
                    if !has_default && !covered {
                        exit_states.push(entry.clone());
                    }
                    // Lexical bindings declared inside a clause are scoped
                    // to the switch body: the close replays on every state
                    // the clauses produced — each pending break exit
                    // included — BEFORE the join, so a shadowed or
                    // clause-declared binding cannot be unioned into the
                    // post-switch state, and the clauses' writes to
                    // bindings that PREDATE the switch survive.
                    let shadows = self.split_scope_shadows_close_exits(
                        shadow_base,
                        break_base,
                        return_base,
                        throw_base,
                    );
                    // The `break` exits, with the state each captured at
                    // its own point (scope-closed above, like every state
                    // that crossed the switch body's scope).
                    exit_states.extend(self.drain_break_exits(break_base, None));
                    if last_falls {
                        if let Some(mut end) = last_end {
                            Self::close_lexical_scope(&mut end, &shadows);
                            exit_states.push(end);
                        }
                    }
                    let reaches = !exit_states.is_empty();
                    let mut joined = match exit_states.split_first() {
                        Some((first, rest)) => {
                            let mut joined = first.clone();
                            for state in rest {
                                joined = self.join_layer_states(&joined, state);
                            }
                            joined
                        }
                        // No path leaves the switch normally: the
                        // post-switch state is unreachable; restore the
                        // entry to keep the layers sane.
                        None => entry.clone(),
                    };
                    if reaches {
                        self.flag_conditionally_defined_vars(&mut joined, &exit_states);
                    }
                    self.restore_layer_state(joined);
                    path_alive = reaches;
                }
                crate::flow_slice_content::SliceStatement::Try {
                    block,
                    catch,
                    finally,
                    pending_break_contributes_undefined,
                    pending_break_following_return_targets,
                } => {
                    // The catch / finally clauses are entered from ANY
                    // throw point of the try block, so they start from the
                    // JOIN of the try's ENTRY state with the state
                    // captured at each call / `throw` inside the block:
                    // the checker enters the catch from every one of
                    // those points, so a write between two throw points
                    // is exactly as visible to the clause as the checker
                    // has it. Every try-internal write is additionally
                    // flagged (an ELIDED call is a throw point this model
                    // never captures — the flag is the fail-closed net),
                    // and the overlay carries none of the try's narrow
                    // facts (tsgo: a `catch` / `finally` body reads the
                    // pre-try type, never the narrowed one). Past the
                    // statement, a clause-established narrow survives ONLY
                    // when no `catch` exists — the abrupt paths then leave
                    // the frame, so the normal-completion path's facts
                    // hold (tsgo narrows there) — minus any the finally
                    // clause's own writes killed. Return inference
                    // aggregates every authored return contribution,
                    // including a try return whose runtime completion is
                    // overridden by an abrupt finally.
                    let mut entry = self.layer_state();
                    self.complete_param_writes(&mut entry);
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    let mut own: Vec<FlowContribution> = Vec::new();
                    let mut exit_states: Vec<FlowLayerState> = Vec::new();
                    let (try_contributors, try_end, try_written) =
                        match self.eval_try_clause(&entry, block, None, true) {
                            Ok(clause) => clause,
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        };
                    own.extend(try_contributors);
                    let try_narrowings = try_end.narrowings.clone();
                    if block.can_fall_through {
                        exit_states.push(try_end);
                    }
                    // The try block's throw points. A catch consumes them
                    // into its entry join; with no catch the throw paths
                    // leave the frame (through the finally), so they stay
                    // on the stack for an OUTER try's catch to consume.
                    let block_throws: Vec<FlowLayerState> = if catch.is_some() {
                        self.throw_points.split_off(throw_base)
                    } else {
                        self.throw_points[throw_base..].to_vec()
                    };
                    let mut catch_written = None;
                    if let Some(catch) = catch {
                        let mut catch_start = entry.clone();
                        for state in &block_throws {
                            catch_start = self.join_layer_states(&catch_start, state);
                        }
                        self.flag_clause_writes(&mut catch_start, &try_written);
                        let (catch_contributors, catch_end, written) = match self.eval_try_clause(
                            &catch_start,
                            &catch.region,
                            catch.param.as_ref(),
                            finally.is_some(),
                        ) {
                            Ok(clause) => clause,
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        };
                        own.extend(catch_contributors);
                        catch_written = Some(written);
                        if catch.region.can_fall_through {
                            exit_states.push(catch_end);
                        }
                    }
                    // The pre-finally state joins every NORMAL completion
                    // of the try/catch. With none, no state reaches past
                    // the try — the entry stands in only to keep the
                    // finally clause's own evaluation well-formed. The
                    // clause writes stay flagged through it: the finally
                    // (and the post-statement path) runs on the throw paths
                    // too.
                    let mut pre_finally = match exit_states.split_first() {
                        Some((first, rest)) => {
                            let mut joined = first.clone();
                            for state in rest {
                                joined = self.join_layer_states(&joined, state);
                            }
                            joined
                        }
                        None => entry.clone(),
                    };
                    self.flag_clause_writes(&mut pre_finally, &try_written);
                    if let Some(catch_written) = &catch_written {
                        self.flag_clause_writes(&mut pre_finally, catch_written);
                    }
                    if !exit_states.is_empty() {
                        self.flag_conditionally_defined_vars(&mut pre_finally, &exit_states);
                    }
                    match finally {
                        Some(finally) => {
                            // The finally BODY runs on every completion:
                            // its start joins the normal completions with
                            // the try's ENTRY (a throw can precede every
                            // try-internal write — the checker reads the
                            // pre-try value inside the finally too), every
                            // throw point of the clauses, and every
                            // pending abrupt edge's pre-state (`break` and
                            // `return` both cross the finally before their
                            // completion proceeds). Its overlay is
                            // the ENTRY's, whatever the clauses
                            // established (tsgo: a narrow from the try
                            // does not apply inside the finally). The
                            // dual does NOT hold: the finally's own
                            // writes never merge into a pending abrupt
                            // edge's continuation — the edge keeps the
                            // value its point captured (tsgo, measured).
                            // And the state PAST the statement is not the
                            // finally body's wide start either: only the
                            // normal completions reach it, plus the
                            // finally's own writes.
                            let mut finally_start = self.join_layer_states(&pre_finally, &entry);
                            finally_start.narrowings = entry.narrowings.clone();
                            let clause_throws = self.throw_points[throw_base..].to_vec();
                            for state in block_throws.iter().chain(clause_throws.iter()) {
                                finally_start = self.join_layer_states(&finally_start, state);
                            }
                            let pending_exits: Vec<FlowLayerState> = self.break_exits[break_base..]
                                .iter()
                                .map(|exit| exit.state.clone())
                                .collect();
                            for state in &pending_exits {
                                finally_start = self.join_layer_states(&finally_start, state);
                            }
                            let pending_returns = self.return_edges[return_base..].to_vec();
                            for state in &pending_returns {
                                finally_start = self.join_layer_states(&finally_start, state);
                            }
                            let finally_break_base = self.break_exits.len();
                            let finally_return_base = self.return_edges.len();
                            let (finally_contributors, finally_end, finally_written) =
                                match self.eval_try_clause(&finally_start, finally, None, false) {
                                    Ok(clause) => clause,
                                    Err(failure) => return (Err(failure), region.can_fall_through),
                                };
                            // The post-statement state: the normal
                            // completions (pre_finally, with its flags and
                            // the entry's overlay) plus exactly the
                            // finally's own writes.
                            let mut post = pre_finally.clone();
                            post.narrowings = entry.narrowings.clone();
                            for name in &finally_written.0 {
                                if let Some(node) = finally_end.locals.get(name) {
                                    post.locals.insert(name.clone(), *node);
                                }
                            }
                            for name in &finally_written.1 {
                                if let Some(node) = finally_end.var_locals.get(name) {
                                    post.var_locals.insert(name.clone(), *node);
                                }
                            }
                            for ordinal in &finally_written.2 {
                                if let Some(node) = finally_end.param_writes.get(ordinal) {
                                    post.param_writes.insert(*ordinal, *node);
                                }
                            }
                            self.restore_layer_state(post);
                            if catch.is_none() {
                                // No catch: the abrupt paths leave the
                                // frame, so past the statement the
                                // normal-completion path's narrow facts
                                // hold again — re-establish the try's,
                                // minus any the finally's own writes
                                // killed — and the clause-write flags lose
                                // their reason: no path past the statement
                                // can have skipped those writes.
                                let mut killed: rustc_hash::FxHashSet<
                                    crate::flow_slice_content::SliceNarrowRoot,
                                > = rustc_hash::FxHashSet::default();
                                for name in finally_written.0.iter().chain(finally_written.1.iter())
                                {
                                    killed.insert(
                                        crate::flow_slice_content::SliceNarrowRoot::Local(
                                            Arc::from(name.as_str()),
                                        ),
                                    );
                                }
                                for ordinal in &finally_written.2 {
                                    killed.insert(
                                        crate::flow_slice_content::SliceNarrowRoot::Param(*ordinal),
                                    );
                                }
                                let restored: Vec<_> = try_narrowings
                                    .iter()
                                    .filter(|fact| {
                                        !entry.narrowings.contains(fact)
                                            && !killed.contains(&fact.0.root)
                                    })
                                    .cloned()
                                    .collect();
                                self.narrowings.extend(restored);
                                for name in &try_written.0 {
                                    if !entry.conditional_lexicals.contains(name) {
                                        self.conditional_lexicals.remove(name);
                                    }
                                }
                                for name in &try_written.1 {
                                    if !entry.var_conditional_locals.contains(name) {
                                        self.var_conditional_locals.remove(name);
                                    }
                                }
                                for ordinal in &try_written.2 {
                                    if !entry.conditional_params.contains(ordinal) {
                                        self.conditional_params.remove(ordinal);
                                    }
                                }
                            }
                            // The checker aggregates authored returns even
                            // when an abrupt finally overrides an earlier
                            // completion at runtime.
                            own.extend(finally_contributors);
                            if !finally.can_fall_through {
                                if *pending_break_contributes_undefined
                                    && finally_break_base > break_base
                                {
                                    self.implicit_undefined_seen = true;
                                }
                                // Control edges remain runtime-honest: an
                                // abrupt finally replaces pending try/catch
                                // returns with its own return edges before an
                                // OUTER finally is entered.
                                let finally_returns =
                                    self.return_edges.split_off(finally_return_base);
                                self.return_edges.truncate(return_base);
                                self.return_edges.extend(finally_returns);
                                let retained_pending_breaks: Vec<FlowBreakExit> = self.break_exits
                                    [break_base..finally_break_base]
                                    .iter()
                                    .filter(|exit| {
                                        exit.target.as_ref().is_some_and(|target| {
                                            pending_break_following_return_targets.contains(target)
                                        })
                                    })
                                    .cloned()
                                    .map(|mut exit| {
                                        exit.state.inference_only_path = true;
                                        exit
                                    })
                                    .collect();
                                let finally_breaks = self.break_exits.split_off(finally_break_base);
                                self.break_exits.truncate(break_base);
                                self.break_exits.extend(retained_pending_breaks);
                                self.break_exits.extend(finally_breaks);
                            }
                            path_alive = !exit_states.is_empty() && finally.can_fall_through;
                        }
                        None => {
                            // A catch clause exists (a bare `try` is
                            // syntactically impossible without either
                            // clause): its antecedent joins the flow past
                            // the statement, so no clause-established
                            // narrow survives — even when the catch itself
                            // returns (tsgo, measured).
                            pre_finally.narrowings = entry.narrowings;
                            self.restore_layer_state(pre_finally);
                            path_alive = !exit_states.is_empty();
                        }
                    }
                    contributors.extend(own);
                }
                crate::flow_slice_content::SliceStatement::Labeled { label, body } => {
                    // The edge past the label joins every path that
                    // reaches it: the body's own fall-through end AND the
                    // state captured at each `break` naming the label —
                    // never the pre-statement layers (a write inside the
                    // body IS a reaching definition) and never the body's
                    // end state alone (a break before it carries a state
                    // of its own). The narrowing overlay rides each edge
                    // state, so the join's intersection is exactly "a fact
                    // holds past the label only when every path into it
                    // established it".
                    let mut entry = self.layer_state();
                    self.complete_param_writes(&mut entry);
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    let shadow_base = self.scope_shadows.len();
                    let (result, body_falls) = self.eval_region(body);
                    let body_contributors = match result {
                        Ok(contributors) => contributors,
                        Err(failure) => return (Err(failure), region.can_fall_through),
                    };
                    contributors.extend(body_contributors);
                    let mut end = self.layer_state();
                    // The body's scope close replays on every state the
                    // body produced — each pending break exit included —
                    // BEFORE the join, so a shadowed or body-declared
                    // binding cannot be unioned into the post-statement
                    // state.
                    let shadows = self.split_scope_shadows_close_exits(
                        shadow_base,
                        break_base,
                        return_base,
                        throw_base,
                    );
                    let mut exits: Vec<FlowLayerState> =
                        self.drain_break_exits(break_base, Some(label));
                    Self::close_lexical_scope(&mut end, &shadows);
                    if body_falls {
                        exits.push(end);
                    }
                    let reaches = !exits.is_empty();
                    let mut joined = match exits.split_first() {
                        Some((first, rest)) => {
                            let mut joined = first.clone();
                            for state in rest {
                                joined = self.join_layer_states(&joined, state);
                            }
                            joined
                        }
                        // No path leaves the body: the post-statement
                        // state is unreachable; restore the entry to keep
                        // the layers sane.
                        None => entry.clone(),
                    };
                    if reaches {
                        // A `var` the body first defines has no reaching
                        // definition on an edge that skips the definition
                        // (a break before it): flag it, exactly like the
                        // switch's own exit join does.
                        self.flag_conditionally_defined_vars(&mut joined, &exits);
                    }
                    self.restore_layer_state(joined);
                    path_alive = reaches;
                }
                crate::flow_slice_content::SliceStatement::Block(block) => {
                    // Bindings are block-scoped: a `const` inside a block
                    // never escapes it — and a write to a binding that
                    // PREDATES the block survives it, because the block
                    // executes unconditionally on every path that reaches
                    // it. The scope close, not a layer restore, expresses
                    // both halves at once — and it replays on every
                    // pending break exit the block's evaluation captured,
                    // since an exit rides its captured bindings across the
                    // block's boundary.
                    let shadow_base = self.scope_shadows.len();
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    let (result, block_falls) = self.eval_region(block);
                    let block_contributors = match result {
                        Ok(contributors) => contributors,
                        Err(failure) => return (Err(failure), region.can_fall_through),
                    };
                    contributors.extend(block_contributors);
                    let mut end = self.layer_state();
                    let shadows = self.split_scope_shadows_close_exits(
                        shadow_base,
                        break_base,
                        return_base,
                        throw_base,
                    );
                    Self::close_lexical_scope(&mut end, &shadows);
                    self.restore_layer_state(end);
                    path_alive = block_falls;
                }
                crate::flow_slice_content::SliceStatement::Break { target } => {
                    // The edge past the absorbing construct is THIS state:
                    // capture it complete and end the region's path (the
                    // lowering already proved the target exists).
                    let mut state = self.layer_state();
                    self.complete_param_writes(&mut state);
                    self.break_exits.push(FlowBreakExit {
                        target: target.clone(),
                        state,
                    });
                    path_alive = false;
                }
                crate::flow_slice_content::SliceStatement::Throw => {
                    // A throw point the enclosing `try`'s catch / finally
                    // is entered from — then the path ends here.
                    self.capture_throw_point();
                    path_alive = false;
                }
                crate::flow_slice_content::SliceStatement::ThrowPoint => {
                    // A bare call statement: value-neutral, but a throw
                    // point for an enclosing `try`.
                    self.capture_throw_point();
                }
                crate::flow_slice_content::SliceStatement::Binding {
                    name,
                    kind,
                    init,
                    declared,
                    widening_literal,
                } => {
                    // A lexical declaration shadows any outer same-named
                    // binding for the extent of its block scope: record
                    // the shadow BEFORE binding, so the scope close can
                    // restore the shadowed value without losing the
                    // scope's writes to bindings that predate it.
                    if !matches!(kind, crate::flow_slice_content::SliceBindingKind::Var) {
                        self.record_scope_shadow(name.as_ref());
                    }
                    // An authored annotation is the binding's DECLARED
                    // type, seeded HERE (in source order), never at
                    // region entry — a forward reference stays unbound.
                    // tsc's `getTypeAtFlowAssignment` decides what an
                    // annotated declarator's binding holds:
                    //
                    //   - no initializer ⇒ the declared type verbatim
                    //     (`var y: number | undefined;` is
                    //     `number | undefined`, not the unbound `any`);
                    //   - an initializer with a NON-UNION declared type ⇒
                    //     the declared type verbatim, never the
                    //     initializer's literal and never the widened
                    //     initializer (`let n: number = 1` is `number`,
                    //     `let v: "s" = "s"` is `"s"`, `let u: unknown = 1`
                    //     is `unknown`);
                    //   - an initializer with a UNION declared type ⇒
                    //     `getAssignmentReducedType`, below.
                    if let Some(declared) = declared.as_ref() {
                        let no_init_var_with_reaching_value = init.is_none()
                            && matches!(kind, crate::flow_slice_content::SliceBindingKind::Var)
                            && (self.var_locals.contains_key(name.as_ref())
                                || self.param_names.iter().enumerate().any(|(ordinal, param)| {
                                    param.name.as_deref() == Some(name.as_ref())
                                        && (self.param_writes.contains_key(&(ordinal as u32))
                                            || self.params.get(ordinal).is_some())
                                }));
                        // THE root-identifier gate at the declarator
                        // annotation. `const v: Info` in a frame that
                        // declares its own `Info` names the LOCAL one;
                        // the shared shallow pass resolved it in owner
                        // scope. The initializer's own gate cannot stand
                        // in — the non-union arm below binds the DECLARED
                        // node and never evaluates the initializer.
                        if declared
                            .shadowed()
                            .iter()
                            .any(|name| self.owner_scope_answers_name(name))
                        {
                            // POSITIONAL: the DECLARED type of this one
                            // binding has no modelled value. The binding
                            // holds the marker, every sibling statement
                            // keeps evaluating, and a body that never
                            // reads this binding still publishes its
                            // return (degraded, never warm).
                            let marker = self.unmodeled_position();
                            self.set_declared_local(name, *kind, Some(marker));
                            if !no_init_var_with_reaching_value {
                                self.bind_local(name, *kind, marker, false, false);
                            }
                            continue;
                        }
                        let declared_node = self.lower_body_type(declared.ty());
                        self.set_declared_local(name, *kind, Some(declared_node));
                        // An initializer-less `var` declaration has no
                        // runtime write. Keep the authored declaration as the
                        // stable authority for later assignments, but do not
                        // replace a value already reaching this statement.
                        if no_init_var_with_reaching_value {
                            continue;
                        }
                        let arms = self.dispatch.union_arms_of(declared_node);
                        match (init, arms) {
                            (None, _) | (Some(_), None) => {
                                self.bind_local(name, *kind, declared_node, false, false);
                                continue;
                            }
                            (Some(init), Some(arms)) => {
                                let node = match self.eval_assignment_expr(init) {
                                    Positional::Value(init_node) => self.assignment_reduced_union(
                                        declared_node,
                                        &arms,
                                        init_node,
                                    ),
                                    // A hold / unmodelled initializer
                                    // cannot select constituents: the whole
                                    // declared union is the honest
                                    // superset, degraded.
                                    Positional::Hold | Positional::Unmodeled => {
                                        self.record_degradation(
                                            crate::semantic_query::FlowReturnDegradation::UnreducedDeclaredUnion,
                                        );
                                        declared_node
                                    }
                                };
                                self.bind_local(name, *kind, node, false, false);
                                continue;
                            }
                        }
                    }
                    self.set_declared_local(name, *kind, None);
                    // A binding OUTSIDE the slice's value-selected slot
                    // set never even LOWERS — the content producer elides
                    // the whole declaration, so nothing here can observe
                    // an unselected sibling.
                    if let Some(init) = init {
                        match self.eval_expr(init) {
                            Positional::Value(node) => {
                                self.bind_local(name, *kind, node, *widening_literal, false);
                            }
                            Positional::Hold => {}
                            // An UNMODELLED initializer binds the typed
                            // marker — never a fabricated `any`, which is
                            // indistinguishable from an authored one at
                            // every downstream gate. The declaration
                            // itself is not a return contribution, so the
                            // degradation is recorded only where the
                            // binding is OBSERVED (`read_local` folds the
                            // `FailedBindingInitializer` membership); an
                            // unobserved unmodelled binding degrades
                            // nothing.
                            Positional::Unmodeled => {
                                let marker = super::flow_return_callee::unmodeled_position_marker(
                                    self.dispatch,
                                );
                                self.bind_local(name, *kind, marker, false, true);
                            }
                        }
                    }
                }
                crate::flow_slice_content::SliceStatement::Assignment { target, value, .. } => {
                    // THE applied write: a whole-binding `=` at statement
                    // position retypes the binding IN SOURCE ORDER, so the
                    // reads after it see the written value and the typed
                    // unapplied-write degradation never seeds. An
                    // unmodelled right-hand side binds the typed marker
                    // with the failed-initializer membership, exactly like
                    // an unmodelled declarator initializer.
                    let holds_before = self.holds.len();
                    let outcome = if self.target_has_declared_union(target) {
                        self.eval_assignment_expr(value)
                    } else {
                        self.eval_expr(value)
                    };
                    match outcome {
                        Positional::Value(node) => {
                            self.holds.truncate(holds_before);
                            self.apply_write(target, node, false);
                        }
                        Positional::Hold => {
                            self.holds.truncate(holds_before);
                        }
                        Positional::Unmodeled => {
                            self.holds.truncate(holds_before);
                            let marker =
                                super::flow_return_callee::unmodeled_position_marker(self.dispatch);
                            self.apply_write(target, marker, true);
                        }
                    }
                }
                crate::flow_slice_content::SliceStatement::Assertion { subject, target } => {
                    // A same-file assertion call: the narrowing fact lives
                    // in the callee's declared return, and it PERSISTS for
                    // the rest of the region (there is no arm scope to
                    // truncate it — the assertion is unconditional the
                    // moment the statement evaluates). The call itself is
                    // a throw point FIRST: if it throws, the narrow never
                    // happened, so the snapshot precedes the fact. A
                    // TARGETLESS `asserts v` narrows by truthiness: the
                    // definitely-falsy arms leave the subject's type.
                    self.capture_throw_point();
                    let fact = match target {
                        Some(target) => self.narrow_to_predicate_target(subject, target, false),
                        None => self.narrow_truthy(subject, false),
                    };
                    match fact {
                        GuardNarrowing::Narrowed(subject, node) => {
                            self.narrowings.push((subject, node));
                        }
                        GuardNarrowing::Impossible => {
                            if slice_statements_have_non_subject_return(
                                region.statements.iter().skip(statement_index + 1),
                                &|expr| slice_expr_is_exact_subject_read(expr, subject),
                            ) {
                                self.record_degradation(FlowReturnDegradation::FlowGap(
                                    crate::semantic_query::FlowGap::GuardNarrowing,
                                ));
                            }
                            path_alive = false;
                        }
                        GuardNarrowing::Unchanged => {}
                    }
                }
                crate::flow_slice_content::SliceStatement::TransparentLoop => {}
                crate::flow_slice_content::SliceStatement::Unsupported(kind) => {
                    return (
                        Err(FlowReturnFailure::Unsupported(match kind {
                            crate::flow_slice_content::SliceUnsupported::Loop => {
                                FlowReturnUnsupported::Loop
                            }
                            crate::flow_slice_content::SliceUnsupported::Jump => {
                                FlowReturnUnsupported::Jump
                            }
                            crate::flow_slice_content::SliceUnsupported::InvokedClosureEffect => {
                                FlowReturnUnsupported::InvokedClosureEffect
                            }
                            crate::flow_slice_content::SliceUnsupported::With => {
                                FlowReturnUnsupported::With
                            }
                            crate::flow_slice_content::SliceUnsupported::ModuleDeclaration => {
                                FlowReturnUnsupported::ModuleDeclaration
                            }
                        })),
                        region.can_fall_through,
                    );
                }
            }
        }
        (Ok(contributors), region.can_fall_through && path_alive)
    }

    /// Seed the authored type authority of selected `var` declarations before
    /// executing the frame. The declaration is hoisted for assignment
    /// checking, while its initializer (if any) and its runtime reaching value
    /// remain source-ordered in [`Self::eval_region`]. An annotation whose
    /// frame gate is unresolved stays source-ordered so an unreachable or
    /// unobserved declaration cannot degrade the frame merely by existing.
    fn seed_hoisted_var_declarations(&mut self, region: &crate::flow_slice_content::SliceRegion) {
        use crate::flow_slice_content::{SliceBindingKind, SliceStatement};
        for statement in region.statements.iter() {
            match statement {
                SliceStatement::Binding {
                    name,
                    kind: SliceBindingKind::Var,
                    declared: Some(declared),
                    ..
                } if !declared
                    .shadowed()
                    .iter()
                    .any(|name| self.owner_scope_answers_name(name)) =>
                {
                    let node = self.lower_body_type(declared.ty());
                    self.set_declared_local(name, SliceBindingKind::Var, Some(node));
                }
                SliceStatement::If {
                    consequent,
                    alternate,
                    ..
                } => {
                    self.seed_hoisted_var_declarations(consequent);
                    if let Some(alternate) = alternate.as_deref() {
                        self.seed_hoisted_var_declarations(alternate);
                    }
                }
                SliceStatement::Block(body) => {
                    self.seed_hoisted_var_declarations(body);
                }
                SliceStatement::Labeled { body, .. } => {
                    self.seed_hoisted_var_declarations(body);
                }
                SliceStatement::Switch { cases, .. } => {
                    for case in cases.iter() {
                        self.seed_hoisted_var_declarations(&case.region);
                    }
                }
                SliceStatement::Try {
                    block,
                    catch,
                    finally,
                    ..
                } => {
                    self.seed_hoisted_var_declarations(block);
                    if let Some(catch) = catch.as_deref() {
                        self.seed_hoisted_var_declarations(&catch.region);
                    }
                    if let Some(finally) = finally.as_deref() {
                        self.seed_hoisted_var_declarations(finally);
                    }
                }
                _ => {}
            }
        }
    }

    /// Evaluate a nested function value's signature: bind its OWN type
    /// parameters in scope (the SAME binder environment the root
    /// evaluation uses), lower its parameters, evaluate its body in a
    /// fresh frame seeded with the CAPTURED enclosing bindings (holds the
    /// nested evaluation met ride the outer frame's hold set), and
    /// compose the `Signature` node.
    ///
    /// Closure capture: the content lowering classified a nested read of
    /// an ENCLOSING parameter / local as a by-name local read, so the
    /// nested frame starts from a SNAPSHOT of the enclosing layers taken
    /// at the function value's own position. Enclosing parameters seed
    /// the function-scoped layer by name (they are the outermost frame
    /// scope, and a redeclaring enclosing `var` still wins); the
    /// enclosing lexical locals seed the lexical layer, so the nested
    /// frame's own bindings shadow them and the membership flags stay
    /// layer-exact.
    fn eval_nested_function_signature(
        &mut self,
        nested_params: &[crate::flow_slice_content::SliceParam],
        type_parameters: &[crate::flow_slice_content::SliceTypeParam],
        mutable_capture_authorities: &[crate::flow_slice_content::SliceCaptureAuthority],
        declared_return: Option<&crate::flow_slice_content::GatedType>,
        body: &crate::flow_slice_content::SliceRegion,
        can_fall_through: bool,
    ) -> SemanticNodeId {
        let graph = self.dispatch.graph();
        // The nested function's OWN type parameters are binders in scope
        // for the parameter / return lowering (a `<T>(x: T) => x` keeps
        // `<T>`), COMPOSED over the enclosing frame's environment: the
        // nested signature sits inside that frame, so every binder in
        // scope there is in scope here too.
        let binder_env = self.dispatch.flow_binder_env(
            self.canonical,
            self.owner,
            type_parameters,
            Some(self.binder_env),
        );
        // The SAME signature gate the root evaluation takes. A nested
        // signature sits inside the enclosing frame's body, so its
        // annotations, its type-parameter constraints / defaults, and
        // its parameter defaults were all gated against that frame; an
        // owner-scope answer for one of those names is the WRONG binding.
        //
        // POSITIONAL, per signature slot — same rule as the root
        // entrance: a shadowed slot carries the marker at ITS ordinal and
        // degrades the enclosing result; every other slot of the same
        // signature keeps its modelled value.
        let mut type_param_decls = binder_env.type_param_decls.clone();
        for (ordinal, clause_param) in type_parameters.iter().enumerate() {
            if !clause_param
                .constraint
                .iter()
                .chain(clause_param.default.iter())
                .any(|gated| signature_answer_is_frame_shadowed(self.dispatch, &binder_env, gated))
            {
                continue;
            }
            // The shadowed CONSTRAINT / DEFAULT slot carries the marker.
            // Recording the degradation alone would leave the WRONG
            // (owner-scope) resolution sitting in the published clause —
            // the leak these rows exist to catch, now merely annotated.
            let marker = self.unmodeled_position();
            if let Some(decl) = type_param_decls.get_mut(ordinal) {
                if decl.constraint.is_some() {
                    decl.constraint = Some(marker);
                }
                if decl.default.is_some() {
                    decl.default = Some(marker);
                }
            }
        }
        let mut params: Vec<SemanticNodeId> = Vec::with_capacity(nested_params.len());
        let mut signature_params: Vec<crate::semantic_query::FunctionParam> =
            Vec::with_capacity(nested_params.len());
        for param in nested_params.iter() {
            if signature_answer_is_frame_shadowed(self.dispatch, &binder_env, &param.ty) {
                let node = self.unmodeled_position();
                params.push(node);
                signature_params.push(crate::semantic_query::FunctionParam {
                    name: param.name.clone(),
                    ty: node,
                    optional: param.optional,
                    rest: param.rest,
                    span: None,
                });
                continue;
            }
            let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
            let node = self.dispatch.shallow_lower_type_expr_with_context(
                param.ty.ty(),
                &binder_env.env,
                &binder_env.scope,
                &binder_env.name_resolution,
                binder_env.scope_payload.as_ref(),
                &binder_env.shadowing,
                &mut substitutions,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            );
            params.push(node);
            signature_params.push(crate::semantic_query::FunctionParam {
                name: param.name.clone(),
                ty: node,
                optional: param.optional,
                rest: param.rest,
                span: None,
            });
        }
        // A DECLARED return annotation wins over the body join, full stop:
        // the checker checks the body AGAINST the annotation, and the
        // signature's return IS the annotation — the body cannot change
        // the answer, so it is never evaluated for the signature's
        // return. The annotation lowers under the nested signature's own
        // binder environment, behind the same per-slot frame gate every
        // parameter annotation takes: a shadowed answer carries the typed
        // marker in the RETURN position and degrades the enclosing result.
        if let Some(declared) = declared_return {
            let return_type =
                if signature_answer_is_frame_shadowed(self.dispatch, &binder_env, declared) {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                    );
                    self.unmodeled_position()
                } else {
                    let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                    self.dispatch.shallow_lower_type_expr_with_context(
                        declared.ty(),
                        &binder_env.env,
                        &binder_env.scope,
                        &binder_env.name_resolution,
                        binder_env.scope_payload.as_ref(),
                        &binder_env.shadowing,
                        &mut substitutions,
                        crate::semantic_query::ProjectionReductionContext::structural_transit(),
                    )
                };
            return graph.intern_node(SemanticNodeData::Signature {
                kind: crate::semantic_query::SignatureKind::Call,
                params: Arc::from(signature_params.into_boxed_slice()),
                return_type,
                type_parameters: Arc::from(type_param_decls.into_boxed_slice()),
                signature_span: None,
                return_type_span: None,
                // A nested function value's synthesized signature has no
                occurrence: None,
                return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(
                    return_type,
                ),
            });
        }
        // The captured function-scope layer: the enclosing parameters BY
        // NAME, overlaid by the enclosing `var` layer (a redeclaring
        // enclosing `var` shares the parameter's slot and still wins).
        let mut captured_locals = self.locals.clone();
        let mut captured_var_locals = self.var_locals.clone();
        let mut captured_declared_locals = self.declared_locals.clone();
        let mut captured_var_declared_locals = self.var_declared_locals.clone();
        for authority in mutable_capture_authorities {
            let base = if authority
                .declared
                .shadowed()
                .iter()
                .any(|name| self.owner_scope_answers_name(name))
            {
                self.record_degradation(
                    crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                );
                self.unmodeled_position()
            } else {
                self.lower_body_type(authority.declared.ty())
            };
            let node = match &authority.source {
                crate::flow_slice_content::SliceCaptureAuthoritySource::Parameter {
                    key: Some(key),
                    has_default,
                } => self
                    .destructured_param_element_node(base, key, *has_default)
                    .unwrap_or_else(|| {
                        self.record_degradation(
                            crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                        );
                        self.unmodeled_position()
                    }),
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(_)
                | crate::flow_slice_content::SliceCaptureAuthoritySource::Parameter {
                    key: None,
                    ..
                } => base,
            };
            let name = authority.name.to_string();
            match authority.source {
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(
                    crate::flow_slice_content::SliceBindingKind::Var,
                ) => {
                    captured_var_declared_locals.insert(name.clone(), node);
                    captured_var_locals.insert(name, node);
                }
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(
                    crate::flow_slice_content::SliceBindingKind::Let,
                ) => {
                    captured_declared_locals.insert(name.clone(), node);
                    captured_locals.entry(name).or_insert(node);
                }
                crate::flow_slice_content::SliceCaptureAuthoritySource::Parameter { .. } => {
                    captured_declared_locals.insert(name.clone(), node);
                    captured_locals.insert(name, node);
                }
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(
                    crate::flow_slice_content::SliceBindingKind::Const,
                ) => {}
            }
        }
        for (ordinal, param) in self.param_names.iter().enumerate() {
            let (Some(name), Some(node)) = (param.name.as_ref(), self.params.get(ordinal)) else {
                continue;
            };
            captured_var_locals.entry(name.to_string()).or_insert(*node);
        }
        let nested_holds;
        let nested_degradation;
        let nested_bare_return_seen;
        let nested_implicit_undefined_seen;
        let (contributors, nested_body_falls_through) = {
            let mut nested_evaluator = FlowEvaluator {
                dispatch: self.dispatch,
                self_slot: None,
                canonical: self.canonical,
                owner: self.owner,
                params: &params,
                param_names: nested_params,
                binder_env: &binder_env,
                locals: captured_locals,
                declared_locals: captured_declared_locals,
                var_locals: captured_var_locals,
                var_declared_locals: captured_var_declared_locals,
                widening_locals: self.widening_locals.clone(),
                var_widening_locals: self.var_widening_locals.clone(),
                bare_return_seen: false,
                implicit_undefined_seen: false,
                // A nested function value always evaluates its WHOLE
                // return (its signature's return type) — the member
                // filter is a top-level demand axis.
                member_filter: None,
                holds: Vec::new(),
                degradation: None,
                pending_statement_gap: None,
                degraded_locals: self.degraded_locals.clone(),
                var_degraded_locals: self.var_degraded_locals.clone(),
                var_conditional_locals: self.var_conditional_locals.clone(),
                conditional_arm_nesting: 0,
                // The nested frame captures the enclosing layers' VALUES
                // by name, not their flow facts: a guard narrow lives and
                // dies with the arm that established it, and the checker
                // itself does not honour a narrowing of a mutable binding
                // across a closure boundary.
                narrowings: Vec::new(),
                param_writes: rustc_hash::FxHashMap::default(),
                conditional_lexicals: rustc_hash::FxHashSet::default(),
                conditional_params: rustc_hash::FxHashSet::default(),
                inference_only_path: false,
                call_fresh_literal_returns: Vec::new(),
                break_exits: Vec::new(),
                return_edges: Vec::new(),
                throw_points: Vec::new(),
                collect_throw_points: false,
                scope_shadows: Vec::new(),
                call_evidence: Vec::new(),
                executed_walk: ExecutedSliceWalk::default(),
            };
            nested_evaluator.seed_hoisted_var_declarations(body);
            let (outcome, nested_body_falls_through) = nested_evaluator.eval_region(body);
            nested_evaluator.promote_pending_statement_gap();
            nested_holds = nested_evaluator.holds.clone();
            nested_degradation = nested_evaluator.degradation;
            nested_bare_return_seen = nested_evaluator.bare_return_seen;
            nested_implicit_undefined_seen = nested_evaluator.implicit_undefined_seen;
            self.holds.append(&mut nested_evaluator.holds);
            // A call the NESTED body evaluated is still an evaluated call
            // of this evaluation run: the evidence rides the enclosing
            // ledger exactly as the nested holds do — and so does the
            // nested walk (a nested abort shortens THIS run's ledger).
            self.call_evidence
                .append(&mut nested_evaluator.call_evidence);
            self.executed_walk.absorb(nested_evaluator.executed_walk);
            (outcome, nested_body_falls_through)
        };
        // A degraded nested body degrades the enclosing value that
        // embeds its signature.
        if let Some(degradation) = nested_degradation {
            self.record_degradation(degradation);
        }
        // A nested body's OWN frame-level failure — an unmodelled control
        // surface, an empty hold-only cycle — is a fact about the NESTED
        // function's return position, not about the frame that embeds its
        // signature. Propagating it outward is what deleted
        // `{ label: "x", go: (n) => { while (…) { return n } return 0 } }`
        // whole, where the checker publishes
        // `{ label: string; go: (n: number) => number }`. The signature
        // survives with its parameters intact and the typed marker in its
        // RETURN position.
        //
        // A nested function value's body is its own join; its holds ride
        // the OUTER frame's component, so no fixed point closes here and
        // the freshness bit has no later consumer.
        let return_type = match contributors.and_then(|contributors| {
            self.dispatch.join_flow_return_contributors(
                contributors,
                can_fall_through && nested_body_falls_through,
                nested_bare_return_seen,
                nested_implicit_undefined_seen,
                &nested_holds,
                nested_degradation,
            )
        }) {
            Ok((result, _fresh_seed)) => result.return_type(),
            Err(_) => self.unmodeled_position(),
        };
        graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from(signature_params.into_boxed_slice()),
            return_type,
            type_parameters: Arc::from(type_param_decls.into_boxed_slice()),
            signature_span: None,
            return_type_span: None,
            // A nested function value's synthesized signature has no
            // authored occurrence anchor and no served position: the
            // return carrier is the interned node itself.
            occurrence: None,
            return_carrier: crate::semantic_query::SignatureReturnCarrier::Declared(return_type),
        })
    }

    /// The CALLEE's own type-parameter clause at a direct-call site.
    ///
    /// Names come from the shallow per-file FUNCTION PROGRAM INDEX — the
    /// one authority that answers for every position it serves, the
    /// value registry included, a namespace-scoped function included.
    ///
    /// Three outcomes, not two: a clause that was READ and found empty is
    /// an EMPTY clause — a statement about the callee, which only
    /// [`CalleeClause::read_from_program_entry`] can make because only it
    /// is handed the callee's index entry — while a clause that could not
    /// be read at all is [`CalleeClauseLookup::Unavailable`]. Collapsing
    /// the second into the first is how a serve miss becomes "the callee
    /// is not generic": the callee's return is handed back verbatim,
    /// binders and all, with no degradation and full warm admission —
    /// the exact leak this module exists to make inexpressible.
    ///
    /// A DEFAULT is a body lowering, not a shallow fact, so it is
    /// demanded separately and ONLY for the parameters the index flagged
    /// as authoring one AND whose call site actually leaves inference
    /// with nothing to produce: an ordinary generic callee never pays
    /// for it, and neither does a defaulted parameter the call infers.
    /// A default that IS needed and cannot be recovered is
    /// `Unavailable`, never a fabricated `unknown` — an `unknown` there
    /// is indistinguishable from the honest interim and would be warm
    /// admitted.
    fn direct_callee_clause(
        &mut self,
        target: &verter_semantic::analysis::function_program::FunctionProgramKey,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> CalleeClauseLookup {
        let Some(serve) = self.dispatch.ctx.ensure_indexed_ready_serve(self.canonical) else {
            return CalleeClauseLookup::Unavailable;
        };
        let decl_bodies = serve.indexed.shallow_state.decl_bodies();
        let index = decl_bodies.function_program_index();
        let Some(matched) = index.get(target) else {
            return CalleeClauseLookup::Unavailable;
        };
        let entry = matched.entry();
        // The lowered clause is demanded lazily and at most once, and
        // only when some parameter's default is actually needed.
        let mut lowered: Option<Option<Vec<crate::flow_slice_content::SliceTypeParam>>> = None;
        // The clause is BUILT by its owning module, from the entry the
        // index answered with. This route reads the authority and hands
        // it over; it cannot assemble a clause out of nothing, because
        // the constructors that would let it are private there.
        CalleeClause::read_from_program_entry(matched, site, |ordinal, param| {
            let clause =
                lowered.get_or_insert_with(|| decl_bodies.function_type_param_clause(entry));
            // Matched by ORDINAL, with the name as a cross-check: the
            // shallow index and the lowered clause both walk the SAME
            // authored clause in declaration order, so the ordinal is the
            // identity and the name is not (a duplicate spelling would
            // silently take the first slot's default). A disagreement
            // means the two views are not the same clause, which is a
            // miss, not a best guess.
            let slice = clause.as_ref()?.get(ordinal)?;
            if slice.name != param.name {
                return None;
            }
            slice.default.as_ref().and_then(|gated| {
                self.dispatch.lower_type_expr_in_owner_scope_with_context(
                    self.canonical,
                    target.declaration.owner,
                    gated.ty(),
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                )
            })
        })
    }

    /// The `UnrepresentableCallee` DEGRADATION: the typed unresolved
    /// MARKER at this call position, `ReturnOnly` by contract.
    ///
    /// The marker rather than a modeled `any`, because this degradation is
    /// classified [`PartialReasonSet::FLOW_RETURN_UNINFERRED`], and that
    /// class's whole claim is that the position the substrate could not
    /// type says so in the graph instead of fabricating a value. A
    /// fabricated `any` is indistinguishable from an authored one at every
    /// downstream gate: an overloaded callee published `flag: any` warm
    /// and clean where the checker says `boolean`.
    fn degraded_unrepresentable_callee(&mut self) -> Positional<CallValue> {
        self.record_degradation(
            crate::semantic_query::FlowReturnDegradation::UnrepresentableCallee,
        );
        Positional::Value(CallValue::unmodeled_position(self.dispatch))
    }

    /// The call-bucket return of an already-lowered CALLEE TYPE — the one
    /// place a call whose callee is a resolved value TYPE (rather than a
    /// served flow position) takes its value from.
    fn call_return_of_callee_node(
        &mut self,
        callee_node: SemanticNodeId,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> Positional<CallValue> {
        let resolved = self.dispatch.resolve_signature_source_carrier(
            callee_node,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        );
        // An OVERLOADED callee is not answerable at a CALL site, whether
        // or not the group has an implementation. TypeScript picks the
        // FIRST signature whose parameters accept the arguments;
        // `select_signature_function` deliberately selects the LAST
        // (which is what the signature UTILITIES want — `ReturnType<typeof
        // f>` over an overloaded `f` IS the last overload's return). For
        // an AMBIENT group (`declare function f(…)` ×3) that hands back
        // the last declaration's return as the call's value, cleanly and
        // warm, with no visible signature the call would ever select.
        //
        // Picking the right overload needs argument-driven overload
        // resolution, which this substrate does not perform; the answer is
        // the `UnrepresentableCallee` degradation — the typed positional
        // marker, `ReturnOnly` by contract.
        if self
            .dispatch
            .signature_bucket_arity(resolved, super::build::SignatureBucket::Call)
            > 1
        {
            return self.degraded_unrepresentable_callee();
        }
        let Some(function_node) = self
            .dispatch
            .select_signature_function(resolved, super::build::SignatureBucket::Call)
        else {
            return self.degraded_unrepresentable_callee();
        };
        // A resolved callee VALUE TYPE was composed from a DECLARED
        // signature, lowered in file owner scope where the callee's own
        // clause is invisible — so every spelling of a clause parameter,
        // the resolved same-named declaration included, is that
        // parameter.
        match CallValue::of_signature_node(
            self.dispatch,
            function_node,
            site,
            ReturnOrigin::OwnerScopeDeclared,
        ) {
            SignatureCall::Value(value) => Positional::Value(value),
            SignatureCall::NotCallable | SignatureCall::ClauseUnavailable => {
                self.degraded_unrepresentable_callee()
            }
            // The callee's own return position is a semantic MISS — no
            // value to transfer. That is a fact about THIS call, not
            // about the body it sits in.
            SignatureCall::ReturnMiss => Positional::Unmodeled,
        }
    }

    /// Evaluate one flow expression to a graph node.
    ///
    /// [`Positional`] — so this function cannot report a FRAME failure at
    /// all. Every condition it meets is a fact about the POSITION it is
    /// standing on, and the type says so.
    fn eval_expr(
        &mut self,
        expr: &crate::flow_slice_content::SliceExpr,
    ) -> Positional<SemanticNodeId> {
        let graph = self.dispatch.graph();
        match expr {
            crate::flow_slice_content::SliceExpr::Type(leaf) => {
                Positional::Value(self.lower_body_type(leaf.ty()))
            }
            crate::flow_slice_content::SliceExpr::FrameShadowed { inner, shadowed } => {
                // The root-identifier gate's decision point. The content
                // half found that this leaf's answer names bindings the
                // FRAME owns, and the shared shallow-pass lowering that
                // produced it resolves names in FILE OWNER SCOPE. If the
                // owner scope ANSWERS one of those names, evaluating the
                // leaf would publish an unrelated module-scope (or
                // cross-file imported) symbol's type for a
                // function-local binding — cleanly and warm. Fail closed
                // instead; the name is RESOLVED (never free), so there is
                // no honest value to publish.
                //
                // When the owner scope answers NOTHING, the frame-owned
                // name is genuinely unresolvable from here and the leaf
                // evaluates unchanged: its own typed miss carrier is the
                // honest answer, exactly as for any other unresolved
                // reference.
                if shadowed
                    .iter()
                    .any(|name| self.owner_scope_answers_name(name))
                {
                    return Positional::Unmodeled;
                }
                // The owner scope answers NOTHING — but the FRAME may.
                // A member path rooted at one of the frame's own bindings
                // (`x.a.b` over parameter `x`, `node.props.value` over a
                // reaching local) resolves its root through the frame's
                // substitution and projects the tail segments through the
                // one shared path projection, exactly as the owner-scope
                // lowering projects a free root's tail. Only when the
                // frame carries no such root does the leaf evaluate
                // unchanged: its own typed miss carrier is the honest
                // answer, exactly as for any other unresolved reference.
                if let crate::flow_slice_content::SliceExpr::Type(leaf) = inner.as_ref() {
                    if let Some(node) = self.eval_frame_rooted_typeof_path(leaf.ty()) {
                        return node;
                    }
                }
                self.eval_expr(inner)
            }
            // A parameter ordinal the frame's own parameter list does not
            // carry: the slice and the signature disagree about this
            // frame's arity. That is a fact about this REFERENCE, not
            // about the body around it.
            crate::flow_slice_content::SliceExpr::Param { ordinal } => {
                // A guard narrow (or an applied write) substitutes the
                // parameter's CURRENT value positionally.
                let subject = crate::flow_slice_content::SliceNarrowSubject {
                    root: crate::flow_slice_content::SliceNarrowRoot::Param(*ordinal),
                    path: Arc::from(Vec::new().into_boxed_slice()),
                };
                if let Some(node) = self.narrowed_read(&subject) {
                    return Positional::Value(node);
                }
                // A parameter written inside a `try` clause and observed
                // past a possible throw point: the value is one path's,
                // not the join's — fail closed at the read.
                if self.conditional_params.contains(ordinal) {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
                    );
                }
                if let Some(node) = self.param_writes.get(ordinal).copied() {
                    return Positional::Value(node);
                }
                match self.params.get(*ordinal as usize).copied() {
                    Some(node) => Positional::Value(node),
                    None => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceExpr::OptionalAnyChain { root } => {
                match self.eval_expr(root) {
                    Positional::Value(node) if self.node_is_semantic_any(node) => {
                        Positional::Value(
                            graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
                        )
                    }
                    Positional::Value(_) => {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            crate::semantic_query::FlowGap::UnmodeledExpression,
                        ));
                        Positional::Unmodeled
                    }
                    Positional::Hold => Positional::Hold,
                    Positional::Unmodeled => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceExpr::Local {
                name,
                param,
                captured,
            } => {
                // The READ folds the binding's membership flags into this
                // evaluation's degradation channel. A plain unbound local
                // (a not-yet-assigned hoisted `var` / TDZ forward
                // reference) stays the undegraded implicit-`any`, EXCEPT
                // when the binding redeclares a parameter — then the
                // parameter is still the reaching value.
                if !captured {
                    let subject = crate::flow_slice_content::SliceNarrowSubject {
                        root: crate::flow_slice_content::SliceNarrowRoot::Local(Arc::from(
                            name.as_ref(),
                        )),
                        path: Arc::from(Vec::new().into_boxed_slice()),
                    };
                    if let Some(node) = self.narrowed_read(&subject) {
                        return Positional::Value(node);
                    }
                }
                match self.read_local(name.as_ref()) {
                    Some(node) => Positional::Value(node),
                    // A CAPTURED binding the seeded snapshot does not
                    // carry has no honest value: it is neither the
                    // same-frame implicit-`any` nor a file-scope name, so
                    // the POSITION carries the marker.
                    None if *captured => Positional::Unmodeled,
                    None => {
                        match param.and_then(|ordinal| self.params.get(ordinal as usize).copied()) {
                            Some(node) => Positional::Value(node),
                            None => {
                                self.record_degradation(FlowReturnDegradation::FlowGap(
                                    crate::semantic_query::FlowGap::UnmodeledExpression,
                                ));
                                Positional::Unmodeled
                            }
                        }
                    }
                }
            }
            crate::flow_slice_content::SliceExpr::Object { entries } => {
                self.eval_object_literal(entries, false)
            }
            crate::flow_slice_content::SliceExpr::NestedFunctionValue {
                gap,
                params: nested_params,
                type_parameters,
                mutable_capture_authorities,
                declared_return,
                body,
                can_fall_through,
            } => {
                if let Some(gap) = gap {
                    self.record_degradation(FlowReturnDegradation::FlowGap(*gap));
                }
                // The nested function's signature: a DECLARED return
                // annotation decides it outright; a body-derived return
                // evaluates through the same flow machinery in a FRESH
                // frame (the nested function's own params / locals).
                Positional::Value(self.eval_nested_function_signature(
                    nested_params,
                    type_parameters,
                    mutable_capture_authorities,
                    declared_return.as_ref(),
                    body,
                    *can_fall_through,
                ))
            }
            // EVERY call form, through the ONE call sink. `CallValue`'s
            // constructors all decide what happens to the callee's own
            // type-parameter clause, so no arm below can hand a callee's
            // return back to this frame untouched by accident — only by
            // asking for `own_frame_binder` by name.
            crate::flow_slice_content::SliceExpr::Call(call, site) => {
                // A call is a throw point: an enclosing `try`'s catch /
                // finally can be entered from HERE, with the state as it
                // stands BEFORE the call (an enclosing write has not
                // applied yet — exactly the checker's antecedent).
                self.capture_throw_point();
                match self.eval_call(call, *site) {
                    Positional::Value(value) => Positional::Value(value.into_node()),
                    Positional::Hold => Positional::Hold,
                    Positional::Unmodeled => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceExpr::SemanticAny => Positional::Value(
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            ),
            crate::flow_slice_content::SliceExpr::Gap(gap) => {
                self.record_degradation(FlowReturnDegradation::FlowGap(*gap));
                Positional::Unmodeled
            }
            // A name the frame's lexical authority resolved to a
            // function-local binding the content half does not model
            // (a destructuring element, a local `class` / `enum` /
            // `namespace` / `import =`, a `catch` parameter, a nested
            // function declaration read as a value). The name is
            // RESOLVED — never free — so there is no honest value to
            // publish: fail closed with the typed no-value failure
            // rather than bind an unrelated same-named declaration.
            crate::flow_slice_content::SliceExpr::UnmodeledBinding => Positional::Unmodeled,
            // A conditional expression's branch VALUES. Each arm was
            // lowered as a flow expression, so a call in a branch already
            // took the one call sink above — the union here only joins
            // the results, through the same normalizing interner the
            // `if` / `return` twin's contributor join uses, so the two
            // spellings of one branch answer alike. The consequent
            // evaluates under the test's POSITIVE narrow, the alternate
            // under its NEGATED one — the same overlay the `if` twin
            // applies, arm-scoped.
            crate::flow_slice_content::SliceExpr::Union { arms, guard } => {
                let mut nodes = Vec::with_capacity(arms.len());
                for (index, arm) in arms.iter().enumerate() {
                    // A coinductive HOLD inside a branch cannot be
                    // represented as a partial union arm — the SCC
                    // discharge joins whole contributions, not fragments
                    // of one. The ARM is the unmodelled position; the rest
                    // of the union survives, degraded.
                    let holds_before = self.holds.len();
                    let narrow_len = self.narrowings.len();
                    let possible = self.apply_guard_scoped(guard, index == 0);
                    if !possible
                        && !guard_has_subject_matching(guard, &|subject| {
                            slice_expr_is_exact_subject_read(arm, subject)
                        })
                    {
                        self.record_degradation(FlowReturnDegradation::FlowGap(
                            crate::semantic_query::FlowGap::GuardNarrowing,
                        ));
                    }
                    let outcome = possible.then(|| self.eval_expr(arm));
                    self.narrowings.truncate(narrow_len);
                    if let Some(outcome) = outcome {
                        nodes.push(self.settle_composite_part(outcome, holds_before));
                    }
                }
                Positional::Value(
                    self.dispatch
                        .intern_normalized_union_or_intersection(&nodes, true),
                )
            }
            // A call the content half could not route through the call
            // carrier: the only answer available was the shallow pass's
            // UNREDUCED `ReturnType<callee>`, which carries the callee's
            // own binders and skipped its overload group entirely.
            // Publishing it is a warm-admissible wrong answer with a
            // FOREIGN binder in it, so the evaluation fails closed.
            crate::flow_slice_content::SliceExpr::UnreducedCallValue => Positional::Unmodeled,
            // Content the demand slice did not select: never lowered,
            // never evaluable. Reaching one is a planner/content mismatch
            // at THIS position — the marker, never a fabricated `any` and
            // never the enclosing structure.
            crate::flow_slice_content::SliceExpr::Elided => Positional::Unmodeled,
        }
    }

    fn node_is_semantic_any(&self, root: SemanticNodeId) -> bool {
        let graph = self.dispatch.graph();
        let mut pending = vec![root];
        let mut seen = Vec::new();
        while let Some(node) = pending.pop() {
            if seen.contains(&node) {
                continue;
            }
            seen.push(node);
            match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Primitive(PrimitiveKind::Any)) => return true,
                Some(SemanticNodeData::Alias(target)) => pending.push(*target),
                Some(SemanticNodeData::Union(arms))
                | Some(SemanticNodeData::Intersection(arms)) => {
                    pending.extend(arms.iter().copied());
                }
                _ => {}
            }
        }
        false
    }

    /// Evaluate one argument of an authored call IN THIS FRAME: a bare
    /// identifier names a frame binding (a parameter or a reaching
    /// local), which owner-scope lowering cannot see; every other shape
    /// lowers through the owner-scope indexed evaluation.
    fn eval_indexed_call_argument(
        &mut self,
        expression: &verter_type_expr::IndexedValueExpression,
    ) -> Option<SemanticNodeId> {
        // A bare identifier names a frame binding (a parameter or a
        // reaching local), which owner-scope lowering cannot see. The
        // indexed lowering spells one as either a bare `Ref` or a
        // single-segment `typeof` path — both are the same read here.
        let bare_name = match expression {
            verter_type_expr::IndexedValueExpression::Value(verter_type_expr::TypeExpr::Ref {
                name,
                type_arguments,
            }) if type_arguments.is_empty() && !name.contains('.') => Some(name.as_ref()),
            verter_type_expr::IndexedValueExpression::Value(
                verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef { path, type_args }),
            ) if type_args.is_empty() && path.len() == 1 => Some(path[0].as_str()),
            _ => None,
        };
        if let Some(name) = bare_name {
            // The narrowing overlay answers a bare-argument read exactly
            // like the frame's own `Param` / `Local` carriers do.
            let param_ordinal = self
                .param_names
                .iter()
                .position(|param| param.name.as_deref() == Some(name))
                .map(|ordinal| ordinal as u32);
            let local_subject = crate::flow_slice_content::SliceNarrowSubject {
                root: crate::flow_slice_content::SliceNarrowRoot::Local(Arc::from(name)),
                path: Arc::from(Vec::new().into_boxed_slice()),
            };
            if let Some(node) = self.narrowed_read(&local_subject) {
                return Some(node);
            }
            if let Some(node) = param_ordinal.and_then(|ordinal| {
                self.narrowed_read(&crate::flow_slice_content::SliceNarrowSubject {
                    root: crate::flow_slice_content::SliceNarrowRoot::Param(ordinal),
                    path: Arc::from(Vec::new().into_boxed_slice()),
                })
            }) {
                return Some(node);
            }
            if let Some(node) = self.read_local(name) {
                return Some(node);
            }
            if let Some(ordinal) = param_ordinal {
                if let Some(node) = self
                    .param_writes
                    .get(&ordinal)
                    .or_else(|| self.params.get(ordinal as usize))
                {
                    return Some(*node);
                }
            }
        }
        self.dispatch.evaluate_indexed_value_expression_node_inner(
            self.canonical,
            self.owner,
            expression,
            false,
        )
    }

    /// The direct-call target's callee VALUE TYPE through the same
    /// owner-scope path the annotated-callee arm uses — never the
    /// initializer's flow position.
    fn direct_callee_value_node(
        &self,
        target: &verter_semantic::analysis::function_program::FunctionProgramKey,
    ) -> Option<SemanticNodeId> {
        let callee_expr = verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
            path: target
                .declaration
                .name
                .split('.')
                .map(str::to_string)
                .collect(),
            type_args: Vec::new(),
        });
        self.dispatch.lower_type_expr_in_owner_scope_with_context(
            self.canonical,
            self.owner,
            &callee_expr,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )
    }

    /// Whether one call site's callee — as a settled signature group —
    /// needs the executor's applicability machinery: an OVERLOADED group
    /// (argument-driven first-applicable selection), or a generic
    /// signature whose call SUPPLIES inference evidence (arguments or
    /// explicit type arguments), which this rail's clause transfer would
    /// otherwise answer `unknown` for.
    fn call_group_needs_executor(
        &self,
        node: SemanticNodeId,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> bool {
        let Some((_, call_sigs, construct_sigs)) = self.dispatch.settle_signature_group(node)
        else {
            return false;
        };
        if call_sigs.len() + construct_sigs.len() > 1 {
            return true;
        }
        let supplies_evidence =
            site.supplies_parameter_ordinal(0) || site.has_explicit_type_arguments();
        if !supplies_evidence {
            return false;
        }
        call_sigs
            .iter()
            .chain(construct_sigs.iter())
            .any(|signature| {
                matches!(
                    self.dispatch.graph().node_data(*signature).as_deref(),
                    Some(SemanticNodeData::Signature {
                        type_parameters,
                        ..
                    }) if !type_parameters.is_empty()
                )
            })
    }

    /// The call-executor route of one authored call: overload selection
    /// and argument-driven clause inference are the executor's
    /// applicability machinery, so this rail re-reads the authored call
    /// from the retained snapshot (argument expressions and explicit type
    /// arguments are parse facts, never re-derived), mints the
    /// content-free key, and folds the typed outcome back into the
    /// frame's vocabulary — a completed, already-instantiated return
    /// through the callee gate's named no-transfer constructor, an
    /// SCC back-edge as a hold, a typed refusal as the
    /// `UnrepresentableCallee` degradation this rail already defines for
    /// a callee it cannot represent.
    fn eval_call_via_resolve_call(
        &mut self,
        callee: SemanticNodeId,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> Option<Positional<CallValue>> {
        let Some(serve) = self.dispatch.ctx.ensure_indexed_ready_serve(self.canonical) else {
            return Some(self.degraded_unrepresentable_callee());
        };
        let memo = serve.indexed.shallow_state.decl_bodies();
        let Some(indexed) = memo.indexed_call_expression_at(site.span()) else {
            return Some(self.degraded_unrepresentable_callee());
        };
        let call = indexed.as_ref();
        let mut args = Vec::with_capacity(call.args.len());
        for argument in call.args.iter() {
            let Some(ty) = self.eval_indexed_call_argument(&argument.expression) else {
                // An argument this substrate cannot type leaves
                // applicability without its evidence: the executor
                // refuses as surely, and degrading here is the same
                // typed marker with one less hop.
                return Some(self.degraded_unrepresentable_callee());
            };
            args.push(crate::semantic_query::CallArgKey::Eager {
                ty,
                spread: argument.spread,
                context_sensitive: argument.context_sensitive,
                literal_mode: match argument.literal_mode {
                    verter_type_expr::IndexedValueLiteralMode::Widened => {
                        crate::semantic_query::ArgumentLiteralMode::Widened
                    }
                    verter_type_expr::IndexedValueLiteralMode::Literal => {
                        crate::semantic_query::ArgumentLiteralMode::Literal
                    }
                },
            });
        }
        let mut explicit_type_args = Vec::with_capacity(call.explicit_type_args.len());
        for argument in call.explicit_type_args.iter() {
            let Some(node) = self.dispatch.lower_type_expr_in_owner_scope_with_mode(
                self.canonical,
                self.owner,
                argument,
                crate::semantic_query::ProjectionMode::Navigate,
            ) else {
                return Some(self.degraded_unrepresentable_callee());
            };
            explicit_type_args.push(node);
        }
        // A member call's receiver rides the key: `.call` / `.apply`
        // rebase and `this`-typed methods read it — the same indexed
        // lowering the callee came from, evaluated in the same scope.
        let receiver = match call.receiver.as_deref() {
            Some(receiver) => match self.eval_indexed_call_argument(receiver) {
                Some(node) => Some(node),
                None => return Some(self.degraded_unrepresentable_callee()),
            },
            None => None,
        };
        let key = crate::semantic_query::ResolveCallKey {
            point: crate::semantic_query::ProgramPointId {
                canonical_id: Arc::from(self.canonical),
                offset: call.point,
            },
            callee,
            kind: crate::semantic_query::CallKind::Call,
            receiver,
            args: Arc::from(args.into_boxed_slice()),
            explicit_type_args: Arc::from(explicit_type_args.into_boxed_slice()),
            flow: crate::semantic_query::FlowNarrowingKey::empty(),
            context: self.dispatch.resolve_call_context_for(self.canonical),
        };
        let step = self.dispatch.execute_resolve_call(key);
        match step {
            super::call_resolve::ResolveCallStep::Complete(result) => {
                // Fresh-literal provenance: a call that closed on a FRESH
                // literal return (a generic callee's naked-binder return
                // fixed to a fresh-preserved literal) feeds the flow
                // frame's freshness widening — the return join widens it,
                // a value position keeps it.
                if let crate::semantic_query::ResolvedCallResult::Selected {
                    return_type,
                    fresh_literal_return: true,
                    ..
                } = &result
                {
                    self.call_fresh_literal_returns.push(*return_type);
                }
                Some(Positional::Value(CallValue::of_resolved_call(
                    self.dispatch,
                    super::return_equation::resolved_call_return_type(&result),
                )))
            }
            // An SCC back-edge through the executor: the close discharges
            // it on the component's admitted results.
            super::call_resolve::ResolveCallStep::Hold(_) => Some(Positional::Hold),
            // A PROVEN refusal — the callee is not callable, or no
            // overload accepts the authored arguments — is the typed
            // `UnrepresentableCallee` degradation this rail already
            // defines; the executor never widens it to `any`, and
            // neither does this rail.
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::NotCallable
                | crate::semantic_query::ResolveCallFailure::NoApplicableOverload,
            ) => Some(self.degraded_unrepresentable_callee()),
            // An UNDECIDED executor (the machinery cannot decide this
            // shape, or a budget edge) is NOT a refusal: the caller
            // falls back to this rail's own read of the same call, which
            // is never worse than the answer it gave without the
            // executor.
            super::call_resolve::ResolveCallStep::Degraded(
                crate::semantic_query::ResolveCallFailure::Undecidable
                | crate::semantic_query::ResolveCallFailure::Budget,
            ) => None,
        }
    }

    /// Evaluate one CALL to the value it contributes to this frame.
    ///
    /// The ONE place a callee's return becomes a caller's value — which
    /// makes it the ONE place the evaluation's call evidence is recorded:
    /// a call that evaluates to a value or a coinductive hold, without
    /// minting a fresh degradation, deposits its span (plus whether every
    /// relation outcome the resolution consumed was decided) onto the
    /// evidence ledger the discharge-report producer claims call and
    /// relation obligations from. An unmodelled or freshly-degraded call
    /// deposits nothing — its obligations stay unclaimed.
    fn eval_call(
        &mut self,
        call: &crate::flow_slice_content::SliceCall,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> Positional<CallValue> {
        let undecided_before = self.dispatch.dispatch_txn.borrow().call.undecided_relations;
        let degradation_before = self.degradation;
        let value = self.eval_call_value(call, site);
        // A call whose evaluation minted the frame's FIRST degradation
        // did not decide its occurrence (an already-degraded frame never
        // seals, so evidence accuracy past the first degradation cannot
        // affect admission).
        let newly_degraded = degradation_before.is_none() && self.degradation.is_some();
        if !matches!(value, Positional::Unmodeled) && !newly_degraded {
            let relations_decided =
                self.dispatch.dispatch_txn.borrow().call.undecided_relations == undecided_before;
            self.call_evidence.push(FlowCallEvidence {
                span: site.span(),
                relations_decided,
            });
        }
        value
    }

    /// The call sink's value computation ([`Self::eval_call`] records the
    /// evidence around it). Every arm returns a [`CallValue`], whose
    /// constructors each decide what happens to the CALLEE's own
    /// type-parameter clause — the rule cannot be silently skipped at a
    /// new arm, only chosen.
    ///
    /// [`Positional`], exactly as in [`Self::eval_expr`]: a call this
    /// substrate cannot resolve is an unmodelled POSITION, and the type
    /// leaves no way to say otherwise.
    fn eval_call_value(
        &mut self,
        call: &crate::flow_slice_content::SliceCall,
        site: crate::flow_slice_content::SliceCallSite,
    ) -> Positional<CallValue> {
        let graph = self.dispatch.graph();
        match call {
            crate::flow_slice_content::SliceCall::Nested(function) => {
                // An IIFE: the call's value is the nested function's
                // evaluated return. The nested function's signature node
                // carries its OWN clause — `(<T>(x: T): T => x)("a")`
                // declares `T` right there — so the same clause rule the
                // resolved-callee route applies has to apply here, and it
                // does, because both routes take their value from the ONE
                // signature reader.
                let signature = match self.eval_expr(function) {
                    Positional::Value(signature) => signature,
                    Positional::Hold => return Positional::Hold,
                    Positional::Unmodeled => return Positional::Unmodeled,
                };
                // A nested function value's signature is COMPOSED here:
                // its return is the flow join of its own body, evaluated
                // with its clause bound. A resolved same-named
                // declaration inside it is therefore a foreign symbol.
                match CallValue::of_signature_node(
                    self.dispatch,
                    signature,
                    site,
                    ReturnOrigin::ClauseScoped,
                ) {
                    SignatureCall::Value(value) => Positional::Value(value),
                    // An IIFE whose composed signature is not callable,
                    // whose return position missed, or whose clause could
                    // not be recovered: the CALL has no modelled value.
                    // The enclosing structure still does.
                    SignatureCall::NotCallable
                    | SignatureCall::ReturnMiss
                    | SignatureCall::ClauseUnavailable => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceCall::Direct(target) => {
                // An exact same-file direct call — a Flow obligation edge
                // through the ONE key construction when the callee's return
                // is body-derived, or its DECLARED carrier when the callee
                // annotates one (a declared return always wins over the
                // body). A back-edge to an in-flight target is a
                // coinductive hold (neither a contributor nor a failure);
                // an empty-cycle outcome is a hold the SCC close discharges
                // on the component's admitted returns; every other outcome
                // contributes the callee's return or its typed failure.
                let prepared = self.dispatch.ctx.prepared_value_decl_return_only(
                    self.canonical,
                    target.declaration.owner,
                    target.declaration.name.as_ref(),
                );
                // A value declaration carrying an AUTHORED annotation is
                // typed by that annotation, full stop: the initializer
                // only has to be assignable to it. `const f: () => 42 =
                // () => 42` IS `() => 42`, so `f()` is `42` — taking the
                // initializer's own inferred signature here would publish
                // `number`, confidently and warm, for a callee whose
                // declared type says otherwise. The annotated callee is
                // therefore resolved as a VALUE TYPE through the same
                // shared path a parenthesised or ambient callee takes,
                // never through the initializer's flow position.
                if prepared.as_ref().is_some_and(|prepared| {
                    matches!(
                        prepared.type_annotation.classification,
                        verter_type_expr::facts::ValueAnnotationClass::Direct
                            | verter_type_expr::facts::ValueAnnotationClass::TypeOfAlias
                    ) && matches!(
                        prepared.type_annotation.annotation,
                        Some(verter_type_expr::facts::SemanticTypeSource::Authored(_))
                    )
                }) {
                    let callee = verter_type_expr::TypeExpr::TypeOf(verter_type_expr::ValueRef {
                        path: target
                            .declaration
                            .name
                            .split('.')
                            .map(str::to_string)
                            .collect(),
                        type_args: Vec::new(),
                    });
                    let Some(callee_node) =
                        self.dispatch.lower_type_expr_in_owner_scope_with_context(
                            self.canonical,
                            self.owner,
                            &callee,
                            crate::semantic_query::ProjectionReductionContext::structural_transit(),
                        )
                    else {
                        return self.degraded_unrepresentable_callee();
                    };
                    return self.call_return_of_callee_node(callee_node, site);
                }
                let ordinal = match &target.part {
                    verter_type_expr::facts::FunctionPartIdentity::DeclarationBody => {
                        target.overload_ordinal as usize
                    }
                    _ => 0,
                };
                // An OVERLOADED callee is not answerable by this rail's
                // lone-signature read: TypeScript resolves an overloaded
                // call by ARGUMENTS, picking the FIRST applicable
                // signature in declaration order, and the function-program
                // index reaches only one entry of the group (the trailing
                // implementation the language HIDES, for a bodied group).
                // Argument-driven selection is the call executor's
                // applicability machinery, so the authored call routes
                // through it — the ordered group, the authored argument
                // literals, and a typed refusal when nothing applies,
                // never a warm-admitted wrong answer and never the
                // implementation's own signature.
                if prepared
                    .as_ref()
                    .is_some_and(|prepared| prepared.signatures.len() > 1)
                {
                    let Some(callee) = self.direct_callee_value_node(target) else {
                        return self.degraded_unrepresentable_callee();
                    };
                    // An undecided executor maps to the same degradation
                    // this rail would publish without it — there is no
                    // lone-signature read to fall back to.
                    return self
                        .eval_call_via_resolve_call(callee, site)
                        .unwrap_or_else(|| self.degraded_unrepresentable_callee());
                }
                let source = prepared.as_ref().and_then(|prepared| {
                    prepared
                        .signatures
                        .get(ordinal)
                        .map(|signature| signature.return_source.clone())
                });
                // The callee's OWN type-parameter clause. Whatever the
                // callee answers with — its body-derived flow return or
                // its DECLARED carrier — is expressed IN those binders, so
                // handing it back verbatim publishes the CALLEE's generic
                // parameter as THIS frame's value. Under the file-scoped
                // name-keyed binder identity that node is shared with
                // every same-named clause in the file, so an enclosing
                // `class Holder<T>` would then substitute the caller's
                // `Holder<number>` into a value that has nothing to do
                // with it — cleanly and warm.
                //
                // Instantiating those parameters is the same rule the
                // sibling callee-TYPE / signature-node routes apply
                // (`CallValue::of_signature_node`), so EVERY route to one
                // callee answers alike. Call-site instantiation proper —
                // explicit type arguments AND argument inference — is
                // not performed here; a DECLARED DEFAULT is already exact
                // (`f<T = number>()` IS `number`), and `unknown` is the
                // interim answer everywhere else — exact for the one shape
                // TS itself cannot infer (`bare<T>(): T` called with no
                // arguments IS `unknown`).
                //
                // The clause is read from the per-file FUNCTION PROGRAM
                // INDEX, keyed by the target's exact program identity
                // (part + overload ordinal), NOT from the prepared value
                // declaration: a direct-call target is a served position
                // of THIS file by construction, while the value registry
                // does not carry every one of them — a namespace-scoped
                // function has no prepared declaration, and reading the
                // clause from there would silently leave exactly those
                // callees leaking their binder.
                let callee_clause = match self.direct_callee_clause(target, site) {
                    CalleeClauseLookup::Clause(clause) => clause,
                    // The callee's clause could not be READ. Handing its
                    // return back with nothing instantiated is the leak;
                    // this is the `UnrepresentableCallee` degradation the
                    // rail already defines — usable, `ReturnOnly`, never
                    // warm.
                    CalleeClauseLookup::Unavailable => {
                        return self.degraded_unrepresentable_callee()
                    }
                };
                // Argument-driven clause inference (and explicit type
                // arguments) are the call executor's domain too: a
                // generic callee whose call SUPPLIES inference evidence
                // routes through it, binding the clause from the authored
                // argument literals with constraint / declared-default
                // fallback — rather than this rail's `unknown` interim
                // for every parameter the arguments could have bound. A
                // call supplying NO evidence keeps the rail's answer: a
                // declared default is exact, and `unknown` is the
                // checker's own answer for an unbound parameter.
                if callee_clause.is_generic()
                    && (site.supplies_parameter_ordinal(0) || site.has_explicit_type_arguments())
                {
                    if let Some(callee) = self.direct_callee_value_node(target) {
                        if let Some(value) = self.eval_call_via_resolve_call(callee, site) {
                            return value;
                        }
                    }
                }
                // A target the value registry does not carry as a
                // prepared declaration (a namespace-scoped function) is
                // only reachable through the body-derived demand.
                let source = source.unwrap_or_else(|| {
                    verter_type_expr::facts::FunctionReturnSource::Flow(
                        verter_type_expr::facts::FlowFunctionReturnIdentity {
                            anchor: verter_type_expr::locators::AuthoredAnchor {
                                canonical_id: Arc::from(self.canonical),
                                owner: target.declaration.owner,
                                symbol: Arc::clone(&target.declaration.name),
                                space: verter_type_expr::locators::LocatorSymbolSpace::Value,
                            },
                            function_part: target.part.clone(),
                            overload_ordinal: target.overload_ordinal,
                        },
                    )
                });
                match &source {
                    verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                        let key = self.dispatch.flow_return_key_for(identity);
                        let pending_before = self
                            .dispatch
                            .dispatch_txn
                            .borrow()
                            .obligations
                            .pending()
                            .pending_len();
                        match self.dispatch.execute_flow_return(key.clone()) {
                            FlowReturnStep::Complete(result) => {
                                // A degraded callee value degrades every
                                // consumer of that value: absorb the
                                // callee's typed reason into this frame.
                                if let Some(degradation) = result.degradation() {
                                    self.record_degradation(degradation);
                                }
                                // A callee that pops as a PROVISIONAL
                                // member of THIS component leaves its
                                // result provisional until the close's
                                // equation fixed point — the call is an
                                // edge. A callee that closed its own SCC
                                // independently is final: no edge.
                                if self
                                    .dispatch
                                    .dispatch_txn
                                    .borrow()
                                    .obligations
                                    .pending()
                                    .pending_len()
                                    > pending_before
                                {
                                    // The hold carries the SAME clause this
                                    // arm just applied: the fixed point
                                    // repeats this transfer, so it owes the
                                    // same obligation.
                                    self.holds
                                        .push(HeldCallee::foreign(key, callee_clause.clone()));
                                }
                                Positional::Value(CallValue::of_served_return(
                                    self.dispatch,
                                    &callee_clause,
                                    result.return_type(),
                                    ReturnOrigin::ClauseScoped,
                                ))
                            }
                            FlowReturnStep::Hold(key) => {
                                self.holds
                                    .push(HeldCallee::foreign(*key.clone(), callee_clause));
                                Positional::Hold
                            }
                            FlowReturnStep::NoValue(FlowReturnFailure::EmptyCycle) => {
                                // An empty-cycle callee IS a hold — the SCC
                                // close discharges it (and its callers) on
                                // the component's admitted returns.
                                self.holds.push(HeldCallee::foreign(key, callee_clause));
                                Positional::Hold
                            }
                            // The CALLEE's frame failed. That is a fact
                            // about the callee's body, and it reaches this
                            // frame at exactly one place: the call.
                            // Re-raising it here is what made one helper's
                            // unmodelled control surface delete the whole
                            // caller's surface.
                            FlowReturnStep::NoValue(_) => Positional::Unmodeled,
                        }
                    }
                    source => match self
                        .dispatch
                        .execute_function_return_source(source, self.canonical)
                    {
                        // The callee's DECLARED return locator, lowered in
                        // file owner scope where its own clause is not in
                        // scope: the resolved same-named declaration IS
                        // the clause parameter, misresolved.
                        super::flow_return::FunctionReturnNode::Declared(hot) => {
                            Positional::Value(CallValue::of_served_return(
                                self.dispatch,
                                &callee_clause,
                                hot.node(),
                                ReturnOrigin::OwnerScopeDeclared,
                            ))
                        }
                        // A declared locator that would not raise, and a
                        // signature with NO recoverable return carrier,
                        // both leave this CALL without a value.
                        super::flow_return::FunctionReturnNode::DeclaredMiss
                        | super::flow_return::FunctionReturnNode::Absent => Positional::Unmodeled,
                        super::flow_return::FunctionReturnNode::Flow(_)
                        | super::flow_return::FunctionReturnNode::NoValue(_) => {
                            unreachable!("a Declared/Absent source never reaches the flow rail")
                        }
                    },
                }
            }
            crate::flow_slice_content::SliceCall::OnBinding {
                param,
                name,
                captured,
            } => {
                // A call on a function-typed binding: the call's value is
                // the binding's signature return. Calling an `any`-typed
                // or unbound binding is `any` EXACTLY (the implicit-`any`
                // call); calling a binding whose value is neither
                // callable nor `any` is the `NonCallableBinding`
                // DEGRADATION — a modeled `any`, not the real semantics.
                // The binding's own reaching definition wins; the
                // parameter ordinal is the FALLBACK a `var` redeclaring
                // a parameter name resolves to before its declarator
                // runs (mirrors the `Local` read).
                let node = self.read_local(name.as_ref()).or_else(|| {
                    param.and_then(|ordinal| self.params.get(ordinal as usize).copied())
                });
                let Some(node) = node else {
                    // A CAPTURED callee the seeded snapshot does not carry
                    // has no honest value: the POSITION carries the marker
                    // rather than the same-frame implicit-`any` call.
                    if *captured {
                        return Positional::Value(self.unmodeled_call_position());
                    }
                    return Positional::Value(CallValue::modeled_any(self.dispatch));
                };
                // A function-typed BINDING carries the callee's clause on
                // its own signature node — `const id = <T>(x: T): T => x`
                // binds `T` there — so this route takes its value from the
                // same signature reader the resolved-callee route does. A
                // binding is otherwise indistinguishable from any other
                // callee: nothing about "the callee happens to be a local"
                // makes its binders this frame's to publish.
                // A binding's value is either a nested function value's
                // COMPOSED signature or a lowered function-TYPE
                // annotation, and BOTH are clause-scoped: the composed
                // signature is built in the callee's own frame, and a
                // `<T>(x: T) => T` annotation lowers with its clause in
                // scope, so both spell the clause as binders. The one
                // shape that could carry an owner-scope misresolution —
                // `typeof declaredFn` — does not reach a `Signature`
                // node here at all (it resolves to a non-callable
                // surface and degrades), so nothing on this route needs
                // the owner-scope claim while claiming it destroys a
                // correct arm: a same-named FOREIGN declaration reached
                // through the callee's body is a different symbol, and
                // the IIFE route — the same body, invoked directly —
                // already keeps it.
                // An overloaded or inference-bearing binding-held callee
                // resolves through the executor, never the first
                // signature's raw return; an undecided executor falls
                // back to the rail's own signature read (overloads keep
                // the typed degradation — the read cannot pick).
                if self.call_group_needs_executor(node, site) {
                    let overloaded = matches!(
                        self.dispatch.settle_signature_group(node),
                        Some((_, call_sigs, construct_sigs))
                            if call_sigs.len() + construct_sigs.len() > 1
                    );
                    if let Some(value) = self.eval_call_via_resolve_call(node, site) {
                        return value;
                    }
                    if overloaded {
                        return self.degraded_unrepresentable_callee();
                    }
                }
                match CallValue::of_signature_node(
                    self.dispatch,
                    node,
                    site,
                    ReturnOrigin::ClauseScoped,
                ) {
                    SignatureCall::Value(value) => Positional::Value(value),
                    // The binding's signature has no transferable return:
                    // its return position missed, or a needed clause
                    // default could not be recovered. Positional.
                    SignatureCall::ReturnMiss | SignatureCall::ClauseUnavailable => {
                        Positional::Unmodeled
                    }
                    SignatureCall::NotCallable
                        if matches!(
                            graph.node_data(node).as_deref(),
                            Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
                        ) =>
                    {
                        Positional::Value(CallValue::modeled_any(self.dispatch))
                    }
                    SignatureCall::NotCallable => {
                        self.record_degradation(
                            crate::semantic_query::FlowReturnDegradation::NonCallableBinding,
                        );
                        Positional::Value(CallValue::modeled_any(self.dispatch))
                    }
                }
            }
            crate::flow_slice_content::SliceCall::LocalFunctionShadow => {
                // A call to a hoisted nested function declaration: the
                // declaration shadows every outer same-name callee; exact
                // recovery of the nested declaration's own return is not
                // implemented. The CALL carries the marker — never the
                // outer callee's value, and never the enclosing frame.
                Positional::Unmodeled
            }
            crate::flow_slice_content::SliceCall::DirectSelf => {
                // Only the frame that OWNS a flow slot can hold on it.
                let Some(self_slot) = self.self_slot else {
                    return Positional::Unmodeled;
                };
                match self.dispatch.execute_flow_return(self_slot.clone()) {
                    FlowReturnStep::Hold(_) => {
                        // The one hold whose target's binders are THIS
                        // frame's own: a self-call's callee IS the caller,
                        // so the fixed point must leave them alone.
                        self.holds.push(HeldCallee::own_frame(self_slot.clone()));
                        Positional::Hold
                    }
                    FlowReturnStep::Complete(_) => {
                        unreachable!("a same-slot recursive edge is always a hold in flight")
                    }
                    FlowReturnStep::NoValue(_) => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceCall::Symbolic(ty) => {
                // The symbolic `ReturnType<typeof …>` carrier: lower the
                // callee, resolve its signature through the same builtin
                // `ReturnType` reduction every consumer uses, and take the
                // call-bucket return — an unrepresentable / unresolvable
                // callee is the `UnrepresentableCallee` DEGRADATION: a
                // usable modeled-`any`, `ReturnOnly` by contract.
                let verter_type_expr::TypeExpr::Ref {
                    name,
                    type_arguments,
                } = ty
                else {
                    return self.degraded_unrepresentable_callee();
                };
                if name.as_ref() != "ReturnType" || type_arguments.len() != 1 {
                    return self.degraded_unrepresentable_callee();
                }
                // A member-call callee rooted at a FRAME binding
                // (`fn.call`, `local.call`, `instance.run`): the owner
                // scope answers nothing for the root, but the frame
                // substitutes it — resolve the root through the frame and
                // project the member tail, exactly as the frame-rooted
                // `typeof` leaf does. Everything else lowers in owner
                // scope as before.
                let mut frame_rooted = false;
                let Some(callee_node) =
                    (match self.frame_rooted_typeof_path_node(&type_arguments[0]) {
                        Some(node) => {
                            frame_rooted = true;
                            Some(node)
                        }
                        None => self.dispatch.lower_type_expr_in_owner_scope_with_context(
                            self.canonical,
                            self.owner,
                            &type_arguments[0],
                            crate::semantic_query::ProjectionReductionContext::structural_transit(),
                        ),
                    })
                else {
                    return self.degraded_unrepresentable_callee();
                };
                // A FRAME-ROOTED member callee carries receiver semantics
                // this rail's lone signature read is blind to (the ambient
                // `.call` / `.apply` rebase, a `this`-typed method): route
                // it through the executor first. The fallback is exactly
                // the answer this rail gave before the executor existed —
                // a frame-rooted member callee lowered to nothing then, so
                // nothing that worked can regress. An undecided executor
                // (`None`) keeps the rail's own read below.
                if frame_rooted {
                    if let Some(value) = self.eval_call_via_resolve_call(callee_node, site) {
                        return value;
                    }
                }
                // An overloaded or inference-bearing symbolic callee
                // resolves through the executor exactly like a direct
                // one — the lone non-generic read stays on this rail,
                // and an undecided executor falls back to it.
                if self.call_group_needs_executor(callee_node, site) {
                    let overloaded = matches!(
                        self.dispatch.settle_signature_group(callee_node),
                        Some((_, call_sigs, construct_sigs))
                            if call_sigs.len() + construct_sigs.len() > 1
                    );
                    if let Some(value) = self.eval_call_via_resolve_call(callee_node, site) {
                        return value;
                    }
                    if overloaded {
                        return self.degraded_unrepresentable_callee();
                    }
                }
                self.call_return_of_callee_node(callee_node, site)
            }
        }
    }
}
