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
    FlowDemandCarrier, FlowEvaluationProvenance, FlowPlanRefusal, ObservedFlowConvergence,
};
use super::dispatch_txn::{
    CompletedFlowReturnMember, FlowReturnPendingOutcome, FlowReturnPendingState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
};
use super::flow_products::{
    join_frame_products, DefiniteAssignment, DefiniteAssignmentProduct, FlowBindingLayer,
    FlowFrameBindings, FlowFrameJoinOutcome, FlowNarrowingFact, FlowProductBudget,
    FlowProductBudgetExceeded, FlowProductStore, FlowProductSubject, NarrowingProduct,
    ReachingTypeProduct, WideningMembership,
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
pub(super) const NO_VALUE_REASON_CLASS: PartialReasonSet = PartialReasonSet::FLOW_RETURN_NO_SURFACE;

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
/// The partial class of a verdict-less close: the demand installed no
/// proof layer, so the evaluated value flows unproven. The class follows
/// the recorded CAUSE, because the causes sit on opposite sides of the
/// consumer containment axis:
///
/// - a BUDGET refusal (the obligation-set cap, or the slice budget read
///   through the retained plan) is a statement about the REQUEST — the
///   same axis as the adjacent slice-budget refusal — and takes
///   [`PartialReasonSet::BUDGET_EXCEEDED`], which every Vue macro
///   projection lane refuses to contain. Landing it on the contained
///   degraded-success class let a value-deriving lane publish `type:
///   null` for every member while reporting Complete;
/// - a TORN prepare-time view (the retained artifacts were missing at
///   preparation while the evaluation still produced a value) is a
///   transient-state statement, non-deterministic by nature, and takes
///   [`PartialReasonSet::UNSTABLE_STATE`];
/// - an UNPLANNABLE demand — and the deliberate refused-member-batch
///   close, which records no cause — keeps the contained
///   [`PartialReasonSet::FLOW_RETURN_UNVERIFIED`]: the member set is
///   complete and the value usable, merely unverified.
pub(super) fn plan_refusal_reason_class(refusal: Option<FlowPlanRefusal>) -> PartialReasonSet {
    match refusal {
        Some(FlowPlanRefusal::Budget) => PartialReasonSet::BUDGET_EXCEEDED,
        Some(FlowPlanRefusal::TornView) => PartialReasonSet::UNSTABLE_STATE,
        Some(FlowPlanRefusal::Unplannable) | None => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
    }
}

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

/// The partial class of a finalizer PARTIAL verdict: the typed
/// [`FlowPartialReason`] and the value's own typed degradation, UNIONED —
/// a cause recorded on either channel must reach the consumer, because
/// the causes sit on opposite sides of the containment axis. A torn,
/// stale, foreign, or incoherent proof state (`NoDemandInstalled` /
/// `StaleBasis` / `ForeignProvenance` / `ObligationSetMismatch`, plus a
/// stale/panic/internal typed failure) is a transient-state statement and
/// takes [`PartialReasonSet::UNSTABLE_STATE`]; non-convergence and a
/// budget-class failure are resource-cap statements and take
/// [`PartialReasonSet::BUDGET_EXCEEDED`]; a cancellation takes
/// [`PartialReasonSet::CANCELLED`]. The evidence-shaped causes — a typed
/// obligation gap, an unprovable operation contract, the degraded-value
/// echo — keep the contained degraded-success classes, and so does the
/// bare `IncompleteObligations` echo: an obligation left PENDING is the
/// close's own withholding signature (a genuinely budget-refused
/// obligation reaches the ledger as a typed `Failed` budget record and
/// takes the budget class here). Classifying a faulting cause onto a
/// contained class would let every Vue macro projection lane publish and
/// warm around work that never ran.
///
/// [`FlowPartialReason`]: super::flow_solve::FlowPartialReason
pub(super) fn flow_partial_reason_class(
    reason: &super::flow_solve::FlowPartialReason,
    degradation: Option<FlowReturnDegradation>,
) -> PartialReasonSet {
    use super::flow_solve::{FlowFailureClass, FlowPartialReason};
    let value_class = match degradation {
        Some(degradation) => degradation_reason_class(degradation),
        None => PartialReasonSet::default(),
    };
    let reason_class = match reason {
        // Two ECHO reasons contribute nothing beyond the value's own
        // class: `DegradedValue` restates the degradation the value
        // already carries, and a bare pending-obligation echo is the
        // close's own withholding signature (a genuinely budget-refused
        // obligation reaches the ledger as a typed `Failed` budget
        // record instead). Widening either onto the frame-wide class
        // would erase a positional degradation's faithful siblings; the
        // frame-wide contained class covers only the degradation-free
        // spelling.
        FlowPartialReason::DegradedValue | FlowPartialReason::IncompleteObligations => {
            match degradation {
                Some(_) => PartialReasonSet::default(),
                None => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
            }
        }
        FlowPartialReason::Gap(_)
        | FlowPartialReason::OperationNotProvable
        | FlowPartialReason::ResultContractMismatch => PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        FlowPartialReason::NoDemandInstalled
        | FlowPartialReason::StaleBasis
        | FlowPartialReason::ForeignProvenance
        | FlowPartialReason::ObligationSetMismatch => PartialReasonSet::UNSTABLE_STATE,
        FlowPartialReason::NonConverged => PartialReasonSet::BUDGET_EXCEEDED,
        FlowPartialReason::Failed(failure) => match failure.class {
            FlowFailureClass::BudgetExhausted => PartialReasonSet::BUDGET_EXCEEDED,
            FlowFailureClass::Cancelled => PartialReasonSet::CANCELLED,
            FlowFailureClass::StaleBasis | FlowFailureClass::Panic | FlowFailureClass::Internal => {
                PartialReasonSet::UNSTABLE_STATE
            }
        },
    };
    value_class.union(reason_class)
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

        /// When armed, `prepare_flow_return_demand` plans under a ZERO
        /// obligation budget, so the demand planner returns its typed
        /// budget refusal before constructing a single obligation —
        /// modelling a real demand whose obligation set exceeds the
        /// request's resource policy without authoring a function large
        /// enough to trip the production cap. Refuse-only: no demand
        /// installs, no proof can mint, nothing can warm.
        pub(crate) zero_obligation_budget: AtomicBool,

        /// When armed, the NEXT demand-site derivation sees the function's
        /// own file as UNSERVED and refuses — modelling the store read
        /// that did not serve an artifact the resolved declaration slot
        /// was minted over. Consumed by that first derivation (one shot),
        /// so the evaluation's own derivation then succeeds: exactly the
        /// torn shape the transient class exists for — preparation
        /// refuses while the evaluation still produces a value. There is
        /// no way to author this race; the slot presents it. Refuse-only:
        /// no demand installs, no proof can mint, nothing can warm.
        pub(crate) unserved_demand_site: AtomicBool,

        /// When armed, the INJECTED unproven member records a
        /// preparation refusal, while the enclosing root — of any
        /// domain, including a flow root that plans a demand of its own
        /// — prepares normally. Consumed by the injection (one shot).
        ///
        /// It exists because the cause under test must provably come
        /// from the MEMBER: a globally armed slot would refuse the
        /// root's own preparation too, and the resulting class would say
        /// nothing about whether a member's cause survives its deferral.
        /// Refuse-only: a recorded refusal withholds proof and can never
        /// mint, promote, or warm anything.
        pub(crate) refuse_injected_member_demand: AtomicBool,

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
    /// The recorded preparation refusal when `verdict` is `None` because
    /// no demand installed — the close classifies the unproven outcome by
    /// this cause ([`plan_refusal_reason_class`]). `None` for the
    /// refused-member-batch withholding, whose own cause is carried
    /// separately below.
    plan_refusal: Option<FlowPlanRefusal>,
    /// The partiality classes the REFUSED MEMBERS' recorded causes belong
    /// to. The root's own preparation can be spotless while a member's
    /// budget edge or torn view is what withheld the verdict, so the two
    /// causes are unioned rather than either one standing alone.
    member_batch_partial_reasons: PartialReasonSet,
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

/// A demand site that could not be derived: the typed no-value failure
/// the evaluation surfaces, the roots observed before the failure, and
/// the preparation-refusal class this particular edge belongs to.
///
/// The refusal class is decided HERE and not re-derived from `failure`
/// because the site is the only place that can tell the two apart: a
/// store read that did not serve the artifact is a TRANSIENT torn view,
/// while an index lookup that found no such function is a DETERMINISTIC
/// statement about the program. Both spell
/// [`FlowReturnFailure::Missing`], so a caller matching on the failure
/// alone would classify the transient edge as contained and under-fault
/// every consumer of an unstable read.
struct FlowSliceDemandSiteError {
    /// The typed no-value failure the evaluation path surfaces.
    failure: FlowReturnFailure,
    /// The roots observed before the failure (empty when the failure
    /// preceded the serve).
    self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The preparation-refusal class this edge belongs to.
    refusal: FlowPlanRefusal,
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
            self.fold_observed_frame_into_top(&observed);
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
                    plan_refusal,
                    member_batch_partial_reasons,
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
                    // boolean rails now agree. The class follows BOTH
                    // recorded causes — the value's degradation AND the
                    // finalizer's typed reason: a stale basis or an
                    // undischarged obligation over a non-degraded value
                    // must fault the lanes, never fall to the contained
                    // class.
                    Some(FlowSolveOutcome::Partial(partial)) => {
                        output.cache_suppress = true;
                        output.result_is_partial = true;
                        output.partial_reasons =
                            flow_partial_reason_class(&partial.reason, partial.value.degradation());
                    }
                    // The demand could not be planned at all, or a
                    // refused member batch withheld the root's proof:
                    // unproven, ReturnOnly. The partial class follows the
                    // recorded CAUSE — a budget refusal takes the faulting
                    // request class its sibling slice-budget axis takes, a
                    // torn prepare-time view takes the unstable-state
                    // class, and only a genuinely unplannable demand (or
                    // the member-batch withholding, which records no
                    // cause) keeps the contained degraded-success class.
                    None => {
                        output.cache_suppress = true;
                        output.result_is_partial = true;
                        output.partial_reasons = plan_refusal_reason_class(plan_refusal)
                            .union(member_batch_partial_reasons);
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
                FlowRootClose::NoValue(failure) => {
                    // The inline path produces NO memo read, so the
                    // universal read funnel never sees this failure: fold
                    // its rails into the ENCLOSING build here — exactly as
                    // the machinery root's build output does and the
                    // consumer-side hold arm does — so the request-partial
                    // sticky survives even a lenient composition that
                    // absorbs the typed failure into a usable answer. The
                    // typed failure itself still rides the returned step.
                    self.fold_cache_read_rails(true, true, NO_VALUE_REASON_CLASS);
                    FlowReturnStep::NoValue(failure)
                }
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

    /// The final IDEMPOTENT pre-seal closure of a flow result: one
    /// canonical re-close of the whole-return node, run exactly once per
    /// close — after the component's fixed point, the post-convergence
    /// literal widening, and the per-key substitution, and strictly
    /// BEFORE `finalize_flow_demand` seals the value (the proof then
    /// covers exactly the closed value). It never runs in a forbidden
    /// position: it touches no flow-slice hash input, no raw-evidence
    /// storage, no `MergedDecl` or other ordered merge carrier, no
    /// already-sealed completion, and no display path.
    ///
    /// Scope is deliberately the TOP-LEVEL composite only — the join flow
    /// construction derives is UNION-kinded, and "normalization normalizes
    /// only the already-demanded semantic portion". A UNION top re-closes
    /// through the canonical authority (flatten, lattice absorption,
    /// structural `T | T = T`) — with an O(1) canonicality test first: a
    /// top whose payload carries the at-rest `Canonical` origin category
    /// was minted by a COMPLETE canonicalization — the canonical builder
    /// refuses that stamp on incomplete evidence (over-cap arm set,
    /// exhausted compare budget, dangling arm, undecided peek all mint
    /// `CanonicalUnproven`) — so its list IS in canonical form (a
    /// complete canonicalization is deterministic over immutable node
    /// data) and the closure returns it unchanged WITHOUT re-running the
    /// pipeline — no evidence deposit and no evidence-epoch advance on
    /// the pure no-op path. Any other tag pays the full idempotent
    /// re-close: `CanonicalUnproven` (so a budget-degraded list
    /// resurfacing in a later request re-deposits its `incomplete`
    /// evidence — ReturnOnly, never warm-classified), and a
    /// canonical-form list that first interned under a bypass mint (the
    /// first-wins tag).
    ///
    /// An INTERSECTION top passes through verbatim — but NOT because flow
    /// cannot derive one. It can: the per-key substitution inside this
    /// same close path rebuilds intersections (`substitute.rs`), and
    /// predicate narrowing constructs one directly. The passthrough is
    /// sound because every flow-derived intersection top is already
    /// closed by its constructor — predicate narrowing and the
    /// order-safe substitution arm route through the canonical
    /// authority, while the possibly-callable substitution arm is an
    /// overload-ORDERED carrier the commutative route must not reorder —
    /// and it is checker-faithful: the pinned compiler preserves the
    /// same duplicated intersection for the composed-generic case
    /// (`declare function tag<T>(v: T): T & (() => void)` composed over
    /// a function argument declaration-emits
    /// `(() => void) & (() => void)`, measured against tsc 7.0.2).
    /// Every other shape passes through verbatim.
    ///
    /// Canonicalization evidence deposits ambiently through the single
    /// disposition funnel: inspected file self-roots reach the enclosing
    /// build's memo entry, and an `Incomplete` comparison suppresses
    /// warm admission (ReturnOnly) without altering the value.
    pub(super) fn close_flow_result_pre_seal(&self, result: FlowReturnResult) -> FlowReturnResult {
        let node = result.return_type();
        let Some(data) = self.graph().node_data(node) else {
            return result;
        };
        let SemanticNodeData::Union(members) = &*data else {
            return result;
        };
        if members.origin_category()
            == crate::semantic_query::composite::CompositeOriginCategory::Canonical
        {
            // O(1) canonicality: a canonical-minted list is canonical form;
            // re-closing it is the idempotence no-op — skip the pipeline.
            return result;
        }
        let members: Vec<crate::semantic_query::SemanticNodeId> = members.iter().copied().collect();
        drop(data);
        let closed = self.intern_normalized_union_or_intersection(&members, true);
        if closed == node {
            return result;
        }
        result.with_return_type(self.graph().as_ref(), closed)
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
        let knobs = &self.ctx.host_for_fact_tracer_install().flow_fault_injection;
        let refuse_member = knobs
            .refuse_injected_member_demand
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        self.prepare_flow_return_demand(&key, idx);
        if refuse_member {
            // Recorded DIRECTLY rather than by refusing one of the
            // preparation's own edges. Which edge a given member's
            // preparation would refuse on depends on its planning inputs
            // — a zero budget is satisfied by an empty obligation set,
            // and a site refusal installs no carrier — so driving the
            // cause through the planner made this slot's outcome depend
            // on the member's incidental shape. Those edges have their
            // own tests; this slot's subject is whether a recorded cause
            // SURVIVES the member's deferral to the root that consumed
            // its value, and that is what it presents.
            self.record_flow_plan_refusal(idx, FlowPlanRefusal::TornView);
        }
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
        let site = match self.flow_slice_demand_site(key) {
            Ok(site) => site,
            // The SITE classifies its own edge: a store read that did not
            // serve is transient, an unrepresentable demand is not. A
            // blanket `Unplannable` here would report every torn
            // prepare-time read as a contained unverified result.
            Err(err) => {
                self.record_flow_plan_refusal(frame_idx, err.refusal);
                return;
            }
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
            // The slice-budget refusal installs no demand: the
            // evaluation's own hash-node lookup normally reaches the same
            // typed failure, and if a racing recompute hands the
            // evaluation a planned slice instead, the close still
            // classifies the unproven value as the budget edge it is.
            Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::BudgetExceeded(
                _,
            )) => {
                self.record_flow_plan_refusal(frame_idx, FlowPlanRefusal::Budget);
                return;
            }
            // No retained outcome at all: a torn prepare-time view — the
            // evaluation may still find one, so the close classifies the
            // unproven value as unstable state, never as a verified-shape
            // degradation.
            None => {
                self.record_flow_plan_refusal(frame_idx, FlowPlanRefusal::TornView);
                return;
            }
        };
        let Some(bound) = flow_slice.bound_graph_for(&site.slice_key_function) else {
            self.record_flow_plan_refusal(frame_idx, FlowPlanRefusal::TornView);
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
        let resources = FlowResourcePolicy::default();
        #[cfg(any(test, feature = "test-support"))]
        let resources = if self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .zero_obligation_budget
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            FlowResourcePolicy {
                max_obligations: 0,
                ..resources
            }
        } else {
            resources
        };
        let request = FlowDemandRequest {
            query: SemanticQueryKey::FlowReturn(Box::new(key.clone())),
            // The in-flight observation identity of this demand: derived
            // from the SAME provenance mint the carrier and the evaluation
            // outcome bear — never a cache-candidate axis.
            input_basis: verter_identity::identity::InputBasisId::from_canonical(&provenance),
            resources,
            additional_requirements: Arc::from([]),
        };
        let plan = match build_flow_demand_plan(request, &bound, &planned, &site.inventory) {
            Ok(plan) => plan,
            Err(error) => {
                use super::flow_solve::FlowDemandPlanError as E;
                let refusal = match error {
                    // Both budget axes are statements about the REQUEST.
                    E::SliceBudget(_) | E::ObligationBudget { .. } => FlowPlanRefusal::Budget,
                    // Retained artifacts that do not match the demand's
                    // bound graph: a torn intermediate view.
                    E::BasisKeyMismatch
                    | E::SelectionOutOfRange
                    | E::SelectionDemandMismatch
                    | E::SelectionProvenanceMismatch => FlowPlanRefusal::TornView,
                    E::UnregisteredOperation | E::NotAnEnabledRoot | E::UnrepresentableDemand => {
                        FlowPlanRefusal::Unplannable
                    }
                };
                self.record_flow_plan_refusal(frame_idx, refusal);
                return;
            }
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

    /// Record WHY the demand preparation installed no proof layer on the
    /// open frame, so the frame close classifies the unproven outcome onto
    /// the cause's partial class rather than the one merged
    /// degraded-success class ([`plan_refusal_reason_class`]).
    fn record_flow_plan_refusal(&self, frame_idx: usize, refusal: FlowPlanRefusal) {
        let mut txn = self.dispatch_txn.borrow_mut();
        if let Some(state) = txn
            .reentry_mut()
            .frame_mut_for_update(frame_idx)
            .and_then(super::dispatch_txn::ObligationFrame::flow_return_mut)
        {
            state.plan_refusal = Some(refusal);
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
                // A contract-DOMAIN obligation of a product-bearing domain
                // additionally requires the frame's own product evidence:
                // the evaluation must have produced a product in that
                // domain. A frame that never computed one has nothing to
                // discharge the domain with, so the demand cannot seal and
                // finalizes unproven — a complete flow result always rests
                // on product evidence.
                FlowObligationBasis::DemandRoot { .. } => {
                    whole_selection_executed
                        && domain_product_evidence(spec.requirement(), witness.products)
                }
                FlowObligationBasis::FamilyCoverage { .. } => whole_selection_executed,
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
        // The recorded preparation refusal, when the demand could not be
        // planned — the close classifies a verdict-less unproven outcome
        // by this cause.
        let plan_refusal = flow_state.plan_refusal;
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
                    plan_refusal,
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
        let mut member_batch_partial_reasons = PartialReasonSet::default();
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
                Ok(outcome) => {
                    member_batch_unproven = outcome.flow_batch_unproven;
                    member_batch_partial_reasons = outcome.flow_batch_partial_reasons;
                }
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
                // The final idempotent pre-seal closure: after fixed
                // point, widening and substitution, before the seal — the
                // proof and both publish channels see the closed value.
                let result = self.close_flow_result_pre_seal(result);
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
                            plan_refusal,
                            member_batch_partial_reasons,
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
                                // Both recorded causes — the value's
                                // degradation and the finalizer's typed
                                // reason — reach the rails.
                                Some(FlowSolveOutcome::Partial(partial)) => {
                                    flow_partial_reason_class(
                                        &partial.reason,
                                        partial.value.degradation(),
                                    )
                                }
                                // Verdict-less: classify by the recorded
                                // preparation refusal — a budget edge and
                                // a torn view fault consumers the merged
                                // degraded-success class is contained by.
                                None => plan_refusal_reason_class(plan_refusal),
                                // A no-value verdict cannot arise on this
                                // arm (a failed evaluation closes through
                                // the no-value path). Should it ever
                                // reach here, it takes the SAME class the
                                // machinery root's twin arm takes — a
                                // milder class on one of two twins is a
                                // fail-closed asymmetry, not a saving.
                                Some(FlowSolveOutcome::NoValue(_)) => NO_VALUE_REASON_CLASS,
                                // Handled by the arm above.
                                Some(FlowSolveOutcome::Complete(_)) => unreachable!(),
                            };
                            // A refused member batch carries its own
                            // causes: the root's clean preparation must
                            // not report a member's budget edge as the
                            // contained unverified class.
                            self.fold_cache_read_rails(
                                true,
                                true,
                                reasons.union(member_batch_partial_reasons),
                            );
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
                // The SEED's surviving fresh literal arms carry through
                // the fixed point (the constructor re-filters them to the
                // joined return's kept constituents); hold-target arms
                // join pinned — the component's own widening is the
                // all-fresh-seed rule below, and per-constituent
                // freshness never crosses a recursive edge.
                let seed_fresh: Vec<SemanticNodeId> = current[i]
                    .as_ref()
                    .map(|result| result.fresh_literal_arms().to_vec())
                    .unwrap_or_default();
                let next = FlowReturnResult::new_with_fresh_literal_arms(
                    graph,
                    self.intern_normalized_union_or_intersection(&flat, true),
                    current[i]
                        .as_ref()
                        .is_some_and(|result| result.can_fall_through),
                    degradation,
                    &seed_fresh,
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
    ) -> Result<FlowSliceDemandSite, FlowSliceDemandSiteError> {
        // The evaluation models the whole-return point and the
        // single-named-member projection point, both at the empty input
        // point. Any other demand/input point fails CLOSED with a typed
        // no-value outcome — never a silently widened whole-return result,
        // never a sibling materialisation the narrower demand did not ask
        // for.
        if !key.input.is_empty() {
            return Err(FlowSliceDemandSiteError {
                failure: FlowReturnFailure::UnmodeledDemandPoint,
                self_roots: Vec::new(),
                refusal: FlowPlanRefusal::Unplannable,
            });
        }
        let demanded_member: Option<Arc<str>> = if key.demand.is_whole_return() {
            None
        } else {
            match flow_demanded_member_name(&key.demand) {
                Some(name) => Some(name),
                None => {
                    return Err(FlowSliceDemandSiteError {
                        failure: FlowReturnFailure::UnmodeledDemandPoint,
                        self_roots: Vec::new(),
                        refusal: FlowPlanRefusal::Unplannable,
                    });
                }
            }
        };
        let canonical = key.function.declaration_slot.defining_canonical.as_ref();
        let owner = key.function.declaration_slot.owner;
        let name = key.function.declaration_slot.merged_symbol_name.as_ref();
        #[cfg(any(test, feature = "test-support"))]
        let unserved = self
            .ctx
            .host_for_fact_tracer_install()
            .flow_fault_injection
            .unserved_demand_site
            .swap(false, std::sync::atomic::Ordering::Relaxed);
        #[cfg(not(any(test, feature = "test-support")))]
        let unserved = false;
        let served = (!unserved)
            .then(|| self.ctx.ensure_indexed_ready_serve(canonical))
            .flatten();
        let Some(serve) = served else {
            // The store did not serve the function's own file. The
            // canonical here came from a RESOLVED declaration slot, so
            // the file was served when that slot was minted: a later
            // non-serve is a statement about the store's state, not
            // about the program. It therefore faults consumers as the
            // transient read it is, rather than passing as a contained
            // "we could not verify this" — the class a genuinely absent
            // function position below takes.
            return Err(FlowSliceDemandSiteError {
                failure: FlowReturnFailure::Missing,
                self_roots: Vec::new(),
                refusal: FlowPlanRefusal::TornView,
            });
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
            // The served file genuinely holds no such function position:
            // a deterministic statement about the program, not a torn
            // read.
            return Err(FlowSliceDemandSiteError {
                failure: FlowReturnFailure::Missing,
                self_roots,
                refusal: FlowPlanRefusal::Unplannable,
            });
        };
        // A body whose own bytes could not be read has no exact-content
        // axis, so no content-addressed key can be built for it: fail
        // closed rather than key on a constant every unreadable body
        // shares.
        let Some(flow_body_exact_hash) = entry.flow_body_exact_hash else {
            return Err(FlowSliceDemandSiteError {
                failure: FlowReturnFailure::Unresolved,
                self_roots,
                refusal: FlowPlanRefusal::TornView,
            });
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
            return Err(FlowSliceDemandSiteError {
                failure: FlowReturnFailure::Unresolved,
                self_roots,
                refusal: FlowPlanRefusal::TornView,
            });
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
            Err(err) => return degraded(err.failure, err.self_roots),
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
        // The product budget of THIS frame's merges: the installed demand
        // plan's own convergence policy and obligation frontier. A frame
        // whose demand could not be planned runs the substrate default
        // and can never mint a proof anyway, so it never silently gains a
        // more permissive budget than a proven frame.
        let product_budget = self
            .flow_demand_carrier_of(key)
            .map_or_else(FlowProductBudget::default, |carrier| {
                FlowProductBudget::for_demand_plan(&carrier.plan)
            });
        let mut evaluator = FlowEvaluator {
            dispatch: self,
            self_slot: Some(key),
            canonical,
            owner,
            params: &params,
            param_names: &ir.params,
            binder_env: &binder_env,
            bindings: FlowFrameBindings::new(),
            products: FlowProductStore::new(),
            product_budget,
            product_budget_exceeded: None,
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
            conditional_arm_nesting: 0,
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
        let product_budget_exceeded;
        let frame_products;
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
            product_budget_exceeded = evaluator.product_budget_exceeded;
            frame_products = std::mem::take(&mut evaluator.products);
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
        //
        // The PRODUCT budget is the same class of edge and rides the same
        // exit: a frame merge that exhausted the demand plan's own
        // convergence policy or obligation frontier has no joined state,
        // so the frame's remaining products are not the ones a converged
        // solve would have produced. It reports the typed budget failure
        // and shortens the walk ledger exactly as the connected-demand
        // trip does — a budget-exhausted frame is never admitted warm.
        let contributors = match (self.connected_demand_trip(), product_budget_exceeded) {
            (Some(_), _) | (None, Some(_)) => {
                executed_walk.aborted = true;
                Err(FlowReturnFailure::Budget(
                    verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
                ))
            }
            (None, None) => contributors,
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
            products: &frame_products,
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
        // The sealed result's FRESH set: every fresh literal value any
        // contribution carries, minus every value some contribution
        // spells PINNED at its own top level (pinned wins, by literal
        // VALUE — measured against the pinned checker). The constructor
        // then filters the survivors to the final return's kept top-level
        // constituents, so join-widening and union absorption drop their
        // values with them. A wholly-fresh contribution (a bare literal,
        // a hold-marked equation arm) pins nothing.
        let mut fresh_candidates: Vec<SemanticNodeId> = Vec::new();
        let mut pinned_blockers: Vec<SemanticNodeId> = Vec::new();
        for contribution in &contributors {
            for value in &contribution.fresh_values {
                if !fresh_candidates.contains(value) {
                    fresh_candidates.push(*value);
                }
            }
            if contribution.fresh_literal {
                continue;
            }
            for lit in top_level_literal_nodes_in(graph, contribution.node) {
                let data = graph.node_data(lit);
                let fresh_here = contribution
                    .fresh_values
                    .iter()
                    .any(|value| graph.node_data(*value) == data);
                if !fresh_here && !pinned_blockers.contains(&lit) {
                    pinned_blockers.push(lit);
                }
            }
        }
        let sealed_fresh: Vec<SemanticNodeId> = fresh_candidates
            .into_iter()
            .filter(|value| {
                let data = graph.node_data(*value);
                !pinned_blockers
                    .iter()
                    .any(|pinned| graph.node_data(*pinned) == data)
            })
            .collect();
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
        // A multi-arm join aggregates over EVALUATED constituents, exactly
        // as the checker's return aggregation does: an alias-instantiation
        // contributor whose one-level expansion is a UNION enters the join
        // as that union, so the canonical union below flattens and dedups
        // it (`if (c) { return fu("x") } return "x" as const` for
        // `fu<T>(v: T): UA<T>` with `UA<T> = T | undefined` reads
        // `"x" | undefined` — the checker's own flatten). A non-union
        // expansion keeps its shallow carrier (the checker keeps the alias
        // label for those constituents), and a lone contributor keeps its
        // carrier untouched — nothing materialises without a join to
        // normalise.
        if arms.len() >= 2 {
            for arm in arms.iter_mut() {
                if !matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::InstantiationRef { .. })
                ) {
                    continue;
                }
                if let Some(expanded) = self.expand_alias_instantiation_one_level(*arm) {
                    if matches!(
                        graph.node_data(expanded).as_deref(),
                        Some(SemanticNodeData::Union(_))
                    ) {
                        *arm = expanded;
                    }
                }
            }
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
            FlowReturnResult::new_with_fresh_literal_arms(
                graph,
                return_type,
                can_fall_through,
                degradation,
                &sealed_fresh,
            ),
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

/// The bare identifier one indexed argument expression reads, if it is
/// exactly a frame-binding read: a bare `Ref` or a single-segment
/// `typeof` path — the two spellings the indexed lowering mints for one.
fn indexed_bare_name(expression: &verter_type_expr::IndexedValueExpression) -> Option<&str> {
    match expression {
        verter_type_expr::IndexedValueExpression::Value(verter_type_expr::TypeExpr::Ref {
            name,
            type_arguments,
        }) if type_arguments.is_empty() && !name.contains('.') => Some(name.as_ref()),
        verter_type_expr::IndexedValueExpression::Value(verter_type_expr::TypeExpr::TypeOf(
            verter_type_expr::ValueRef { path, type_args },
        )) if type_args.is_empty() && path.len() == 1 => Some(path[0].as_str()),
        _ => None,
    }
}

/// Widen a FRESH value at a widening READ position: a lone literal
/// widens to its primitive, and a UNION widens every literal arm — the
/// value a widening-literal `const` bound from an all-fresh conditional
/// carries (`const v = f ? 1 : "s"` reads `string | number` at a member
/// position; a narrowed read of the same binding widens the surviving
/// arm the same way). Sound only where every literal arm is KNOWN
/// fresh, which the widening-locals membership guarantees: it admits a
/// union-valued binding only when every conditional leaf was a bare
/// literal.
fn widen_fresh_read_node(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    let graph = dispatch.graph();
    if let Some(SemanticNodeData::Union(members)) = graph.node_data(node).as_deref() {
        let widened: Vec<SemanticNodeId> = members
            .iter()
            .map(|member| widen_literal_node(dispatch, *member))
            .collect();
        if widened.as_slice() == members.as_ref() {
            return node;
        }
        return dispatch.intern_normalized_union_or_intersection(&widened, true);
    }
    widen_literal_node(dispatch, node)
}

/// Widen exactly the listed literal values within `node`: the node
/// itself when listed, or the listed constituents of a union. Everything
/// else passes through unchanged — an authored pinned arm beside a fresh
/// one keeps its literal.
fn widen_values_within(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    values: &[SemanticNodeId],
) -> SemanticNodeId {
    if values.contains(&node) {
        return widen_literal_node(dispatch, node);
    }
    let graph = dispatch.graph();
    if let Some(SemanticNodeData::Union(members)) = graph.node_data(node).as_deref() {
        let widened: Vec<SemanticNodeId> = members
            .iter()
            .map(|member| {
                if values.contains(member) {
                    widen_literal_node(dispatch, *member)
                } else {
                    *member
                }
            })
            .collect();
        if widened.as_slice() == members.as_ref() {
            return node;
        }
        return dispatch.intern_normalized_union_or_intersection(&widened, true);
    }
    node
}

/// The top-level LITERAL constituents of one value node — the shared
/// walk behind the evaluator's freshness fold and the join's sealed
/// fresh set.
fn top_level_literal_nodes_in(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    node: SemanticNodeId,
) -> Vec<SemanticNodeId> {
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Literal(_)) => vec![node],
        Some(SemanticNodeData::Union(members)) => members
            .iter()
            .copied()
            .filter(|member| {
                matches!(
                    graph.node_data(*member).as_deref(),
                    Some(SemanticNodeData::Literal(_))
                )
            })
            .collect(),
        _ => Vec::new(),
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

/// One branch-local narrowing verdict. There is deliberately no
/// "impossible branch" variant: the checker's rule for a guard edge no
/// arm survives is that the SUBJECT reads `never` while the edge stays
/// ALIVE — narrowing impossibility never removes an edge, so a
/// contributor on that edge that reads a different binding still
/// contributes its own narrowed type (measured: `if (typeof x ===
/// "string" && typeof y === "string")` over `x: number` types `return y`
/// as `string`, and a conjunction whose later fact removes its last
/// survivor contributes `never` for that subject to an enclosing
/// disjunction rather than removing the alternative). Each guard family
/// converts an empty survivor set ([`ArmFilter::NoSurvivor`]) under its
/// own measured checker rule.
enum GuardNarrowing {
    Unchanged,
    Narrowed(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    ),
}

/// One union-arm filter verdict, BEFORE the calling guard family applies
/// its empty-survivor rule. `NoSurvivor` is a positive proof that every
/// arm is off the tested edge — never "unrecognized" — and the caller
/// decides what the checker does with it: most filters collapse the
/// subject to `never` (the edge stays alive), while the two measured
/// exceptions (a truthiness/equality test through a member that does not
/// DISCRIMINATE its parent's arms) establish nothing.
enum ArmFilter {
    Unchanged,
    Narrowed(SemanticNodeId),
    NoSurvivor,
}

/// One union arm's verdict against a runtime guard test. `NoMatch` is
/// PROVED non-inhabitance of the tested edge — never "unrecognized". An
/// arm the graph cannot classify (`any`, `unknown`, a memberless `{}`
/// surface, an unresolved carrier, an undecided relation) is
/// `Unclassified`: the checker still narrows such an arm, so it stays
/// possible on BOTH edges of the test and the narrow records the typed
/// guard gap instead of silently deciding either reading. This is what
/// keeps an empty survivor set a positive proof: a subject reads `never`
/// on an edge only when every arm is proved off it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ArmGuardClass {
    Match,
    NoMatch,
    Unclassified,
}

/// One union arm's key-presence verdict for a `"key" in subject` test.
/// The `in` guard needs one more state than [`ArmGuardClass`] because an
/// OPTIONAL member is its own proof shape: the arm provably stays on
/// BOTH edges exactly as declared — a value of the arm's type may lack
/// the key (negated edge), and the checker's positive edge keeps the arm
/// UNCHANGED too (measured: `if ("k" in x) x.k` reads `string |
/// undefined` for `k?: string`, byte-identical to the guard-free read —
/// `in` establishes key PRESENCE and says nothing about the value; the
/// absent-key `undefined` is the member READ's own fact, folded at
/// `project_segments_navigate`). `Always`/`Never` are per-edge PROOFS (a
/// required member / a proven-absent key on a closed surface); `Unknown`
/// proves nothing and keeps the arm on both edges with the gap recorded.
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
    /// The frame's binding-NAME resolution authority: the one place an
    /// authored name becomes a typed [`FlowProductSubject`]. It holds
    /// names, never semantic state, and its two scope layers are two
    /// DISJOINT slot spaces — a lexical `const` / `let` binding and a
    /// function-scoped `var` of the same name are different subjects, so
    /// neither can read the other's product.
    bindings: FlowFrameBindings,
    /// The frame's SEMANTIC STATE: the reaching types (with their
    /// literal-widening provenance), declared types, definite-assignment
    /// facts, and guard facts of every subject, in ONE product store.
    /// Block / `if`-arm evaluation saves and restores this store;
    /// function-scoped subjects survive a lexical scope close because the
    /// close replays exactly the scope's own declaration shadows.
    products: FlowProductStore,
    /// The product budget every frame merge runs under — derived from the
    /// installed demand plan's own convergence policy and obligation
    /// frontier ([`FlowProductBudget::for_demand_plan`]), never a private
    /// constant. A frame with no installed demand runs the substrate
    /// default and can never mint a proof anyway.
    product_budget: FlowProductBudget,
    /// The first typed product-budget exhaustion a frame merge observed.
    /// A budget-exhausted merge has no joined state, so the evaluation
    /// fails with the typed budget reason and can never be admitted warm.
    product_budget_exceeded: Option<FlowProductBudgetExceeded>,
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
    /// How many `if` arms enclose the statement being evaluated. A plain
    /// block NEVER increments it — a block executes unconditionally, so a
    /// `var` it declares has exactly one reaching definition.
    conditional_arm_nesting: u32,
    /// Whether the current path exists only for return inference after an
    /// abrupt `finally` replaced the pending break at runtime.
    inference_only_path: bool,
    /// COMPLETED calls in this frame that closed with fresh-preserved
    /// literal deposits, recorded by their authored call-site span — the
    /// call-SITE identity a consuming position matches against, never the
    /// interned literal value, so a sibling arm's authored pin of the
    /// same value can never borrow a call's freshness. The whole return
    /// being one of the deposits is the naked case (fresh at the return
    /// join); every listed deposit widens at a value (member /
    /// mutable-declaration) position and seeds a `const` binding's
    /// widening membership.
    call_fresh_literal_returns: Vec<FreshCallReturn>,
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
    /// The frame's converged product state — the evidence a
    /// product-bearing contract domain is discharged from. An obligation
    /// whose domain carries a product but whose frame produced none is
    /// never claimed, so a demand cannot seal without product evidence.
    products: &'w FlowProductStore,
}

/// One return-site contribution: the evaluated node plus whether it came
/// from a FRESH literal source (a bare literal return argument, or a read
/// of a widening-literal `const`). tsc widens a fresh literal return only
/// when the deduplicated contributor set has exactly ONE member, so the
/// freshness bit must survive to the join.
#[derive(Clone)]
struct FlowContribution {
    /// The evaluated contributor node.
    node: SemanticNodeId,
    /// The contributor is a fresh (widening) literal source.
    fresh_literal: bool,
    /// The contributor is reached only through an overridden break's
    /// return-inference suffix edge.
    inference_only: bool,
    /// The FRESH literal values this contribution carries into the join,
    /// at its own top level: the node itself for a bare-literal return,
    /// the membership's values for a widening-local read, a completed
    /// call's kept fresh deposits and callee-authored fresh arms, and
    /// the per-arm collection of a ternary argument. The join folds
    /// these pinned-wins into the sealed result's fresh set; they never
    /// affect the join's own widening decision (`fresh_literal` owns
    /// that, unchanged).
    fresh_values: Vec<SemanticNodeId>,
}

/// One evaluated arm of a mixed value position (a ternary initializer,
/// assignment right-hand side, or ternary return argument): the settled
/// node, whether the arm as a WHOLE is a fresh spelling, and the fresh
/// literal values it carries at its own top level
/// ([`FlowEvaluator::position_fresh_values`]).
struct EvolvingPart {
    node: SemanticNodeId,
    fresh: bool,
    fresh_values: Vec<SemanticNodeId>,
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
    subject: FlowProductSubject,
    /// The outer binding's reaching-type and definite-assignment products
    /// at scope entry (`None` = the name had no reaching value there, so
    /// the scope close drops both).
    prior: Option<(ReachingTypeProduct, DefiniteAssignmentProduct)>,
    prior_declared: Option<SemanticNodeId>,
}

/// One COMPLETED call that closed with fresh-preserved literal deposits,
/// recorded by its authored call-site span so a sibling return arm's
/// authored pin of the same interned literal value can never borrow the
/// call's freshness.
#[derive(Clone, Debug)]
struct FreshCallReturn {
    /// The authored call expression's span (frame coordinates) — the
    /// call-SITE identity a consuming position matches against.
    span: verter_span::Span,
    /// The call's settled return node.
    node: SemanticNodeId,
    /// The fresh literal deposits kept at the return's top level. The
    /// whole return being ONE of these is the naked case — fresh at the
    /// return join; every listed value widens at a value position.
    values: Arc<[SemanticNodeId]>,
}

/// One point-in-time snapshot of the evaluator's SEMANTIC STATE — the
/// per-subject, per-domain products a multi-path construct (`switch`
/// dispatch and fall-through, `try` / `catch` / `finally`) joins over.
///
/// The whole state is ONE [`FlowProductStore`]: reaching types (with
/// their literal-widening provenance), declared types, definite-assignment
/// facts, and the narrowing overlay are domains of one store keyed by one
/// typed subject vocabulary, not parallel layers keyed by name. The
/// narrowing overlay rides the state too: a guard or assertion fact lives
/// on the path that established it, so a join INTERSECTS the overlay (a
/// narrow holds past a join only when every joined path carries it) and a
/// clause start restores the entering overlay — a narrow established in a
/// `try` block or a sibling `case` can never leak across the boundary.
#[derive(Clone)]
struct FlowLayerState {
    products: FlowProductStore,
    /// Whether this snapshot exists only for the return-inference suffix
    /// of a break that an abrupt `finally` replaced at runtime.
    inference_only_path: bool,
}

/// One entry of a narrowing-overlay snapshot: the subject's guard facts
/// as they stood at the mark.
type NarrowingSnapshot = Vec<(FlowProductSubject, Option<NarrowingProduct>)>;

/// Whether the frame's product state carries the evidence a contract
/// DOMAIN obligation discharges on.
///
/// A domain the product lattice carries (`flow_product_kind` is `Some`)
/// discharges only from a real product the evaluation computed in that
/// domain — a store with no product there proves nothing about it. A
/// domain the lattice carries no product for discharges on the
/// enumeration evidence alone, exactly as it did before the value path
/// moved onto the products, and a fact-family requirement is not a domain
/// obligation at all.
fn domain_product_evidence(
    requirement: &super::flow_solve::FlowRequirement,
    products: &FlowProductStore,
) -> bool {
    let super::flow_solve::FlowRequirementKind::Domain(domain) = requirement.requirement else {
        return true;
    };
    if super::flow_products::flow_product_kind(domain).is_none() {
        return true;
    }
    !products.subjects_in(domain).is_empty()
}

/// Every guard fact `products` holds, in canonical subject order.
fn narrowing_facts_of(products: &FlowProductStore) -> Vec<FlowNarrowingFact> {
    products
        .subjects_in(super::flow_solve::FlowDomain::Narrowing)
        .into_iter()
        .filter_map(|subject| products.narrowing(&subject).cloned())
        .flat_map(|product| product.facts().to_vec())
        .collect()
}

/// Replace `into`'s whole narrowing overlay with `from`'s.
fn replace_narrowings(from: &FlowProductStore, into: &mut FlowProductStore) {
    for subject in into.subjects_in(super::flow_solve::FlowDomain::Narrowing) {
        into.remove(super::flow_solve::FlowDomain::Narrowing, &subject);
    }
    for subject in from.subjects_in(super::flow_solve::FlowDomain::Narrowing) {
        if let Some(product) = from.narrowing(&subject) {
            into.set_narrowing(&subject, product.clone());
        }
    }
}

/// The parameter ordinals holding an applied whole-slot write in `store`,
/// in canonical subject order.
fn store_param_ordinals(store: &FlowProductStore) -> Vec<u32> {
    store
        .subjects_in(super::flow_solve::FlowDomain::ReachingType)
        .into_iter()
        .filter_map(|subject| match subject {
            FlowProductSubject::FrameParam(ordinal) => Some(ordinal),
            FlowProductSubject::FrameBinding(_) | FlowProductSubject::GraphNode { .. } => None,
        })
        .collect()
}

/// Record `path` under `root` narrowing to `node` in `products` — the ONE
/// narrowing write, shared by the live overlay and by the snapshot states
/// a `finally` bakes a fact into, so the two can never diverge. A fact
/// about the SAME position replaces the earlier one.
fn push_narrowing_into(
    products: &mut FlowProductStore,
    root: &FlowProductSubject,
    path: &Arc<[Arc<str>]>,
    node: SemanticNodeId,
) {
    let mut facts: Vec<FlowNarrowingFact> = products
        .narrowing(root)
        .map(|product| {
            product
                .facts()
                .iter()
                .filter(|fact| fact.path != *path)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    facts.push(FlowNarrowingFact {
        subject: root.clone(),
        path: Arc::clone(path),
        narrowed_to: node,
    });
    products.set_narrowing(root, NarrowingProduct::new(facts));
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
            let value = self.widen_value_position_read(member_value, value);
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
            // A `unique symbol` key names exactly ONE nominal property, and
            // the value channel carries that uniqueness: the read keeps the
            // `typeof` carrier whose DECLARING identity is the name. Name
            // the member by that identity — and by NOTHING else: a `typeof`
            // value that carries no nominal identity is an unread key (a
            // deferred shell the channel did not resolve), which leaves the
            // key SET unknown exactly like every other unread value, never
            // over-named from the authored spelling.
            Some(SemanticNodeData::TypeOf(_) | SemanticNodeData::TypeOfNominal(_)) => self
                .dispatch
                .unique_symbol_identity_for_typeof_node(node)
                .map(crate::semantic_query::AuthoredPropertyKey::UniqueSymbol),
            // A NON-unique `symbol` key genuinely provisions an index
            // signature rather than one property, and is over-named here.
            // That is not a new divergence: it is exactly what the leaf
            // answer this replaces already did, and telling the two apart
            // needs the key's own uniqueness, which a bare `symbol` value
            // does not have.
            Some(SemanticNodeData::Primitive(PrimitiveKind::Symbol)) => {
                self.authored_symbol_key(authored)
            }
            // Anything else — an OPEN `string` / `number` key, an
            // unresolved read — leaves the surface's key SET unknown.
            _ => None,
        }
    }

    /// Name a symbol-valued member from its AUTHORED key — the same carrier
    /// the whole-literal leaf answer produced, resolved by the same
    /// downstream reader. The fallback when the value channel carries no
    /// nominal identity of its own.
    fn authored_symbol_key(
        &self,
        authored: &verter_type_expr::AuthoredPropertyKey<
            verter_type_expr::TypeExpr,
            verter_type_expr::facts::ValueDeclIdentityPart,
        >,
    ) -> Option<crate::semantic_query::AuthoredPropertyKey> {
        match authored.cloned_known() {
            Some(known) => Some(crate::semantic_query::AuthoredPropertyKey::from_known(
                known,
            )),
            None => match authored {
                verter_type_expr::AuthoredPropertyKey::Computed(ty) => Some(
                    crate::semantic_query::AuthoredPropertyKey::Computed(self.lower_key_type(ty)),
                ),
                _ => None,
            },
        }
    }

    fn record_degradation(&mut self, degradation: crate::semantic_query::FlowReturnDegradation) {
        self.degradation.get_or_insert(degradation);
    }

    // ── The frame's product accessors ──────────────────────────────
    //
    // Every read and write of the frame's semantic state goes through
    // this block: a name becomes a typed subject exactly once, and the
    // products are then addressed by that subject. There is no second
    // state layer a site could reach around these accessors to.

    /// The subject of `name` in `layer`, allocating its slot on first use.
    fn subject(&mut self, layer: FlowBindingLayer, name: &str) -> FlowProductSubject {
        self.bindings.subject(layer, name)
    }

    /// The subject of `name` in `layer` when this frame already resolved
    /// one. A name no declaration, write, or read ever resolved has no
    /// subject and therefore no product — the read-side counterpart, so a
    /// pure read never grows the frame's binding table.
    fn resolved_subject(&self, layer: FlowBindingLayer, name: &str) -> Option<FlowProductSubject> {
        self.bindings
            .resolved(layer, name)
            .map(FlowProductSubject::FrameBinding)
    }

    /// The subject of one formal parameter, by ordinal.
    fn param_subject(ordinal: u32) -> FlowProductSubject {
        FlowFrameBindings::param(ordinal)
    }

    /// The subject a narrowing root is held under. The narrowing overlay
    /// is rooted at the authored NAME — it never distinguished the two
    /// scope layers and does not start to here — so a local root always
    /// resolves to the frame's lexical slot for that name: one stable
    /// subject per name, independent of which layer currently binds it.
    fn narrow_subject(
        &mut self,
        root: &crate::flow_slice_content::SliceNarrowRoot,
    ) -> FlowProductSubject {
        match root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                Self::param_subject(*ordinal)
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                self.subject(FlowBindingLayer::Lexical, name)
            }
        }
    }

    /// The narrowing root's subject when the frame already resolved one.
    fn resolved_narrow_subject(
        &self,
        root: &crate::flow_slice_content::SliceNarrowRoot,
    ) -> Option<FlowProductSubject> {
        match root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                Some(Self::param_subject(*ordinal))
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                self.resolved_subject(FlowBindingLayer::Lexical, name)
            }
        }
    }

    /// The reaching value of `name` in `layer`.
    fn local_value(&self, layer: FlowBindingLayer, name: &str) -> Option<SemanticNodeId> {
        self.resolved_subject(layer, name)
            .and_then(|subject| self.products.reaching(&subject))
    }

    /// The declared (annotation) type of `name` in `layer`.
    fn local_declared(&self, layer: FlowBindingLayer, name: &str) -> Option<SemanticNodeId> {
        self.resolved_subject(layer, name)
            .and_then(|subject| self.products.declared_type(&subject))
    }

    /// The definite-assignment product of `name` in `layer`.
    fn local_assignment(&self, layer: FlowBindingLayer, name: &str) -> DefiniteAssignmentProduct {
        self.resolved_subject(layer, name)
            .map(|subject| self.products.assignment(&subject))
            .unwrap_or_default()
    }

    /// The literal-widening membership of `name` in `layer`.
    fn local_widening(&self, layer: FlowBindingLayer, name: &str) -> Option<WideningMembership> {
        self.resolved_subject(layer, name)
            .and_then(|subject| self.products.widening(&subject).cloned())
    }

    /// The parameter ordinal's applied whole-slot write, when one was made.
    fn param_write(&self, ordinal: u32) -> Option<SemanticNodeId> {
        self.products.reaching(&Self::param_subject(ordinal))
    }

    /// The snapshot of the frame's narrowing overlay — the mark a guard
    /// scope restores to. Restoring a mark is what keeps a narrow inside
    /// the arm that established it.
    fn narrowing_snapshot(&self) -> NarrowingSnapshot {
        self.products
            .subjects_in(super::flow_solve::FlowDomain::Narrowing)
            .into_iter()
            .map(|subject| {
                let facts = self.products.narrowing(&subject).cloned();
                (subject, facts)
            })
            .collect()
    }

    /// Restore the overlay to `mark`: every subject the mark recorded
    /// regains exactly its recorded facts, and every subject narrowed
    /// since loses its facts.
    fn restore_narrowings(&mut self, mark: NarrowingSnapshot) {
        for subject in self
            .products
            .subjects_in(super::flow_solve::FlowDomain::Narrowing)
        {
            self.products
                .remove(super::flow_solve::FlowDomain::Narrowing, &subject);
        }
        for (subject, facts) in mark {
            if let Some(facts) = facts {
                self.products.set_narrowing(&subject, facts);
            }
        }
    }

    /// Record one guard fact: `subject`'s member `path` narrows to `node`.
    /// A later fact about the SAME position replaces the earlier one, so
    /// a read always observes the newest established narrow.
    fn push_narrowing(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        node: SemanticNodeId,
    ) {
        let root = self.narrow_subject(&subject.root);
        push_narrowing_into(&mut self.products, &root, &subject.path, node);
    }

    /// Drop every guard fact rooted at `root` — what a write to the
    /// binding does to the facts about the value it replaced.
    fn clear_narrowings_of(&mut self, root: &crate::flow_slice_content::SliceNarrowRoot) {
        if let Some(subject) = self.resolved_narrow_subject(root) {
            self.products
                .remove(super::flow_solve::FlowDomain::Narrowing, &subject);
        }
    }

    /// The guard facts established since `mark`, as authored narrowing
    /// subjects — the per-disjunct overlay a union-of-facts guard folds.
    fn narrowings_since(
        &self,
        mark: &NarrowingSnapshot,
    ) -> Vec<(
        crate::flow_slice_content::SliceNarrowSubject,
        SemanticNodeId,
    )> {
        let mut applied = Vec::new();
        for subject in self
            .products
            .subjects_in(super::flow_solve::FlowDomain::Narrowing)
        {
            let before = mark
                .iter()
                .find(|(candidate, _)| *candidate == subject)
                .and_then(|(_, facts)| facts.as_ref());
            let Some(now) = self.products.narrowing(&subject) else {
                continue;
            };
            let Some(root) = self.authored_narrow_root(&subject) else {
                continue;
            };
            for fact in now.facts() {
                if before.is_some_and(|before| before.facts().contains(fact)) {
                    continue;
                }
                applied.push((
                    crate::flow_slice_content::SliceNarrowSubject {
                        root: root.clone(),
                        path: Arc::clone(&fact.path),
                    },
                    fact.narrowed_to,
                ));
            }
        }
        applied
    }

    /// The authored narrowing root of a product subject — the inverse of
    /// [`Self::narrow_subject`], resolved through the frame's own binding
    /// table so a subject can never be spelled back as a name the frame
    /// never resolved.
    fn authored_narrow_root(
        &self,
        subject: &FlowProductSubject,
    ) -> Option<crate::flow_slice_content::SliceNarrowRoot> {
        match subject {
            FlowProductSubject::FrameParam(ordinal) => {
                Some(crate::flow_slice_content::SliceNarrowRoot::Param(*ordinal))
            }
            FlowProductSubject::FrameBinding(slot) => self
                .bindings
                .name(*slot)
                .map(|name| crate::flow_slice_content::SliceNarrowRoot::Local(Arc::clone(name))),
            FlowProductSubject::GraphNode { .. } => None,
        }
    }

    /// Join two frame states through the ONE frame join route. A merge
    /// the domain rules could not model, or one that exhausts the demand
    /// plan's own product budget, records the typed failure and keeps the
    /// ENTERING state: the evaluation still returns a usable value, and
    /// the recorded failure is what refuses its warm admission.
    fn join_states(&mut self, a: &FlowLayerState, b: &FlowLayerState) -> FlowLayerState {
        let joined = match join_frame_products(
            self.dispatch,
            &self.product_budget,
            &a.products,
            &b.products,
        ) {
            FlowFrameJoinOutcome::Joined(products) => products,
            FlowFrameJoinOutcome::Gap(_) => {
                self.record_degradation(
                    crate::semantic_query::FlowReturnDegradation::UnresolvedValue,
                );
                a.products.clone()
            }
            FlowFrameJoinOutcome::BudgetExceeded(exceeded) => {
                self.product_budget_exceeded.get_or_insert(exceeded);
                a.products.clone()
            }
        };
        FlowLayerState {
            products: joined,
            // A joined path is inference-only only when every incoming
            // edge is. One ordinary runtime edge makes the merged
            // continuation ordinary.
            inference_only_path: a.inference_only_path && b.inference_only_path,
        }
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
        widening: Option<WideningMembership>,
        degraded: bool,
    ) {
        let function_scoped = kind == crate::flow_slice_content::SliceBindingKind::Var;
        let layer = if function_scoped {
            FlowBindingLayer::Function
        } else {
            FlowBindingLayer::Lexical
        };
        let subject = self.subject(layer, name);
        // The definite-assignment product of the binding: assigned on
        // this path, with the two read-observable flags this declaration
        // establishes. A function-scoped binding recorded under non-zero
        // conditional-arm nesting is one arm's definition; an
        // unconditional rebind of the same name clears the flag.
        let single_path = if function_scoped {
            self.conditional_arm_nesting > 0
        } else {
            // The lexical single-path fact is established by a clause
            // boundary, not by a declaration: a rebind never clears it.
            self.products.assignment(&subject).single_path()
        };
        self.products.set_assignment(
            &subject,
            DefiniteAssignmentProduct::assigned()
                .with_single_path(single_path)
                .with_failed_initializer(degraded),
        );
        // The reaching value and its literal-widening provenance ride ONE
        // product, so a restore can never split a binding from its own
        // freshness membership.
        self.products.set_reaching_type(
            &subject,
            ReachingTypeProduct::of(node).with_widening(widening),
        );
        // A (re)binding replaces the binding's value: every narrow fact a
        // guard established about the OLD value — at the root or under
        // any member path — dies with it.
        let root = crate::flow_slice_content::SliceNarrowRoot::Local(Arc::from(name));
        self.clear_narrowings_of(&root);
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
                let subject = self.subject(FlowBindingLayer::Function, name);
                self.products.set_declared_type(&subject, Some(node));
            }
        } else {
            let subject = self.subject(FlowBindingLayer::Lexical, name);
            self.products.set_declared_type(&subject, declared);
        }
    }

    /// The declared authority governing a write target, after the same
    /// lazy destructured-parameter bootstrap assignment application uses —
    /// so RHS evaluation context and target authority cannot diverge.
    /// `None` is an EVOLVING target: no annotation supplies a type, the
    /// binding's type is the join of what the writes assign.
    fn target_declared_node(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
    ) -> Option<SemanticNodeId> {
        if let crate::flow_slice_content::SliceNarrowRoot::Local(name) = &target.root {
            if self.local_value(FlowBindingLayer::Lexical, name).is_none()
                && self.local_value(FlowBindingLayer::Function, name).is_none()
            {
                self.seed_destructured_param_element(name.as_ref());
            }
        }
        match &target.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                self.params.get(*ordinal as usize).copied()
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                if self.local_value(FlowBindingLayer::Lexical, name).is_some() {
                    self.local_declared(FlowBindingLayer::Lexical, name)
                } else {
                    self.local_declared(FlowBindingLayer::Function, name)
                }
            }
        }
    }

    /// Apply the checker's assignment typing rule to an evaluated RHS.
    /// Annotated unions select their comparable declared constituents;
    /// annotated non-unions retain the declared type; an EVOLVING target
    /// keeps the value as evaluated — the freshness-directed widening
    /// already happened at [`Self::eval_evolving_rhs`], the one place the
    /// fresh/pinned split is known (a bare literal widens, a
    /// const-asserted or otherwise pinned literal stays — the checker's
    /// own fresh/regular literal split, measured per assignment).
    fn assignment_node_for_target(
        &mut self,
        target: &crate::flow_slice_content::SliceNarrowSubject,
        value: SemanticNodeId,
    ) -> SemanticNodeId {
        let Some(declared) = self.target_declared_node(target) else {
            // Reuse an equivalent reaching-definition arm when one already
            // exists. Primitive nodes can originate in distinct lowered
            // arenas; joining two ids that both spell `number` would otherwise
            // manufacture `number | number` instead of deduplicating the
            // assignment path.
            let current = match &target.root {
                crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => self
                    .param_write(*ordinal)
                    .or_else(|| self.params.get(*ordinal as usize).copied()),
                crate::flow_slice_content::SliceNarrowRoot::Local(name) => self
                    .local_value(FlowBindingLayer::Lexical, name)
                    .or_else(|| self.local_value(FlowBindingLayer::Function, name)),
            };
            if let Some(current) = current {
                let value_data = self.dispatch.graph().node_data(value);
                if let Some(existing) = self
                    .union_arms_or_self(current)
                    .into_iter()
                    .find(|node| self.dispatch.graph().node_data(*node) == value_data)
                {
                    return existing;
                }
            }
            return value;
        };
        match self.dispatch.union_arms_of(declared) {
            Some(arms) => self.assignment_reduced_union(declared, &arms, value),
            None => declared,
        }
    }

    /// Evaluate an applied write's right-hand side for an EVOLVING target
    /// (no declared authority), applying the checker's per-assignment
    /// literal widening: a FRESH position — a bare literal, a fresh
    /// ternary arm, a read of a widening-literal `const` — widens to its
    /// primitive; a PINNED position — a const assertion, a type
    /// assertion, a pinned-`const` read, a callee's literal return —
    /// keeps its literal. A fresh and a pinned spelling of the SAME
    /// literal collapse to the pinned literal BEFORE widening (measured:
    /// `let v; v = c ? 1 : 1 as const` reads `1`, never `number` — the
    /// checker's union collapse keeps the regular literal).
    fn eval_evolving_rhs(
        &mut self,
        expr: &crate::flow_slice_content::SliceExpr,
        freshness: &crate::flow_slice_content::SliceFreshness,
    ) -> Positional<SemanticNodeId> {
        if let (
            crate::flow_slice_content::SliceExpr::Union { .. },
            crate::flow_slice_content::SliceFreshness::PerArm(_),
        ) = (expr, freshness)
        {
            let mut parts: Vec<EvolvingPart> = Vec::new();
            self.collect_evolving_parts(expr, freshness, &mut parts);
            // Pinned-wins collapse of equal constituents, THEN widen the
            // surviving fresh ones. Node data (not id) is the equality,
            // exactly as the reaching-definition dedup below compares.
            let pinned = self.pinned_literal_blockers(&parts);
            let mut merged: Vec<EvolvingPart> = Vec::new();
            for part in parts {
                let data = self.dispatch.graph().node_data(part.node);
                match merged
                    .iter_mut()
                    .find(|existing| self.dispatch.graph().node_data(existing.node) == data)
                {
                    Some(existing) => {
                        existing.fresh &= part.fresh;
                        existing.fresh_values.extend(part.fresh_values);
                    }
                    None => merged.push(part),
                }
            }
            let nodes: Vec<SemanticNodeId> = merged
                .into_iter()
                .map(|part| {
                    if part.fresh {
                        widen_fresh_read_node(self.dispatch, part.node)
                    } else {
                        // The arm keeps its own shape, but a fresh value
                        // it CARRIES (a call's kept deposit or authored
                        // fresh arm) still widens at this write position
                        // — unless a sibling arm pinned the same value.
                        let values = self.uncancelled_fresh_values(&part.fresh_values, &pinned);
                        widen_values_within(self.dispatch, part.node, &values)
                    }
                })
                .collect();
            return Positional::Value(
                self.dispatch
                    .intern_normalized_union_or_intersection(&nodes, true),
            );
        }
        let outcome = self.eval_expr(expr);
        let Positional::Value(node) = outcome else {
            return outcome;
        };
        let fresh = matches!(freshness, crate::flow_slice_content::SliceFreshness::Fresh);
        Positional::Value(if fresh {
            widen_fresh_read_node(self.dispatch, node)
        } else {
            // A non-fresh spelling may still carry read-side widening
            // provenance: a widening-membership local, or a completed
            // fresh-literal call — the same value-position rule the
            // object-member read applies.
            self.widen_value_position_read(expr, node)
        })
    }

    /// The recursive collector behind [`Self::eval_evolving_rhs`] and the
    /// ternary return argument: a ternary's arms evaluate under the SAME
    /// guard scoping as the ordinary `SliceExpr::Union` evaluation
    /// (positive reading on the consequent, negated on the alternate,
    /// arm-scoped narrows) and each contributes one [`EvolvingPart`]. The
    /// freshness tree mirrors the lowering's own descent, so a
    /// Union/PerArm pair aligns by construction; a mismatch means the
    /// mirror missed a form — the widening decision is then UNPROVEN, so
    /// the whole position fails toward the typed gap (unwidened,
    /// degraded, never warm) rather than guessing either direction.
    fn collect_evolving_parts(
        &mut self,
        expr: &crate::flow_slice_content::SliceExpr,
        freshness: &crate::flow_slice_content::SliceFreshness,
        parts: &mut Vec<EvolvingPart>,
    ) {
        if let crate::flow_slice_content::SliceExpr::Union { arms, guard } = expr {
            match freshness {
                crate::flow_slice_content::SliceFreshness::PerArm(facts)
                    if facts.len() == arms.len() =>
                {
                    for (index, (arm, fact)) in arms.iter().zip(facts.iter()).enumerate() {
                        let mark = self.narrowing_snapshot();
                        self.apply_guard_scoped(guard, index == 0);
                        self.collect_evolving_parts(arm, fact, parts);
                        self.restore_narrowings(mark);
                    }
                    return;
                }
                _ => {
                    self.record_degradation(FlowReturnDegradation::FlowGap(
                        crate::semantic_query::FlowGap::UnmodeledExpression,
                    ));
                }
            }
        }
        let holds_before = self.holds.len();
        let outcome = self.eval_expr(expr);
        let node = self.settle_composite_part(outcome, holds_before);
        let bare_literal = matches!(freshness, crate::flow_slice_content::SliceFreshness::Fresh);
        let fresh = bare_literal
            || self.reads_widening_literal_local(expr)
            || self
                .fresh_call_return_for(expr, node)
                .is_some_and(|call| call.values.contains(&node));
        let fresh_values = self.position_fresh_values(expr, node, bare_literal);
        parts.push(EvolvingPart {
            node,
            fresh,
            fresh_values,
        });
    }

    /// The literal values any part contributes PINNED — its top-level
    /// literal constituents outside its own fresh set. A pinned spelling
    /// of a value cancels every sibling's freshness for that value
    /// (pinned wins, measured), so these are the fold's blockers.
    fn pinned_literal_blockers(&self, parts: &[EvolvingPart]) -> Vec<SemanticNodeId> {
        let graph = self.dispatch.graph();
        let mut pinned = Vec::new();
        for part in parts {
            for lit in self.top_level_literal_nodes(part.node) {
                let data = graph.node_data(lit);
                let fresh_here = part.fresh
                    || part
                        .fresh_values
                        .iter()
                        .any(|value| graph.node_data(*value) == data);
                if !fresh_here && !pinned.contains(&lit) {
                    pinned.push(lit);
                }
            }
        }
        pinned
    }

    /// `values` minus every entry whose literal VALUE some sibling
    /// contributed pinned.
    fn uncancelled_fresh_values(
        &self,
        values: &[SemanticNodeId],
        pinned: &[SemanticNodeId],
    ) -> Vec<SemanticNodeId> {
        let graph = self.dispatch.graph();
        values
            .iter()
            .copied()
            .filter(|value| {
                let data = graph.node_data(*value);
                !pinned.iter().any(|p| graph.node_data(*p) == data)
            })
            .collect()
    }

    /// The top-level LITERAL constituents of one value node: the node
    /// itself when it is a literal, a union's literal members, nothing
    /// otherwise (intersection reduction pins its literal — measured).
    fn top_level_literal_nodes(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        top_level_literal_nodes_in(self.dispatch.graph(), node)
    }

    /// The FRESH literal values one evaluated position carries, at its
    /// own top level: the node itself for a bare-literal spelling, the
    /// widening membership's values for a local read (`All` = every
    /// kept literal), and a completed call's kept fresh values (argument
    /// deposits plus the callee's own authored fresh arms). Every other
    /// spelling carries none — annotations, assertions, and structural
    /// values are pinned.
    fn position_fresh_values(
        &self,
        expr: &crate::flow_slice_content::SliceExpr,
        node: SemanticNodeId,
        bare_literal: bool,
    ) -> Vec<SemanticNodeId> {
        let graph = self.dispatch.graph();
        if bare_literal
            && matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Literal(_))
            )
        {
            return vec![node];
        }
        if let crate::flow_slice_content::SliceExpr::Local {
            name,
            captured: false,
            ..
        } = expr
        {
            match self.membership_of(name.as_ref()) {
                Some(WideningMembership::All) => return self.top_level_literal_nodes(node),
                Some(WideningMembership::Partial(values)) => {
                    let values = values.clone();
                    return self
                        .top_level_literal_nodes(node)
                        .into_iter()
                        .filter(|kept| {
                            let data = graph.node_data(*kept);
                            values.iter().any(|value| graph.node_data(*value) == data)
                        })
                        .collect();
                }
                None => {}
            }
        }
        if let Some(call) = self.fresh_call_return_for(expr, node) {
            let values = Arc::clone(&call.values);
            if values.contains(&node) {
                // The WHOLE return is the fresh deposit — the naked case.
                return vec![node];
            }
            return self
                .top_level_literal_nodes(node)
                .into_iter()
                .filter(|kept| {
                    let data = graph.node_data(*kept);
                    values.iter().any(|value| graph.node_data(*value) == data)
                })
                .collect();
        }
        Vec::new()
    }

    /// Read the newest narrow fact for exactly `subject`, if a guard
    /// established one.
    fn narrowed_read(
        &self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
    ) -> Option<SemanticNodeId> {
        let root = self.resolved_narrow_subject(&subject.root)?;
        self.products
            .narrowing(&root)?
            .facts()
            .iter()
            .find(|fact| fact.path == subject.path)
            .map(|fact| fact.narrowed_to)
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
                self.clear_narrowings_of(&target.root);
                let subject = Self::param_subject(ordinal);
                self.products
                    .set_reaching_type(&subject, ReachingTypeProduct::of(node));
                self.products.set_assignment(
                    &subject,
                    self.products
                        .assignment(&subject)
                        .with_state(DefiniteAssignment::Assigned),
                );
                if degraded {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnmodeledPosition,
                    );
                }
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                let kind = if self.local_value(FlowBindingLayer::Lexical, name).is_some() {
                    crate::flow_slice_content::SliceBindingKind::Let
                } else {
                    crate::flow_slice_content::SliceBindingKind::Var
                };
                // `bind_local` itself clears the narrow facts about the
                // replaced value (the invalidation cannot be forgotten at
                // one of the two write sites).
                self.bind_local(name, kind, node, None, degraded);
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
        consequent: &FlowProductStore,
        consequent_falls: bool,
        alternate: Option<&FlowProductStore>,
        alternate_falls: bool,
        entry: &FlowProductStore,
    ) {
        // The pre-`if` state is the LIVE one again (the caller restored
        // it). Each SURVIVING arm contributes its end value (or the
        // pre-`if` value when it never wrote the binding); a TERMINATED
        // arm contributes NOTHING — with an explicit `else`, every path
        // past the `if` took the surviving arm. A missing `else` is the
        // implicit alternate: it always survives, with the pre-`if`
        // value.
        for (subject, name, before) in self.live_bindings(FlowBindingLayer::Lexical) {
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(consequent.reaching(&subject).unwrap_or(before));
            }
            match alternate {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.reaching(&subject).unwrap_or(before));
                }
                None => contributors.push(before),
                _ => {}
            }
            if contributors.is_empty() || contributors.iter().all(|node| *node == before) {
                continue;
            }
            let joined = self
                .dispatch
                .intern_normalized_union_or_intersection(&contributors, true);
            self.bind_local(
                &name,
                crate::flow_slice_content::SliceBindingKind::Let,
                joined,
                None,
                false,
            );
        }
        for (subject, name, before) in self.live_bindings(FlowBindingLayer::Function) {
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(consequent.reaching(&subject).unwrap_or(before));
            }
            match alternate {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.reaching(&subject).unwrap_or(before));
                }
                None => contributors.push(before),
                _ => {}
            }
            // An arm WROTE the binding when its value moved OR the write
            // raised the single-path fact during an arm (an unchanged
            // value still went through the write) — on an arm whose path
            // SURVIVES the `if`. A terminated arm's writes never reach the
            // join, so its flag-raising is folded the same way. The join
            // folds both, so the flag must not survive either way.
            let written_in_arm = (consequent_falls || alternate_falls)
                && !entry.assignment(&subject).single_path()
                && self.products.assignment(&subject).single_path();
            if contributors.is_empty()
                || (contributors.iter().all(|node| *node == before) && !written_in_arm)
            {
                continue;
            }
            let joined = self
                .dispatch
                .intern_normalized_union_or_intersection(&contributors, true);
            self.bind_local(
                &name,
                crate::flow_slice_content::SliceBindingKind::Var,
                joined,
                None,
                false,
            );
        }
        // Hoisted `var`s DECLARED inside an arm: the state restore scopes
        // them away, but `var` hoisting means the binding itself survives
        // the `if` — even when the arm's path terminates (hoisting is
        // static). Merge them back with the single-path fact INTACT — on
        // the paths that never took the arm the binding has no reaching
        // definition, which is exactly what that fact fails closed on at a
        // read.
        let mut declared_in_arm: Vec<FlowProductSubject> = Vec::new();
        for store in std::iter::once(consequent).chain(alternate) {
            for (subject, _, _) in self.store_bindings(store, FlowBindingLayer::Function) {
                if self.products.reaching(&subject).is_none() && !declared_in_arm.contains(&subject)
                {
                    declared_in_arm.push(subject);
                }
            }
        }
        for subject in declared_in_arm {
            let consequent_node = consequent.reaching(&subject);
            let alternate_node = alternate.and_then(|store| store.reaching(&subject));
            let node = match (consequent_node, alternate_node) {
                (Some(consequent), Some(alternate)) => self
                    .dispatch
                    .intern_normalized_union_or_intersection(&[consequent, alternate], true),
                (Some(consequent), None) => consequent,
                (None, Some(alternate)) => alternate,
                (None, None) => continue,
            };
            self.products
                .set_reaching_type(&subject, ReachingTypeProduct::of(node));
            self.products.set_assignment(
                &subject,
                self.products
                    .assignment(&subject)
                    .with_state(DefiniteAssignment::Assigned)
                    .with_single_path(true),
            );
        }
        let mut param_ordinals: Vec<u32> = Vec::new();
        for store in std::iter::once(consequent).chain(alternate) {
            for ordinal in store_param_ordinals(store) {
                if !param_ordinals.contains(&ordinal) {
                    param_ordinals.push(ordinal);
                }
            }
        }
        param_ordinals.sort_unstable();
        for ordinal in param_ordinals {
            let subject = Self::param_subject(ordinal);
            let before = self.products.reaching(&subject);
            let fallback = before.or_else(|| self.params.get(ordinal as usize).copied());
            let Some(fallback) = fallback else {
                continue;
            };
            let mut contributors: Vec<SemanticNodeId> = Vec::with_capacity(2);
            if consequent_falls {
                contributors.push(consequent.reaching(&subject).unwrap_or(fallback));
            }
            match alternate {
                Some(alternate) if alternate_falls => {
                    contributors.push(alternate.reaching(&subject).unwrap_or(fallback));
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
            self.products
                .set_reaching_type(&subject, ReachingTypeProduct::of(joined));
        }
    }

    /// Rewind to the state a conditional arm was entered with, keeping
    /// exactly the facts an arm establishes that OUTLIVE it.
    ///
    /// - Reaching values rewind in both layers, and so do parameter
    ///   writes: a whole-binding write escapes an arm only through the
    ///   branch JOIN, never as the raw arm value.
    /// - The LEXICAL layer rewinds wholly — its declared types, widening
    ///   provenance and failed-initializer facts are the arm's own — with
    ///   one exception: the single-path fact is established by a clause
    ///   boundary, not by the arm, so it stays.
    /// - The FUNCTION-scoped layer keeps what `var` hoisting makes
    ///   function-wide: declared types and definite-assignment facts. The
    ///   single-path fact in particular is what the branch join reads to
    ///   tell an arm's write from an untouched binding.
    fn restore_arm_entry(&mut self, entry: &FlowProductStore) {
        use super::flow_solve::FlowDomain;
        let mut subjects: Vec<FlowProductSubject> = self.products.subjects();
        for subject in entry.subjects() {
            if !subjects.contains(&subject) {
                subjects.push(subject);
            }
        }
        for subject in subjects {
            let lexical = matches!(
                &subject,
                FlowProductSubject::FrameBinding(slot)
                    if self.bindings.layer(*slot) == Some(FlowBindingLayer::Lexical)
            );
            match entry.reaching_type(&subject) {
                Some(reaching) => self.products.set_reaching_type(&subject, reaching.clone()),
                None => self.products.remove(FlowDomain::ReachingType, &subject),
            }
            if !lexical {
                continue;
            }
            self.products
                .set_declared_type(&subject, entry.declared_type(&subject));
            let held = self.products.assignment(&subject);
            self.products.set_assignment(
                &subject,
                entry
                    .assignment(&subject)
                    .with_single_path(held.single_path()),
            );
        }
    }

    /// The narrowing root a written subject's facts live under: a
    /// parameter is its own root; a binding's facts are rooted at its
    /// authored NAME, which is the frame's lexical slot for that name.
    fn narrow_root_of(&mut self, subject: &FlowProductSubject) -> Option<FlowProductSubject> {
        match subject {
            FlowProductSubject::FrameParam(_) => Some(subject.clone()),
            FlowProductSubject::FrameBinding(slot) => {
                let name = Arc::clone(self.bindings.name(*slot)?);
                Some(self.subject(FlowBindingLayer::Lexical, name.as_ref()))
            }
            FlowProductSubject::GraphNode { .. } => None,
        }
    }

    /// The frame's live bindings in `layer` that carry a reaching value,
    /// in canonical subject order: the subject, its authored name, and its
    /// current value.
    fn live_bindings(
        &self,
        layer: FlowBindingLayer,
    ) -> Vec<(FlowProductSubject, Arc<str>, SemanticNodeId)> {
        self.store_bindings(&self.products, layer)
    }

    /// The bindings of `layer` carrying a reaching value in `store`, in
    /// canonical subject order.
    fn store_bindings(
        &self,
        store: &FlowProductStore,
        layer: FlowBindingLayer,
    ) -> Vec<(FlowProductSubject, Arc<str>, SemanticNodeId)> {
        store
            .subjects_in(super::flow_solve::FlowDomain::ReachingType)
            .into_iter()
            .filter_map(|subject| {
                let FlowProductSubject::FrameBinding(slot) = &subject else {
                    return None;
                };
                let slot = *slot;
                if self.bindings.layer(slot) != Some(layer) {
                    return None;
                }
                let name = Arc::clone(self.bindings.name(slot)?);
                let value = store.reaching(&subject)?;
                Some((subject, name, value))
            })
            .collect()
    }

    /// Snapshot the live binding layers.
    fn layer_state(&self) -> FlowLayerState {
        FlowLayerState {
            products: self.products.clone(),
            inference_only_path: self.inference_only_path,
        }
    }

    /// Restore a snapshot into the live state.
    fn restore_layer_state(&mut self, state: FlowLayerState) {
        self.products = state.products;
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
        use super::flow_solve::FlowDomain;
        for shadow in shadows.iter().rev() {
            state
                .products
                .set_declared_type(&shadow.subject, shadow.prior_declared);
            match &shadow.prior {
                // The outer binding's own products come back whole — the
                // reaching value carries its widening membership, and the
                // definite-assignment product carries both read-observable
                // flags, so a scope close can never split a binding from
                // its own facts.
                Some((reaching, assignment)) => {
                    state
                        .products
                        .set_reaching_type(&shadow.subject, reaching.clone());
                    state.products.set_assignment(&shadow.subject, *assignment);
                }
                None => {
                    state
                        .products
                        .remove(FlowDomain::ReachingType, &shadow.subject);
                    state
                        .products
                        .remove(FlowDomain::DefiniteAssignment, &shadow.subject);
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
        let subject = self.subject(FlowBindingLayer::Lexical, name);
        let prior = self
            .products
            .reaching_type(&subject)
            .filter(|reaching| reaching.united().is_some())
            .cloned()
            .map(|reaching| (reaching, self.products.assignment(&subject)));
        let prior_declared = self.products.declared_type(&subject);
        self.scope_shadows.push(ScopeShadow {
            subject,
            prior,
            prior_declared,
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
            let subject = Self::param_subject(ordinal as u32);
            if state.products.reaching(&subject).is_some() {
                continue;
            }
            if let Some(node) = self.params.get(ordinal) {
                state
                    .products
                    .set_reaching_type(&subject, ReachingTypeProduct::of(*node));
            }
        }
    }

    /// The bindings whose value moved between two states — a write (or a
    /// write-bearing join) on one path that the other path never saw.
    /// Read at a clause boundary where an abrupt exit could have preceded
    /// the write: observing such a binding must fail closed.
    fn written_between(
        &self,
        before: &FlowLayerState,
        after: &FlowLayerState,
    ) -> Vec<FlowProductSubject> {
        after
            .products
            .subjects_in(super::flow_solve::FlowDomain::ReachingType)
            .into_iter()
            .filter(|subject| {
                after.products.reaching(subject).is_some()
                    && before.products.reaching(subject) != after.products.reaching(subject)
            })
            .collect()
    }

    /// Flag a set of try-internal writes on a clause-entry state: a read
    /// of any of them in the clause fails closed (the throw can precede
    /// the write, so the value is one path's, not the join's).
    fn flag_clause_writes(&self, state: &mut FlowLayerState, written: &[FlowProductSubject]) {
        for subject in written {
            let flagged = state.products.assignment(subject).with_single_path(true);
            state.products.set_assignment(subject, flagged);
        }
    }

    /// Flag the function-scoped bindings a fall-through edge carries that
    /// the dispatch edge (the state at the construct's entry) never
    /// defined: their value has no reaching definition on the dispatch
    /// path, which is exactly what the single-path fact fails closed on at
    /// a read.
    fn flag_fallthrough_only_vars(&self, start: &mut FlowLayerState, entry: &FlowLayerState) {
        let fallthrough_only: Vec<FlowProductSubject> = self
            .store_bindings(&start.products, FlowBindingLayer::Function)
            .into_iter()
            .filter(|(subject, _, _)| entry.products.reaching(subject).is_none())
            .map(|(subject, _, _)| subject)
            .collect();
        for subject in fallthrough_only {
            let flagged = start.products.assignment(&subject).with_single_path(true);
            start.products.set_assignment(&subject, flagged);
        }
    }
    /// Flag the function-scoped bindings of a joined state that some
    /// normal-exit path never defined: their surviving value has no
    /// reaching definition on that path, which is exactly what the
    /// single-path fact fails closed on at a read.
    fn flag_conditionally_defined_vars(
        &self,
        joined: &mut FlowLayerState,
        exit_states: &[FlowLayerState],
    ) {
        let conditionally_defined: Vec<FlowProductSubject> = self
            .store_bindings(&joined.products, FlowBindingLayer::Function)
            .into_iter()
            .filter(|(subject, _, _)| {
                exit_states
                    .iter()
                    .any(|state| state.products.reaching(subject).is_none())
            })
            .map(|(subject, _, _)| subject)
            .collect();
        for subject in conditionally_defined {
            let flagged = joined.products.assignment(&subject).with_single_path(true);
            joined.products.set_assignment(&subject, flagged);
        }
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
            Vec<FlowProductSubject>,
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
                None,
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
        if self.local_value(FlowBindingLayer::Lexical, name).is_none()
            && self.local_value(FlowBindingLayer::Function, name).is_none()
        {
            self.seed_destructured_param_element(name);
        }
        // The lexical layer's conditional flag comes from the
        // try-clause-write membership (a binding whose surviving value is
        // one path's, not the join's); a block-scoped conditional binding
        // otherwise never escapes its arm.
        let (node, degraded, conditional) =
            if let Some(node) = self.local_value(FlowBindingLayer::Lexical, name) {
                let assignment = self.local_assignment(FlowBindingLayer::Lexical, name);
                (
                    node,
                    assignment.failed_initializer(),
                    assignment.single_path(),
                )
            } else {
                let node = self
                    .local_value(FlowBindingLayer::Function, name)
                    .or_else(|| self.local_declared(FlowBindingLayer::Function, name))?;
                let assignment = self.local_assignment(FlowBindingLayer::Function, name);
                (
                    node,
                    assignment.failed_initializer(),
                    assignment.single_path(),
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
            None,
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
                (self.local_value(FlowBindingLayer::Lexical, head).is_some()
                    || self.local_value(FlowBindingLayer::Function, head).is_some())
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
            if self
                .products
                .assignment(&Self::param_subject(ordinal))
                .single_path()
            {
                self.record_degradation(
                    crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
                );
            }
            self.param_write(ordinal)
                .or_else(|| self.params.get(ordinal as usize).copied())
        } else {
            self.read_local(head)
        }?;
        let segments: Vec<Arc<str>> = value_ref.path[1..]
            .iter()
            .map(|segment| Arc::from(segment.as_str()))
            .collect();
        self.project_segments_navigate(root, &segments)
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
    /// LITERAL union node. This is the assignment path's
    /// reaching-definition dedup domain only — a narrow's iteration
    /// domain is [`Self::enumerated_union_arms_or_self`], which
    /// additionally enumerates through identity carriers.
    fn union_arms_or_self(&self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.to_vec(),
            _ => vec![node],
        }
    }

    /// The iteration domain every narrow filters: the union arms of
    /// `node`, enumerated THROUGH identity carriers. An alias /
    /// merged-decl / `DeclRef` / `InstantiationRef` subject peels
    /// through the one shared `Instantiate` dispatch
    /// ([`ProjectSemanticDispatch::unwrap_identity_carrier_for_relation`]
    /// — the same unwrap the `in` classifier and the relation engine
    /// use), and a union found behind the carrier contributes its
    /// members, recursively, so an alias of an alias flattens exactly
    /// as the checker's union normalization does. A non-union arm stays
    /// the ORIGINAL node: the published narrow keeps the authored
    /// carrier, not its expansion. Treating a carrier as ONE opaque arm
    /// was a defect: every narrow computed over an alias-typed subject
    /// found nothing to filter and published the WHOLE alias — a
    /// superset of the checker's type — complete and warm.
    ///
    /// A carrier the engine cannot resolve stays one opaque arm AND
    /// records the typed `FlowGap::GuardNarrowing`: the narrow then
    /// retains a superset (the sound direction — dropping a real
    /// contributor is strictly worse than widening) and the gap keeps
    /// it `ReturnOnly`, never warm. "Nothing filtered" must mean PROVED
    /// unchanged, never "could not enumerate".
    fn enumerated_union_arms_or_self(&mut self, node: SemanticNodeId) -> Vec<SemanticNodeId> {
        let mut arms = Vec::new();
        let mut expanded = rustc_hash::FxHashSet::default();
        let mut gapped = false;
        self.collect_enumerated_arms(node, &mut arms, &mut expanded, &mut gapped);
        if gapped {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        arms
    }

    /// [`Self::enumerated_union_arms_or_self`]'s recursion. `expanded`
    /// holds every union node whose members were already contributed, so
    /// a cyclic carrier chain terminates without dropping or duplicating
    /// a contributor.
    fn collect_enumerated_arms(
        &self,
        node: SemanticNodeId,
        arms: &mut Vec<SemanticNodeId>,
        expanded: &mut rustc_hash::FxHashSet<SemanticNodeId>,
        gapped: &mut bool,
    ) {
        if let Some(SemanticNodeData::Union(members)) =
            self.dispatch.graph().node_data(node).as_deref()
        {
            if expanded.insert(node) {
                for member in members.iter().copied() {
                    self.collect_enumerated_arms(member, arms, expanded, gapped);
                }
            }
            return;
        }
        match self.dispatch.unwrap_identity_carrier_for_relation(node) {
            super::relation::IdentityCarrierUnwrap::Concrete(concrete)
                if concrete != node
                    && matches!(
                        self.dispatch.graph().node_data(concrete).as_deref(),
                        Some(SemanticNodeData::Union(_))
                    ) =>
            {
                self.collect_enumerated_arms(concrete, arms, expanded, gapped);
            }
            super::relation::IdentityCarrierUnwrap::Concrete(_) => arms.push(node),
            super::relation::IdentityCarrierUnwrap::Unresolvable => {
                arms.push(node);
                *gapped = true;
            }
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
                    .param_write(*ordinal)
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
        // The TERMINAL hop's member optionality folds into the read's
        // value: reading `x.k` where `k` is declared optional includes
        // the absent-key `undefined` — the checker's member-READ rule
        // (indexed access included; measured under `--strict`,
        // `exactOptionalPropertyTypes` off — the oracle profile).
        // Presence of a key and non-`undefined`-ness of its value are two
        // different facts: an `in` guard establishes the first and never
        // strips the second, so the fold lives HERE, on the read, not in
        // any guard. Terminal ONLY: an intermediate optional hop read
        // without a discharging narrow is an error program, and the
        // checker's own recovery projects the tail off the non-`undefined`
        // part — folding mid-path would instead miss the tail on the
        // fabricated `undefined` arm. The fold is PROOF-gated
        // ([`Self::member_read_optionality`]): only a surface that
        // provably declares the member optional folds; an undecidable
        // surface keeps the shared projection's answer unchanged.
        let Some((terminal, prefix)) = segments.split_last() else {
            return Some(base);
        };
        let parent = if prefix.is_empty() {
            base
        } else {
            self.project_path_navigate(base, prefix)?
        };
        let optional = self.member_read_optionality(parent, terminal.as_ref()) == Some(true);
        let value = self.project_path_navigate(parent, std::slice::from_ref(terminal))?;
        Some(if optional {
            self.fold_optional_read_undefined(value)
        } else {
            value
        })
    }

    /// The shared `ProjectPath { mode: Navigate }` walk over a non-empty
    /// member path — the projection half of
    /// [`Self::project_segments_navigate`].
    fn project_path_navigate(
        &mut self,
        base: SemanticNodeId,
        segments: &[Arc<str>],
    ) -> Option<SemanticNodeId> {
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

    /// Whether reading member `key` off `base` provably includes the
    /// absent-key `undefined`: `Some(true)` when a surface declares the
    /// member OPTIONAL (for a union, on ANY arm — that arm's read carries
    /// `undefined`, so the union's does), `Some(false)` when every arm
    /// declares it required, `None` when nothing is proved (a type
    /// parameter, an index-signature surface, an unresolvable carrier, a
    /// proven-absent key — the projection's own miss answers those).
    /// Union arms flatten on a seen-guarded worklist; each non-union arm
    /// answers through THE key-presence authority
    /// ([`Self::arm_in_presence`], heritage/intersection arms included),
    /// so the `in` guard and the member read can never disagree about a
    /// key's declaration.
    fn member_read_optionality(&mut self, base: SemanticNodeId, key: &str) -> Option<bool> {
        let mut pending = vec![base];
        let mut seen: Vec<SemanticNodeId> = Vec::new();
        let mut any_optional = false;
        let mut all_required = true;
        while let Some(node) = pending.pop() {
            if seen.contains(&node) {
                continue;
            }
            seen.push(node);
            let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(node) {
                super::relation::IdentityCarrierUnwrap::Concrete(concrete) => concrete,
                super::relation::IdentityCarrierUnwrap::Unresolvable => {
                    all_required = false;
                    continue;
                }
            };
            if concrete != node {
                if seen.contains(&concrete) {
                    continue;
                }
                seen.push(concrete);
            }
            if let Some(SemanticNodeData::Union(arms)) =
                self.dispatch.graph().node_data(concrete).as_deref()
            {
                pending.extend(arms.iter().copied());
                continue;
            }
            match self.arm_in_presence(concrete, key) {
                InArmPresence::Optional => any_optional = true,
                InArmPresence::Always => {}
                InArmPresence::Never | InArmPresence::Unknown => all_required = false,
            }
        }
        if any_optional {
            Some(true)
        } else if all_required {
            Some(false)
        } else {
            None
        }
    }

    /// Union the absent-key `undefined` into an optional member's read
    /// value. `any` / `unknown` absorb it, and a value that already
    /// carries an `undefined` arm (the member's own explicit
    /// `| undefined`) gains no duplicate.
    fn fold_optional_read_undefined(&mut self, value: SemanticNodeId) -> SemanticNodeId {
        let data = self.dispatch.graph().node_data(value);
        match data.as_deref() {
            Some(SemanticNodeData::Primitive(
                PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Undefined,
            )) => return value,
            Some(SemanticNodeData::Union(arms)) => {
                let arms: Vec<SemanticNodeId> = arms.to_vec();
                if arms.iter().any(|arm| self.arm_reduces_to_undefined(*arm)) {
                    return value;
                }
            }
            _ => {
                if self.arm_reduces_to_undefined(value) {
                    return value;
                }
            }
        }
        let undefined = self
            .dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
        self.dispatch
            .intern_normalized_union_or_intersection(&[value, undefined], true)
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

    /// Whether `a` and `b` can have a common inhabitant, through the SAME
    /// sole relation authority the assignability question goes to.
    ///
    /// The evaluator owns NO relation classifier of its own. `Disjoint`
    /// carries the authority's disjointness PROOF (which this consumer then
    /// applies to the narrow), `Overlaps` means no such proof exists, and
    /// `Undecided` is no fact at all — the caller records the typed
    /// nominal-relation gap and never treats it as either answer.
    fn comparable(
        &self,
        a: SemanticNodeId,
        b: SemanticNodeId,
    ) -> super::relation::ComparabilityVerdict {
        self.dispatch.nodes_comparable(a, b)
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
    ) {
        use crate::flow_slice_content::SliceGuard;
        let fact = match guard {
            SliceGuard::None => return,
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
            // symmetry the lowering's `!` uses). A later conjunct that
            // empties its subject pushes that subject's `never` overlay —
            // the overlay's newest-wins read is the "final alternative
            // overlay" rule an enclosing disjunction consumes.
            SliceGuard::And(parts) => {
                if positive {
                    for part in parts.iter() {
                        self.apply_guard_scoped(part, true);
                    }
                } else {
                    self.apply_guard_union(parts, false);
                }
                return;
            }
            SliceGuard::Or(parts) => {
                if positive {
                    self.apply_guard_union(parts, true);
                } else {
                    for part in parts.iter() {
                        self.apply_guard_scoped(part, false);
                    }
                }
                return;
            }
        };
        match fact {
            GuardNarrowing::Unchanged => {}
            GuardNarrowing::Narrowed(subject, node) => {
                self.push_narrowing(&subject, node);
            }
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
    ) {
        let mut alternatives: Vec<
            Vec<(
                crate::flow_slice_content::SliceNarrowSubject,
                SemanticNodeId,
            )>,
        > = Vec::with_capacity(parts.len());
        for part in parts.iter() {
            let mark = self.narrowing_snapshot();
            self.apply_guard_scoped(part, positive);
            let applied = self.narrowings_since(&mark);
            self.restore_narrowings(mark);
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
            self.push_narrowing(&subject, node);
        }
    }

    /// Filter `subject`'s arms by a per-arm predicate, joining the
    /// survivors back into the narrow's node. An empty survivor set is a
    /// positive PROOF that every arm is off the tested edge, distinct
    /// from an unchanged/undecidable fact — the CALLER converts it under
    /// its guard family's measured checker rule (usually a `never`
    /// subject on a still-alive edge).
    fn narrow_arms_by(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        mut keep: impl FnMut(&mut Self, SemanticNodeId) -> Option<bool>,
    ) -> ArmFilter {
        let Some(current) = self.subject_current_node(subject) else {
            return ArmFilter::Unchanged;
        };
        let arms = self.enumerated_union_arms_or_self(current);
        let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        for arm in &arms {
            match keep(self, *arm) {
                Some(true) => survivors.push(*arm),
                Some(false) => {}
                None => return ArmFilter::Unchanged,
            }
        }
        if survivors.is_empty() {
            return ArmFilter::NoSurvivor;
        }
        if survivors.len() == arms.len() {
            return ArmFilter::Unchanged;
        }
        let node = self
            .dispatch
            .intern_normalized_union_or_intersection(&survivors, true);
        ArmFilter::Narrowed(node)
    }

    /// The graph's `never` — what a subject reads on a guard edge every
    /// arm is proved off of. The edge itself stays alive.
    fn never_node(&self) -> SemanticNodeId {
        self.dispatch
            .graph()
            .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never))
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
        let Some(current) = self.subject_current_node(subject) else {
            return GuardNarrowing::Unchanged;
        };
        let arms = self.enumerated_union_arms_or_self(current);
        let mut out: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        let mut changed = false;
        let mut unclassified = false;
        for arm in &arms {
            // The non-primitive `object` inhabits BOTH the `"object"` and
            // the `"function"` runtime kinds (functions are objects), so
            // the positive `"function"` edge narrows it to the global
            // `Function` surface (measured: `x: object` reads `Function`
            // inside `typeof x === "function"`); an unavailable lib
            // surface keeps the arm possible, degraded. The other edges
            // keep the plain classification: `"object"` matches on both
            // edges, scalar kinds are proved off, and the negated
            // `"function"` edge keeps the arm unchanged — the checker has
            // no "object minus functions" type to narrow to.
            if kind == crate::flow_slice_content::SliceTypeofKind::Function
                && !negated
                && matches!(
                    self.dispatch.graph().node_data(*arm).as_deref(),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Object))
                )
            {
                match self.lower_global_function_surface() {
                    Some(function) => {
                        out.push(function);
                        changed = true;
                    }
                    None => {
                        out.push(*arm);
                        unclassified = true;
                    }
                }
                continue;
            }
            match self.arm_typeof_class(*arm, kind) {
                ArmGuardClass::Match => {
                    if negated {
                        changed = true;
                    } else {
                        out.push(*arm);
                    }
                }
                ArmGuardClass::NoMatch => {
                    if negated {
                        out.push(*arm);
                    } else {
                        changed = true;
                    }
                }
                ArmGuardClass::Unclassified => {
                    unclassified = true;
                    out.push(*arm);
                }
            }
        }
        if unclassified {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        if out.is_empty() {
            // Every arm is proved off the tested edge: the subject reads
            // `never` and the edge stays alive.
            return GuardNarrowing::Narrowed(subject.clone(), self.never_node());
        }
        if !changed && out.len() == arms.len() {
            return GuardNarrowing::Unchanged;
        }
        let node = self
            .dispatch
            .intern_normalized_union_or_intersection(&out, true);
        GuardNarrowing::Narrowed(subject.clone(), node)
    }

    /// The global `Function` surface, lowered through the owner scope the
    /// way an `instanceof` constructor reference is. `None` when the lib
    /// surface is unavailable — including a lowering that only reaches an
    /// UNRESOLVED bare reference or an opaque carrier, which must never
    /// publish as a resolved narrow — so the caller keeps the arm
    /// possible, degraded, never proved off an edge.
    fn lower_global_function_surface(&mut self) -> Option<SemanticNodeId> {
        let ty = verter_type_expr::TypeExpr::Ref {
            name: Arc::from("Function"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        };
        let node = self.dispatch.lower_type_expr_in_owner_scope_with_context(
            self.canonical,
            self.owner,
            &ty,
            crate::semantic_query::ProjectionReductionContext::structural_transit(),
        )?;
        match self.dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::BareRef(_)) | Some(SemanticNodeData::Opaque(_)) | None => None,
            _ => Some(node),
        }
    }

    /// One union arm's verdict against the runtime type a `typeof`
    /// comparison names. A primitive is its own kind (`null` is the
    /// operator's `"object"` quirk); a literal its primitive's; objects,
    /// arrays, tuples and spread programs are `"object"`, signatures
    /// `"function"`; `never` is uninhabited, off both edges. Anything
    /// the graph cannot place under exactly one runtime kind — `any`,
    /// `unknown`, a memberless `{}` surface (primitives inhabit it), an
    /// unresolved carrier — is `Unclassified`: `NoMatch` means PROVED
    /// non-inhabitance of the tested edge, never "unrecognized". The one
    /// dual-kind value domain — the non-primitive `object`, which also
    /// inhabits `"function"` — is intercepted by [`Self::narrow_typeof`]
    /// on the positive `"function"` edge before this classification;
    /// the `Object` mapping here serves the remaining edges, where the
    /// checker keeps the arm (negated `"function"`, either `"object"`
    /// edge) or proves it off (scalar kinds).
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
            // A template-literal type denotes only strings, whatever its
            // placeholders resolve to.
            Some(SemanticNodeData::TemplateLiteral { .. }) => Some(SliceTypeofKind::String),
            _ => None,
        };
        match classified {
            Some(observed) if observed == kind => ArmGuardClass::Match,
            Some(_) => ArmGuardClass::NoMatch,
            None => ArmGuardClass::Unclassified,
        }
    }

    /// A bare truthiness test keeps every arm the shared truthiness-domain
    /// authority proves CAN take the requested edge — the flow frame holds
    /// no truthiness rule of its own: per enumerated arm it CONSUMES the
    /// demand-scoped [`ClassifyTruthinessDomain`] fact (settling identity
    /// carriers through the same shared unwrap the enumeration and the
    /// relation engine use) and reads the tested edge's bucket. Broad
    /// primitives such as `boolean`, `string`, and `number` inhabit both
    /// buckets, so both edges keep them; an UNDECIDED bucket keeps the arm
    /// on the tested edge AND records the typed guard gap — the checker
    /// decides such arms, so treating "undecided" as "proved on/off the
    /// edge" would publish a wrong narrow clean and warm. An edge no arm
    /// survives narrows the subject to `never` WITHOUT killing the branch:
    /// the checker keeps the branch's syntactic returns, typed through the
    /// `never` subject (measured: `` `item-${string}` | "none" `` under
    /// `if (v)` reads `{ v: never }` from the falsy edge's `return { v }`).
    ///
    /// [`ClassifyTruthinessDomain`]: crate::semantic_query::SemanticQueryKey::ClassifyTruthinessDomain
    fn narrow_truthy(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        negated: bool,
    ) -> GuardNarrowing {
        // A truthiness test over a PROPERTY narrows two references, both
        // measured from the checker: the tested reference itself (below),
        // and the property's PARENT — per-arm, an arm survives the tested
        // edge iff ANY leaf of its projected member can take it, so a
        // literal discriminant (`ok: true` / `ok: false`) filters both
        // edges while a broad member (`v: string`) keeps its arm on the
        // falsy edge (`""` inhabits it). The parent fact rides the
        // narrowing overlay directly; the leaf fact is the returned
        // narrow, so both land under the same guard scope.
        let mut undecided = false;
        if !subject.path.is_empty() {
            let parent_subject = crate::flow_slice_content::SliceNarrowSubject {
                root: subject.root.clone(),
                path: Arc::from(
                    subject.path[..subject.path.len() - 1]
                        .to_vec()
                        .into_boxed_slice(),
                ),
            };
            let last: Arc<[Arc<str>]> = Arc::from(
                subject.path[subject.path.len() - 1..]
                    .to_vec()
                    .into_boxed_slice(),
            );
            let parent_fact = self.narrow_arms_by(&parent_subject, |this, arm| {
                let member = this.project_segments_navigate(arm, &last)?;
                let leaves = this.enumerated_union_arms_or_self(member);
                Some(
                    leaves
                        .iter()
                        .any(|leaf| match this.arm_truthiness_edge(*leaf, negated) {
                            crate::semantic_query::TruthinessInhabitance::Yes => true,
                            crate::semantic_query::TruthinessInhabitance::No => false,
                            crate::semantic_query::TruthinessInhabitance::Undecided => {
                                undecided = true;
                                true
                            }
                        }),
                )
            });
            match parent_fact {
                // Every parent arm is proved off the tested edge. The
                // checker filters a PARENT through a property test only
                // when the member DISCRIMINATES its arms; a filter that
                // would empty the parent proves the member's edge verdict
                // identical across the arms (or the parent non-union),
                // and the checker then keeps the parent's declared type
                // unchanged (measured: `{ ok: false }`, and a two-arm
                // union whose `ok` is `false` in both, keep their types
                // inside `if (x.ok)`). Only the tested LEAF collapses —
                // it does below.
                ArmFilter::NoSurvivor => {}
                ArmFilter::Narrowed(node) => {
                    self.push_narrowing(&parent_subject, node);
                }
                ArmFilter::Unchanged => {}
            }
        }
        let fact = self.narrow_arms_by(subject, |this, arm| {
            Some(match this.arm_truthiness_edge(arm, negated) {
                crate::semantic_query::TruthinessInhabitance::Yes => true,
                crate::semantic_query::TruthinessInhabitance::No => false,
                crate::semantic_query::TruthinessInhabitance::Undecided => {
                    undecided = true;
                    true
                }
            })
        });
        if undecided {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        match fact {
            ArmFilter::NoSurvivor => GuardNarrowing::Narrowed(subject.clone(), self.never_node()),
            ArmFilter::Narrowed(node) => GuardNarrowing::Narrowed(subject.clone(), node),
            ArmFilter::Unchanged => GuardNarrowing::Unchanged,
        }
    }

    /// The tested edge's bucket of one arm's truthiness domain, CONSUMED
    /// from the sole authority ([`ClassifyTruthinessDomain`]): the falsy
    /// bucket on the negated edge, the truthy bucket otherwise. The arm is
    /// settled through the shared identity-carrier unwrap first (the same
    /// `Instantiate`-backed peel the arm enumeration and the relation
    /// engine use) so an alias/`DeclRef` arm classifies by its concrete
    /// structure; an unresolvable carrier is `Undecided` — reported, never
    /// guessed.
    ///
    /// [`ClassifyTruthinessDomain`]: crate::semantic_query::SemanticQueryKey::ClassifyTruthinessDomain
    fn arm_truthiness_edge(
        &mut self,
        arm: SemanticNodeId,
        negated: bool,
    ) -> crate::semantic_query::TruthinessInhabitance {
        let settled = match self.dispatch.unwrap_identity_carrier_for_relation(arm) {
            super::relation::IdentityCarrierUnwrap::Concrete(concrete) => concrete,
            super::relation::IdentityCarrierUnwrap::Unresolvable => {
                return crate::semantic_query::TruthinessInhabitance::Undecided;
            }
        };
        let domain = self.dispatch.classify_truthiness_domain_read(settled).value;
        if negated {
            domain.falsy
        } else {
            domain.truthy
        }
    }

    /// `subject === literal`. An EMPTY subject path filters the binding's
    /// own arms by overlap with the literal (either assignability
    /// direction — the two spellings of "the same literal"); when no arm
    /// filters (the subject's whole type is a BROAD arm the literal only
    /// narrows, `x === "a"` over `x: string`) the positive reading narrows
    /// the subject to the literal itself — the checker's own rule for a
    /// literal strictly narrower than the declared type. A non-empty path
    /// is a DISCRIMINANT, filtering the arms of the tested property's
    /// PARENT reference — the root itself for a one-segment path, the
    /// enclosing reference for a deeper one. The checker never selects a
    /// ROOT arm through a nested discriminant: doing so DROPS the
    /// constituents whose nested member differs (a SUBSET of the
    /// checker's type — strictly worse than widening), while the parent
    /// reference is exactly what it narrows (`m.meta.kind === "one"`
    /// narrows `m.meta`, never `m`).
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
            // An arm whose relation to the literal the oracle cannot
            // decide (a deferred form such as a template-literal arm)
            // stays possible on BOTH edges and degrades the result: the
            // checker decides such relations (`"none"` is off the
            // `` `item-${string}` `` edge), so treating "undecided" as
            // "proved unchanged" published a superset clean and warm.
            let mut undecided = false;
            let narrowed = self.narrow_arms_by(subject, |this, arm| {
                if matches!(
                    this.dispatch.graph().node_data(arm).as_deref(),
                    Some(SemanticNodeData::Primitive(
                        PrimitiveKind::Any | PrimitiveKind::Unknown
                    ))
                ) {
                    return None;
                }
                let (Some(forward), Some(backward)) = (
                    this.assignable(arm, literal_node),
                    this.assignable(literal_node, arm),
                ) else {
                    undecided = true;
                    return Some(true);
                };
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
            if undecided {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    crate::semantic_query::FlowGap::GuardNarrowing,
                ));
            }
            match narrowed {
                // No arm overlaps the literal at all: the subject reads
                // `never` on this edge and the edge stays alive
                // (measured: `x: "a"` is `never` inside `if (x === "b")`,
                // and only its own reads collapse — a sibling binding's
                // contributor keeps its type).
                ArmFilter::NoSurvivor => {
                    return GuardNarrowing::Narrowed(subject.clone(), self.never_node());
                }
                ArmFilter::Narrowed(node) => {
                    return GuardNarrowing::Narrowed(subject.clone(), node);
                }
                ArmFilter::Unchanged => {}
            }
            if !negated {
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
            GuardNarrowing::Unchanged
        } else {
            // The discriminant narrows the tested property's PARENT
            // reference, so the fact lands at the parent subject: a
            // later read of the parent, or of any member projected from
            // it, resolves against the surviving arms — while the root
            // keeps every constituent for two segments and beyond.
            let parent_subject = crate::flow_slice_content::SliceNarrowSubject {
                root: subject.root.clone(),
                path: Arc::from(
                    subject.path[..subject.path.len() - 1]
                        .to_vec()
                        .into_boxed_slice(),
                ),
            };
            let last: Arc<[Arc<str>]> = Arc::from(
                subject.path[subject.path.len() - 1..]
                    .to_vec()
                    .into_boxed_slice(),
            );
            // A parent arm whose projected discriminant the relation
            // oracle cannot compare stays possible on BOTH edges and
            // degrades the result — undecided is never "proved off this
            // edge" nor "proved unchanged".
            let mut undecided = false;
            let mut projected: Vec<SemanticNodeId> = Vec::new();
            let narrowed = self.narrow_arms_by(&parent_subject, |this, arm| {
                let member = this.project_segments_navigate(arm, &last)?;
                projected.push(member);
                let verdict = if negated {
                    // Excluding one literal removes a parent arm only when
                    // the projected member is wholly that literal. A named
                    // alias can project a broad discriminant union without
                    // exposing its root constituents; `"a"` fits
                    // `"a" | "b"`, but its negative edge remains possible.
                    this.assignable(member, literal_node)
                        .map(|covered| !covered)
                } else {
                    this.assignable(literal_node, member)
                };
                match verdict {
                    Some(keep) => Some(keep),
                    None => {
                        undecided = true;
                        Some(true)
                    }
                }
            });
            if undecided {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    crate::semantic_query::FlowGap::GuardNarrowing,
                ));
            }
            match narrowed {
                // The checker filters a PARENT through a member test only
                // when the member DISCRIMINATES its arms. A no-survivor
                // filter whose projections DIFFER is the genuine
                // discriminant case with no matching arm: the parent reads
                // `never` and the edge stays alive (measured:
                // `{ kind: "a" } | { kind: "c" }` under `x.kind === "b"`).
                // A non-union parent, or one whose member projects
                // identically in every arm, is never discriminated — the
                // checker keeps its declared type — so no fact lands.
                ArmFilter::NoSurvivor => {
                    if projected.len() > 1 && projected.windows(2).any(|pair| pair[0] != pair[1]) {
                        GuardNarrowing::Narrowed(parent_subject, self.never_node())
                    } else {
                        GuardNarrowing::Unchanged
                    }
                }
                ArmFilter::Narrowed(node) => GuardNarrowing::Narrowed(parent_subject, node),
                ArmFilter::Unchanged => GuardNarrowing::Unchanged,
            }
        }
    }

    /// Bake a narrow verdict into a state SNAPSHOT's reaching-definition
    /// layer, so the binding reads the narrowed node on that edge. Used
    /// where two differently-narrowed edges JOIN: the overlay intersection
    /// would erase both facts (they differ), while the
    /// reaching-definition join unions the two narrowed values — the
    /// checker's own rule for a fall-through-joined switch case start.
    fn bake_narrow_into_state(
        &mut self,
        state: &mut FlowLayerState,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        node: SemanticNodeId,
    ) {
        // A fact about a PATH reference (a nested discriminant's parent)
        // cannot ride the reaching-definition layers — those carry whole
        // bindings — so it rides the state's narrowing overlay. The
        // fall-through JOIN intersects overlays, so a deep fact two
        // edges disagree on is dropped there: the joined start then
        // reads the un-narrowed parent — a checker-superset, the sound
        // direction — never another clause's value.
        if !subject.path.is_empty() {
            let root = self.narrow_subject(&subject.root);
            push_narrowing_into(&mut state.products, &root, &subject.path, node);
            return;
        }
        // A whole-binding fact rides the reaching-TYPE product, keeping
        // the binding's own literal-widening provenance: a narrow
        // replaces the value, never the freshness the value carries.
        let rebind = |products: &mut FlowProductStore, target: &FlowProductSubject| {
            let widening = products.widening(target).cloned();
            products.set_reaching_type(
                target,
                ReachingTypeProduct::of(node).with_widening(widening),
            );
        };
        match &subject.root {
            crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => {
                rebind(&mut state.products, &Self::param_subject(*ordinal));
            }
            crate::flow_slice_content::SliceNarrowRoot::Local(name) => {
                let lexical = self.subject(FlowBindingLayer::Lexical, name);
                if state.products.reaching(&lexical).is_some() {
                    rebind(&mut state.products, &lexical);
                    return;
                }
                let function = self.subject(FlowBindingLayer::Function, name);
                if state.products.reaching(&function).is_some() {
                    rebind(&mut state.products, &function);
                }
            }
        }
    }

    /// The arms of the discriminant's PARENT reference that NO case test
    /// covers: the remainder the default clause's dispatch edge narrows
    /// to, and — when empty with no default authored — the proof the
    /// no-matching-case path is dead (the ONE exhaustiveness verdict).
    /// `None` when anything is undecidable (a projection miss, an
    /// undecided relation): the caller then narrows nothing, keeps the
    /// no-match path live, and DEGRADES — a declined probe leaves a
    /// checker-superset on those edges, never a clean one.
    ///
    /// "Covers" is per-LEAF mutual assignability between the arm's
    /// projected member and a test literal
    /// ([`Self::switch_member_covered`]): a broad leaf a literal merely
    /// fits (`string` under `case "a":`) is NOT covered — the checker's
    /// default edge keeps it. Returns the surviving arms and the arm
    /// count, so the caller can tell "no narrow established"
    /// (survivors == arms) from a real one.
    fn switch_discriminant_remainder(
        &mut self,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        tests: &[crate::flow_slice_content::SliceGuardLiteral],
    ) -> Option<(Vec<SemanticNodeId>, usize)> {
        // The narrow lands at the tested property's PARENT reference
        // (the root itself for a whole-subject or one-segment
        // discriminant), so the arms are the parent's — a nested
        // discriminant must never select among ROOT arms: that DROPS the
        // constituents whose nested member differs, a subset of the
        // checker's type. The probe reads the live layers WITHOUT
        // folding a membership flag: asking about coverage is not an
        // observation of the binding's value, so it must not degrade
        // one.
        let root_subject = crate::flow_slice_content::SliceNarrowSubject {
            root: subject.root.clone(),
            path: Arc::from(Vec::new().into_boxed_slice()),
        };
        let root = if let Some(node) = self.narrowed_read(&root_subject) {
            node
        } else {
            match &subject.root {
                crate::flow_slice_content::SliceNarrowRoot::Param(ordinal) => self
                    .param_write(*ordinal)
                    .or_else(|| self.params.get(*ordinal as usize).copied())?,
                crate::flow_slice_content::SliceNarrowRoot::Local(name) => self
                    .local_value(FlowBindingLayer::Lexical, name)
                    .or_else(|| self.local_value(FlowBindingLayer::Function, name))?,
            }
        };
        let parent = if subject.path.len() > 1 {
            let parent_subject = crate::flow_slice_content::SliceNarrowSubject {
                root: subject.root.clone(),
                path: Arc::from(
                    subject.path[..subject.path.len() - 1]
                        .to_vec()
                        .into_boxed_slice(),
                ),
            };
            if let Some(node) = self.narrowed_read(&parent_subject) {
                node
            } else {
                self.project_segments_navigate(root, &subject.path[..subject.path.len() - 1])?
            }
        } else {
            root
        };
        let arms = self.enumerated_union_arms_or_self(parent);
        // `boolean` decomposes into its two literal arms for coverage —
        // the checker's own reading of `case true:` / `case false:` over
        // a boolean discriminant. The check peels the arm's identity
        // carrier so an alias to `boolean` decomposes exactly as the
        // authored primitive does.
        let arms: Vec<SemanticNodeId> = arms
            .into_iter()
            .flat_map(|arm| {
                let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(arm) {
                    super::relation::IdentityCarrierUnwrap::Concrete(node) => node,
                    super::relation::IdentityCarrierUnwrap::Unresolvable => arm,
                };
                if matches!(
                    self.dispatch.graph().node_data(concrete).as_deref(),
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
        for arm in &arms {
            let member = if subject.path.is_empty() {
                *arm
            } else {
                self.project_segments_navigate(*arm, &subject.path[subject.path.len() - 1..])?
            };
            if !self.switch_member_covered(member, &test_nodes)? {
                survivors.push(*arm);
            }
        }
        let total = arms.len();
        Some((survivors, total))
    }

    /// Whether every runtime value of one arm's tested member is matched
    /// by some carried case test — the coverage relation behind the
    /// switch remainder and the exhaustiveness verdict. Coverage is
    /// decided per LEAF of the member: its enumerated union arms, with a
    /// `boolean` leaf decomposing into its two literals — the checker's
    /// own reading, which proves `case "a"` + `case "b"` exhaustive over
    /// a `kind: "a" | "b"` member exactly as over two discriminated
    /// arms. `None` when a leaf relation is undecided or the member's
    /// arms cannot be enumerated: coverage is then unknowable and the
    /// whole probe declines — the callers degrade instead of publishing
    /// a clean unnarrowed edge or a false liveness verdict.
    fn switch_member_covered(
        &mut self,
        member: SemanticNodeId,
        tests: &[SemanticNodeId],
    ) -> Option<bool> {
        let mut leaves = Vec::new();
        let mut expanded = rustc_hash::FxHashSet::default();
        let mut gapped = false;
        self.collect_enumerated_arms(member, &mut leaves, &mut expanded, &mut gapped);
        if gapped {
            return None;
        }
        let leaves: Vec<SemanticNodeId> = leaves
            .into_iter()
            .flat_map(|leaf| {
                let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(leaf) {
                    super::relation::IdentityCarrierUnwrap::Concrete(node) => node,
                    super::relation::IdentityCarrierUnwrap::Unresolvable => leaf,
                };
                if matches!(
                    self.dispatch.graph().node_data(concrete).as_deref(),
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
                    vec![leaf]
                }
            })
            .collect();
        'leaves: for leaf in &leaves {
            for test in tests {
                if self.assignable(*leaf, *test)? && self.assignable(*test, *leaf)? {
                    continue 'leaves;
                }
            }
            return Some(false);
        }
        Some(true)
    }

    /// `subject instanceof Ctor`, applying the checker's own per-arm
    /// narrowing (the same rule [`Self::narrow_to_predicate_target`]
    /// applies to `x is T`): on the positive edge an arm assignable to
    /// the constructor's instance type survives as itself, an arm the
    /// instance type is assignable to narrows TO the instance type (the
    /// downcast reading), and an unrelated arm keeps the checker's
    /// intersection unless provably disjoint — the one proved dead
    /// reading. The negated edge drops only an arm proved to BE the
    /// tested class (node identity with the instance type); structural
    /// assignability alone cannot prove derivation, so an assignable
    /// but non-identical arm stays possible, degraded. The instance
    /// type resolves as a bare type reference in owner scope — the same
    /// lowering any authored annotation of that name takes. The
    /// lowering mints this fact only for a constructor name it proved
    /// to be the module's single same-file `class` declaration left
    /// free by the frame, which is exactly when that type reference IS
    /// the compared value's instance type; every other right-hand side
    /// reaches the evaluator as a typed gap, never as a fact over the
    /// wrong binding.
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
        if negated {
            // The checker's negated edge drops only an arm PROVED to be
            // the tested class family; every other arm stays exactly as
            // declared. Structural assignability over-approximates that
            // proof — a same-shape but underived arm is assignable yet
            // the checker keeps it — so the only structural proof of
            // "this arm IS the tested class" is node identity with the
            // instance type. An assignable-but-not-identical arm may or
            // may not be derived: it stays possible, degraded.
            let mut gapped = false;
            let fact = self.narrow_arms_by(subject, |this, arm| {
                if this.instanceof_arm_is_unclassifiable(arm) {
                    gapped = true;
                    return Some(true);
                }
                Some(match this.assignable(arm, instance) {
                    Some(true) => {
                        if arm == instance {
                            false
                        } else {
                            gapped = true;
                            true
                        }
                    }
                    Some(false) => true,
                    None => {
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
            return match fact {
                // Every arm IS the tested class: excluding it empties the
                // subject — `never`, on a still-alive edge.
                ArmFilter::NoSurvivor => {
                    GuardNarrowing::Narrowed(subject.clone(), self.never_node())
                }
                ArmFilter::Narrowed(node) => GuardNarrowing::Narrowed(subject.clone(), node),
                ArmFilter::Unchanged => GuardNarrowing::Unchanged,
            };
        }
        // The POSITIVE edge, measured against the checker over a
        // local-class matrix:
        //
        // * `null` / `undefined` arms are STRIPPED first — they never
        //   survive the positive edge and never enter the fallback
        //   intersection (`L | null` narrows to `L & K`, never
        //   `(L | null) & K`).
        // * an arm IDENTICAL to the instance type survives as itself,
        //   and with any surviving arm every unrelated arm is DROPPED
        //   (`string | K` narrows to `K`; `K | L` to `K` — never to
        //   `K | (K & L)`).
        // * when NO arm is related in EITHER direction, the checker
        //   intersects the WHOLE remaining subject with the instance
        //   type (`string | L` narrows to `(string | L) & K`) — a
        //   primitive arm stays INSIDE the intersection, never dropped;
        //   dropping it published a strict subset of the checker's
        //   type, clean and warm.
        // * an arm ASSIGNABLE to the instance type without being it —
        //   or one the instance type is assignable to — is the
        //   direction structural assignability cannot decide: a genuine
        //   subclass relationship and a same-shape underived
        //   constructor are indistinguishable, and the checker treats
        //   them differently (the derived arm survives or downcasts;
        //   the underived twin routes through the subtype fallback).
        //   The subject stays UNCHANGED behind the typed guard gap — a
        //   superset, ReturnOnly, never a partial narrow that could
        //   drop a real contributor.
        //
        // Each unrelated-arm proof is per-arm and the instance type is
        // a class instance (not a union), so the checker's union-level
        // fallback clauses (candidate-subtype-of-subject,
        // subject-assignable-to-candidate) are provably closed off when
        // every remaining arm is unrelated in both directions: the
        // whole-subject intersection is exact, not an approximation.
        let Some(current) = self.subject_current_node(subject) else {
            return GuardNarrowing::Unchanged;
        };
        let arms = self.enumerated_union_arms_or_self(current);
        let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        let mut remainder: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
        for arm in &arms {
            if self.instanceof_arm_is_unclassifiable(*arm) {
                self.record_degradation(FlowReturnDegradation::FlowGap(
                    crate::semantic_query::FlowGap::GuardNarrowing,
                ));
                return GuardNarrowing::Unchanged;
            }
            if matches!(
                self.dispatch.graph().node_data(*arm).as_deref(),
                Some(SemanticNodeData::Primitive(
                    PrimitiveKind::Null | PrimitiveKind::Undefined | PrimitiveKind::Never,
                ))
            ) {
                continue;
            }
            if *arm == instance {
                survivors.push(*arm);
                remainder.push(*arm);
                continue;
            }
            match (
                self.assignable(*arm, instance),
                self.assignable(instance, *arm),
            ) {
                (Some(false), Some(false)) => remainder.push(*arm),
                _ => {
                    self.record_degradation(FlowReturnDegradation::FlowGap(
                        crate::semantic_query::FlowGap::GuardNarrowing,
                    ));
                    return GuardNarrowing::Unchanged;
                }
            }
        }
        if !survivors.is_empty() {
            if survivors.len() == arms.len() {
                return GuardNarrowing::Unchanged;
            }
            let node = self
                .dispatch
                .intern_normalized_union_or_intersection(&survivors, true);
            return GuardNarrowing::Narrowed(subject.clone(), node);
        }
        if remainder.is_empty() {
            // Only nullish / `never` arms: no runtime value can take the
            // positive edge, which reads `never` while the edge stays
            // alive — a non-subject contributor on it keeps its own type.
            return GuardNarrowing::Narrowed(subject.clone(), self.never_node());
        }
        // No survivor and every remaining arm proved unrelated: the
        // whole remaining subject intersects the instance type. When
        // nothing was stripped the AUTHORED subject node is kept as the
        // intersection's subject arm, preserving its spelling.
        let subject_node = if remainder.len() == arms.len() {
            current
        } else {
            self.dispatch
                .intern_normalized_union_or_intersection(&remainder, true)
        };
        let node = self
            .dispatch
            .intern_normalized_union_or_intersection(&[subject_node, instance], false);
        GuardNarrowing::Narrowed(subject.clone(), node)
    }

    /// Whether one union arm is beyond `instanceof` classification
    /// entirely: a top-shaped arm (`any`, `unknown`, a memberless `{}`
    /// surface), an Opaque carrier, or a node with no data. The checker
    /// narrows those arms to the instance type; without that capability
    /// they stay possible on both edges and degrade the result instead
    /// of killing a branch.
    fn instanceof_arm_is_unclassifiable(&self, arm: SemanticNodeId) -> bool {
        match self.dispatch.graph().node_data(arm).as_deref() {
            Some(SemanticNodeData::Primitive(PrimitiveKind::Any | PrimitiveKind::Unknown))
            | Some(SemanticNodeData::Opaque(_))
            | None => true,
            Some(SemanticNodeData::Object(surface)) => surface.closed().is_empty(),
            _ => false,
        }
    }

    /// `"key" in subject` follows the checker's TWO regimes (measured;
    /// the checker's own `in`-narrowing split):
    ///
    /// * a KNOWN key — some arm proves it present, required or optional —
    ///   filters the arms per edge ([`InArmPresence`]): a REQUIRED member
    ///   proves the arm off the negated edge, a proven-ABSENT key on a
    ///   closed surface proves it off the positive edge, and an OPTIONAL
    ///   member proves EXACT retention on both edges (presence of the key
    ///   is a separate fact from the value's non-`undefined`-ness, which
    ///   the member READ carries). A filter no arm survives collapses the
    ///   subject to `never` on a still-alive edge.
    /// * an UNKNOWN key — NO arm proves it present — never filters: the
    ///   checker's positive edge is the WHOLE subject intersected with
    ///   `Record<key, unknown>`, a carrier the substrate cannot mint, so
    ///   the subject stays unchanged as a typed superset behind the guard
    ///   gap — never a dropped edge; the negated edge keeps the subject
    ///   unchanged exactly.
    ///
    /// An arm whose key set the graph cannot decide — a type parameter,
    /// an index-signature surface, an unresolvable carrier — stays
    /// possible on BOTH edges, leaves the regime undecided, and records
    /// the typed guard gap: the checker narrows such an arm, so deciding
    /// either way would fabricate a dead edge or a clean warm superset.
    fn narrow_in(
        &mut self,
        key: &Arc<str>,
        subject: &crate::flow_slice_content::SliceNarrowSubject,
        negated: bool,
    ) -> GuardNarrowing {
        let Some(current) = self.subject_current_node(subject) else {
            return GuardNarrowing::Unchanged;
        };
        let arms = self.enumerated_union_arms_or_self(current);
        let presences: Vec<InArmPresence> = arms
            .iter()
            .map(|arm| self.arm_in_presence(*arm, key))
            .collect();
        let key_is_known = presences
            .iter()
            .any(|presence| matches!(presence, InArmPresence::Always | InArmPresence::Optional));
        let mut gapped = false;
        let fact = if key_is_known {
            let mut survivors: Vec<SemanticNodeId> = Vec::with_capacity(arms.len());
            for (arm, presence) in arms.iter().zip(presences.iter()) {
                let keep = match presence {
                    InArmPresence::Always => !negated,
                    InArmPresence::Never => negated,
                    InArmPresence::Optional => true,
                    InArmPresence::Unknown => {
                        gapped = true;
                        true
                    }
                };
                if keep {
                    survivors.push(*arm);
                }
            }
            if survivors.is_empty() {
                // Every arm carries the key and the test excludes it: the
                // subject reads `never` on the negated edge and the edge
                // stays alive.
                GuardNarrowing::Narrowed(subject.clone(), self.never_node())
            } else if survivors.len() == arms.len() {
                GuardNarrowing::Unchanged
            } else {
                let node = self
                    .dispatch
                    .intern_normalized_union_or_intersection(&survivors, true);
                GuardNarrowing::Narrowed(subject.clone(), node)
            }
        } else if !negated {
            // Unknown key, positive edge: the unmintable
            // `(subject) & Record<key, unknown>` stays a typed superset.
            gapped = true;
            GuardNarrowing::Unchanged
        } else {
            // Unknown key, negated edge: the checker keeps the subject
            // unchanged — exact when every arm's absence is proved; an
            // undecided arm could flip the regime, so it degrades.
            if presences
                .iter()
                .any(|presence| matches!(presence, InArmPresence::Unknown))
            {
                gapped = true;
            }
            GuardNarrowing::Unchanged
        };
        if gapped {
            self.record_degradation(FlowReturnDegradation::FlowGap(
                crate::semantic_query::FlowGap::GuardNarrowing,
            ));
        }
        fact
    }

    /// One union arm's key-presence verdict for `"key" in subject`,
    /// decided from the arm's OWN closed surface(s): identity carriers
    /// unwrap through the shared instantiate dispatch; a closed object
    /// surface (no index signature) answers directly — a required member
    /// is `Always`, a proven-absent key `Never`, an optional member
    /// `Optional` — and an INTERSECTION (a heritage arm, `extends`
    /// included) folds its constituents by the checker's own rule: the
    /// member is required if ANY constituent declares it required (every
    /// value of the intersection carries it — decisive even beside an
    /// undecidable sibling), optional if every decidable constituent is
    /// optional-or-absent with at least one optional, absent only when
    /// EVERY constituent proves absence. Any other shape — a type
    /// parameter, an open index-signature surface, a primitive, a nested
    /// union, an unresolvable carrier — is `Unknown` (for the whole
    /// intersection unless a required constituent already decided it):
    /// nothing about the runtime key set is proved, so neither edge of
    /// the test may drop the arm. Constituents walk on a seen-guarded
    /// worklist, cycle-safe by revisit discharge.
    fn arm_in_presence(&mut self, arm: SemanticNodeId, key: &str) -> InArmPresence {
        let mut pending = vec![arm];
        let mut seen: Vec<SemanticNodeId> = Vec::new();
        let mut any_required = false;
        let mut any_optional = false;
        let mut any_unknown = false;
        while let Some(node) = pending.pop() {
            if seen.contains(&node) {
                continue;
            }
            seen.push(node);
            let concrete = match self.dispatch.unwrap_identity_carrier_for_relation(node) {
                super::relation::IdentityCarrierUnwrap::Concrete(concrete) => concrete,
                super::relation::IdentityCarrierUnwrap::Unresolvable => {
                    any_unknown = true;
                    continue;
                }
            };
            if concrete != node {
                if seen.contains(&concrete) {
                    continue;
                }
                seen.push(concrete);
            }
            match self.dispatch.graph().node_data(concrete).as_deref() {
                Some(SemanticNodeData::Intersection(arms)) => {
                    pending.extend(arms.iter().copied());
                }
                Some(SemanticNodeData::Object(surface)) => {
                    if surface.closed().has_index_signature()
                        || !surface.index_signatures.is_empty()
                    {
                        any_unknown = true;
                        continue;
                    }
                    match surface.project_string_key(key) {
                        crate::semantic_query::SurfaceKeyProjection::Exact(member) => {
                            if member.optional {
                                any_optional = true;
                            } else {
                                any_required = true;
                            }
                        }
                        crate::semantic_query::SurfaceKeyProjection::AbsentProven => {}
                    }
                }
                _ => {
                    any_unknown = true;
                }
            }
        }
        if any_required {
            InArmPresence::Always
        } else if any_unknown {
            InArmPresence::Unknown
        } else if any_optional {
            InArmPresence::Optional
        } else {
            InArmPresence::Never
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
    /// fact, and whether the narrow-direction obligation it asked was
    /// decided. This is what the guard twin's call evidence is recorded
    /// from: a fact the evaluator could not consume at all (a
    /// frame-shadowed target, an unmodelled subject) is
    /// [`PredicateNarrowConsumption::NotConsumed`] — no evidence; a
    /// consumed fact whose REVERSE-ASSIGNABILITY ask (the narrow-direction
    /// obligation above) answered `None` is
    /// [`PredicateNarrowConsumption::Undecided`] — evidence with that
    /// obligation left unclaimed. The `Comparable` ask is deliberately NOT
    /// folded into this verdict: its undecided case is recorded as the
    /// typed `NominalRelation` gap (and cannot warm) but does not un-consume
    /// the predicate — the evaluator did read and apply the checker's
    /// intersection rule either way.
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
            let arms = self.enumerated_union_arms_or_self(current);
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
            // Disjointness is not decided here. The evaluator owns no
            // relation classifier: it asks the shared authority whether the
            // subject and the predicate target can overlap, and consumes the
            // authority's disjointness PROOF. An undecided verdict is a typed
            // gap, never a guessed direction.
            use super::relation::ComparabilityVerdict;
            let comparable = self.comparable(current, target_node);
            if matches!(comparable, ComparabilityVerdict::Undecided) {
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
            let ComparabilityVerdict::Disjoint(ref proof) = comparable else {
                let intersection = self
                    .dispatch
                    .intern_normalized_union_or_intersection(&[current, target_node], false);
                return (
                    GuardNarrowing::Narrowed(subject.clone(), intersection),
                    consumption,
                );
            };
            // The pair is PROVED disjoint, and the proof carries the
            // CHECKER'S intersection-reduction answer for exactly this
            // pair. A unit-discriminant conflict (disjoint tags, distinct
            // `unique symbol` identities, a conflicting shared REQUIRED
            // member whose values are both unit types) reduces the
            // intersection to `never`; a conflict reachable only through
            // non-unit member values keeps `A & B`, and the checker-kept
            // intersection is the value this narrow must publish. The
            // collapse class is the authority's payload on its own proof —
            // the evaluator decides nothing about which disjoint pairs
            // reduce.
            if !proof.checker_reduces_intersection_to_never() {
                let intersection = self
                    .dispatch
                    .intern_normalized_union_or_intersection(&[current, target_node], false);
                return (
                    GuardNarrowing::Narrowed(subject.clone(), intersection),
                    consumption,
                );
            }
            // The target is PROVED disjoint from the whole subject through a
            // checker collapse criterion — the intersection reduces to
            // `never`, so the subject reads `never` on the positive edge
            // while the edge stays alive: a contributor there that reads a
            // different binding keeps its own type.
            return (
                GuardNarrowing::Narrowed(subject.clone(), self.never_node()),
                consumption,
            );
        }
        let mut undecided = false;
        let fact = self.narrow_arms_by(subject, |this, arm| {
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
        let fact = match fact {
            // The predicate covers the subject entirely: excluding its
            // target empties the subject — `never`, on a still-alive edge.
            ArmFilter::NoSurvivor => GuardNarrowing::Narrowed(subject.clone(), self.never_node()),
            ArmFilter::Narrowed(node) => GuardNarrowing::Narrowed(subject.clone(), node),
            ArmFilter::Unchanged => GuardNarrowing::Unchanged,
        };
        (fact, consumption)
    }

    /// Whether one local carries a WIDENING literal, in the layer that
    /// currently answers reads of `name`. A pure predicate — it folds no
    /// degradation, because asking about widening is not an observation
    /// of the binding's value.
    fn widening_of(&self, name: &str) -> bool {
        matches!(self.membership_of(name), Some(WideningMembership::All))
    }

    /// The full widening MEMBERSHIP of a local binding, if it has one —
    /// the layer resolution mirrors [`Self::widening_of`], which is its
    /// `All`-only projection.
    fn membership_of(&self, name: &str) -> Option<WideningMembership> {
        if self.local_value(FlowBindingLayer::Lexical, name).is_some() {
            return self.local_widening(FlowBindingLayer::Lexical, name);
        }
        if self.local_value(FlowBindingLayer::Function, name).is_some() {
            return self.local_widening(FlowBindingLayer::Function, name);
        }
        None
    }

    /// The widening membership one PLAIN (non-mixed) binding initializer
    /// establishes: an all-fresh literal tree on an unannotated `const`
    /// (`All` — the classic widening-literal binding), a read of a local
    /// that already carries membership (`const w = v` propagates it; a
    /// `let` widens it at the declaration), or a completed fresh-literal
    /// call (`All` when the whole return is the deposit, `Partial` over a
    /// union-carried one). Authored pins, annotated initializers, and
    /// every other spelling establish none.
    fn binding_init_membership(
        &self,
        kind: crate::flow_slice_content::SliceBindingKind,
        init: &crate::flow_slice_content::SliceExpr,
        node: SemanticNodeId,
        freshness: &crate::flow_slice_content::SliceFreshness,
    ) -> Option<WideningMembership> {
        if kind == crate::flow_slice_content::SliceBindingKind::Const && freshness.all_fresh() {
            return Some(WideningMembership::All);
        }
        if let crate::flow_slice_content::SliceExpr::Local {
            name,
            captured: false,
            ..
        } = init
        {
            if let Some(membership) = self.membership_of(name.as_ref()) {
                return Some(membership.clone());
            }
        }
        if let Some(call) = self.fresh_call_return_for(init, node) {
            return Some(if call.values.contains(&node) {
                WideningMembership::All
            } else {
                WideningMembership::Partial(Arc::clone(&call.values))
            });
        }
        None
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
            Some(node) => Ok(Some(self.widen_value_position_read(&member.value, node))),
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

    /// The completed fresh-literal call this expression IS (transparently)
    /// a read of, matched by the authored call-site SPAN — never by the
    /// interned literal value, so a sibling arm's authored pin of the same
    /// value can never borrow a call's freshness.
    fn fresh_call_return_for(
        &self,
        expr: &crate::flow_slice_content::SliceExpr,
        node: SemanticNodeId,
    ) -> Option<&FreshCallReturn> {
        match expr {
            crate::flow_slice_content::SliceExpr::Call(_, site) => {
                let span = site.span();
                self.call_fresh_literal_returns
                    .iter()
                    .rev()
                    .find(|call| call.span == span && call.node == node)
            }
            crate::flow_slice_content::SliceExpr::FrameShadowed { inner, .. } => {
                self.fresh_call_return_for(inner, node)
            }
            _ => None,
        }
    }

    /// Widen `node` at a VALUE (member / mutable-declaration) position
    /// when the read carries widening provenance: a completed
    /// fresh-literal call (the whole value listed → it widens; a
    /// union-carried deposit → exactly the listed constituents widen), or
    /// a widening-membership local read (`All` → a lone literal widens to
    /// its primitive and a union widens every literal arm; `Partial` →
    /// exactly the recorded fresh values). Every other read passes
    /// through unchanged.
    fn widen_value_position_read(
        &self,
        expr: &crate::flow_slice_content::SliceExpr,
        node: SemanticNodeId,
    ) -> SemanticNodeId {
        if let Some(call) = self.fresh_call_return_for(expr, node) {
            if call.values.contains(&node) {
                return widen_fresh_read_node(self.dispatch, node);
            }
            return widen_values_within(self.dispatch, node, &call.values);
        }
        if let crate::flow_slice_content::SliceExpr::Local { name, .. } = expr {
            match self.membership_of(name.as_ref()) {
                Some(WideningMembership::All) => {
                    return widen_fresh_read_node(self.dispatch, node);
                }
                Some(WideningMembership::Partial(values)) => {
                    return widen_values_within(self.dispatch, node, &values);
                }
                None => {}
            }
        }
        node
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
        for statement in region.statements.iter() {
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
                    freshness,
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
                                fresh_values: Vec::new(),
                            }),
                            Ok(None) => {}
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        }
                        self.capture_return_edge();
                        continue;
                    }
                    match argument {
                        Some(expr) => {
                            let bare_literal = matches!(
                                freshness,
                                crate::flow_slice_content::SliceFreshness::Fresh
                            );
                            // A FRESH literal contribution is a bare literal
                            // return argument or a read of a
                            // widening-literal `const`. The join decides
                            // whether it widens: tsc widens only a lone
                            // contributor (`return 1` is `number`, but
                            // `if (c) return 1; return 0` is `0 | 1`).
                            let mut fresh_literal =
                                bare_literal || self.reads_widening_literal_local(expr);
                            // A `return f(…)` whose callee pops as a
                            // PROVISIONAL member of this component is
                            // fresh-NEUTRAL: its value is re-derived by the
                            // equation fixed point, so the component's own
                            // freshness (not this arm) decides. Treating it
                            // as non-fresh would make widening depend on
                            // whether the callee was already in flight —
                            // i.e. on demand ORDER.
                            let holds_before = self.holds.len();
                            // A TERNARY argument evaluates PER ARM (the
                            // same guard-scoped walk `eval_expr` performs
                            // over the union), so each arm's carried
                            // freshness — a bare fresh literal, a
                            // membership read, a call's kept fresh values
                            // — enters the join instead of vanishing into
                            // the joined node.
                            let evaluated = match (expr, freshness) {
                                (
                                    crate::flow_slice_content::SliceExpr::Union { .. },
                                    crate::flow_slice_content::SliceFreshness::PerArm(_),
                                ) => {
                                    let mut parts: Vec<EvolvingPart> = Vec::new();
                                    self.collect_evolving_parts(expr, freshness, &mut parts);
                                    let nodes: Vec<SemanticNodeId> =
                                        parts.iter().map(|part| part.node).collect();
                                    let node = self
                                        .dispatch
                                        .intern_normalized_union_or_intersection(&nodes, true);
                                    // Pinned wins per literal VALUE: a
                                    // sibling arm's authored pin of the
                                    // same literal cancels the freshness.
                                    let pinned = self.pinned_literal_blockers(&parts);
                                    let mut fresh_values = Vec::new();
                                    for part in &parts {
                                        if part.fresh {
                                            fresh_values
                                                .extend(self.top_level_literal_nodes(part.node));
                                        }
                                        fresh_values.extend_from_slice(&part.fresh_values);
                                    }
                                    let fresh_values =
                                        self.uncancelled_fresh_values(&fresh_values, &pinned);
                                    Some((node, fresh_values))
                                }
                                _ => {
                                    let outcome = self.eval_expr(expr);
                                    self.settle(outcome).map(|node| {
                                        let fresh_values =
                                            self.position_fresh_values(expr, node, bare_literal);
                                        (node, fresh_values)
                                    })
                                }
                            };
                            if let Some((node, fresh_values)) = evaluated {
                                fresh_literal |= self.holds.len() > holds_before;
                                // A COMPLETED call that closed on a
                                // WHOLE-return fresh literal feeds the
                                // same join — matched by its authored
                                // call-site span, never by the interned
                                // literal value, so a sibling arm's
                                // authored pin of the same value is never
                                // fresh. A union-carried fresh deposit
                                // stays pinned at the return position (the
                                // checker widens it only at value
                                // positions).
                                fresh_literal |= self
                                    .fresh_call_return_for(expr, node)
                                    .is_some_and(|call| call.values.contains(&node));
                                contributors.push(FlowContribution {
                                    node,
                                    fresh_literal,
                                    inference_only: self.inference_only_path,
                                    fresh_values,
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
                    let entry_products = self.products.clone();
                    let narrow_mark = self.narrowing_snapshot();
                    let shadow_base = self.scope_shadows.len();
                    let break_base = self.break_exits.len();
                    let return_base = self.return_edges.len();
                    let throw_base = self.throw_points.len();
                    self.conditional_arm_nesting += 1;
                    self.apply_guard_scoped(guard, true);
                    let (consequent_result, consequent_falls) = self.eval_region(consequent);
                    self.restore_narrowings(narrow_mark.clone());
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
                    let consequent_products = consequent_state.products;
                    self.restore_arm_entry(&entry_products);
                    let consequent_contributors = match consequent_result {
                        Ok(contributors) => contributors,
                        Err(failure) => {
                            self.conditional_arm_nesting -= 1;
                            return (Err(failure), region.can_fall_through);
                        }
                    };
                    contributors.extend(consequent_contributors);
                    let alternate_layers = if let Some(alternate) = alternate {
                        let shadow_base = self.scope_shadows.len();
                        let break_base = self.break_exits.len();
                        let return_base = self.return_edges.len();
                        let throw_base = self.throw_points.len();
                        self.apply_guard_scoped(guard, false);
                        let (alternate_result, alternate_falls) = self.eval_region(alternate);
                        self.restore_narrowings(narrow_mark.clone());
                        let shadows = self.split_scope_shadows_close_exits(
                            shadow_base,
                            break_base,
                            return_base,
                            throw_base,
                        );
                        let mut alternate_state = self.layer_state();
                        Self::close_lexical_scope(&mut alternate_state, &shadows);
                        let alternate_products = alternate_state.products;
                        self.restore_arm_entry(&entry_products);
                        let alternate_contributors = match alternate_result {
                            Ok(contributors) => contributors,
                            Err(failure) => {
                                self.conditional_arm_nesting -= 1;
                                return (Err(failure), region.can_fall_through);
                            }
                        };
                        contributors.extend(alternate_contributors);
                        Some((alternate_products, alternate_falls))
                    } else {
                        None
                    };
                    self.conditional_arm_nesting -= 1;
                    self.restore_arm_entry(&entry_products);
                    let (alternate_products, alternate_falls) = match &alternate_layers {
                        Some((products, falls)) => (Some(products), *falls),
                        // No `else`: the implicit alternate always
                        // reaches past the `if` — narrowing
                        // impossibility collapses subject READS to
                        // `never`, it never removes the edge.
                        None => (None, true),
                    };
                    self.join_arm_writes(
                        &consequent_products,
                        consequent_falls,
                        alternate_products,
                        alternate_falls,
                        &entry_products,
                    );
                    // The surviving edge's facts. Exactly one arm
                    // terminating means every path past the `if` took the
                    // OTHER reading of the test — apply its facts to the
                    // rest of the region, exactly where an arm-scoped
                    // truncation does not erase them. (Both arms reaching
                    // establishes nothing; both terminating makes the rest
                    // of the region unreachable.) An edge no arm survives
                    // stays alive with its subject narrowed to `never` —
                    // the application never kills the path.
                    if !consequent_falls && alternate_falls {
                        self.apply_guard_scoped(guard, false);
                    } else if consequent_falls && !alternate_falls {
                        self.apply_guard_scoped(guard, true);
                    }
                    path_alive = consequent_falls || alternate_falls;
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
                    // A DECLINED remainder probe (an unlowerable test, a
                    // projection miss, an undecided relation) leaves the
                    // no-matching-case path live over arms the checker may
                    // prove covered — a superset, so it degrades: the
                    // liveness verdict is then unproven, never clean.
                    let covered = !has_default
                        && discriminant.as_ref().is_some_and(|subject| {
                            match self.switch_discriminant_remainder(subject, &tests) {
                                Some((remainder, _)) => remainder.is_empty(),
                                None => {
                                    self.record_degradation(FlowReturnDegradation::FlowGap(
                                        crate::semantic_query::FlowGap::GuardNarrowing,
                                    ));
                                    false
                                }
                            }
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
                                // A test no discriminant arm matches bakes
                                // the subject's `never` narrow instead of
                                // killing the dispatch edge: the checker
                                // keeps the clause's contributors typed
                                // through the `never` subject (measured:
                                // `switch (x) { case "b": return 1 }` over
                                // `x: "a"` still contributes `1`).
                                crate::flow_slice_content::SliceSwitchTest::Literal(test) => {
                                    match self.narrow_eq_literal(subject, test, false) {
                                        GuardNarrowing::Narrowed(fact_subject, node) => {
                                            self.bake_narrow_into_state(
                                                &mut dispatch,
                                                &fact_subject,
                                                node,
                                            );
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
                                            // The remainder's arms are the
                                            // PARENT reference's, so the
                                            // fact lands there — the root
                                            // for a shallow discriminant,
                                            // the enclosing reference for
                                            // a nested one.
                                            let parent_subject =
                                                crate::flow_slice_content::SliceNarrowSubject {
                                                    root: subject.root.clone(),
                                                    path: Arc::from(
                                                        subject.path[..subject
                                                            .path
                                                            .len()
                                                            .saturating_sub(1)]
                                                            .to_vec()
                                                            .into_boxed_slice(),
                                                    ),
                                                };
                                            self.bake_narrow_into_state(
                                                &mut dispatch,
                                                &parent_subject,
                                                node,
                                            );
                                        }
                                    } else {
                                        // A DECLINED probe leaves this
                                        // edge carrying the WHOLE
                                        // discriminant where the checker
                                        // subtracts the matched cases — a
                                        // superset, so it degrades rather
                                        // than publishing clean.
                                        self.record_degradation(FlowReturnDegradation::FlowGap(
                                            crate::semantic_query::FlowGap::GuardNarrowing,
                                        ));
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
                                let mut start = self.join_states(&dispatch, end);
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
                                joined = self.join_states(&joined, state);
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
                    let try_narrowings = narrowing_facts_of(&try_end.products);
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
                            catch_start = self.join_states(&catch_start, state);
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
                                joined = self.join_states(&joined, state);
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
                            let mut finally_start = self.join_states(&pre_finally, &entry);
                            replace_narrowings(&entry.products, &mut finally_start.products);
                            let clause_throws = self.throw_points[throw_base..].to_vec();
                            for state in block_throws.iter().chain(clause_throws.iter()) {
                                finally_start = self.join_states(&finally_start, state);
                            }
                            let pending_exits: Vec<FlowLayerState> = self.break_exits[break_base..]
                                .iter()
                                .map(|exit| exit.state.clone())
                                .collect();
                            for state in &pending_exits {
                                finally_start = self.join_states(&finally_start, state);
                            }
                            let pending_returns = self.return_edges[return_base..].to_vec();
                            for state in &pending_returns {
                                finally_start = self.join_states(&finally_start, state);
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
                            replace_narrowings(&entry.products, &mut post.products);
                            for subject in &finally_written {
                                if let Some(reaching) = finally_end.products.reaching_type(subject)
                                {
                                    post.products.set_reaching_type(subject, reaching.clone());
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
                                let mut killed: Vec<FlowProductSubject> = Vec::new();
                                for subject in &finally_written {
                                    if let Some(root) = self.narrow_root_of(subject) {
                                        if !killed.contains(&root) {
                                            killed.push(root);
                                        }
                                    }
                                }
                                let entry_facts = narrowing_facts_of(&entry.products);
                                let restored: Vec<FlowNarrowingFact> = try_narrowings
                                    .iter()
                                    .filter(|fact| {
                                        !entry_facts.contains(fact)
                                            && !killed.contains(&fact.subject)
                                    })
                                    .cloned()
                                    .collect();
                                for fact in restored {
                                    push_narrowing_into(
                                        &mut self.products,
                                        &fact.subject,
                                        &fact.path,
                                        fact.narrowed_to,
                                    );
                                }
                                for subject in &try_written {
                                    if entry.products.assignment(subject).single_path() {
                                        continue;
                                    }
                                    let cleared =
                                        self.products.assignment(subject).with_single_path(false);
                                    self.products.set_assignment(subject, cleared);
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
                            replace_narrowings(&entry.products, &mut pre_finally.products);
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
                                joined = self.join_states(&joined, state);
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
                    freshness,
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
                            && (self
                                .local_value(FlowBindingLayer::Function, name.as_ref())
                                .is_some()
                                || self.param_names.iter().enumerate().any(|(ordinal, param)| {
                                    param.name.as_deref() == Some(name.as_ref())
                                        && (self.param_write(ordinal as u32).is_some()
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
                                self.bind_local(name, *kind, marker, None, false);
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
                                self.bind_local(name, *kind, declared_node, None, false);
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
                                self.bind_local(name, *kind, node, None, false);
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
                        // A MIXED-freshness conditional `const`
                        // initializer (a bare fresh literal arm beside an
                        // authored pin, a call, a reference): evaluate
                        // per-arm so the binding records EXACTLY which
                        // values widen at a widening read — pinned-wins
                        // collapse of equal constituents, then Partial
                        // membership over the surviving fresh values. An
                        // all-fresh or all-pinned tree keeps the plain
                        // path below.
                        if *kind == crate::flow_slice_content::SliceBindingKind::Const
                            && freshness.is_mixed()
                            && matches!(init, crate::flow_slice_content::SliceExpr::Union { .. })
                        {
                            let mut parts: Vec<EvolvingPart> = Vec::new();
                            self.collect_evolving_parts(init, freshness, &mut parts);
                            let pinned = self.pinned_literal_blockers(&parts);
                            let mut merged: Vec<EvolvingPart> = Vec::new();
                            for part in parts {
                                let data = self.dispatch.graph().node_data(part.node);
                                match merged.iter_mut().find(|existing| {
                                    self.dispatch.graph().node_data(existing.node) == data
                                }) {
                                    Some(existing) => {
                                        existing.fresh &= part.fresh;
                                        existing.fresh_values.extend(part.fresh_values);
                                    }
                                    None => merged.push(part),
                                }
                            }
                            let nodes: Vec<SemanticNodeId> =
                                merged.iter().map(|part| part.node).collect();
                            let value = self
                                .dispatch
                                .intern_normalized_union_or_intersection(&nodes, true);
                            let all_fresh = merged.iter().all(|part| part.fresh);
                            let mut fresh_values: Vec<SemanticNodeId> = Vec::new();
                            for part in &merged {
                                if part.fresh {
                                    fresh_values.push(part.node);
                                } else {
                                    // A pinned-shaped arm may still CARRY
                                    // fresh values (a call's kept deposit
                                    // or authored fresh arm) — those enter
                                    // the membership so a widening read of
                                    // the binding widens them.
                                    fresh_values.extend_from_slice(&part.fresh_values);
                                }
                            }
                            let fresh_values =
                                self.uncancelled_fresh_values(&fresh_values, &pinned);
                            let membership = if fresh_values.is_empty() {
                                None
                            } else if all_fresh {
                                Some(WideningMembership::All)
                            } else {
                                Some(WideningMembership::Partial(Arc::from(
                                    fresh_values.into_boxed_slice(),
                                )))
                            };
                            self.bind_local(name, *kind, value, membership, false);
                            continue;
                        }
                        match self.eval_expr(init) {
                            Positional::Value(node) => {
                                let membership =
                                    self.binding_init_membership(*kind, init, node, freshness);
                                match kind {
                                    crate::flow_slice_content::SliceBindingKind::Const => {
                                        self.bind_local(name, *kind, node, membership, false);
                                    }
                                    // A MUTABLE declaration widens its
                                    // fresh provenance AT the declaration
                                    // (`let a = idInf(1)` is `number`),
                                    // exactly as a bare literal
                                    // initializer already lowered widened.
                                    crate::flow_slice_content::SliceBindingKind::Let
                                    | crate::flow_slice_content::SliceBindingKind::Var => {
                                        let node = match &membership {
                                            Some(WideningMembership::All) => {
                                                widen_fresh_read_node(self.dispatch, node)
                                            }
                                            Some(WideningMembership::Partial(values)) => {
                                                widen_values_within(self.dispatch, node, values)
                                            }
                                            None => node,
                                        };
                                        self.bind_local(name, *kind, node, None, false);
                                    }
                                }
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
                                self.bind_local(name, *kind, marker, None, true);
                            }
                        }
                    }
                }
                crate::flow_slice_content::SliceStatement::Assignment {
                    target,
                    value,
                    freshness,
                    ..
                } => {
                    // THE applied write: a whole-binding `=` at statement
                    // position retypes the binding IN SOURCE ORDER, so the
                    // reads after it see the written value and the typed
                    // unapplied-write degradation never seeds. An
                    // unmodelled right-hand side binds the typed marker
                    // with the failed-initializer membership, exactly like
                    // an unmodelled declarator initializer. The declared
                    // authority picks the evaluation: a declared UNION
                    // needs the pre-widening assignment view (constituent
                    // selection), a declared non-union discards the value
                    // (the declared type wins), and an EVOLVING target
                    // takes the freshness-directed widening.
                    let holds_before = self.holds.len();
                    let declared = self.target_declared_node(target);
                    let outcome = match declared {
                        Some(node) if self.dispatch.union_arms_of(node).is_some() => {
                            self.eval_assignment_expr(value)
                        }
                        Some(_) => self.eval_expr(value),
                        None => self.eval_evolving_rhs(value, freshness),
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
                            self.push_narrowing(&subject, node);
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
        // The nested frame inherits the enclosing frame's binding table and
        // product state by NAME, minus the facts that do not cross a
        // closure boundary: the narrowing overlay (a guard narrow lives and
        // dies with the arm that established it, and the checker itself
        // does not honour a narrowing of a mutable binding across a closure
        // boundary), the enclosing frame's applied parameter writes (the
        // nested signature has its own parameter slots), and the lexical
        // single-path facts (a clause boundary of the enclosing frame is
        // not one of the nested body).
        let mut captured_bindings = self.bindings.clone();
        let mut captured_products = self.products.clone();
        for subject in captured_products.subjects_in(super::flow_solve::FlowDomain::Narrowing) {
            captured_products.remove(super::flow_solve::FlowDomain::Narrowing, &subject);
        }
        for domain in super::flow_products::FLOW_FRAME_DOMAINS {
            for subject in captured_products.subjects_in(domain) {
                match &subject {
                    FlowProductSubject::FrameParam(_) => {
                        captured_products.remove(domain, &subject);
                    }
                    FlowProductSubject::FrameBinding(slot)
                        if domain == super::flow_solve::FlowDomain::DefiniteAssignment
                            && captured_bindings.layer(*slot)
                                == Some(FlowBindingLayer::Lexical) =>
                    {
                        let cleared = captured_products
                            .assignment(&subject)
                            .with_single_path(false);
                        captured_products.set_assignment(&subject, cleared);
                    }
                    _ => {}
                }
            }
        }
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
            let name = authority.name.as_ref();
            match authority.source {
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(
                    crate::flow_slice_content::SliceBindingKind::Var,
                ) => {
                    let subject = captured_bindings.subject(FlowBindingLayer::Function, name);
                    captured_products.set_declared_type(&subject, Some(node));
                    captured_products.set_reaching_type(&subject, ReachingTypeProduct::of(node));
                }
                crate::flow_slice_content::SliceCaptureAuthoritySource::Local(
                    crate::flow_slice_content::SliceBindingKind::Let,
                ) => {
                    let subject = captured_bindings.subject(FlowBindingLayer::Lexical, name);
                    captured_products.set_declared_type(&subject, Some(node));
                    if captured_products.reaching(&subject).is_none() {
                        captured_products
                            .set_reaching_type(&subject, ReachingTypeProduct::of(node));
                    }
                }
                crate::flow_slice_content::SliceCaptureAuthoritySource::Parameter { .. } => {
                    let subject = captured_bindings.subject(FlowBindingLayer::Lexical, name);
                    captured_products.set_declared_type(&subject, Some(node));
                    captured_products.set_reaching_type(&subject, ReachingTypeProduct::of(node));
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
            let subject = captured_bindings.subject(FlowBindingLayer::Function, name);
            if captured_products.reaching(&subject).is_none() {
                captured_products.set_reaching_type(&subject, ReachingTypeProduct::of(*node));
            }
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
                bindings: captured_bindings,
                products: captured_products,
                product_budget: self.product_budget,
                product_budget_exceeded: None,
                bare_return_seen: false,
                implicit_undefined_seen: false,
                // A nested function value always evaluates its WHOLE
                // return (its signature's return type) — the member
                // filter is a top-level demand axis.
                member_filter: None,
                holds: Vec::new(),
                degradation: None,
                pending_statement_gap: None,
                conditional_arm_nesting: 0,
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
                if self
                    .products
                    .assignment(&Self::param_subject(*ordinal))
                    .single_path()
                {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
                    );
                }
                if let Some(node) = self.param_write(*ordinal) {
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
                    let mark = self.narrowing_snapshot();
                    self.apply_guard_scoped(guard, index == 0);
                    let outcome = self.eval_expr(arm);
                    self.restore_narrowings(mark);
                    nodes.push(self.settle_composite_part(outcome, holds_before));
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
                Some(
                    composite @ (SemanticNodeData::Union(_) | SemanticNodeData::Intersection(_)),
                ) => {
                    let arms = composite.composite_members().expect("composite arm");
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
        let bare_name = indexed_bare_name(expression);
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
                    .param_write(ordinal)
                    .or_else(|| self.params.get(ordinal as usize).copied())
                {
                    return Some(node);
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
            // A read of a WIDENING-literal `const` is a FRESH literal
            // source exactly as a bare literal argument is (the checker
            // widens `wrap(a)` for `const a = "x"` identically to
            // `wrap("x")`); the indexed lowering classifies every
            // reference as pinned because only this frame knows the
            // binding's widening membership.
            let reads_widening_local =
                indexed_bare_name(&argument.expression).is_some_and(|name| self.widening_of(name));
            args.push(crate::semantic_query::CallArgKey::Eager {
                ty,
                spread: argument.spread,
                context_sensitive: argument.context_sensitive,
                literal_mode: match argument.literal_mode {
                    verter_type_expr::IndexedValueLiteralMode::Widened => {
                        crate::semantic_query::ArgumentLiteralMode::Widened
                    }
                    verter_type_expr::IndexedValueLiteralMode::Literal => {
                        if reads_widening_local {
                            crate::semantic_query::ArgumentLiteralMode::Widened
                        } else {
                            crate::semantic_query::ArgumentLiteralMode::Literal
                        }
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
                    fresh_literal_returns,
                    ..
                } = &result
                {
                    if !fresh_literal_returns.is_empty() {
                        self.call_fresh_literal_returns.push(FreshCallReturn {
                            span: site.span(),
                            node: *return_type,
                            values: Arc::clone(fresh_literal_returns),
                        });
                    }
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
                                let value = CallValue::of_served_return(
                                    self.dispatch,
                                    &callee_clause,
                                    result.return_type(),
                                    ReturnOrigin::ClauseScoped,
                                );
                                // The callee's sealed fresh literal arms
                                // stay fresh across this direct-call rail
                                // exactly as across the executor route:
                                // record the call's fresh provenance so a
                                // value position widens them, the return
                                // join keeps them, and a `const` binding
                                // records its widening membership.
                                if !result.fresh_literal_arms().is_empty() {
                                    self.call_fresh_literal_returns.push(FreshCallReturn {
                                        span: site.span(),
                                        node: value.node(),
                                        values: Arc::clone(result.fresh_literal_arms()),
                                    });
                                }
                                Positional::Value(value)
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

#[cfg(test)]
mod plan_refusal_class_tests {
    use super::*;

    /// The three preparation-refusal causes map onto three
    /// consumer-distinct partial classes, and the split is load-bearing:
    /// the two REQUEST/STATE causes must fault the Vue macro projection
    /// lanes (which refuse to contain `BUDGET_EXCEEDED` and
    /// `UNSTABLE_STATE`), while only the unplannable-demand cause — and
    /// the cause-less member-batch withholding — may keep the contained
    /// degraded-success class. The torn-view arm is a RACE (the
    /// prepare-time read missing what the evaluation then finds) with no
    /// deterministic public-boundary fixture, so its routing is pinned
    /// here; the budget arm additionally has the public-boundary proof
    /// (`obligation_budget_refusal_takes_the_faulting_request_class`).
    /// Mutation: merging either faulting arm back onto
    /// `FLOW_RETURN_UNVERIFIED` fails its assertion.
    #[test]
    fn plan_refusal_classes_split_by_cause() {
        assert_eq!(
            plan_refusal_reason_class(Some(FlowPlanRefusal::Budget)),
            PartialReasonSet::BUDGET_EXCEEDED,
        );
        assert_eq!(
            plan_refusal_reason_class(Some(FlowPlanRefusal::TornView)),
            PartialReasonSet::UNSTABLE_STATE,
        );
        assert_eq!(
            plan_refusal_reason_class(Some(FlowPlanRefusal::Unplannable)),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
        assert_eq!(
            plan_refusal_reason_class(None),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
    }

    /// The finalizer-partial classifier is the twin of the plan-refusal
    /// split, applied to the verdict channel: a torn/stale/foreign or
    /// incoherent proof state faults as UNSTABLE_STATE, undischarged or
    /// non-converged work as BUDGET_EXCEEDED, a cancellation as
    /// CANCELLED — while only the evidence-shaped causes (a typed gap,
    /// an unprovable contract, the degraded-value echo) keep the
    /// contained classes. Mutation: classifying any faulting reason from
    /// the value's degradation alone (the dropped-cause default) fails
    /// the non-degraded rows below.
    #[test]
    fn finalizer_partial_classes_split_by_reason() {
        use super::super::flow_solve::{FlowFailure, FlowFailureClass, FlowPartialReason};

        let faulting = [
            (
                FlowPartialReason::StaleBasis,
                PartialReasonSet::UNSTABLE_STATE,
            ),
            (
                FlowPartialReason::NoDemandInstalled,
                PartialReasonSet::UNSTABLE_STATE,
            ),
            (
                FlowPartialReason::ForeignProvenance,
                PartialReasonSet::UNSTABLE_STATE,
            ),
            (
                FlowPartialReason::ObligationSetMismatch,
                PartialReasonSet::UNSTABLE_STATE,
            ),
            (
                FlowPartialReason::NonConverged,
                PartialReasonSet::BUDGET_EXCEEDED,
            ),
            (
                FlowPartialReason::Failed(FlowFailure {
                    class: FlowFailureClass::BudgetExhausted,
                }),
                PartialReasonSet::BUDGET_EXCEEDED,
            ),
            (
                FlowPartialReason::Failed(FlowFailure {
                    class: FlowFailureClass::Cancelled,
                }),
                PartialReasonSet::CANCELLED,
            ),
            (
                FlowPartialReason::Failed(FlowFailure {
                    class: FlowFailureClass::Internal,
                }),
                PartialReasonSet::UNSTABLE_STATE,
            ),
        ];
        for (reason, expected) in faulting {
            // A non-degraded value must not launder the faulting cause
            // into the contained class.
            assert_eq!(flow_partial_reason_class(&reason, None), expected);
            // A degraded value ADDS its own class; the faulting cause
            // still reaches the consumer.
            let with_degradation =
                flow_partial_reason_class(&reason, Some(FlowReturnDegradation::UnmodeledPosition));
            assert_eq!(
                with_degradation,
                expected.union(PartialReasonSet::FLOW_RETURN_UNINFERRED)
            );
        }

        // The contained causes: evidence-shaped, not faulting.
        assert_eq!(
            flow_partial_reason_class(
                &FlowPartialReason::Gap(crate::semantic_query::FlowGap::GuardNarrowing),
                None
            ),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
        assert_eq!(
            flow_partial_reason_class(&FlowPartialReason::OperationNotProvable, None),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
        // The bare pending-obligation echo is the close's own withholding
        // signature, not a member fault: a genuinely budget-refused
        // obligation reaches the ledger as a typed `Failed` budget
        // record (asserted BUDGET_EXCEEDED above) — the two must not
        // collapse onto one class in either direction. Over a DEGRADED
        // value the echo adds nothing: widening a positional marker onto
        // the frame-wide class would erase its faithful siblings.
        assert_eq!(
            flow_partial_reason_class(&FlowPartialReason::IncompleteObligations, None),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
        assert_eq!(
            flow_partial_reason_class(
                &FlowPartialReason::IncompleteObligations,
                Some(FlowReturnDegradation::UnmodeledPosition)
            ),
            PartialReasonSet::FLOW_RETURN_UNINFERRED,
        );
        // The degraded-value echo classifies from the degradation itself
        // — a positional marker stays the positional class, never
        // widened by the echo.
        assert_eq!(
            flow_partial_reason_class(
                &FlowPartialReason::DegradedValue,
                Some(FlowReturnDegradation::UnmodeledPosition)
            ),
            PartialReasonSet::FLOW_RETURN_UNINFERRED,
        );
        assert_eq!(
            flow_partial_reason_class(&FlowPartialReason::DegradedValue, None),
            PartialReasonSet::FLOW_RETURN_UNVERIFIED,
        );
    }
}
