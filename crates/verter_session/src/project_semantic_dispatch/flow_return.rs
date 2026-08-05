//! The whole-function `FlowReturn` authority.
//!
//! One `SemanticQueryKey::FlowReturn` producer through
//! [`ProjectSemanticDispatch`]: the demanded function's complete body is
//! evaluated through the lazy whole-body flow IR
//! ([`crate::flow_ir::WholeFunctionFlowIrNode`]) on the shared tagged
//! obligation runtime — return sites, `if` reachability, bare return,
//! fallthrough, primitive widening, unions, parameters and simple local
//! reaching definitions, object returns (spread delegated to
//! `ProjectObjectSpread`), symbolic call returns (`ReturnType<typeof …>`
//! / `any` carriers), return-free loop transparency, and direct same-slot
//! recursion through coinductive holds.
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
            // The canonical production point: whole return, empty input.
            // The axes are KEY DATA — a narrower demand or a contextual
            // input is a distinct cache and re-entry identity, never an
            // implicit default.
            demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
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
                    FlowReturnStep::Complete(result) => FunctionReturnNode::Flow(result),
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
        let (outcome, self_roots, holds, materialized) = self.evaluate_flow_return(&key);
        self.flow_frame_close(idx, outcome, holds, self_roots, materialized)
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
        let (outcome, self_roots, holds, materialized) = self.evaluate_flow_return(key);
        match self.flow_frame_close_root(idx, outcome, holds, self_roots, materialized) {
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
    fn flow_frame_close(
        &self,
        idx: usize,
        outcome: FlowReturnPendingOutcome,
        holds: Vec<FlowReturnKey>,
        self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        materialized: crate::semantic_query::demand::MaterializedSet,
    ) -> FlowReturnStep {
        match self.flow_frame_pop(idx, outcome, holds, self_roots, materialized, false) {
            FlowFramePop::Provisional(step) => step,
            FlowFramePop::RootClose(close) => match close {
                FlowRootClose::Complete(result, _, _) => FlowReturnStep::Complete(result),
                FlowRootClose::Degraded(failure) => FlowReturnStep::Degraded(failure),
            },
        }
    }

    /// Close the machinery ROOT frame.
    fn flow_frame_close_root(
        &self,
        idx: usize,
        outcome: FlowReturnPendingOutcome,
        holds: Vec<FlowReturnKey>,
        self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        materialized: crate::semantic_query::demand::MaterializedSet,
    ) -> FlowRootClose {
        match self.flow_frame_pop(idx, outcome, holds, self_roots, materialized, true) {
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
        outcome: FlowReturnPendingOutcome,
        holds: Vec<FlowReturnKey>,
        self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        materialized: crate::semantic_query::demand::MaterializedSet,
        machinery_root: bool,
    ) -> FlowFramePop {
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
        // A budget edge on the frame poisons the whole component.
        let outcome = if budget_cap.is_some() {
            FlowReturnPendingOutcome::Degraded(FlowReturnFailure::Budget(
                verter_type_expr::facts::InferenceUnavailableReason::WorkBudgetExceeded,
            ))
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
                FlowReturnPendingOutcome::Degraded(failure) => FlowReturnStep::Degraded(*failure),
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
        if !flow_members.is_empty() {
            let mut entries: Vec<super::dispatch_txn::FlowDischargeEntry> =
                Vec::with_capacity(flow_members.len() + 1);
            entries.push(super::dispatch_txn::FlowDischargeEntry {
                key: root_key.clone(),
                outcome: outcome.clone(),
                holds: holds.clone(),
            });
            for member in flow_members.iter() {
                entries.push(super::dispatch_txn::FlowDischargeEntry {
                    key: member.key.clone(),
                    outcome: member.outcome.clone(),
                    holds: member.holds.clone(),
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
        let component_degraded = matches!(outcome, FlowReturnPendingOutcome::Degraded(_))
            || flow_members
                .iter()
                .any(|member| matches!(member.outcome, FlowReturnPendingOutcome::Degraded(_)))
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
                FlowReturnPendingOutcome::Degraded(failure) => failure,
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
            FlowReturnPendingOutcome::Degraded(_) => {
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
                FlowReturnPendingOutcome::Degraded(_) => None,
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
                let mut degradation = current[i].as_ref().and_then(|result| result.degradation);
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
        for (entry, discharged) in entries.iter_mut().zip(current) {
            if let Some(result) = discharged {
                entry.outcome = FlowReturnPendingOutcome::Complete(result);
            }
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
        type_parameters: &[crate::flow_ir::FlowIrTypeParam],
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
    fn evaluate_flow_return(
        &self,
        key: &FlowReturnKey,
    ) -> (
        FlowReturnPendingOutcome,
        Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
        Vec<FlowReturnKey>,
        crate::semantic_query::demand::MaterializedSet,
    ) {
        use crate::semantic_query::demand::{MaterializedPoint, MaterializedSet};
        let degraded =
            |failure: FlowReturnFailure,
             self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>| {
                (
                    FlowReturnPendingOutcome::Degraded(failure),
                    self_roots,
                    Vec::new(),
                    MaterializedSet::empty(),
                )
            };
        // The evaluation models exactly the whole-return / empty-input
        // point today. Any other demand/input point fails CLOSED with a
        // typed no-value outcome — never a silently widened whole-return
        // result, never a sibling materialisation the narrower demand did
        // not ask for.
        if !key.demand.is_whole_return() || !key.input.is_empty() {
            return degraded(FlowReturnFailure::UnmodeledDemandPoint, Vec::new());
        }
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
        let slice_key = crate::cache_runtime::flow_slice_node::FlowSliceHashKey {
            function: crate::cache_runtime::flow_slice_node::FlowSliceFunctionKey {
                canonical_id: Arc::from(canonical),
                function: entry.key.clone(),
                flow_body_stable_hash: entry.flow_body_stable_hash,
                parse_env_hash: key.context.parse_env_hash,
                parser_version: crate::file_artifact_store::CURRENT_PARSER_VERSION,
            },
            demand: crate::cache_runtime::flow_slice_node::FlowSliceDemandIdentity {
                projection_path: Arc::from(Vec::<Arc<str>>::new().into_boxed_slice()),
            },
        };
        let flow_slice = self.ctx.project_type_store().flow_slice();
        match crate::cache_runtime::lookup(flow_slice.hash_node(), slice_key.clone(), self.ctx) {
            None => {
                // The skeleton source could not serve the pinned content
                // version (a torn view between the served index and the
                // retained snapshot): undecided, never a fabricated slice.
                return degraded(FlowReturnFailure::Unresolved, self_roots);
            }
            Some(crate::cache_runtime::flow_slice_node::FlowSliceHashOutcome::BudgetExceeded(
                exceeded,
            )) => {
                tracing::debug!(
                    axis = ?exceeded.axis,
                    limit = exceeded.limit,
                    observed = exceeded.observed,
                    "flow-slice budget exceeded: typed Budget failure, ReturnOnly"
                );
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
                if crate::cache_runtime::lookup(flow_slice.lowered_node(), lowered_key, self.ctx)
                    .is_none()
                {
                    return degraded(FlowReturnFailure::Unresolved, self_roots);
                }
            }
        }
        let Some(ir) = indexed
            .shallow_state
            .decl_bodies()
            .whole_function_flow_ir(entry)
        else {
            return degraded(FlowReturnFailure::Missing, self_roots);
        };
        // A budget edge in one leaf's expression lowering stops the whole
        // evaluation with the typed reason (the scanner's `Unavailable`
        // verdict for the same leaf).
        if let Some(reason) = ir.budget_failure {
            return degraded(FlowReturnFailure::Budget(reason), self_roots);
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
            binder_env: &binder_env,
            locals: rustc_hash::FxHashMap::default(),
            holds: Vec::new(),
            degradation: None,
            degraded_locals: rustc_hash::FxHashSet::default(),
        };
        let holds;
        let degradation;
        let (contributors, _) = {
            let outcome = evaluator.eval_region(&ir.body);
            holds = std::mem::take(&mut evaluator.holds);
            degradation = evaluator.degradation;
            outcome
        };
        let contributors = match contributors {
            Ok(contributors) => contributors,
            Err(failure) => {
                return (
                    FlowReturnPendingOutcome::Degraded(failure),
                    self_roots,
                    holds,
                    MaterializedSet::empty(),
                );
            }
        };
        let result = match self.join_flow_return_contributors(
            contributors,
            ir.can_fall_through,
            &holds,
            degradation,
        ) {
            Ok(result) => result,
            Err(failure) => {
                return (
                    FlowReturnPendingOutcome::Degraded(failure),
                    self_roots,
                    holds,
                    MaterializedSet::empty(),
                );
            }
        };
        // §3.4: record the point this compute ACTUALLY materialised — the
        // whole-return point it just evaluated (the demand gate above
        // proves it is the only point this evaluation serves). Recorded by
        // the compute, never re-derived from the nominal key at publish.
        let materialized =
            MaterializedSet::single(MaterializedPoint::new(key.demand.point.clone()));
        (
            FlowReturnPendingOutcome::Complete(result),
            self_roots,
            holds,
            materialized,
        )
    }

    /// Join one function's return-site contributors with the fallthrough
    /// seed: a fall-through body adds `undefined` (or `void` when it has no
    /// return at all); a body that terminates with NO contribution and no
    /// hold (a throw-only body) is `never`; a HOLD-only body with no
    /// fallthrough is the empty recursive cycle — a typed failure, never
    /// `never`.
    fn join_flow_return_contributors(
        &self,
        contributors: Vec<SemanticNodeId>,
        can_fall_through: bool,
        holds: &[FlowReturnKey],
        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    ) -> Result<FlowReturnResult, FlowReturnFailure> {
        let graph = self.graph();
        let mut arms: Vec<SemanticNodeId> = contributors;
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
        Ok(FlowReturnResult {
            return_type,
            can_fall_through,
            degradation,
        })
    }
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
    /// The function's OWN binder environment (parameters + body leaves
    /// lower under it).
    binder_env: &'b FlowBinderEnv,
    locals: rustc_hash::FxHashMap<String, SemanticNodeId>,
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
}

impl<'d, 'b> FlowEvaluator<'d, 'b> {
    /// Record a typed degradation (first-observed reason wins,
    /// deterministic in source order).
    fn record_degradation(&mut self, degradation: crate::semantic_query::FlowReturnDegradation) {
        self.degradation.get_or_insert(degradation);
    }

    /// Evaluate one region, returning its contributor nodes and whether
    /// the region falls through (mirrors the IR's reachability — this
    /// recomputes nothing, it only evaluates contributors).
    fn eval_region(
        &mut self,
        region: &crate::flow_ir::FlowIrRegion,
    ) -> (Result<Vec<SemanticNodeId>, FlowReturnFailure>, bool) {
        let mut contributors = Vec::new();
        for statement in region.statements.iter() {
            match statement {
                crate::flow_ir::FlowIrStatement::Return { argument } => {
                    let node =
                        match argument {
                            Some(expr) => match self.eval_expr(expr) {
                                Ok(node) => node,
                                Err(failure) => return (Err(failure), region.can_fall_through),
                            },
                            None => Some(self.dispatch.graph().intern_node(
                                SemanticNodeData::Primitive(PrimitiveKind::Undefined),
                            )),
                        };
                    // A hold is neither a contributor nor a failure.
                    if let Some(node) = node {
                        contributors.push(node);
                    }
                }
                crate::flow_ir::FlowIrStatement::If {
                    consequent,
                    alternate,
                    ..
                } => {
                    // Bindings are block-scoped: each `if` arm evaluates
                    // under its own local scope (the reaching definitions
                    // of a `const` inside an arm never escape it).
                    let saved = self.locals.clone();
                    let saved_degraded = self.degraded_locals.clone();
                    let (consequent_result, _) = self.eval_region(consequent);
                    let consequent_contributors = match consequent_result {
                        Ok(contributors) => contributors,
                        Err(failure) => return (Err(failure), region.can_fall_through),
                    };
                    contributors.extend(consequent_contributors);
                    self.locals = saved.clone();
                    self.degraded_locals = saved_degraded.clone();
                    if let Some(alternate) = alternate {
                        let (alternate_result, _) = self.eval_region(alternate);
                        let alternate_contributors = match alternate_result {
                            Ok(contributors) => contributors,
                            Err(failure) => return (Err(failure), region.can_fall_through),
                        };
                        contributors.extend(alternate_contributors);
                    }
                    self.locals = saved;
                    self.degraded_locals = saved_degraded;
                }
                crate::flow_ir::FlowIrStatement::Block(block) => {
                    // Bindings are block-scoped: a `const` inside a block
                    // never escapes it.
                    let saved = self.locals.clone();
                    let saved_degraded = self.degraded_locals.clone();
                    let (result, _) = self.eval_region(block);
                    let block_contributors = match result {
                        Ok(contributors) => contributors,
                        Err(failure) => return (Err(failure), region.can_fall_through),
                    };
                    contributors.extend(block_contributors);
                    self.locals = saved;
                    self.degraded_locals = saved_degraded;
                }
                crate::flow_ir::FlowIrStatement::Binding { name, init, .. } => {
                    if let Some(init) = init {
                        match self.eval_expr(init) {
                            Ok(Some(node)) => {
                                self.degraded_locals.remove(name.as_ref());
                                self.locals.insert(name.to_string(), node);
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
                                self.degraded_locals.insert(name.to_string());
                                self.locals.insert(
                                    name.to_string(),
                                    self.dispatch
                                        .graph()
                                        .intern_node(SemanticNodeData::Primitive(
                                            PrimitiveKind::Any,
                                        )),
                                );
                            }
                        }
                    }
                }
                crate::flow_ir::FlowIrStatement::Effect(_) => {}
                crate::flow_ir::FlowIrStatement::TransparentLoop => {}
                crate::flow_ir::FlowIrStatement::Unsupported(kind) => {
                    return (
                        Err(FlowReturnFailure::Unsupported(match kind {
                            crate::flow_ir::FlowIrUnsupported::Loop => FlowReturnUnsupported::Loop,
                            crate::flow_ir::FlowIrUnsupported::Switch => {
                                FlowReturnUnsupported::Switch
                            }
                            crate::flow_ir::FlowIrUnsupported::Try => FlowReturnUnsupported::Try,
                            crate::flow_ir::FlowIrUnsupported::Labeled => {
                                FlowReturnUnsupported::Labeled
                            }
                            crate::flow_ir::FlowIrUnsupported::Jump => FlowReturnUnsupported::Jump,
                            crate::flow_ir::FlowIrUnsupported::With => FlowReturnUnsupported::With,
                            crate::flow_ir::FlowIrUnsupported::ModuleDeclaration => {
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
    /// FRESH frame (the nested function's own params / locals; holds the
    /// nested evaluation met ride the outer frame's hold set), and compose
    /// the `Signature` node.
    fn eval_nested_function_signature(
        &mut self,
        nested_params: &[crate::flow_ir::FlowIrParam],
        type_parameters: &[crate::flow_ir::FlowIrTypeParam],
        body: &crate::flow_ir::FlowIrRegion,
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
        let nested_holds;
        let nested_degradation;
        let (contributors, _) = {
            let mut nested_evaluator = FlowEvaluator {
                dispatch: self.dispatch,
                key: self.key,
                canonical: self.canonical,
                owner: self.owner,
                params: &params,
                binder_env: &binder_env,
                locals: rustc_hash::FxHashMap::default(),
                holds: Vec::new(),
                degradation: None,
                degraded_locals: rustc_hash::FxHashSet::default(),
            };
            let outcome = nested_evaluator.eval_region(body);
            nested_holds = nested_evaluator.holds.clone();
            nested_degradation = nested_evaluator.degradation;
            self.holds.append(&mut nested_evaluator.holds);
            outcome
        };
        // A degraded nested body degrades the enclosing value that
        // embeds its signature.
        if let Some(degradation) = nested_degradation {
            self.record_degradation(degradation);
        }
        let contributors = contributors?;
        let result = self.dispatch.join_flow_return_contributors(
            contributors,
            can_fall_through,
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
        expr: &crate::flow_ir::FlowIrExpr,
    ) -> Result<Option<SemanticNodeId>, FlowReturnFailure> {
        let graph = self.dispatch.graph();
        match expr {
            crate::flow_ir::FlowIrExpr::Type(ty) => {
                // A fully lowered leaf: lowers under the function's OWN
                // binder environment (a body leaf referencing a root
                // binder keeps the binder, never an outer same-name
                // resolution).
                let mut substitutions: Vec<(Arc<str>, SemanticNodeId)> = Vec::new();
                Ok(Some(self.dispatch.shallow_lower_type_expr_with_context(
                    ty,
                    &self.binder_env.env,
                    &self.binder_env.scope,
                    &self.binder_env.name_resolution,
                    self.binder_env.scope_payload.as_ref(),
                    &self.binder_env.shadowing,
                    &mut substitutions,
                    crate::semantic_query::ProjectionReductionContext::structural_transit(),
                )))
            }
            crate::flow_ir::FlowIrExpr::Param { ordinal } => self
                .params
                .get(*ordinal as usize)
                .copied()
                .map(Some)
                .ok_or(FlowReturnFailure::Unresolved),
            crate::flow_ir::FlowIrExpr::Local { name } => {
                // Observing a binding whose initializer FAILED is the
                // `FailedBindingInitializer` degradation: the value is a
                // modeled `any`, not the initializer's real type. A plain
                // unbound local (hoisted `var` / TDZ forward reference)
                // stays the undegraded implicit-`any`.
                if self.degraded_locals.contains(name.as_ref()) {
                    self.record_degradation(
                        crate::semantic_query::FlowReturnDegradation::FailedBindingInitializer,
                    );
                }
                Ok(Some(
                    self.locals.get(name.as_ref()).copied().unwrap_or_else(|| {
                        graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any))
                    }),
                ))
            }
            crate::flow_ir::FlowIrExpr::Object { members } => {
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
            crate::flow_ir::FlowIrExpr::NestedFunctionValue {
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
            crate::flow_ir::FlowIrExpr::NestedCall(function) => {
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
            crate::flow_ir::FlowIrExpr::DirectCall(target) => {
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
            crate::flow_ir::FlowIrExpr::CallOnBinding { param, name } => {
                // A call on a function-typed binding: the call's value is
                // the binding's signature return. Calling an `any`-typed
                // or unbound binding is `any` EXACTLY (the implicit-`any`
                // call); calling a binding whose value is neither
                // callable nor `any` is the `NonCallableBinding`
                // DEGRADATION — a modeled `any`, not the real semantics.
                let node = match param {
                    Some(ordinal) => self.params.get(*ordinal as usize).copied(),
                    None => self.locals.get(name.as_ref()).copied(),
                };
                let Some(node) = node else {
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
            crate::flow_ir::FlowIrExpr::LocalFunctionShadow => {
                // A call to a hoisted nested function declaration: the
                // declaration shadows every outer same-name callee; exact
                // recovery of the nested declaration's own return is not
                // implemented — fail closed, never bind the outer callee.
                Err(FlowReturnFailure::Unresolved)
            }
            crate::flow_ir::FlowIrExpr::DirectSelfCall => {
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
            crate::flow_ir::FlowIrExpr::SymbolicCall(ty) => {
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
            crate::flow_ir::FlowIrExpr::Any => Ok(Some(
                graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Any)),
            )),
        }
    }
}
