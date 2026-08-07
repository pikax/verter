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
//! Only a COMPLETE evaluation admits into the family memo; every degraded
//! shape is a typed `FlowReturnFailure` through `ReturnOnly` (never
//! admitted, never `never`).

use std::sync::Arc;

use super::dispatch_txn::{
    CompletedFlowReturnMember, FlowReturnPendingOutcome, FlowReturnPendingState,
    ObligationFrameDomain, ObligationIdentity, PendingObligation, PendingObligationDomain,
};
use super::flow_return_callee::{
    CallValue, CalleeClause, CalleeClauseLookup, HeldCallee, ReturnOrigin, SignatureCall,
};
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::resolver_core::{FactVersionRef, ProgramAnalysisFactRef};
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnFailure, FlowReturnKey, FlowReturnResult, FlowReturnStep,
    FlowReturnUnsupported, PartialReasonSet, PrimitiveKind, QueryError, QueryResult,
    SemanticNodeData, SemanticNodeId, SemanticQueryKey, SemanticQueryValue,
};

/// The consumer outcome of one sealed function-return demand
/// ([`ProjectSemanticDispatch::execute_function_return_source`]).
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

/// What one [`FunctionReturnNode`] does to the ENCLOSING composition's
/// admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ConsumerFold {
    /// A complete answer: nothing folds.
    Clean,
    /// The answer is NOT complete — either a usable value with an opaque
    /// interior, or no value at all. BOTH rails fold: the request-partial
    /// sticky (gating component-meta / shape / materialize warm) and the
    /// build-local taint.
    ///
    /// The two shapes fold ALIKE, and that is the decision. Suppressing
    /// only the build-local taint for the no-value shape left the request
    /// unmarked, so `get_component_meta` published the surface as
    /// COMPLETE and WARM: six measured programs answered `props: []`,
    /// `synthesis_should_suppress: false`, with a warm cache hit on
    /// replay, where the checker has an answer. "The consumer decides
    /// what a contained failure means for its own surface" is true of the
    /// VALUE — which is why the value still returns — but it is not true
    /// of the ADMISSION rail: a surface built around a failure the
    /// consumer could not see is not a complete result, and publishing it
    /// warm is the wrong-and-warm class this substrate exists to close.
    /// Localisation is the POSITIONAL rule's job, and it does that job
    /// one level down, inside the evaluator, where the marker keeps every
    /// modelled sibling; by the time a no-value outcome reaches HERE the
    /// evaluation has already declined to localise it.
    ///
    /// The carried reason set is what the consumers disagree over, and it
    /// is derived per-OUTCOME rather than per-arm: see
    /// [`degradation_reason_class`] for a degraded success (whose surface
    /// is either faithful or merely unverified) and
    /// [`NO_VALUE_REASON_CLASS`] for a no-value outcome (which has no
    /// surface at all, so only a consumer that splices the AUTHORED
    /// declaration can contain it).
    Partial(PartialReasonSet),
}

/// The partial class EVERY typed NO-VALUE [`FlowReturnFailure`] carries:
/// [`PartialReasonSet::FLOW_RETURN_UNVERIFIED`], the TSC-lane-contained
/// class.
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
/// an empty props object for a component that declares props.
///
/// A per-variant match here would be a constant-returning stub. The
/// distinctions that DO matter are recorded elsewhere and survive: the
/// class-member inference rail records its own precise
/// `BUDGET_EXCEEDED` into the file-level aggregate (pinned by
/// `tsc_class_inference_budget_is_exact_partial_and_non_cacheable`), and
/// the typed `FlowReturnFailure` itself is what the flow-return consumers
/// branch on.
const NO_VALUE_REASON_CLASS: PartialReasonSet = PartialReasonSet::FLOW_RETURN_UNVERIFIED;

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

impl FunctionReturnNode {
    /// How this outcome folds into the enclosing composition's admission.
    ///
    /// The exhaustive match is the point: a new arm does not compile until
    /// it has said what it does to the enclosing result, which is what one
    /// classification at the ONE sealed consumer entry buys over a
    /// condition re-spelled at each call site.
    ///
    /// Only two of the five arms are LIVE at that entry
    /// ([`ProjectSemanticDispatch::execute_function_return_source`] calls
    /// this inside its `Flow` arm alone, and its `Declared` / `Absent`
    /// arms return before reaching it), so [`Self::Declared`],
    /// [`Self::Absent`] and [`Self::DeclaredMiss`] are classified here for
    /// the TYPE, not for a live call. They are still stated, because the
    /// classification is a property of the outcome rather than of the one
    /// site that currently asks.
    pub(crate) fn consumer_fold(&self) -> ConsumerFold {
        match self {
            // A declared locator that raised, and a body-derived result
            // with NO degradation, are the two complete answers. An ABSENT
            // return carrier is a FACT about the signature (a bodiless
            // overload, a synthesized signature), not a failure to compute
            // one.
            Self::Declared(_) | Self::Absent => ConsumerFold::Clean,
            Self::Flow(result) => match result.degradation() {
                None => ConsumerFold::Clean,
                Some(degradation) => ConsumerFold::Partial(degradation_reason_class(degradation)),
            },
            // A declared locator that could not be raised is a RESOLUTION
            // miss rather than a body-derived inference, but it lands on
            // the same side of the consumer axis: the TSC splice is
            // unaffected, a value-derived surface is not.
            Self::DeclaredMiss => ConsumerFold::Partial(NO_VALUE_REASON_CLASS),
            Self::NoValue(_) => ConsumerFold::Partial(NO_VALUE_REASON_CLASS),
        }
    }
}

/// The popped root's close outcome.
enum FlowRootClose {
    /// Complete evaluation: the result (possibly a DEGRADED success —
    /// the caller still receives the value; only admission is refused),
    /// the component's UNIONED self-roots (every drained member's file
    /// roots across both domains), and the materialised point set the
    /// root's compute actually produced (§3.4).
    Complete(
        FlowReturnResult,
        Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        crate::semantic_query::demand::MaterializedSet,
    ),
    /// Typed NO-VALUE failure — `ReturnOnly`, never admitted.
    NoValue(FlowReturnFailure),
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

    /// The demand-parameterised half of [`Self::flow_return_key_for`] —
    /// still the ONE construction point (the whole-return wrapper
    /// delegates here; the audited host seam passes the caller's
    /// demand). The input axis stays the canonical EMPTY point: no
    /// production contextual-input producer exists, and a non-empty
    /// point is a distinct cache/re-entry identity a later block mints.
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
                match self.raise_body_slot(locator.slot(), scope_canonical_id) {
                    Some(hot) => FunctionReturnNode::Declared(hot),
                    None => FunctionReturnNode::DeclaredMiss,
                }
            }
            verter_type_expr::facts::FunctionReturnSource::Flow(identity) => {
                let node = match self.execute_flow_return(self.flow_return_key_for(identity)) {
                    // A DEGRADED SUCCESS stays usable — the consumer keeps
                    // the value (interning a miss would be the opposite
                    // collapse).
                    FlowReturnStep::Complete(result) => FunctionReturnNode::Flow(result),
                    FlowReturnStep::NoValue(failure) => FunctionReturnNode::NoValue(failure),
                    // A hold surfacing at a consumer is a demand reentering
                    // its own in-flight component: undecided here, ReturnOnly.
                    FlowReturnStep::Hold(_) => {
                        FunctionReturnNode::NoValue(FlowReturnFailure::Unresolved)
                    }
                };
                // THE cache-read fold — ONE call site, EVERY non-clean arm,
                // no `degradation.is_some()` condition at THAT entry. (The
                // bit is still read for questions that are not the
                // consumer's fold: this module's own admission gate,
                // `scc_publish`'s component-wide publication gate, and the
                // TSC projection's inferred-class-member row.)
                //
                // `build_flow_return` sets `cache_suppress` on the
                // `FlowReturn` query's OWN output. That says nothing about
                // the ENCLOSING composition, and the four consumers each
                // turn a no-value failure into `Opaque(Miss)` / `miss_node`
                // — two of them with no taint at all. So a failure was
                // laundered into a warm-admitted enclosing result exactly
                // as a degraded success was before the success arm was
                // folded; the asymmetry was never a decision.
                //
                // Which RAILS fold is a per-arm FACT, not a per-call-site
                // condition, so it is decided once by the exhaustive
                // [`FunctionReturnNode::consumer_fold`] classification.
                match node.consumer_fold() {
                    ConsumerFold::Clean => {}
                    ConsumerFold::Partial(reasons) => self.fold_flow_return_consumer_rails(reasons),
                }
                node
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
        let prepared =
            self.ctx
                .prepared_value_decl(scope_canonical.as_ref(), scope_owner, &value_name)?;
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
        let demand = crate::semantic_query::ReturnProjectionDemand {
            point: crate::semantic_query::demand::Demand::navigate(
                crate::semantic_query::demand::ProjectionPath::from_segments([
                    crate::semantic_query::PathSegment::Member(
                        crate::semantic_query::PropertyKey::identifier(Arc::clone(&member_name)),
                    ),
                ]),
            ),
        };
        let key = self.flow_return_key_with_demand(&identity, demand);
        match self.execute_flow_return(key) {
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
        debug_assert!(
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
        let (relation_members, flow_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
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
        let evaluated = self.evaluate_flow_return(&key);
        self.flow_frame_close(idx, evaluated)
    }

    /// The family cold-build arm (the `execute(FlowReturn)` reducer).
    /// Runs the root frame and maps the close onto the admission boundary:
    /// a NON-DEGRADED `Complete` ⇒ publish, carrying the compute-recorded
    /// `satisfied_projection`; a DEGRADED SUCCESS ⇒ the value RETURNS
    /// through the SUCCESS carrier with admission suppressed (`ReturnOnly`
    /// — no memo entry, no fact signature, no reverse-index metadata); a
    /// NO-VALUE failure ⇒ `Error(Miss)`, suppressed admission, the typed
    /// failure riding the transaction's root-failure channel.
    pub(super) fn build_flow_return(
        &self,
        key: &FlowReturnKey,
    ) -> QueryBuildOutput<SemanticQueryValue> {
        let fence = self.project_generation_signature();
        let idx = self.flow_frame_open(key);
        let evaluated = self.evaluate_flow_return(key);
        match self.flow_frame_close_root(idx, evaluated) {
            FlowRootClose::Complete(result, scc_self_roots, materialized) => {
                let degraded = result.degradation().is_some();
                let mut output: QueryBuildOutput<SemanticQueryValue> = QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::FlowReturn(Arc::new(result))),
                    fence,
                ))
                .with_observed_self_roots(scc_self_roots);
                // §3.4: the published entry's `satisfied_projection` is
                // the point set the compute ACTUALLY produced — recorded
                // by the evaluation, never the nominal request echoed at
                // publish time.
                output.satisfied_projection = materialized;
                if degraded {
                    // Degraded SUCCESS: a usable value, ReturnOnly by the
                    // split result/carrier contract — it may warm only
                    // under an explicit fact-rooted admission row, and
                    // none exists.
                    output.cache_suppress = true;
                }
                output
            }
            FlowRootClose::NoValue(failure) => {
                let mut output: QueryBuildOutput<SemanticQueryValue> =
                    (QueryResult::Error(QueryError::Miss), fence).into();
                // ReturnOnly: the failure flows to the caller through the
                // transaction's root-failure channel, the memo refuses
                // admission (no warm entry, no fact signature, no
                // reverse-index metadata).
                self.dispatch_txn.borrow_mut().flow.last_root_failure = Some(failure);
                output.cache_suppress = true;
                output
            }
        }
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
                FlowRootClose::Complete(result, _, _) => FlowReturnStep::Complete(result),
                FlowRootClose::NoValue(failure) => FlowReturnStep::NoValue(failure),
            },
        }
    }

    /// Close the machinery ROOT frame.
    fn flow_frame_close_root(&self, idx: usize, evaluated: FlowEvaluationOutcome) -> FlowRootClose {
        match self.flow_frame_pop(idx, evaluated, true) {
            FlowFramePop::RootClose(close) => close,
            FlowFramePop::Provisional(_) => unreachable!(
                "the machinery root frame is always its SCC's root: the stack is \
                 empty below it, so no open assumption can target a deeper frame"
            ),
        }
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
            holds,
            materialized,
            fresh_seed,
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
            // step. NEVER publishes here.
            let step = match &outcome {
                FlowReturnPendingOutcome::Complete(result) => {
                    FlowReturnStep::Complete(result.clone())
                }
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
                }),
            });
            return FlowFramePop::Provisional(step);
        }

        // ── SCC close at this root ──────────────────────────────────
        let mut relation_members = Vec::new();
        let mut flow_members = Vec::new();
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
                    });
                }
            }
        }
        let cyclic = !relation_members.is_empty() || !flow_members.is_empty() || self_cycle;
        // The ONE discharge: every flow member and the root reach the
        // equation fixed point `result_i = seed_i ∪ (⋃ hold targets)`
        // together — an EmptyCycle with no discharged target stays
        // `ReturnOnly` and poisons the component.
        let mut outcome = outcome;
        // A SELF-cycle (holds targeting only this root, with no drained
        // member) discharges through the SAME fixed point: the equation
        // `r = seed ∪ r` converges to the seed, and the shared discharge
        // owns the post-convergence literal-widening decision.
        if !flow_members.is_empty() || !holds.is_empty() {
            let mut entries: Vec<super::dispatch_txn::FlowDischargeEntry> =
                Vec::with_capacity(flow_members.len() + 1);
            entries.push(super::dispatch_txn::FlowDischargeEntry {
                key: root_key.clone(),
                outcome: outcome.clone(),
                holds: holds.clone(),
                fresh_seed,
            });
            for member in flow_members.iter() {
                entries.push(super::dispatch_txn::FlowDischargeEntry {
                    key: member.key.clone(),
                    outcome: member.outcome.clone(),
                    holds: member.holds.clone(),
                    fresh_seed: member.fresh_seed,
                });
            }
            self.discharge_flow_component_to_fixed_point(&mut entries);
            outcome = entries.remove(0).outcome;
            for (member, entry) in flow_members.iter_mut().zip(entries) {
                member.outcome = entry.outcome;
            }
        }
        // Atomic admission: a degraded flow outcome anywhere in the
        // component (the root included) poisons the WHOLE tagged
        // component — nothing publishes, every flight aborts.
        let component_degraded = matches!(outcome, FlowReturnPendingOutcome::NoValue { .. })
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
        if component_degraded {
            self.flow_return_abort_inline_flight(inline_flight.as_ref());
            for member in &relation_members {
                self.relation_abort_inline_flight(member.inline_flight.as_ref());
            }
            self.flow_return_abort_drained_flights(&flow_members);
            return FlowFramePop::RootClose(FlowRootClose::NoValue(match outcome {
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
            }));
        }
        // The published component's self-roots are the UNION of every
        // drained member's roots across BOTH domains (the root's own file,
        // every drained flow member's file, and every relation member's
        // observed node roots): a cross-file edit invalidates the whole
        // component.
        let mut scc_self_roots = self_roots.clone();
        for member in &flow_members {
            for root in &member.self_roots {
                if !scc_self_roots
                    .iter()
                    .any(|(canonical, _)| canonical == &root.0)
                {
                    scc_self_roots.push(root.clone());
                }
            }
        }
        if !relation_members.is_empty() {
            let mut nodes = Vec::with_capacity(relation_members.len() * 2);
            for member in &relation_members {
                nodes.push(member.key.source);
                nodes.push(member.key.target);
            }
            for root in self.observed_self_roots_from_nodes(nodes) {
                if !scc_self_roots
                    .iter()
                    .any(|(canonical, _)| canonical == &root.0)
                {
                    scc_self_roots.push(root);
                }
            }
        }
        // The relation members discharge through the shared coordinator
        // (no relation root — every relation member routes to the
        // completed batch; the flow members queue beside them).
        if (!relation_members.is_empty() || !flow_members.is_empty())
            && self
                .relation_discharge_and_route(false, None, relation_members, flow_members, cyclic)
                .is_err()
        {
            self.flow_return_abort_inline_flight(inline_flight.as_ref());
            return FlowFramePop::RootClose(FlowRootClose::NoValue(FlowReturnFailure::Unresolved));
        }
        // The root's own outcome: the machinery root publishes through
        // the family singleflight; an inline root batch-publishes with
        // the SCC drain and the caller consumes the computed step.
        match outcome {
            FlowReturnPendingOutcome::Complete(result) => {
                if machinery_root {
                    // The machinery root publishes through the family
                    // singleflight, which owns the root's own admission —
                    // so it never claims an inline flight, and this arm
                    // has none to drop.
                    debug_assert!(
                        inline_flight.is_none(),
                        "a machinery root publishes through the family singleflight \
                         and must never hold an inline flight to drop"
                    );
                    FlowFramePop::RootClose(FlowRootClose::Complete(
                        result,
                        scc_self_roots,
                        materialized,
                    ))
                } else {
                    self.dispatch_txn.borrow_mut().flow.completed_members.push(
                        CompletedFlowReturnMember {
                            key: root_key,
                            result: result.clone(),
                            inline_flight,
                            self_roots,
                            materialized,
                        },
                    );
                    FlowFramePop::Provisional(FlowReturnStep::Complete(result))
                }
            }
            FlowReturnPendingOutcome::NoValue { .. } => {
                unreachable!("a degraded root poisons the component above")
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
    ) {
        let index: rustc_hash::FxHashMap<&FlowReturnKey, usize> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| (&entry.key, i))
            .collect();
        let mut current: Vec<Option<FlowReturnResult>> = entries
            .iter()
            .map(|entry| match &entry.outcome {
                FlowReturnPendingOutcome::Complete(result) => Some(result.clone()),
                // A failed member has no SEED of its own. Its observed
                // degradation is NOT lost with the seed — it is read back
                // from the entry's own outcome below, so a member the
                // discharge resurrects carries it into the fixed point.
                FlowReturnPendingOutcome::NoValue { .. } => None,
            })
            .collect();
        loop {
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
                    match index.get(hold.key()).and_then(|j| current[*j].as_ref()) {
                        Some(result) => {
                            // The SAME transfer the call arm performs, so
                            // it applies the SAME rule: a hold target is a
                            // CALLEE, and its admitted return is expressed
                            // in the CALLEE's binders. Joining
                            // `result.return_type()` raw here re-published
                            // exactly the binder the call arm had already
                            // instantiated away — the fixed point ran the
                            // transfer a second time, around the gate. The
                            // hold's own accessor is now the only way to
                            // reach a node from a target's result.
                            arms.push(hold.discharged(self, result.return_type()).into_node());
                            if degradation.is_none() {
                                degradation = result.degradation();
                            }
                        }
                        // A target outside this component, or one that has
                        // not discharged: undecided — the entry cannot move.
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
                FlowReturnPendingOutcome::Complete(_)
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
            entry.outcome = FlowReturnPendingOutcome::Complete(result);
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
            finalized.push((
                tp.name.to_string(),
                intern_binder(&display_name, constraint, default),
            ));
            type_param_decls.push(crate::semantic_query::TypeParamDecl {
                name: display_name,
                constraint,
                default,
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
    /// [`MaterializedSet`]: crate::semantic_query::demand::MaterializedSet
    fn evaluate_flow_return(&self, key: &FlowReturnKey) -> FlowEvaluationOutcome {
        use crate::semantic_query::demand::{MaterializedPoint, MaterializedSet};
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
        if !key.input.is_empty() {
            return degraded(FlowReturnFailure::UnmodeledDemandPoint, Vec::new());
        }
        let demanded_member: Option<Arc<str>> = if key.demand.is_whole_return() {
            None
        } else {
            match flow_demanded_member_name(&key.demand) {
                Some(name) => Some(name),
                None => {
                    return degraded(FlowReturnFailure::UnmodeledDemandPoint, Vec::new());
                }
            }
        };
        let canonical = key.function.declaration_slot.defining_canonical.as_ref();
        let owner = key.function.declaration_slot.owner;
        let name = key.function.declaration_slot.merged_symbol_name.as_ref();
        let Some(serve) = self.ctx.ensure_indexed_ready_serve(canonical) else {
            return degraded(FlowReturnFailure::Missing, Vec::new());
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
        // A body whose own bytes could not be read has no exact-content
        // axis, so no content-addressed key can be built for it: fail
        // closed rather than key on a constant every unreadable body
        // shares.
        let Some(flow_body_exact_hash) = entry.flow_body_exact_hash else {
            return degraded(FlowReturnFailure::Unresolved, self_roots);
        };
        let slice_key_function = crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey {
            canonical_id: Arc::from(canonical),
            function: entry.key.clone(),
            flow_body_stable_hash: entry.flow_body_stable_hash,
            flow_body_exact_hash,
            parse_env_hash: key.context.parse_env_hash,
            parser_version: crate::file_artifact_store::CURRENT_PARSER_VERSION,
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
        let flow_slice = self.ctx.project_type_store().flow_slice();
        let lowered =
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
                    slice_hash,
                )) => {
                    // Hash-then-lower: the minted slice identity keys the
                    // lowered-slice artifact (the key is unconstructible
                    // without it), and the lowered node lowers ONLY the
                    // planned slice. A lowered miss on the pinned content is
                    // a torn view — undecided, never a fabricated slice.
                    let lowered_key = crate::cache_runtime::flow_slice_node::FlowSliceLoweredKey {
                        hash_key: slice_key,
                        slice_hash,
                    };
                    match crate::cache_runtime::lookup(
                        flow_slice.lowered_node(),
                        lowered_key,
                        self.ctx,
                    ) {
                        None => {
                            return degraded(FlowReturnFailure::Unresolved, self_roots);
                        }
                        Some(lowered) => lowered,
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
        let unapplied_write_effect = {
            use verter_semantic::analysis::flow::flow_ir::{FlowEffect, FlowEffectTarget};
            let retypes_slot = |slot: &verter_semantic::analysis::flow::flow_ir::FlowSlot| {
                slot.value_selected
                    || slot.kind == verter_semantic::analysis::flow::SkeletonBindingKind::Param
            };
            lowered.effects.iter().any(|effect| {
                let FlowEffect::Write { target, path, .. } = effect else {
                    return false;
                };
                if !path.is_empty() {
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
        let Some(ir) = indexed
            .shallow_state
            .decl_bodies()
            .flow_slice_content(entry, selection, skeleton)
        else {
            return degraded(FlowReturnFailure::Missing, self_roots);
        };
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
            var_locals: rustc_hash::FxHashMap::default(),
            widening_locals: rustc_hash::FxHashSet::default(),
            var_widening_locals: rustc_hash::FxHashSet::default(),
            bare_return_seen: false,
            member_filter,
            holds: Vec::new(),
            degradation: unapplied_write_effect
                .then_some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect)
                .or_else(|| {
                    signature_position_unmodeled
                        .then_some(crate::semantic_query::FlowReturnDegradation::UnmodeledPosition)
                }),
            degraded_locals: rustc_hash::FxHashSet::default(),
            var_degraded_locals: rustc_hash::FxHashSet::default(),
            var_conditional_locals: rustc_hash::FxHashSet::default(),
            conditional_arm_nesting: 0,
        };
        let holds;
        let degradation;
        let bare_return_seen;
        let (contributors, _) = {
            let outcome = evaluator.eval_region(&ir.body);
            holds = std::mem::take(&mut evaluator.holds);
            degradation = evaluator.degradation;
            bare_return_seen = evaluator.bare_return_seen;
            outcome
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
        let contributors = match self.connected_demand_trip() {
            Some(_) => Err(FlowReturnFailure::Budget(
                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
            )),
            None => contributors,
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
                };
            }
        };
        let (result, fresh_seed) = match self.join_flow_return_contributors(
            contributors,
            ir.can_fall_through,
            bare_return_seen,
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
                };
            }
        };
        // §3.4: record the point this compute ACTUALLY materialised — the
        // whole-return point it just evaluated (the demand gate above
        // proves it is the only point this evaluation serves). Recorded by
        // the compute, never re-derived from the nominal key at publish.
        let materialized =
            MaterializedSet::single(MaterializedPoint::new(key.demand.point.clone()));
        FlowEvaluationOutcome {
            outcome: FlowReturnPendingOutcome::Complete(result),
            self_roots,
            holds,
            materialized,
            fresh_seed,
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
            if arms.contains(&contribution.node) {
                continue;
            }
            arms.push(contribution.node);
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
        let fresh_seed = all_fresh && !bare_return_seen && !can_fall_through;
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
        }
        if can_fall_through {
            if arms.is_empty() {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Void)));
            } else {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined)));
            }
        } else if arms.is_empty() {
            if holds.is_empty() {
                arms.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Never)));
            } else {
                return Err(FlowReturnFailure::EmptyCycle);
            }
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

/// The binder environment for one function's OWN type parameters — see
/// [`ProjectSemanticDispatch::flow_binder_env`]. Carried by the evaluator
/// so parameter and body-leaf lowering resolve the function's binders
/// instead of any outer same-name declaration.
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
    /// The FUNCTION-scoped local layer: `var`-kind reaching definitions.
    /// `var` hoists to function scope, so block / `if` restores never
    /// touch this layer; a lexical same-name binding shadows it only
    /// while its block scope is live (reads consult `locals` first).
    var_locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
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
    /// Names bound to `any` because their initializer FAILED with a
    /// typed flow failure. Observing such a binding is the
    /// `FailedBindingInitializer` degradation; an unobserved failed
    /// binding degrades nothing.
    degraded_locals: rustc_hash::FxHashSet<String>,
    /// The `var`-layer failed-initializer membership (same rule,
    /// function scope).
    var_degraded_locals: rustc_hash::FxHashSet<String>,
    /// The `var`-layer CONDITIONAL-definition membership: names whose
    /// surviving reaching definition was recorded while
    /// [`Self::conditional_arm_nesting`] was non-zero. The function-scoped
    /// layer survives the arm restore by design, but no branch-join
    /// algebra folds the arms, so observing such a binding fails closed.
    var_conditional_locals: rustc_hash::FxHashSet<String>,
    /// How many `if` arms enclose the statement being evaluated. A plain
    /// block NEVER increments it — a block executes unconditionally, so a
    /// `var` it declares has exactly one reaching definition.
    conditional_arm_nesting: u32,
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
            let outcome = self.eval_expr(&member.value);
            let value = self.settle_composite_part(outcome, holds_before);
            // Selective object widening (BL02-class): a member read of a
            // WIDENING-literal local widens to its primitive at the
            // mutable member position (`const b = 1; return { b }`
            // publishes `b: number`), while `as const` / annotated
            // literal locals stay pinned. Direct literal members already
            // widened (or stayed pinned under a const assertion) at IR
            // lowering.
            let value = self.widen_if_widening_local_read(&member.value, value);
            surface_members.push(crate::semantic_query::SurfaceMember {
                key: crate::semantic_query::AuthoredPropertyKey::string(member.key.as_ref()),
                value,
                optional: false,
                readonly: false,
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
        // The lexical layer's conditional flag is always false: a
        // block-scoped conditional binding never escapes its arm.
        let (node, degraded, conditional) = if let Some(node) = self.locals.get(name) {
            (*node, self.degraded_locals.contains(name), false)
        } else {
            let node = *self.var_locals.get(name)?;
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

    /// tsc's `getAssignmentReducedType`: the union of the DECLARED
    /// constituents the initializer is comparable to. The survivors are
    /// DECLARED constituents — never the initializer's own type — so
    /// `let x: string | number = "s"` is `string` (not `"s"`) and
    /// `let x: 1 | 2 = 1` is `1`.
    ///
    /// Comparability is judged by the crate's SOLE relation authority
    /// (`execute_relate_pair`); an undecided constituent or an empty
    /// survivor set fails closed onto the whole declared union with the
    /// typed `UnreducedDeclaredUnion` degradation — never a guess.
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
                | super::dispatch_txn::RelationStep::Assumed => {
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
        self.dispatch
            .intern_normalized_union_or_intersection(&survivors, true)
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
                    if candidate.key.as_ref() == member_name.as_ref() {
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
    fn eval_region(
        &mut self,
        region: &crate::flow_slice_content::SliceRegion,
    ) -> (Result<Vec<FlowContribution>, FlowReturnFailure>, bool) {
        let mut contributors: Vec<FlowContribution> = Vec::new();
        for statement in region.statements.iter() {
            match statement {
                crate::flow_slice_content::SliceStatement::Return {
                    argument,
                    widening_literal,
                } => {
                    if self.member_filter.is_some() {
                        // Member-projection demand: evaluate ONLY the
                        // demanded member of a structural object return.
                        match self.eval_member_projected_return(argument.as_ref()) {
                            Ok(Some(node)) => contributors.push(FlowContribution {
                                node,
                                fresh_literal: false,
                            }),
                            Ok(None) => {}
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        }
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
                                contributors.push(FlowContribution {
                                    node,
                                    fresh_literal,
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
                }
                crate::flow_slice_content::SliceStatement::If {
                    consequent,
                    alternate,
                    ..
                } => {
                    // Bindings are block-scoped: each `if` arm evaluates
                    // under its own local scope (the reaching definitions
                    // of a `const` inside an arm never escape it). The
                    // function-scoped `var` layer DOES survive the
                    // restore, so the arms raise the conditional-arm
                    // nesting: a `var` bound here has no single reaching
                    // definition at the join, and observing it afterwards
                    // fails closed.
                    let saved = self.locals.clone();
                    let saved_degraded = self.degraded_locals.clone();
                    let saved_widening = self.widening_locals.clone();
                    self.conditional_arm_nesting += 1;
                    let (consequent_result, _) = self.eval_region(consequent);
                    let consequent_contributors = match consequent_result {
                        Ok(contributors) => contributors,
                        Err(failure) => {
                            self.conditional_arm_nesting -= 1;
                            return (Err(failure), region.can_fall_through);
                        }
                    };
                    contributors.extend(consequent_contributors);
                    self.locals = saved.clone();
                    self.degraded_locals = saved_degraded.clone();
                    self.widening_locals = saved_widening.clone();
                    if let Some(alternate) = alternate {
                        let (alternate_result, _) = self.eval_region(alternate);
                        let alternate_contributors = match alternate_result {
                            Ok(contributors) => contributors,
                            Err(failure) => {
                                self.conditional_arm_nesting -= 1;
                                return (Err(failure), region.can_fall_through);
                            }
                        };
                        contributors.extend(alternate_contributors);
                    }
                    self.conditional_arm_nesting -= 1;
                    self.locals = saved;
                    self.degraded_locals = saved_degraded;
                    self.widening_locals = saved_widening;
                }
                crate::flow_slice_content::SliceStatement::Block(block) => {
                    // Bindings are block-scoped: a `const` inside a block
                    // never escapes it.
                    let saved = self.locals.clone();
                    let saved_degraded = self.degraded_locals.clone();
                    let saved_widening = self.widening_locals.clone();
                    let (result, _) = self.eval_region(block);
                    let block_contributors = match result {
                        Ok(contributors) => contributors,
                        Err(failure) => return (Err(failure), region.can_fall_through),
                    };
                    contributors.extend(block_contributors);
                    self.locals = saved;
                    self.degraded_locals = saved_degraded;
                    self.widening_locals = saved_widening;
                }
                crate::flow_slice_content::SliceStatement::Binding {
                    name,
                    kind,
                    init,
                    declared,
                    widening_literal,
                } => {
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
                            self.bind_local(name, *kind, marker, false, false);
                            continue;
                        }
                        let declared_node = self.lower_body_type(declared.ty());
                        let arms = self.dispatch.union_arms_of(declared_node);
                        match (init, arms) {
                            (None, _) | (Some(_), None) => {
                                self.bind_local(name, *kind, declared_node, false, false);
                                continue;
                            }
                            (Some(init), Some(arms)) => {
                                let node = match self.eval_expr(init) {
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
                crate::flow_slice_content::SliceStatement::TransparentLoop => {}
                crate::flow_slice_content::SliceStatement::Unsupported(kind) => {
                    return (
                        Err(FlowReturnFailure::Unsupported(match kind {
                            crate::flow_slice_content::SliceUnsupported::Loop => {
                                FlowReturnUnsupported::Loop
                            }
                            crate::flow_slice_content::SliceUnsupported::Switch => {
                                FlowReturnUnsupported::Switch
                            }
                            crate::flow_slice_content::SliceUnsupported::Try => {
                                FlowReturnUnsupported::Try
                            }
                            crate::flow_slice_content::SliceUnsupported::Labeled => {
                                FlowReturnUnsupported::Labeled
                            }
                            crate::flow_slice_content::SliceUnsupported::Jump => {
                                FlowReturnUnsupported::Jump
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
        (Ok(contributors), region.can_fall_through)
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
        // The captured function-scope layer: the enclosing parameters BY
        // NAME, overlaid by the enclosing `var` layer (a redeclaring
        // enclosing `var` shares the parameter's slot and still wins).
        let mut captured_var_locals = self.var_locals.clone();
        for (ordinal, param) in self.param_names.iter().enumerate() {
            let (Some(name), Some(node)) = (param.name.as_ref(), self.params.get(ordinal)) else {
                continue;
            };
            captured_var_locals.entry(name.to_string()).or_insert(*node);
        }
        let nested_holds;
        let nested_degradation;
        let nested_bare_return_seen;
        let (contributors, _) = {
            let mut nested_evaluator = FlowEvaluator {
                dispatch: self.dispatch,
                self_slot: None,
                canonical: self.canonical,
                owner: self.owner,
                params: &params,
                param_names: nested_params,
                binder_env: &binder_env,
                locals: self.locals.clone(),
                var_locals: captured_var_locals,
                widening_locals: self.widening_locals.clone(),
                var_widening_locals: self.var_widening_locals.clone(),
                bare_return_seen: false,
                // A nested function value always evaluates its WHOLE
                // return (its signature's return type) — the member
                // filter is a top-level demand axis.
                member_filter: None,
                holds: Vec::new(),
                degradation: None,
                degraded_locals: self.degraded_locals.clone(),
                var_degraded_locals: self.var_degraded_locals.clone(),
                var_conditional_locals: self.var_conditional_locals.clone(),
                conditional_arm_nesting: 0,
            };
            let outcome = nested_evaluator.eval_region(body);
            nested_holds = nested_evaluator.holds.clone();
            nested_degradation = nested_evaluator.degradation;
            nested_bare_return_seen = nested_evaluator.bare_return_seen;
            self.holds.append(&mut nested_evaluator.holds);
            outcome
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
                can_fall_through,
                nested_bare_return_seen,
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
                self.eval_expr(inner)
            }
            // A parameter ordinal the frame's own parameter list does not
            // carry: the slice and the signature disagree about this
            // frame's arity. That is a fact about this REFERENCE, not
            // about the body around it.
            crate::flow_slice_content::SliceExpr::Param { ordinal } => {
                match self.params.get(*ordinal as usize).copied() {
                    Some(node) => Positional::Value(node),
                    None => Positional::Unmodeled,
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
                match self.read_local(name.as_ref()) {
                    Some(node) => Positional::Value(node),
                    // A CAPTURED binding the seeded snapshot does not
                    // carry has no honest value: it is neither the
                    // same-frame implicit-`any` nor a file-scope name, so
                    // the POSITION carries the marker.
                    None if *captured => Positional::Unmodeled,
                    None => Positional::Value(
                        param
                            .and_then(|ordinal| self.params.get(ordinal as usize).copied())
                            .unwrap_or_else(|| {
                                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any))
                            }),
                    ),
                }
            }
            crate::flow_slice_content::SliceExpr::Object { entries } => {
                self.eval_object_literal(entries)
            }
            crate::flow_slice_content::SliceExpr::NestedFunctionValue {
                params: nested_params,
                type_parameters,
                body,
                can_fall_through,
            } => {
                // The nested function's signature: its body-derived return
                // evaluates through the same flow machinery in a FRESH
                // frame (the nested function's own params / locals).
                Positional::Value(self.eval_nested_function_signature(
                    nested_params,
                    type_parameters,
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
                match self.eval_call(call, *site) {
                    Positional::Value(value) => Positional::Value(value.into_node()),
                    Positional::Hold => Positional::Hold,
                    Positional::Unmodeled => Positional::Unmodeled,
                }
            }
            crate::flow_slice_content::SliceExpr::Any => Positional::Value(
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            ),
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
            // spellings of one branch answer alike.
            crate::flow_slice_content::SliceExpr::Union { arms } => {
                let mut nodes = Vec::with_capacity(arms.len());
                for arm in arms.iter() {
                    // A coinductive HOLD inside a branch cannot be
                    // represented as a partial union arm — the SCC
                    // discharge joins whole contributions, not fragments
                    // of one. The ARM is the unmodelled position; the rest
                    // of the union survives, degraded.
                    let holds_before = self.holds.len();
                    let outcome = self.eval_expr(arm);
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

    /// Evaluate one CALL to the value it contributes to this frame.
    ///
    /// The ONE place a callee's return becomes a caller's value. Every
    /// arm returns a [`CallValue`], whose constructors each decide what
    /// happens to the CALLEE's own type-parameter clause — the rule
    /// cannot be silently skipped at a new arm, only chosen.
    ///
    /// [`Positional`], exactly as in [`Self::eval_expr`]: a call this
    /// substrate cannot resolve is an unmodelled POSITION, and the type
    /// leaves no way to say otherwise.
    fn eval_call(
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
                let prepared = self.dispatch.ctx.prepared_value_decl(
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
                // An OVERLOADED callee is not answerable here, and the
                // predicate for that is the SIZE of the overload group
                // alone.
                //
                // TypeScript resolves an overloaded call by ARGUMENTS,
                // picking the FIRST signature that matches. This rail
                // reaches ONE entry of the group and cannot pick: the
                // function-program index carries a single entry per
                // group, so for a BODIED group it lands on the trailing
                // implementation — the one signature the language HIDES
                // — and for an AMBIENT group (`declare function f(…)`
                // ×3, no implementation at all) it lands on the LAST
                // declaration while the language would pick the first.
                // Gating on "the selected signature has an
                // implementation body" therefore closed only the bodied
                // half and left the ambient half publishing a
                // confidently wrong answer, cleanly and warm.
                //
                // Picking the right overload needs argument-driven
                // overload resolution, which this substrate does not
                // perform. The answer is the `UnrepresentableCallee`
                // degradation it already exists for: the typed positional
                // marker, ReturnOnly by contract — never a warm-admitted
                // wrong answer. A LONE signature is untouched, bodied or
                // not:
                // the rule is overload VISIBILITY, not "any function with
                // a body".
                if prepared
                    .as_ref()
                    .is_some_and(|prepared| prepared.signatures.len() > 1)
                {
                    return self.degraded_unrepresentable_callee();
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
                let Some(callee_node) = self.dispatch.lower_type_expr_in_owner_scope_with_context(
                    self.canonical,
                    self.owner,
                    &type_arguments[0],
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                ) else {
                    return self.degraded_unrepresentable_callee();
                };
                self.call_return_of_callee_node(callee_node, site)
            }
        }
    }
}
