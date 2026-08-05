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
use super::walk::QueryBuildOutput;
use super::ProjectSemanticDispatch;
use crate::resolver_core::{FactVersionRef, ProgramAnalysisFactRef};
use crate::semantic_query::{
    FlowReturnFailure, FlowReturnKey, FlowReturnResult, FlowReturnStep, FlowReturnUnsupported,
    PrimitiveKind, QueryError, QueryResult, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
    SemanticQueryValue,
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
    /// A degraded body-derived evaluation: the typed `FlowReturnFailure`
    /// through `ReturnOnly` (never admitted) — the enclosing composition
    /// marks partial / fails closed.
    Degraded(FlowReturnFailure),
    /// No recoverable return carrier (a bodiless overload or a synthesized
    /// signature) — the consumer's absent-position arm.
    Absent,
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
    Degraded(FlowReturnFailure),
}

/// One flow frame's evaluation result, before the frame closes.
struct FlowEvaluationOutcome {
    /// The frame's decided outcome.
    outcome: FlowReturnPendingOutcome,
    /// The frame's own file roots.
    self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The coinductive hold targets the evaluation met.
    holds: Vec<FlowReturnKey>,
    /// The materialised point set the compute ACTUALLY produced (§3.4).
    materialized: crate::semantic_query::demand::MaterializedSet,
    /// Whether every one of the frame's OWN return contributors was a
    /// FRESH literal (and no bare-return / fallthrough arm joined) — the
    /// post-convergence literal-widening input.
    fresh_seed: bool,
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
                match self.execute_flow_return(self.flow_return_key_for(identity)) {
                    FlowReturnStep::Complete(result) => {
                        // A DEGRADED SUCCESS stays usable — the consumer
                        // keeps the value (interning a miss would be the
                        // opposite collapse) — but the degradation MUST
                        // fold into BOTH enclosing channels HERE, at the
                        // ONE sealed consumer entry, so no composition can
                        // launder a degraded interior into a complete,
                        // warm-admitted enclosing result (C1/SCC-2:
                        // degraded success defaults ReturnOnly; warm needs
                        // an explicit admission row, and none exists):
                        //   - the request partial sticky
                        //     (`mark_request_result_partial`) gates
                        //     component-meta / shape / materialize warm;
                        //   - the build-local taint (`cache_suppress` +
                        //     `result_is_partial`) marks the enclosing
                        //     composition partial / ReturnOnly.
                        if result.degradation.is_some() {
                            self.fold_cache_read_rails(true, true);
                        }
                        FunctionReturnNode::Flow(result)
                    }
                    FlowReturnStep::Degraded(failure) => FunctionReturnNode::Degraded(failure),
                    // A hold surfacing at a consumer is a demand reentering
                    // its own in-flight component: undecided here, ReturnOnly.
                    FlowReturnStep::Hold(_) => {
                        FunctionReturnNode::Degraded(FlowReturnFailure::Unresolved)
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
            FlowReturnStep::Complete(result) if result.degradation.is_none() => {
                Some(result.return_type)
            }
            // Degraded success / typed failure / in-flight hold: the
            // generic unwrap route decides (it already owns these
            // shapes for every other consumer).
            _ => None,
        }
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
        let mut publication = None;
        let read = self.execute_via_cold_build_helper_capturing_publication(
            SemanticQueryKey::FlowReturn(Box::new(key.clone())),
            &mut publication,
        );
        let step = match read.value {
            QueryResult::Value(SemanticQueryValue::FlowReturn(result)) => {
                FlowReturnStep::Complete((*result).clone())
            }
            // A degraded evaluation surfaces `Error(Miss)` to the memo
            // (loud, never a fallback, never admitted); the TYPED failure
            // rides the transaction to this caller (`Unresolved` only when
            // the cold build never ran — a torn or refused read).
            _ => FlowReturnStep::Degraded(
                self.dispatch_txn
                    .borrow_mut()
                    .flow
                    .last_root_failure
                    .take()
                    .unwrap_or(FlowReturnFailure::Unresolved),
            ),
        };
        if let Some(publication) = publication {
            self.flow_return_drain_completed_members(&key, &publication);
        } else {
            // ReturnOnly exit (degraded / undecided): the deferred batch
            // releases WITHOUT publish — no entry, no fact signature, no
            // backfill, no reverse-index metadata.
            self.relation_abort_completed_members();
        }
        step
    }

    /// Drain the SCC-closed member batch onto the root's published
    /// carrier. Relation members publish WITHOUT a relation-root fence
    /// (this root is a flow evaluation); flow members publish through
    /// their own family flights.
    fn flow_return_drain_completed_members(
        &self,
        _root_key: &FlowReturnKey,
        carrier: &crate::semantic_query_memo::PublishedMemoCandidate,
    ) {
        let (relation_members, flow_members) = {
            let mut txn = self.dispatch_txn.borrow_mut();
            (
                std::mem::take(&mut txn.relation.completed_members),
                std::mem::take(&mut txn.flow.completed_members),
            )
        };
        let graph = self.graph();
        for member in relation_members {
            let Some(flight) = member.inline_flight else {
                continue;
            };
            graph.publish_relation_member_fenced(
                Some(self.ctx),
                member.key,
                member.payload,
                carrier.read_set_signature.clone(),
                Arc::clone(&carrier.self_root_canonicals),
                carrier.validated_at_generation,
                None,
                Some(flight),
            );
        }
        for member in flow_members {
            let Some(flight) = member.inline_flight else {
                continue;
            };
            graph.publish_flow_return_member_fenced(
                Some(self.ctx),
                member.key,
                member.result,
                member.materialized,
                carrier.read_set_signature.clone(),
                Arc::clone(&carrier.self_root_canonicals),
                carrier.validated_at_generation,
                Some(flight),
            );
        }
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
            return FlowReturnStep::Degraded(FlowReturnFailure::Budget(
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
                let degraded = result.degradation.is_some();
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
            FlowRootClose::Degraded(failure) => {
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
                FlowRootClose::Degraded(failure) => FlowReturnStep::Degraded(failure),
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
            FlowReturnPendingOutcome::Degraded {
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
                FlowReturnPendingOutcome::Degraded { failure, .. } => {
                    FlowReturnStep::Degraded(*failure)
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
        let component_degraded = matches!(outcome, FlowReturnPendingOutcome::Degraded { .. })
            || flow_members
                .iter()
                .any(|member| matches!(member.outcome, FlowReturnPendingOutcome::Degraded { .. }))
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
            return FlowFramePop::RootClose(FlowRootClose::Degraded(match outcome {
                FlowReturnPendingOutcome::Degraded { failure, .. } => failure,
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
            return FlowFramePop::RootClose(FlowRootClose::Degraded(FlowReturnFailure::Unresolved));
        }
        // The root's own outcome: the machinery root publishes through
        // the family singleflight; an inline root batch-publishes with
        // the SCC drain and the caller consumes the computed step.
        match outcome {
            FlowReturnPendingOutcome::Complete(result) => {
                if machinery_root {
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
            FlowReturnPendingOutcome::Degraded { .. } => {
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
                FlowReturnPendingOutcome::Degraded { .. } => None,
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
                    arms.push(result.return_type);
                }
                let mut ready = true;
                for target in &entries[i].holds {
                    match index.get(target).and_then(|j| current[*j].as_ref()) {
                        Some(result) => {
                            arms.push(result.return_type);
                            if degradation.is_none() {
                                degradation = result.degradation;
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
                let next = FlowReturnResult {
                    return_type: self.intern_normalized_union_or_intersection(&flat, true),
                    can_fall_through: current[i]
                        .as_ref()
                        .is_some_and(|result| result.can_fall_through),
                    degradation,
                };
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
            let Some(mut result) = discharged else {
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
                    | FlowReturnPendingOutcome::Degraded {
                        failure: FlowReturnFailure::EmptyCycle,
                        ..
                    }
            ) {
                continue;
            }
            if component_is_fresh {
                result.return_type = widen_literal_node(self, result.return_type);
            }
            entry.outcome = FlowReturnPendingOutcome::Complete(result);
        }
    }

    // The evaluator
    // ──────────────────────────────────────────────────────────────────

    /// The ONE binder environment for a function's OWN type parameters:
    /// the binders intern as `TypeParam` nodes in the file scope and shadow
    /// every outer same-name resolution. Shared by the root evaluation
    /// (parameters + body leaves) and every nested function value's
    /// signature; an empty clause carries an empty `env`, which reproduces
    /// the owner-scope lowering exactly.
    fn flow_binder_env(
        &self,
        canonical: &str,
        owner: verter_type_expr::TopLevelOwnerId,
        type_parameters: &[crate::flow_slice_content::SliceTypeParam],
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
        // The function's OWN type parameters are binders in scope for the
        // parameter / return lowering (constraints / defaults lower under
        // the OUTER env).
        let mut env: rustc_hash::FxHashMap<String, SemanticNodeId> =
            rustc_hash::FxHashMap::default();
        let mut type_param_decls: Vec<crate::semantic_query::TypeParamDecl> =
            Vec::with_capacity(type_parameters.len());
        for tp in type_parameters.iter() {
            let constraint = tp.constraint.as_ref().and_then(|c| {
                self.lower_type_expr_in_owner_scope_with_context(
                    canonical,
                    owner,
                    c,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                )
            });
            let default = tp.default.as_ref().and_then(|d| {
                self.lower_type_expr_in_owner_scope_with_context(
                    canonical,
                    owner,
                    d,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                )
            });
            let display_name: Arc<str> = Arc::clone(&tp.name);
            let binder = graph.intern_node(SemanticNodeData::TypeParam {
                decl: crate::semantic_query::DeclIdentity::from_scope(
                    &scope,
                    Arc::clone(&display_name),
                ),
                param_index: 0,
                constraint,
                default,
                display_name: Arc::clone(&display_name),
            });
            env.insert(tp.name.to_string(), binder);
            type_param_decls.push(crate::semantic_query::TypeParamDecl {
                name: display_name,
                constraint,
                default,
            });
        }
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
                    outcome: FlowReturnPendingOutcome::Degraded {
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
        let Some(entry) = index.entries.iter().find(|entry| {
            entry.key.declaration.owner == owner
                && entry.key.declaration.name.as_ref() == name
                && entry.key.part == key.function.function_part
                && entry.key.overload_ordinal == key.function.overload_ordinal
        }) else {
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
        let slice_key_function = crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey {
            canonical_id: Arc::from(canonical),
            function: entry.key.clone(),
            flow_body_stable_hash: entry.flow_body_stable_hash,
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
        let binder_env = self.flow_binder_env(canonical, owner, &ir.type_parameters);
        let mut params: Vec<SemanticNodeId> = Vec::with_capacity(ir.params.len());
        for param in ir.params.iter() {
            let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
            let node = self.shallow_lower_type_expr_with_context(
                &param.ty,
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
            key,
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
                .then_some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect),
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
        let contributors = match contributors {
            Ok(contributors) => contributors,
            Err(failure) => {
                return FlowEvaluationOutcome {
                    outcome: FlowReturnPendingOutcome::Degraded {
                        failure,
                        degradation,
                    },
                    self_roots,
                    holds,
                    materialized: MaterializedSet::empty(),
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
                    outcome: FlowReturnPendingOutcome::Degraded {
                        failure,
                        degradation,
                    },
                    self_roots,
                    holds,
                    materialized: MaterializedSet::empty(),
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
        holds: &[FlowReturnKey],
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
            // for its reverse (tsc 7.0.2: `1` for both).
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
                    FlowReturnResult {
                        return_type,
                        can_fall_through,
                        degradation,
                    },
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
            FlowReturnResult {
                return_type,
                can_fall_through,
                degradation,
            },
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

/// The per-frame evaluator state.
struct FlowEvaluator<'d, 'b> {
    dispatch: &'d ProjectSemanticDispatch<'d>,
    key: &'b FlowReturnKey,
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
    holds: Vec<FlowReturnKey>,
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

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Record a typed degradation (first-observed reason wins,
    /// deterministic in source order).
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
        let Some(crate::flow_slice_content::SliceExpr::Object { members }) = argument else {
            return Err(FlowReturnFailure::UnmodeledDemandPoint);
        };
        // Last write wins for duplicate keys (JS object-literal
        // semantics): take the LAST member with the demanded key.
        let Some(member) = members
            .iter()
            .rev()
            .find(|member| member.key.as_ref() == member_name.as_ref())
        else {
            return Err(FlowReturnFailure::UnmodeledDemandPoint);
        };
        match self.eval_expr(&member.value)? {
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
                            match self.eval_expr(expr) {
                                Ok(Some(node)) => {
                                    fresh_literal |= self.holds.len() > holds_before;
                                    contributors.push(FlowContribution {
                                        node,
                                        fresh_literal,
                                    });
                                }
                                // A hold is neither a contributor nor a failure.
                                Ok(None) => {}
                                Err(failure) => return (Err(failure), region.can_fall_through),
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
                        let declared_node = self.lower_body_type(declared);
                        let arms = self.dispatch.union_arms_of(declared_node);
                        match (init, arms) {
                            (None, _) | (Some(_), None) => {
                                self.bind_local(name, *kind, declared_node, false, false);
                                continue;
                            }
                            (Some(init), Some(arms)) => {
                                let node = match self.eval_expr(init) {
                                    Ok(Some(init_node)) => self.assignment_reduced_union(
                                        declared_node,
                                        &arms,
                                        init_node,
                                    ),
                                    // A hold / failed initializer cannot
                                    // select constituents: the whole
                                    // declared union is the honest
                                    // superset, degraded.
                                    Ok(None) | Err(_) => {
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
                            Ok(Some(node)) => {
                                self.bind_local(name, *kind, node, *widening_literal, false);
                            }
                            Ok(None) => {}
                            // A failed initializer binds `any` — the
                            // declaration itself is not a return
                            // contribution; the binding's failure only
                            // surfaces where the binding is OBSERVED: the
                            // observation evaluates to `any` and records
                            // the `FailedBindingInitializer` degradation
                            // (never a poison; an unobserved failed
                            // binding degrades nothing).
                            Err(_) => {
                                let any = self
                                    .dispatch
                                    .graph()
                                    .intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any));
                                self.bind_local(name, *kind, any, false, true);
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
    ) -> Result<SemanticNodeId, FlowReturnFailure> {
        let graph = self.dispatch.graph();
        // The nested function's OWN type parameters are binders in scope
        // for the parameter / return lowering (a `<T>(x: T) => x` keeps
        // `<T>`; constraints / defaults lower under the OUTER env).
        let binder_env = self
            .dispatch
            .flow_binder_env(self.canonical, self.owner, type_parameters);
        let mut params: Vec<SemanticNodeId> = Vec::with_capacity(nested_params.len());
        let mut signature_params: Vec<crate::semantic_query::FunctionParam> =
            Vec::with_capacity(nested_params.len());
        for param in nested_params.iter() {
            let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
            let node = self.dispatch.shallow_lower_type_expr_with_context(
                &param.ty,
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
                key: self.key,
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
        let contributors = contributors?;
        // A nested function value's body is its own join; its holds ride
        // the OUTER frame's component, so no fixed point closes here and
        // the freshness bit has no later consumer.
        let (result, _fresh_seed) = self.dispatch.join_flow_return_contributors(
            contributors,
            can_fall_through,
            nested_bare_return_seen,
            &nested_holds,
            nested_degradation,
        )?;
        Ok(graph.intern_node(SemanticNodeData::Signature {
            kind: crate::semantic_query::SignatureKind::Call,
            params: Arc::from(signature_params.into_boxed_slice()),
            return_type: result.return_type,
            type_parameters: Arc::from(binder_env.type_param_decls.into_boxed_slice()),
            signature_span: None,
            return_type_span: None,
        }))
    }

    /// Evaluate one flow expression to a graph node. `Ok(None)` is a
    /// coinductive HOLD (a same-slot recursive backedge — neither a
    /// contributor nor a failure).
    fn eval_expr(
        &mut self,
        expr: &crate::flow_slice_content::SliceExpr,
    ) -> Result<Option<SemanticNodeId>, FlowReturnFailure> {
        let graph = self.dispatch.graph();
        match expr {
            crate::flow_slice_content::SliceExpr::Type(ty) => Ok(Some(self.lower_body_type(ty))),
            crate::flow_slice_content::SliceExpr::Param { ordinal } => self
                .params
                .get(*ordinal as usize)
                .copied()
                .map(Some)
                .ok_or(FlowReturnFailure::Unresolved),
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
                    Some(node) => Ok(Some(node)),
                    // A CAPTURED binding the seeded snapshot does not
                    // carry has no honest value: it is neither the
                    // same-frame implicit-`any` nor a file-scope name, so
                    // it fails closed.
                    None if *captured => Err(FlowReturnFailure::UnmodeledBinding),
                    None => Ok(Some(
                        param
                            .and_then(|ordinal| self.params.get(ordinal as usize).copied())
                            .unwrap_or_else(|| {
                                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any))
                            }),
                    )),
                }
            }
            crate::flow_slice_content::SliceExpr::Object { members } => {
                // Structural object-literal return: each member value
                // evaluates as a flow expression (parameter / local
                // references substitute); a hold nested in a member value
                // cannot be a plain skip — the whole evaluation is
                // undecided (recursive object construction is beyond the
                // direct same-slot hold the return sites model).
                let mut surface_members = Vec::with_capacity(members.len());
                for member in members.iter() {
                    let Some(value) = self.eval_expr(&member.value)? else {
                        return Err(FlowReturnFailure::Unresolved);
                    };
                    // Selective object widening (BL02-class): a member
                    // read of a WIDENING-literal local widens to its
                    // primitive at the mutable member position (`const
                    // b = 1; return { b }` publishes `b: number`), while
                    // `as const` / annotated literal locals stay pinned.
                    // Direct literal members already widened (or stayed
                    // pinned under a const assertion) at IR lowering.
                    let value = self.widen_if_widening_local_read(&member.value, value);
                    surface_members.push(crate::semantic_query::SurfaceMember {
                        key: crate::semantic_query::AuthoredPropertyKey::string(
                            member.key.as_ref(),
                        ),
                        value,
                        optional: false,
                        readonly: false,
                        method_kind: member.method_kind,
                        has_implementation_body: member.method_kind.is_some(),
                        visibility: verter_type_expr::MemberVisibility::Public,
                        excess_origin: verter_type_expr::ExcessPropertyOrigin::FreshOwn,
                        spans: member.spans,
                        declaration_origin: Some(Arc::from(self.canonical)),
                        declared_in_macro_type_arg:
                            crate::semantic_query::MacroOwnBodyStamp::default(),
                        merge_role: crate::semantic_query::MergeRoleStamp::default(),
                    });
                }
                Ok(Some(graph.intern_node(SemanticNodeData::Object(
                    crate::semantic_query::surface_view! {
                        members: Arc::from(surface_members.into_boxed_slice()),
                        call_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        construct_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        index_signatures: Arc::from(Vec::new().into_boxed_slice()),
                        keyspace: None,
                        has_index_signature: false,
                    },
                ))))
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
                let signature = self.eval_nested_function_signature(
                    nested_params,
                    type_parameters,
                    body,
                    *can_fall_through,
                )?;
                Ok(Some(signature))
            }
            crate::flow_slice_content::SliceExpr::NestedCall(function) => {
                // An IIFE: the call's value is the nested function's
                // evaluated return.
                let signature = match self.eval_expr(function)? {
                    Some(signature) => signature,
                    None => return Ok(None),
                };
                let data = graph.node_data(signature);
                match data.as_deref() {
                    Some(SemanticNodeData::Signature { return_type, .. }) => Ok(Some(*return_type)),
                    _ => Err(FlowReturnFailure::Unresolved),
                }
            }
            crate::flow_slice_content::SliceExpr::DirectCall(target) => {
                // An exact same-file direct call — a Flow obligation edge
                // through the ONE key construction when the callee's return
                // is body-derived, or its DECLARED carrier when the callee
                // annotates one (a declared return always wins over the
                // body). A back-edge to an in-flight target is a
                // coinductive hold (neither a contributor nor a failure);
                // an empty-cycle outcome is a hold the SCC close discharges
                // on the component's admitted returns; every other outcome
                // contributes the callee's return or its typed failure.
                let source = self
                    .dispatch
                    .ctx
                    .prepared_value_decl(
                        self.canonical,
                        target.declaration.owner,
                        target.declaration.name.as_ref(),
                    )
                    .and_then(|prepared| {
                        let ordinal = match &target.part {
                            verter_type_expr::facts::FunctionPartIdentity::DeclarationBody => {
                                target.overload_ordinal as usize
                            }
                            _ => 0,
                        };
                        prepared
                            .signatures
                            .get(ordinal)
                            .map(|signature| signature.return_source.clone())
                    });
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
                                if let Some(degradation) = result.degradation {
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
                                    self.holds.push(key);
                                }
                                Ok(Some(result.return_type))
                            }
                            FlowReturnStep::Hold(key) => {
                                self.holds.push(*key);
                                Ok(None)
                            }
                            FlowReturnStep::Degraded(FlowReturnFailure::EmptyCycle) => {
                                // An empty-cycle callee IS a hold — the SCC
                                // close discharges it (and its callers) on
                                // the component's admitted returns.
                                self.holds.push(key);
                                Ok(None)
                            }
                            FlowReturnStep::Degraded(failure) => Err(failure),
                        }
                    }
                    source => match self
                        .dispatch
                        .execute_function_return_source(source, self.canonical)
                    {
                        super::flow_return::FunctionReturnNode::Declared(hot) => {
                            Ok(Some(hot.node()))
                        }
                        super::flow_return::FunctionReturnNode::DeclaredMiss => {
                            Err(FlowReturnFailure::Unresolved)
                        }
                        super::flow_return::FunctionReturnNode::Absent => {
                            Err(FlowReturnFailure::Missing)
                        }
                        super::flow_return::FunctionReturnNode::Flow(_)
                        | super::flow_return::FunctionReturnNode::Degraded(_) => {
                            unreachable!("a Declared/Absent source never reaches the flow rail")
                        }
                    },
                }
            }
            crate::flow_slice_content::SliceExpr::CallOnBinding {
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
                    // has no honest value: fail closed rather than take
                    // the same-frame implicit-`any` call.
                    if *captured {
                        return Err(FlowReturnFailure::UnmodeledBinding);
                    }
                    return Ok(Some(
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
                    ));
                };
                let data = graph.node_data(node);
                match data.as_deref() {
                    Some(SemanticNodeData::Signature { return_type, .. }) => Ok(Some(*return_type)),
                    Some(SemanticNodeData::Primitive(PrimitiveKind::Any)) => Ok(Some(
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
                    )),
                    _ => {
                        self.record_degradation(
                            crate::semantic_query::FlowReturnDegradation::NonCallableBinding,
                        );
                        Ok(Some(graph.intern_node(SemanticNodeData::Primitive(
                            PrimitiveKind::Any,
                        ))))
                    }
                }
            }
            crate::flow_slice_content::SliceExpr::LocalFunctionShadow => {
                // A call to a hoisted nested function declaration: the
                // declaration shadows every outer same-name callee; exact
                // recovery of the nested declaration's own return is not
                // implemented — fail closed, never bind the outer callee.
                Err(FlowReturnFailure::Unresolved)
            }
            crate::flow_slice_content::SliceExpr::DirectSelfCall => {
                match self.dispatch.execute_flow_return(self.key.clone()) {
                    FlowReturnStep::Hold(_) => {
                        self.holds.push(self.key.clone());
                        Ok(None)
                    }
                    FlowReturnStep::Complete(_) => {
                        unreachable!("a same-slot recursive edge is always a hold in flight")
                    }
                    FlowReturnStep::Degraded(failure) => Err(failure),
                }
            }
            crate::flow_slice_content::SliceExpr::SymbolicCall(ty) => {
                // The symbolic `ReturnType<typeof …>` carrier: lower the
                // callee, resolve its signature through the same builtin
                // `ReturnType` reduction every consumer uses, and take the
                // call-bucket return — an unrepresentable / unresolvable
                // callee is the `UnrepresentableCallee` DEGRADATION: a
                // usable modeled-`any`, `ReturnOnly` by contract.
                let graph = self.dispatch.graph();
                let degraded_any = |evaluator: &mut Self| {
                    evaluator.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::UnrepresentableCallee,
                    );
                    Ok(Some(graph.intern_node(SemanticNodeData::Primitive(
                        PrimitiveKind::Any,
                    ))))
                };
                let verter_type_expr::TypeExpr::Ref {
                    name,
                    type_arguments,
                } = ty
                else {
                    return degraded_any(self);
                };
                if name.as_ref() != "ReturnType" || type_arguments.len() != 1 {
                    return degraded_any(self);
                }
                let Some(callee_node) = self.dispatch.lower_type_expr_in_owner_scope_with_context(
                    self.canonical,
                    self.owner,
                    &type_arguments[0],
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                ) else {
                    return degraded_any(self);
                };
                let resolved = self.dispatch.resolve_signature_source_carrier(
                    callee_node,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                );
                match self
                    .dispatch
                    .select_signature_function(resolved, super::build::SignatureBucket::Call)
                {
                    Some(function_node) => {
                        let data = graph.node_data(function_node);
                        let return_type = match data.as_deref() {
                            Some(SemanticNodeData::Signature { return_type, .. }) => *return_type,
                            _ => {
                                return degraded_any(self);
                            }
                        };

                        // A semantic-miss carrier at the signature's return
                        // position is a DEGRADED nested demand (an in-flight
                        // / failed callee) — propagate the typed failure,
                        // never count the miss as a contributor.
                        if matches!(
                            graph.node_data(return_type).as_deref(),
                            Some(SemanticNodeData::Opaque(QueryError::Miss))
                        ) {
                            return Err(FlowReturnFailure::Unresolved);
                        }
                        // Free signature generics instantiate at `unknown`
                        // (the sb15 rule).
                        Ok(Some(
                            self.dispatch.instantiate_free_signature_params_at_unknown(
                                function_node,
                                return_type,
                            ),
                        ))
                    }
                    None => degraded_any(self),
                }
            }
            crate::flow_slice_content::SliceExpr::Any => Ok(Some(
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            )),
            // A name the frame's lexical authority resolved to a
            // function-local binding the content half does not model
            // (a destructuring element, a local `class` / `enum` /
            // `namespace` / `import =`, a `catch` parameter, a nested
            // function declaration read as a value). The name is
            // RESOLVED — never free — so there is no honest value to
            // publish: fail closed with the typed no-value failure
            // rather than bind an unrelated same-named declaration.
            crate::flow_slice_content::SliceExpr::UnmodeledBinding => {
                Err(FlowReturnFailure::UnmodeledBinding)
            }
            // Content the demand slice did not select: never lowered,
            // never evaluable. Reaching one is a planner/content mismatch
            // — undecided, fail closed; never a fabricated `any`.
            crate::flow_slice_content::SliceExpr::Elided => Err(FlowReturnFailure::Unresolved),
        }
    }
}
