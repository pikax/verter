//! Relation engine — the SOLE relation authority, riding
//! `execute(SemanticQueryKey::Relate)` on the cold-compute frame of the ONE
//! resolver (design `docs/arch/u2-relation-infer-design.md`).
//!
//! Every relation judgement — top-level consumer asks, conditional branch
//! selection, `Extract`/`Exclude` per-arm filtering, the oracle adapter,
//! AND every recursive sub-relation — re-enters the SAME full-key authority
//! [`ProjectSemanticDispatch::execute_relate`]. There is no second engine:
//! the deleted process-global TLS in-flight guard and the bare-pair
//! entry point are replaced by the per-transaction
//! [`super::relation_txn::CheckerTransaction`] reentry/assumption substrate,
//! and the family `execute` path's degenerate `Miss` arm plus the
//! execute-invisibility fence are gone — decided binary judgements admit
//! into the `Relate` family slot and warm-serve through the standard family
//! read.
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

use std::sync::Arc;

use rustc_hash::FxHashMap;

use super::relation_predicates::*;
use super::relation_txn::{
    CompletedSccMember, InferenceInfo, InferenceSession, InferenceSessionState, PendingSccMember,
    PendingVerdict, RelationStep, SessionCheckpoint, StrictFamilyConfig,
};
use super::ProjectSemanticDispatch;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, DeclIdentity, InferBinding, InferableParamSetId,
    InferenceCandidatePriority, InferenceContextKey, LiteralValue, NoInferMask, PrimitiveKind,
    ProjectionReductionContext, QueryError, QueryResult, RecursionOrBudgetCap, RelateKeyId,
    RelateMemoKey, RelationContext, RelationFailureCode, RelationKind, RelationOutcome,
    RelationPayload, RelationPolicy, RelationProof, RelationResult, SemanticNodeData,
    SemanticNodeId, SemanticQueryApi, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
    SubRelationPosition, SubRelationRef, SurfaceView, VariancePhase,
};

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

/// The shape of an in-scope conditional-`infer` pattern (RI-6 scope:
/// object property, tuple head/tail, array element, function inference).
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

/// The detected pattern payload: shape + every inferable parameter site.
#[derive(Debug, Clone)]
pub(crate) struct InferPatternInfo {
    pub(crate) shape: InferPatternShape,
    sites: Vec<InferParamSite>,
}

impl InferPatternInfo {
    /// The session-level candidate priority (the pattern's highest rung).
    fn candidate_priority(&self) -> InferenceCandidatePriority {
        let mut priority = InferenceCandidatePriority::Argument;
        for site in &self.sites {
            priority = match (priority, site.priority) {
                (InferenceCandidatePriority::NakedTypeParameter, _)
                | (_, InferenceCandidatePriority::NakedTypeParameter) => {
                    InferenceCandidatePriority::NakedTypeParameter
                }
                (InferenceCandidatePriority::ReturnType, _)
                | (_, InferenceCandidatePriority::ReturnType) => {
                    InferenceCandidatePriority::ReturnType
                }
                _ => InferenceCandidatePriority::Argument,
            };
        }
        priority
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
    /// Budget exhaustion — PUBLIC payload, never admitted (three-layer
    /// non-admission).
    BudgetExceeded(RelationPayload),
    /// No public value-domain form (Unknown / poisoned SCC) — `Miss`.
    Undecided,
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
            RelationStep::Unknown | RelationStep::BudgetExceeded(_) | RelationStep::Assumed => {
                RelationResult::Unknown
            }
        }
    }

    /// The full default relation identity for `(source, target)` under the
    /// live `R T L J` env (workspace-global host view — the established
    /// one-engine convention), the EMPTY canonical substitution, and the
    /// structural-transit reduction context. The strict-family
    /// configuration in force folds into BOTH the policy (the variance
    /// regime) and the `type_env_hash` (RI-10 key isolation).
    pub(crate) fn relate_key_for(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
    ) -> RelateMemoKey {
        let host = self.ctx.host_for_fact_tracer_install();
        let env = host.host_view_env_hashes();
        let strict = self.relation_strict_config();
        let context = RelationContext {
            resolve_env_hash: env.resolve_env_hash,
            type_env_hash: strict.mix_into_type_env_hash(env.type_env_hash),
            lib_env_hash: env.lib_env_hash,
            project_identity: host.host_view_project_identity().0,
            substitution: crate::semantic_query::SubstitutionCanonicalHash::empty(),
            projection_reduction: ProjectionReductionContext::structural_transit(),
        };
        let mut key = RelateMemoKey::assignable(source, target, context);
        key.policy = RelationPolicy {
            variance: strict.variance_policy(),
            ..RelationPolicy::default()
        };
        key
    }

    /// The normalized strict-family configuration in force on this host
    /// (RI-10). Production always runs the TS-strict default (matching the
    /// pre-activation engine); the per-host test knob flips individual
    /// flags for the paired strict-on/off fixture.
    pub(crate) fn relation_strict_config(&self) -> StrictFamilyConfig {
        let host = self.ctx.host_for_fact_tracer_install();
        let bits = host
            .relation_knobs
            .strict_family_relax_bits
            .load(std::sync::atomic::Ordering::Relaxed);
        StrictFamilyConfig {
            strict_null_checks: bits & 0b01 == 0,
            strict_function_types: bits & 0b10 == 0,
        }
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
        let graph = self.graph();
        graph.record_relation_check();
        // Binding-producing upgrade: an in-scope `infer` pattern on the
        // target (with no explicit inference context) opens this judgement
        // under the pattern's GENERATED session-setup fingerprint (RI-6).
        // The fingerprint is a pure function of the pattern — the setup is
        // frozen at session open, so the admission identity is well
        // defined up front (design §3.3 for this subset).
        let key = self.relation_key_with_inference(key);
        // (1) Reentry intercept.
        {
            let mut txn = self.relation_txn.borrow_mut();
            if let Some(idx) = txn.reentry().find(&key) {
                txn.assumptions.record_assumption(idx);
                return RelationStep::Assumed;
            }
        }
        // (2) Warm read (generation-gated, carrier-validated).
        if let Some(payload) = graph.get_relation_payload(self.ctx, &key) {
            return relation_step_from_payload(&payload);
        }
        // (3) Cold compute.
        if self.relation_txn.borrow().reentry().is_empty() {
            self.execute_relate_root(key)
        } else {
            self.execute_relate_inline(key)
        }
    }

    /// The machinery ROOT path: the full family singleflight
    /// (`execute(Relate)` → warm fast path / cross-thread join / traced
    /// cold build / publish). After a published cold build, drain the
    /// SCC-closed member batch onto the root's SCC-union carrier (design
    /// §2.3 step 4 R-a batched admission).
    fn execute_relate_root(&self, key: RelateMemoKey) -> RelationStep {
        let read = self.execute_via_cold_build_helper(key.to_query_key());
        let (step, published) = match read.value {
            QueryResult::Value(SemanticQueryValue::Relation(payload)) => {
                let published = !read.cache_suppress;
                (relation_step_from_payload(&payload), published)
            }
            // An undecided judgement surfaces `Error(Miss)` — loud, never a
            // fallback, never admitted.
            _ => (RelationStep::Unknown, false),
        };
        if published {
            self.relation_drain_completed_members(&key);
        } else {
            // ReturnOnly exit (poisoned SCC / budget / undecided): the
            // deferred batch releases WITHOUT publish — no entry, no fact
            // signature, no backfill, no reverse-index metadata.
            self.relation_txn.borrow_mut().completed_members.clear();
        }
        step
    }

    /// A nested sub-relation's INLINE cold compute: push a frame, run the
    /// reducer, close the frame through the SCC discharge. The publish is
    /// NEVER direct — it is batched at this frame's SCC close and drained
    /// by the machinery root onto the SCC-union carrier.
    fn execute_relate_inline(&self, key: RelateMemoKey) -> RelationStep {
        let idx = self.relation_frame_open(&key);
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
        let fence = self.project_generation_signature();
        let observed_self_roots = self.observed_self_roots_from_nodes([key.source, key.target]);
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
        let idx = self.relation_frame_open(key);
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(key, &mut bindings);
        match self.relation_frame_close_root(idx, verdict, bindings) {
            RootClose::Decided(payload) => {
                crate::project_semantic_dispatch::walk::QueryBuildOutput::from((
                    QueryResult::Value(SemanticQueryValue::Relation(payload)),
                    fence,
                ))
                .with_observed_self_roots(observed_self_roots)
            }
            RootClose::BudgetExceeded(payload) => {
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
    fn relation_frame_open(&self, key: &RelateMemoKey) -> usize {
        // Snapshot the strict config + pattern BEFORE taking the borrow —
        // `relation_pattern_info` re-borrows the transaction (its
        // per-target cache lives there).
        let strict = self.relation_strict_config();
        let wants_session = key.inference_context.is_some()
            && self.relation_txn.borrow().active_session().is_none();
        let pattern = if wants_session {
            self.relation_pattern_info(key.target)
        } else {
            None
        };
        let mut txn = self.relation_txn.borrow_mut();
        if txn.reentry().is_empty() {
            // Re-snapshot at every relation ROOT so the behavioral branch
            // and the key's strict fold can never diverge (the key reads
            // the live config; the reducer reads this snapshot).
            txn.strict = Some(strict);
        }
        let watermark = txn.scc_ledger.pending_len();
        let idx = txn.reentry_mut().push(key.clone(), watermark);
        if wants_session {
            if let Some(pattern) = pattern {
                let session_id = txn.alloc_session_id();
                let session = InferenceSession {
                    id: session_id,
                    inferable_params: pattern.sites.iter().map(|s| s.node).collect(),
                    variance_phase: VariancePhase::Covariant,
                    candidate_priority: pattern.candidate_priority(),
                    no_infer_mask: NoInferMask::empty(),
                    contextual_inference_mode: ContextualInferenceMode::None,
                    infos: pattern
                        .sites
                        .iter()
                        .map(|site| InferenceInfo {
                            param_node: site.node,
                            param_name: Arc::clone(&site.name),
                            priority: site.priority,
                            const_param_policy: ConstParamPolicy::NonConst,
                            candidates: Vec::new(),
                        })
                        .collect(),
                    state: InferenceSessionState::InProgress,
                };
                // The R-b invariant, asserted at open: the session's
                // GENERATED setup projection reproduces the key's
                // fingerprint exactly (a hand-maintained projection would
                // silently drift — the same diff the
                // `inference_context_key_projects_every_session_setup_axis`
                // guard enforces structurally).
                debug_assert_eq!(
                    Some(session.context_key()),
                    key.inference_context,
                    "the opened session's generated InferenceContextKey must reproduce the relation key's fingerprint"
                );
                txn.sessions.push(session);
                txn.reentry_mut().note_opened_session(idx, session_id);
            }
        }
        idx
    }

    /// Close an INLINE frame: fixate an owned session, classify the pop
    /// (SCC-root vs provisional member), and run the SCC discharge at the
    /// root. Returns the caller-return step (PROVISIONAL for an
    /// unpublished member — never itself the published payload).
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
        let mut session_bindings: Option<Arc<[InferBinding]>> = None;
        let mut session_abandoned = false;
        let (frame, self_cycle) = {
            let mut txn = self.relation_txn.borrow_mut();
            let frame = txn.reentry_mut().pop();
            let self_cycle = frame.assumption_targets.contains(&idx);
            if let Some(sid) = frame.opened_session {
                if let Some(position) = txn.sessions.iter().position(|s| s.id == sid) {
                    if frame.budget_cap.is_some() {
                        txn.sessions[position].state = InferenceSessionState::Abandoned;
                        session_abandoned = true;
                    } else {
                        let combine = |nodes: &[SemanticNodeId], variance: VariancePhase| {
                            self.relation_combine_candidates(nodes, variance)
                        };
                        let mut session = txn.sessions.remove(position);
                        let fixed = session.fixate(combine);
                        let state = session.state;
                        txn.sessions.insert(position, session);
                        match state {
                            InferenceSessionState::CompletedDeterministic => {
                                session_bindings = Some(Arc::from(fixed.into_boxed_slice()));
                            }
                            InferenceSessionState::Abandoned => session_abandoned = true,
                            InferenceSessionState::InProgress => unreachable!(
                                "fixate always resolves the session state before the frame closes"
                            ),
                        }
                    }
                }
            }
            (frame, self_cycle)
        };
        let pending =
            pending_verdict_of(&verdict, &frame.budget_cap, &mut session_bindings, bindings);
        let is_scc_root = match frame.min_open_target {
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
            let mut txn = self.relation_txn.borrow_mut();
            txn.assumptions.propagate_lowlink(frame.min_open_target);
            if let Some(sid) = frame.opened_session {
                txn.session_admission.defer(sid, frame.key.clone());
            }
            txn.scc_ledger.deposit(PendingSccMember {
                key: frame.key,
                verdict: pending,
                session_delta: frame.session_delta,
                opened_session: frame.opened_session,
            });
            return FramePop::Provisional(step);
        }

        // ── SCC close at this root (design §2.3 step 3) ──────────────
        // Drain by the frame's push-time watermark, NEVER by stack index —
        // indices recycle after pops, and a recycled index would let this
        // close steal a pending member of a still-open outer SCC (which
        // would then publish a stale provisional verdict).
        let members = self
            .relation_txn
            .borrow_mut()
            .scc_ledger
            .drain_scc(frame.pending_watermark);
        let cyclic = !members.is_empty() || self_cycle;
        // Row 3 batched poison: ANY Unknown / budget / abandoned-session
        // edge anywhere in the component routes the WHOLE SCC through
        // ReturnOnly — nothing publishes.
        let budget_cap = frame.budget_cap.or_else(|| {
            members.iter().find_map(|m| match &m.verdict {
                PendingVerdict::BudgetExceeded(cap) => Some(*cap),
                _ => None,
            })
        });
        let poisoned = session_abandoned
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

        // Clean close: every member is decided. Discharge verdicts — a
        // member recorded POSITIVE that consumed assumptions re-discharges
        // against the converged state when ANY member closed NEGATIVE
        // (the collapsed-back-edge case, design §2.3 step 3); a
        // non-stable re-discharge (a flip to Unknown) releases the whole
        // batch without publish. Each record carries the member's
        // session-delta flag (row 7: a session-local delta never
        // publishes) and its opened-session token (a binding member
        // admits only through its session's `SessionAdmissionLedger`
        // drain below).
        let any_negative = matches!(pending, PendingVerdict::NotAssignable)
            || members
                .iter()
                .any(|m| matches!(m.verdict, PendingVerdict::NotAssignable));
        let mut discharged: Vec<(
            RelateMemoKey,
            PendingVerdict,
            bool,
            bool,
            Option<super::relation_txn::SessionId>,
        )> = Vec::new();
        let self_assumptive = !frame.assumption_targets.is_empty();
        if let Some(sid) = frame.opened_session {
            self.relation_txn
                .borrow_mut()
                .session_admission
                .defer(sid, frame.key.clone());
        }
        discharged.push((
            frame.key.clone(),
            pending,
            self_assumptive,
            frame.session_delta,
            frame.opened_session,
        ));
        for member in members {
            discharged.push((
                member.key,
                member.verdict,
                true,
                member.session_delta,
                member.opened_session,
            ));
        }
        // The session-close drain gate (design §2.3 step 4): a binding
        // member publishes ONLY when its session's drain returns it AND
        // the session reached `CompletedDeterministic`; an `Abandoned`
        // session — or a ledger that lost the member — releases the whole
        // batch WITHOUT publish.
        {
            let mut txn = self.relation_txn.borrow_mut();
            let mut ledger_ok = true;
            for (key, _, _, _, opened_session) in &discharged {
                let Some(sid) = opened_session else {
                    continue;
                };
                let drained = txn.session_admission.drain(*sid);
                let session_ok = txn
                    .sessions
                    .iter()
                    .find(|s| s.id == *sid)
                    .is_some_and(|s| s.state == InferenceSessionState::CompletedDeterministic);
                if !session_ok || !drained.iter().any(|k| k == key) {
                    ledger_ok = false;
                    break;
                }
            }
            if !ledger_ok {
                drop(txn);
                return FramePop::RootClose(RootClose::Undecided);
            }
        }
        if any_negative {
            let mut substitution: FxHashMap<RelateMemoKey, RelationStep> = discharged
                .iter()
                .map(|(key, verdict, _, _, _)| (key.clone(), relation_step_from_pending(verdict)))
                .collect();
            // Bottom-up over the condensation: re-discharge the POSITIVE
            // assumption-consuming members DEEPEST-FIRST so a shallower
            // member re-runs against the FINAL deeper verdicts. Layout:
            // `discharged[0]` is the SCC root (shallowest); `discharged[1..]`
            // are the drained members in POP order — deepest-popped first —
            // so deepest-first is positions `1..len` in order, with the
            // root LAST. (The reversed scan froze a shallow member against
            // a stale provisional deep `Assignable` before the deep member
            // flipped on its collapsed back-edge.)
            let order: Vec<usize> = (1..discharged.len()).chain(std::iter::once(0)).collect();
            for position in order {
                let (key, verdict, assumptive, _, _) = &discharged[position];
                if !*assumptive || !matches!(verdict, PendingVerdict::Assignable { .. }) {
                    continue;
                }
                let key = key.clone();
                let rerun = self.relation_redischarge(&key, &substitution);
                match rerun {
                    PendingVerdict::Unknown | PendingVerdict::BudgetExceeded(_) => {
                        // Non-stable re-discharge ⇒ release the whole batch
                        // WITHOUT publish (joiners recompute).
                        return FramePop::RootClose(RootClose::Undecided);
                    }
                    stable => {
                        substitution.insert(key.clone(), relation_step_from_pending(&stable));
                        discharged[position].1 = stable;
                    }
                }
            }
        }

        // Publish routing (design §2.3 step 4): decided members queue for
        // the root's batched publish onto the SCC-union carrier; a
        // session-local delta (row 7) never publishes.
        let scc_keys: Arc<[RelateKeyId]> = if cyclic {
            let keys: Vec<RelateKeyId> = discharged
                .iter()
                .map(|(key, _, _, _, _)| self.graph().intern_relate_key(key.clone()))
                .collect();
            Arc::from(keys.into_boxed_slice())
        } else {
            Arc::from(Vec::<RelateKeyId>::new().into_boxed_slice())
        };
        let mut self_publish: Option<RelationPayload> = None;
        let mut self_step: Option<RelationStep> = None;
        let mut completed: Vec<CompletedSccMember> = Vec::new();
        for (position, (key, verdict, _, session_delta, _)) in discharged.into_iter().enumerate() {
            let is_self = position == 0;
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
                    completed.push(CompletedSccMember { key, payload });
                }
            } else if !session_delta {
                completed.push(CompletedSccMember { key, payload });
            }
        }
        self.relation_txn
            .borrow_mut()
            .completed_members
            .extend(completed);
        if let Some(step) = self_step {
            return FramePop::Provisional(step);
        }
        FramePop::RootClose(RootClose::Decided(
            self_publish.expect("the machinery root always produces its own payload"),
        ))
    }

    /// Re-discharge ONE member of a negatively-closed SCC against the
    /// converged state (design §2.3 step 4): the member's cold compute
    /// re-runs through the same `execute(Relate)` dispatch with the SCC's
    /// discharged verdicts as the substitution table, so a stale
    /// SCC-close snapshot is impossible by construction.
    fn relation_redischarge(
        &self,
        key: &RelateMemoKey,
        substitution: &FxHashMap<RelateMemoKey, RelationStep>,
    ) -> PendingVerdict {
        self.relation_txn.borrow_mut().discharge_substitution = substitution
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let mut bindings: Vec<InferBinding> = Vec::new();
        let verdict = self.reduce_relation(key, &mut bindings);
        self.relation_txn
            .borrow_mut()
            .discharge_substitution
            .clear();
        match verdict {
            RelationResult::Assignable { .. } => PendingVerdict::Assignable {
                bindings: Arc::from(bindings.into_boxed_slice()),
            },
            RelationResult::NotAssignable => PendingVerdict::NotAssignable,
            RelationResult::Unknown => PendingVerdict::Unknown,
        }
    }

    /// Fixation combinator (design §4.2 candidate combination): covariant
    /// candidates union (canonicalized), contravariant candidates
    /// intersect, a single candidate binds directly, and an unfixed
    /// parameter deterministically defaults to `unknown`.
    fn relation_combine_candidates(
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
                dedup.sort_by_key(|id| id.0);
                dedup.dedup();
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

    /// Drain the SCC-closed member batch onto the root's published
    /// SCC-union carrier (design §2.3: the published fact set is the UNION
    /// of all SCC members' observed facts, never the bare per-member set).
    fn relation_drain_completed_members(&self, root_key: &RelateMemoKey) {
        let members: Vec<CompletedSccMember> = {
            let mut txn = self.relation_txn.borrow_mut();
            std::mem::take(&mut txn.completed_members)
        };
        if members.is_empty() {
            return;
        }
        let graph = self.graph();
        let Some(carrier) = graph.relation_published_carrier(root_key) else {
            return;
        };
        for member in members {
            // Self-roots: the root pair's origins UNION the member pair's
            // origins (design §1.4: source, target, and every declaration
            // visited during structural descent).
            let member_roots =
                self.observed_self_roots_from_nodes([member.key.source, member.key.target]);
            let mut canonicals: Vec<Arc<str>> = carrier.self_root_canonicals.to_vec();
            for (canonical, _) in &member_roots {
                if !canonicals.iter().any(|root| root == canonical) {
                    canonicals.push(Arc::clone(canonical));
                }
            }
            graph.publish_relation_member(
                Some(self.ctx),
                member.key,
                member.payload,
                carrier.read_set_signature.clone(),
                Arc::from(canonicals.into_boxed_slice()),
                carrier.validated_at_generation,
            );
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // Inference pattern detection + session plumbing
    // ──────────────────────────────────────────────────────────────────

    /// Upgrade a plain relation key with the GENERATED session-setup
    /// fingerprint when the target carries an in-scope `infer` pattern
    /// (RI-6 / R-b). The fingerprint projects the pattern-determined
    /// session setup; it is frozen at session open, so this upgrade is a
    /// pure function of the pattern.
    fn relation_key_with_inference(&self, mut key: RelateMemoKey) -> RelateMemoKey {
        if key.inference_context.is_some() || key.relation != RelationKind::Assignable {
            return key;
        }
        // Only upgrade when a binding could actually occur: the pattern
        // scan is cached per target node on the transaction.
        let Some(pattern) = self.relation_pattern_info(key.target) else {
            return key;
        };
        key.inference_context = Some(self.inference_context_key_for_pattern(&pattern));
        key
    }

    /// The GENERATED [`InferenceContextKey`] projection of a pattern's
    /// session setup (R-b): one line per setup axis.
    fn inference_context_key_for_pattern(&self, pattern: &InferPatternInfo) -> InferenceContextKey {
        InferenceContextKey {
            inferable_params: InferableParamSetId::new(Arc::from(
                pattern
                    .sites
                    .iter()
                    .map(|s| s.node)
                    .collect::<Vec<_>>()
                    .into_boxed_slice(),
            )),
            variance_phase: VariancePhase::Covariant,
            candidate_priority: pattern.candidate_priority(),
            no_infer_mask: NoInferMask::empty(),
            const_param_policy: ConstParamPolicy::NonConst,
            contextual_inference_mode: ContextualInferenceMode::None,
        }
    }

    /// Detect an in-scope conditional-`infer` pattern on `target` (RI-6
    /// scope: bare / object property / tuple head-tail / function
    /// positions — DIRECT `Infer` occupants only; deeper nesting is out
    /// of scope and stays deferred). Cached per target node on the
    /// transaction.
    fn relation_pattern_info(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        if let Some(cached) = self.relation_txn.borrow().pattern_cache.get(&target) {
            return cached.clone();
        }
        let computed = self.relation_pattern_info_uncached(target);
        self.relation_txn
            .borrow_mut()
            .pattern_cache
            .insert(target, computed.clone());
        computed
    }

    fn relation_pattern_info_uncached(&self, target: SemanticNodeId) -> Option<InferPatternInfo> {
        let graph = self.graph();
        match graph.node_data(target).as_deref() {
            Some(SemanticNodeData::Infer { name }) => Some(InferPatternInfo {
                shape: InferPatternShape::Bare,
                sites: vec![InferParamSite {
                    node: target,
                    name: Arc::clone(name),
                    priority: InferenceCandidatePriority::NakedTypeParameter,
                }],
            }),
            Some(SemanticNodeData::Object(view)) => {
                let mut sites = Vec::new();
                for member in view.members.iter() {
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(member.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: member.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty()).then_some(InferPatternInfo {
                    shape: InferPatternShape::ObjectProps,
                    sites,
                })
            }
            Some(SemanticNodeData::Tuple { elements, .. }) => {
                let mut sites = Vec::new();
                for element in elements.iter() {
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(element.value).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: element.value,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                (!sites.is_empty()).then_some(InferPatternInfo {
                    shape: InferPatternShape::TupleHeadTail,
                    sites,
                })
            }
            Some(SemanticNodeData::Array { element, .. }) => {
                if let Some(SemanticNodeData::Infer { name }) = graph.node_data(*element).as_deref()
                {
                    Some(InferPatternInfo {
                        shape: InferPatternShape::ArrayElement,
                        sites: vec![InferParamSite {
                            node: *element,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        }],
                    })
                } else {
                    None
                }
            }
            Some(SemanticNodeData::Signature {
                params,
                return_type,
                ..
            }) => {
                let mut sites = Vec::new();
                for param in params.iter() {
                    if let Some(SemanticNodeData::Infer { name }) =
                        graph.node_data(param.ty).as_deref()
                    {
                        sites.push(InferParamSite {
                            node: param.ty,
                            name: Arc::clone(name),
                            priority: InferenceCandidatePriority::Argument,
                        });
                    }
                }
                if let Some(SemanticNodeData::Infer { name }) =
                    graph.node_data(*return_type).as_deref()
                {
                    sites.push(InferParamSite {
                        node: *return_type,
                        name: Arc::clone(name),
                        priority: InferenceCandidatePriority::ReturnType,
                    });
                }
                (!sites.is_empty()).then_some(InferPatternInfo {
                    shape: InferPatternShape::Function,
                    sites,
                })
            }
            _ => None,
        }
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
        bound: SemanticNodeId,
        position: InferPosition,
    ) {
        let (priority, variance) = match position {
            InferPosition::Covariant => (
                InferenceCandidatePriority::Argument,
                VariancePhase::Covariant,
            ),
            InferPosition::ContravariantParam => (
                InferenceCandidatePriority::Argument,
                VariancePhase::Contravariant,
            ),
            InferPosition::Return => (
                InferenceCandidatePriority::ReturnType,
                VariancePhase::Covariant,
            ),
        };
        let mut txn = self.relation_txn.borrow_mut();
        let active_id = txn.active_session().map(|session| session.id);
        if let Some(session) = txn.active_session_mut() {
            session.deposit(param_node, bound, priority, variance);
        }
        if let Some(depth) = txn.reentry().depth().checked_sub(1) {
            let top_opened_session = txn
                .reentry()
                .frame_opened_session(depth)
                .is_some_and(|opened| Some(opened) == active_id);
            if !top_opened_session {
                txn.reentry_mut().note_session_delta(depth);
            }
        }
    }

    /// Whether an inference session is currently active.
    fn relation_session_active(&self) -> bool {
        self.relation_txn.borrow().active_session().is_some()
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
        self.relation_txn
            .borrow()
            .active_session()
            .map(InferenceSession::checkpoint)
    }

    /// Roll the ACTIVE session's deposits back to `checkpoint` (no-op when
    /// no session is active or no checkpoint was taken).
    fn relation_session_rollback(&self, checkpoint: &Option<SessionCheckpoint>) {
        if let Some(checkpoint) = checkpoint {
            if let Some(session) = self.relation_txn.borrow_mut().active_session_mut() {
                session.rollback_to(checkpoint);
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
    // The lattice adapter: full-key sub-relations from the reducer
    // ──────────────────────────────────────────────────────────────────

    /// The full identity of a sub-relation inside the current frame:
    /// inherits the top frame's relation kind / policy / freshness / env
    /// context, with NO inference context (a pure member judgement is
    /// session-independent; binding deposits route through the session,
    /// never the key).
    fn relation_sub_key(&self, source: SemanticNodeId, target: SemanticNodeId) -> RelateMemoKey {
        let txn = self.relation_txn.borrow();
        match txn.reentry().frames_top_key() {
            Some(top) => RelateMemoKey {
                source,
                target,
                relation: top.relation,
                policy: top.policy,
                source_freshness: top.source_freshness,
                inference_context: None,
                context: top.context,
            },
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
        let graph = self.graph();
        if self.relation_session_active() {
            match position {
                InferPosition::Covariant | InferPosition::Return => {
                    if let Some(SemanticNodeData::Infer { .. }) = graph.node_data(target).as_deref()
                    {
                        self.relation_deposit(target, source, position);
                        return assignable(bindings);
                    }
                }
                InferPosition::ContravariantParam => {
                    if let Some(SemanticNodeData::Infer { .. }) = graph.node_data(source).as_deref()
                    {
                        self.relation_deposit(source, target, position);
                        return assignable(bindings);
                    }
                }
            }
        }
        // The discharge substitution rail (re-discharge, design §2.3 step
        // 4): a member of a negatively-closed SCC re-runs against the
        // converged verdicts.
        {
            let txn = self.relation_txn.borrow();
            if !txn.discharge_substitution.is_empty() {
                let key = self.relation_sub_key(source, target);
                if let Some(step) = txn.discharge_substitution.get(&key) {
                    return match step {
                        RelationStep::Assignable { .. } => assignable(bindings),
                        RelationStep::NotAssignable => RelationResult::NotAssignable,
                        _ => RelationResult::Unknown,
                    };
                }
            }
        }
        let key = self.relation_sub_key(source, target);
        match self.execute_relate(key) {
            RelationStep::Assumed => {
                // The coinductive hypothesis: assumed to hold; the edge is
                // recorded on the frame.
                assignable(bindings)
            }
            RelationStep::Assignable { bindings: sub } => {
                for binding in sub.iter() {
                    if !bindings.iter().any(|b| b.name == binding.name) {
                        bindings.push(binding.clone());
                    }
                }
                assignable(bindings)
            }
            RelationStep::NotAssignable => RelationResult::NotAssignable,
            RelationStep::Unknown => RelationResult::Unknown,
            RelationStep::BudgetExceeded(cap) => {
                let mut txn = self.relation_txn.borrow_mut();
                if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                    txn.reentry_mut().note_budget_edge(depth, cap);
                }
                RelationResult::Unknown
            }
        }
    }

    // ──────────────────────────────────────────────────────────────────
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
        // Axis refusal: the reducer implements the ASSIGNABILITY relation
        // under a regular (widened) source with no excess-property policy.
        // A key on any not-yet-implemented axis (`Identity` / `Subtype` /
        // `StrictSubtype` / `Comparable`, a `Fresh` source, an
        // excess-property or non-default overload-selection policy) must
        // REFUSE — undecided, ReturnOnly, zero admission — never route the
        // ask through the assignability lattice (an `Identity` ask through
        // the `(_, unknown) => Assignable` arm would publish a false
        // verdict). Both strict variance regimes ARE implemented (RI-10).
        if key.relation != RelationKind::Assignable
            || key.source_freshness != crate::semantic_query::FreshnessKey::Regular
            || key.policy.excess_property_check
            || key.policy.overload_selection != crate::semantic_query::OverloadSelectionPolicy::All
        {
            return RelationResult::Unknown;
        }
        // Test-only forced budget knob (D6): trips the work budget on the
        // first driver pass so the typed `BudgetExceeded` outcome and its
        // three-layer non-admission are exercised deterministically.
        let host = self.ctx.host_for_fact_tracer_install();
        if host
            .relation_knobs
            .force_budget_exhaustion
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            let cap = RecursionOrBudgetCap {
                kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
                limit: 0,
            };
            let mut txn = self.relation_txn.borrow_mut();
            if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                txn.reentry_mut().note_budget_edge(depth, cap);
            }
            return RelationResult::Unknown;
        }
        // The binding root's bare-`Infer` arm: `check extends infer X`
        // binds `X := check` for ANY check (the pre-relation semantics,
        // now through the session).
        if self.relation_session_active() {
            if let Some(SemanticNodeData::Infer { .. }) =
                self.graph().node_data(key.target).as_deref()
            {
                self.relation_deposit(key.target, key.source, InferPosition::Covariant);
                return assignable(bindings);
            }
        }
        match self.shallow_relation_check(key.source, key.target) {
            ShallowRelation::Assignable => return assignable(bindings),
            ShallowRelation::NotAssignable => return RelationResult::NotAssignable,
            ShallowRelation::Unknown => {}
        }
        self.decide_relation_with_dispatch(key.source, key.target, bindings)
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
        if source == target {
            return ShallowRelation::Assignable;
        }
        let graph = self.graph();
        let Some(source_data) = graph.node_data(source) else {
            return ShallowRelation::Unknown;
        };
        let Some(target_data) = graph.node_data(target) else {
            return ShallowRelation::Unknown;
        };
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
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => ShallowRelation::Assignable,
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => ShallowRelation::Assignable,
            (_, SemanticNodeData::Primitive(PrimitiveKind::Never)) => {
                ShallowRelation::NotAssignable
            }
            // Strict-family behavioral branch (RI-10), mirrored from the
            // structural reducer's arm order: with `strictNullChecks` OFF,
            // `null` / `undefined` are assignable to every remaining target
            // (`never` already rejected above).
            (SemanticNodeData::Primitive(PrimitiveKind::Null | PrimitiveKind::Undefined), _)
                if !self
                    .relation_txn
                    .borrow()
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
                        return self.decide_relation(materialised, target, bindings);
                    }
                }
            }
        }
        self.decide_relation(source, target, bindings)
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
    ) -> RelationResult {
        if source == target {
            return assignable(bindings);
        }
        let graph = self.graph();
        let budget_limit: u64 = (graph.node_count() as u64).saturating_mul(10).max(4096);
        let mut budget_used: u64 = 0;
        let mut work: Vec<RelateWork> = Vec::new();
        let mut results: Vec<RelationResult> = Vec::new();
        work.push(RelateWork::Eval(source, target));
        while let Some(item) = work.pop() {
            budget_used = budget_used.saturating_add(1);
            if budget_used > budget_limit {
                let cap = RecursionOrBudgetCap {
                    kind: crate::semantic_query::BudgetExceededKind::RelationBudget,
                    limit: budget_limit as u32,
                };
                let mut txn = self.relation_txn.borrow_mut();
                if let Some(depth) = txn.reentry().depth().checked_sub(1) {
                    txn.reentry_mut().note_budget_edge(depth, cap);
                }
                return RelationResult::Unknown;
            }
            match item {
                RelateWork::Eval(s, t) => {
                    self.expand_pair(s, t, bindings, &mut work, &mut results);
                }
                RelateWork::ReduceAnd(n) => {
                    let combined = reduce_and_from_results(&mut results, n);
                    results.push(combined);
                }
                RelateWork::ReduceOr(n) => {
                    let combined = reduce_or_from_results(&mut results, n);
                    results.push(combined);
                }
            }
        }
        results.pop().unwrap_or(RelationResult::Unknown)
    }

    /// Expand a single relate pair into direct result(s) or sub-work
    /// items. Pushes exactly one net result onto `results` by the time all
    /// sub-work drains.
    fn expand_pair(
        &self,
        source: SemanticNodeId,
        target: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
        work: &mut Vec<RelateWork>,
        results: &mut Vec<RelationResult>,
    ) {
        if source == target {
            results.push(assignable(bindings));
            return;
        }
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
            work.push(RelateWork::Eval(inner, target));
            return;
        }
        if let SemanticNodeData::Alias(inner) = &*target_data {
            let inner = *inner;
            drop(source_data);
            drop(target_data);
            work.push(RelateWork::Eval(source, inner));
            return;
        }

        // ── MergedDecl: reduce to its peer-merged object surface ───────
        if let SemanticNodeData::MergedDecl { contributors } = &*source_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(RelateWork::Eval(merged, target));
            return;
        }
        if let SemanticNodeData::MergedDecl { contributors } = &*target_data {
            let contributors = contributors.clone();
            drop(source_data);
            drop(target_data);
            let merged = super::walk::reduce_merged_decl_with_graph(graph, &contributors);
            work.push(RelateWork::Eval(source, merged));
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
            (_, SemanticNodeData::Primitive(PrimitiveKind::Unknown)) => {
                results.push(assignable(bindings));
                return;
            }
            (SemanticNodeData::Primitive(PrimitiveKind::Any), _) => {
                results.push(assignable(bindings));
                return;
            }
            (_, SemanticNodeData::Primitive(PrimitiveKind::Any)) => {
                results.push(assignable(bindings));
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
                .relation_txn
                .borrow()
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
        }

        // ── Deferred shells on either side → Unknown ───────────────────
        if is_deferred(&source_data) || is_deferred(&target_data) {
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

        // ── Type parameters: Unknown unless identical ──────────────────
        if matches!(&*source_data, SemanticNodeData::TypeParam { .. })
            || matches!(&*target_data, SemanticNodeData::TypeParam { .. })
        {
            results.push(RelationResult::Unknown);
            return;
        }

        // ── Infer: bind through the active session (RI-6); without one,
        //    defensive Unknown. ──────────────────────────────────────────
        if let SemanticNodeData::Infer { .. } = &*target_data {
            if self.relation_session_active() {
                self.relation_deposit(target, source, InferPosition::Covariant);
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
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, |m| (*m, target));
            return;
        }
        if let SemanticNodeData::Union(members) = &*target_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_or(work, results, &members, |m| (source, *m));
            return;
        }
        if let SemanticNodeData::Intersection(members) = &*source_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_or(work, results, &members, |m| (*m, target));
            return;
        }
        if let SemanticNodeData::Intersection(members) = &*target_data {
            let members = Arc::clone(members);
            drop(source_data);
            drop(target_data);
            distribute_and(work, results, &members, |m| (source, *m));
            return;
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
                // An `Infer` element position under an active session is
                // covariant-only: the forward arm's deposit IS the binding,
                // and an invariant reverse arm against the `Infer` node
                // (`Infer ≤ element`) is undecidable and would defer the
                // whole pattern. Non-`Infer` mutable arrays KEEP the
                // invariant bidirectional check.
                let infer_element = self.relation_session_active()
                    && matches!(
                        graph.node_data(t_el).as_deref(),
                        Some(SemanticNodeData::Infer { .. })
                    );
                if t_ro || s_ro || infer_element {
                    work.push(RelateWork::Eval(s_el, t_el));
                } else {
                    let forward = vec![
                        RelateWork::Eval(s_el, t_el),
                        RelateWork::Eval(t_el, s_el),
                        RelateWork::ReduceAnd(2),
                    ];
                    push_forward_work(work, forward);
                }
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
                let required_target_len = t_els.iter().filter(|e| !e.optional && !e.rest).count();
                if s_els.len() < required_target_len {
                    results.push(RelationResult::NotAssignable);
                    return;
                }
                // Tuple-inference rest tail (RI-6 in-scope): a trailing
                // `...infer Rest` element binds the remaining source
                // elements as a tuple through the active session.
                let session_rest = if self.relation_session_active() {
                    t_els.iter().position(|e| {
                        e.rest
                            && matches!(
                                graph.node_data(e.value).as_deref(),
                                Some(SemanticNodeData::Infer { .. })
                            )
                    })
                } else {
                    None
                };
                let pair_count = s_els.len().min(t_els.len());
                if let Some(rest_index) = session_rest {
                    let remainder: Vec<crate::semantic_query::TupleElement> = s_els
                        .iter()
                        .skip(rest_index)
                        .map(|e| crate::semantic_query::TupleElement {
                            label: None,
                            value: e.value,
                            optional: false,
                            rest: false,
                        })
                        .collect();
                    let remainder_tuple = graph.intern_node(SemanticNodeData::Tuple {
                        elements: Arc::from(remainder.into_boxed_slice()),
                        readonly: false,
                    });
                    self.relation_deposit(
                        t_els[rest_index].value,
                        remainder_tuple,
                        InferPosition::Covariant,
                    );
                }
                if pair_count == 0 {
                    results.push(assignable(bindings));
                    return;
                }
                let mut forward: Vec<RelateWork> = Vec::new();
                let mut pairs_evaluated: u32 = 0;
                for (position, (s_el, t_el)) in
                    s_els.iter().zip(t_els.iter()).take(pair_count).enumerate()
                {
                    // A rest-tail `Infer` element binds through the
                    // remainder deposit above — skip its pairwise eval.
                    if session_rest == Some(position) {
                        continue;
                    }
                    pairs_evaluated += 1;
                    // Same covariant-only rule as the Array arm: a direct
                    // `Infer` element position under an active session
                    // binds through the forward deposit; the invariant
                    // reverse against the `Infer` node would defer it.
                    let infer_element = self.relation_session_active()
                        && matches!(
                            graph.node_data(t_el.value).as_deref(),
                            Some(SemanticNodeData::Infer { .. })
                        );
                    if s_ro || t_ro || infer_element {
                        forward.push(RelateWork::Eval(s_el.value, t_el.value));
                    } else {
                        // Per-element bidirectional: Eval + Eval + ReduceAnd(2).
                        forward.push(RelateWork::Eval(s_el.value, t_el.value));
                        forward.push(RelateWork::Eval(t_el.value, s_el.value));
                        forward.push(RelateWork::ReduceAnd(2));
                    }
                }
                if pairs_evaluated > 1 {
                    forward.push(RelateWork::ReduceAnd(pairs_evaluated));
                }
                push_forward_work(work, forward);
                return;
            }
            // Tuple ≤ Array (readonly): elementwise check.
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
                for s in s_els.iter() {
                    forward.push(RelateWork::Eval(s.value, t_el));
                }
                if s_els.len() > 1 {
                    forward.push(RelateWork::ReduceAnd(s_els.len() as u32));
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
                params: s_params,
                return_type: s_ret,
                ..
            },
            SemanticNodeData::Signature {
                kind: t_kind,
                params: t_params,
                return_type: t_ret,
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
            let s_params = Arc::clone(s_params);
            let t_params = Arc::clone(t_params);
            let s_ret = *s_ret;
            let t_ret = *t_ret;
            drop(source_data);
            drop(target_data);
            results.push(self.relate_function(&s_params, s_ret, &t_params, t_ret, bindings));
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
    fn unwrap_identity_carrier_for_relation(&self, id: SemanticNodeId) -> IdentityCarrierUnwrap {
        let graph = self.graph();
        let Some(data) = graph.node_data(id) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        let (identity, args): (DeclIdentity, Arc<[SemanticNodeId]>) = match &*data {
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
        let unwrapped = match self.execute_type_node(SemanticQueryKey::Instantiate(
            crate::semantic_query::InstantiateKey::new(
                self.type_slot_for(
                    Arc::clone(&identity.canonical_id),
                    identity.owner,
                    Arc::clone(&identity.decl_name),
                ),
                args,
                self.instantiate_context_for(&identity.canonical_id, transit),
            ),
        )) {
            QueryResult::Value(SemanticQueryOutput {
                value: unwrapped, ..
            }) => self
                .evaluate_deferred_semantic_node_with_context(unwrapped, transit)
                .into_active_query_build_node(self),
            _ => return IdentityCarrierUnwrap::Unresolvable,
        };
        let Some(unwrapped_data) = graph.node_data(unwrapped) else {
            return IdentityCarrierUnwrap::Unresolvable;
        };
        if matches!(
            &*unwrapped_data,
            SemanticNodeData::Opaque(QueryError::DeclPlaceholder { .. })
        ) {
            IdentityCarrierUnwrap::Unresolvable
        } else {
            drop(unwrapped_data);
            IdentityCarrierUnwrap::Concrete(unwrapped)
        }
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
                if view.members.is_empty() && view.index_signatures.len() == 1 {
                    let ix = &view.index_signatures[0];
                    Some(RecordTargetShape::GenericKey {
                        key_type: ix.key_type,
                        value_type: ix.value_type,
                    })
                } else if !view.members.is_empty() && view.index_signatures.is_empty() {
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
                let members = Arc::clone(members);
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
                let mut acc = RelationResult::Assignable {
                    bindings: Arc::from(Vec::new().into_boxed_slice()),
                };
                for member in source_view.members.iter() {
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
            let Some(member) = source_view
                .members
                .iter()
                .find(|m| m.name.as_ref() == key.as_ref())
            else {
                return RelationResult::NotAssignable;
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

    /// Relate two object `SurfaceView`s structurally. Every required
    /// target member must be satisfied by a matching source member (or an
    /// applicable source index signature).
    pub(super) fn relate_objects(
        &self,
        source: &SurfaceView,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for t_prop in target.members.iter() {
            let prop_result =
                if let Some(s_prop) = source.members.iter().find(|p| p.name == t_prop.name) {
                    self.relate_property_pair(s_prop, t_prop, bindings)
                } else if let Some(index_result) =
                    self.relate_property_via_source_index(source, t_prop, bindings)
                {
                    index_result
                } else if t_prop.optional {
                    assignable(bindings)
                } else {
                    RelationResult::NotAssignable
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
            let sig_ok = source.call_signatures.iter().any(|s_sig| {
                // First-match alternatives: a LOSING source overload's
                // inference deposits roll back.
                let checkpoint = self.relation_session_checkpoint();
                let ok = matches!(
                    self.relate_member(*s_sig, *t_sig, bindings, InferPosition::Covariant),
                    RelationResult::Assignable { .. }
                );
                if !ok {
                    self.relation_session_rollback(&checkpoint);
                }
                ok
            });
            if !sig_ok {
                acc = result_and(acc, RelationResult::NotAssignable);
                return acc;
            }
        }
        for t_sig in target.construct_signatures.iter() {
            let sig_ok = source.construct_signatures.iter().any(|s_sig| {
                // First-match alternatives: same rollback rule as the call
                // bucket above.
                let checkpoint = self.relation_session_checkpoint();
                let ok = matches!(
                    self.relate_member(*s_sig, *t_sig, bindings, InferPosition::Covariant),
                    RelationResult::Assignable { .. }
                );
                if !ok {
                    self.relation_session_rollback(&checkpoint);
                }
                ok
            });
            if !sig_ok {
                acc = result_and(acc, RelationResult::NotAssignable);
                return acc;
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
            Some(SemanticNodeData::Opaque(QueryError::RecursiveRef { name })) => Arc::clone(name),
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
        if !target.readonly && source.readonly {
            return RelationResult::NotAssignable;
        }
        // Optional-to-required: a source member that may be ABSENT cannot
        // satisfy a required target member under `strictNullChecks` (the
        // optional's implied `undefined` does not relate to the required
        // value type). With strict null checks relaxed the implied
        // `undefined` collapses and the pair relates on the value types
        // alone (RI-10 behavioral branch).
        if !target.optional && source.optional {
            let strict = self
                .relation_txn
                .borrow()
                .strict
                .unwrap_or(StrictFamilyConfig::TS_STRICT);
            if strict.strict_null_checks {
                return RelationResult::NotAssignable;
            }
        }
        // A `RecursiveRef` member value rebinds to its declaration surface
        // through the shared dispatch so a genuinely recursive type
        // re-enters the authority (the coinductive back-edge) instead of
        // dead-ending on the sentinel.
        let source_value =
            self.resolve_recursive_member_value(source.value, source.declaration_origin.as_ref());
        let target_value =
            self.resolve_recursive_member_value(target.value, target.declaration_origin.as_ref());
        self.relate_member(
            source_value,
            target_value,
            bindings,
            InferPosition::Covariant,
        )
    }

    pub(super) fn relate_property_via_source_index(
        &self,
        source: &SurfaceView,
        target_prop: &crate::semantic_query::SurfaceMember,
        bindings: &mut Vec<InferBinding>,
    ) -> Option<RelationResult> {
        let graph = self.graph();
        let mut matched = false;
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for s_index in source.index_signatures.iter() {
            if !index_signature_applies_to_property(
                graph,
                s_index.key_type,
                target_prop.name.as_ref(),
            ) {
                continue;
            }
            matched = true;
            let r = self.relate_member(
                s_index.value_type,
                target_prop.value,
                bindings,
                InferPosition::Covariant,
            );
            acc = result_and(acc, r);
            if matches!(acc, RelationResult::NotAssignable) {
                return Some(RelationResult::NotAssignable);
            }
        }
        matched.then_some(acc)
    }

    pub(super) fn relate_target_index_signature(
        &self,
        source: &SurfaceView,
        target_index: &crate::semantic_query::IndexSignature,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let graph = self.graph();
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
        for prop in source.members.iter() {
            if !index_signature_applies_to_property(
                graph,
                target_index.key_type,
                prop.name.as_ref(),
            ) {
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
        acc
    }

    /// Relate two [`SemanticNodeData::Signature`] shells. Parameter
    /// variance follows the key's policy (RI-10 behavioral branch):
    /// strictly contravariant under `strictFunctionTypes`, bivariant
    /// otherwise (either direction suffices per parameter pair); the
    /// return is covariant.
    pub(super) fn relate_function(
        &self,
        source_params: &[crate::semantic_query::FunctionParam],
        source_return: SemanticNodeId,
        target_params: &[crate::semantic_query::FunctionParam],
        target_return: SemanticNodeId,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        let source_required = source_params
            .iter()
            .filter(|p| !p.optional && !p.rest)
            .count();
        let target_required = target_params
            .iter()
            .filter(|p| !p.optional && !p.rest)
            .count();
        if target_required < source_required {
            return RelationResult::NotAssignable;
        }
        let bivariant = {
            let txn = self.relation_txn.borrow();
            let strict = txn.strict.unwrap_or(StrictFamilyConfig::TS_STRICT);
            !strict.strict_function_types
        };
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
        for (s_param, t_param) in source_params.iter().zip(target_params.iter()) {
            // Contravariant: target param ≤ source param. Under the
            // bivariant regime either direction discharges the pair.
            let contravariant = self.relate_member(
                t_param.ty,
                s_param.ty,
                bindings,
                InferPosition::ContravariantParam,
            );
            let pair = if bivariant && !matches!(contravariant, RelationResult::Assignable { .. }) {
                result_or(
                    contravariant,
                    self.relate_member(s_param.ty, t_param.ty, bindings, InferPosition::Covariant),
                )
            } else {
                contravariant
            };
            acc = result_and(acc, pair);
            if matches!(acc, RelationResult::NotAssignable) {
                return RelationResult::NotAssignable;
            }
        }
        // Covariant return.
        let r = self.relate_member(
            source_return,
            target_return,
            bindings,
            InferPosition::Return,
        );
        result_and(acc, r)
    }

    /// Relate a function source against an object target carrying call
    /// signatures.
    /// A DIRECT signature source against an Object target: required target
    /// MEMBERS reject (a bare signature has none), and every target
    /// signature bucket must be satisfied by the source's single
    /// matching-kind signature — a bucket of the OTHER kind is unmet.
    fn relate_signature_source_to_object(
        &self,
        source_sig: SemanticNodeId,
        source_kind: crate::semantic_query::SignatureKind,
        target: &SurfaceView,
        bindings: &mut Vec<InferBinding>,
    ) -> RelationResult {
        for m in target.members.iter() {
            if !m.optional {
                return RelationResult::NotAssignable;
            }
        }
        let mut acc = RelationResult::Assignable {
            bindings: Arc::from(Vec::new().into_boxed_slice()),
        };
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
        let mut any_unknown = false;
        for s_sig in group.iter() {
            // First-match alternatives: a LOSING alternative's inference
            // deposits roll back — only the succeeding alternative may
            // contribute candidates to fixation.
            let checkpoint = self.relation_session_checkpoint();
            match self.relate_member(*s_sig, target_sig, bindings, InferPosition::Covariant) {
                r @ RelationResult::Assignable { .. } => return r,
                RelationResult::Unknown => {
                    self.relation_session_rollback(&checkpoint);
                    any_unknown = true;
                }
                RelationResult::NotAssignable => {
                    self.relation_session_rollback(&checkpoint);
                }
            }
        }
        if any_unknown {
            RelationResult::Unknown
        } else {
            RelationResult::NotAssignable
        }
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
enum IdentityCarrierUnwrap {
    Concrete(SemanticNodeId),
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
fn relation_step_from_pending(pending: &PendingVerdict) -> RelationStep {
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
    /// Evaluate `(source, target)`.
    Eval(SemanticNodeId, SemanticNodeId),
    /// Pop `n` prior results, AND them, push one combined result.
    ReduceAnd(u32),
    /// Pop `n` prior results, OR them, push one combined result.
    ReduceOr(u32),
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

fn reduce_or_from_results(results: &mut Vec<RelationResult>, n: u32) -> RelationResult {
    let mut combined = RelationResult::NotAssignable;
    // bounded-loop: drains `n` per-pair results owned by this reducer — fan-out of the originating distribution; total work bounded by `decide_relation` budget (graph-size × 10).
    for _ in 0..n {
        let r = results
            .pop()
            .expect("RelateWork::ReduceOr: result-stack underflow");
        combined = result_or(combined, r);
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

/// Build and push the worklist fan-out for a distribution whose reducer
/// is AND-all.
fn distribute_and<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
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
        forward.push(RelateWork::Eval(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceAnd(n as u32));
    }
    push_forward_work(work, forward);
}

/// Build and push the worklist fan-out for a distribution whose reducer
/// is OR-any.
fn distribute_or<F>(
    work: &mut Vec<RelateWork>,
    results: &mut Vec<RelationResult>,
    members: &[SemanticNodeId],
    mut pairer: F,
) where
    F: FnMut(&SemanticNodeId) -> (SemanticNodeId, SemanticNodeId),
{
    let n = members.len();
    if n == 0 {
        results.push(RelationResult::NotAssignable);
        return;
    }
    let mut forward: Vec<RelateWork> = Vec::with_capacity(n + 1);
    for m in members.iter() {
        let (s, t) = pairer(m);
        forward.push(RelateWork::Eval(s, t));
    }
    if n > 1 {
        forward.push(RelateWork::ReduceOr(n as u32));
    }
    push_forward_work(work, forward);
}

/// O(tag) disjointness for the contravariant-candidate intersection
/// collapse: `true` ONLY for pairs whose intersection is provably empty at
/// tag level — distinct concrete primitives (modulo the
/// `undefined`/`void` widening pair), distinct literals, or a literal
/// against a mismatched base primitive. Conservative `false` for every
/// other shape (the structural Intersection carrier is kept).
fn tag_level_disjoint(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> bool {
    let (Some(a_data), Some(b_data)) = (graph.node_data(a), graph.node_data(b)) else {
        return false;
    };
    fn literal_base(lit: &LiteralValue) -> PrimitiveKind {
        match lit {
            LiteralValue::String(_) => PrimitiveKind::String,
            LiteralValue::Number(_) => PrimitiveKind::Number,
            LiteralValue::Boolean(_) => PrimitiveKind::Boolean,
            LiteralValue::BigInt(_) => PrimitiveKind::BigInt,
        }
    }
    fn concrete(kind: PrimitiveKind) -> bool {
        !matches!(
            kind,
            PrimitiveKind::Any | PrimitiveKind::Unknown | PrimitiveKind::Never
        )
    }
    match (&*a_data, &*b_data) {
        (SemanticNodeData::Primitive(x), SemanticNodeData::Primitive(y)) => {
            let widening_pair = matches!(
                (*x, *y),
                (PrimitiveKind::Undefined, PrimitiveKind::Void)
                    | (PrimitiveKind::Void, PrimitiveKind::Undefined)
            );
            concrete(*x) && concrete(*y) && x != y && !widening_pair
        }
        (SemanticNodeData::Literal(x), SemanticNodeData::Literal(y)) => x != y,
        (SemanticNodeData::Literal(lit), SemanticNodeData::Primitive(prim))
        | (SemanticNodeData::Primitive(prim), SemanticNodeData::Literal(lit)) => {
            concrete(*prim) && literal_base(lit) != *prim
        }
        _ => false,
    }
}
