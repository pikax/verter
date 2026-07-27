//! `CheckerTransaction` — the transient per-relation-root cold-compute frame
//! of the ONE resolver (design `docs/arch/u2-relation-infer-design.md` §2.1).
//!
//! The persistent relation cache lives in the family memo's `Relate` family,
//! keyed by the full §2.7 identity; EVERYTHING in this module is TRANSIENT
//! per-`CheckerTransaction` state and is NEVER a cache key, NEVER thread-local,
//! NEVER process-wide. The transaction rides the dispatch
//! ([`crate::project_semantic_dispatch::ProjectSemanticDispatch`]) as a
//! `RefCell`, exactly like the dispatch's other cold-compute cycle guards
//! (`instantiate_active`, `carrier_normalizing`, `build_local_taint`).
//!
//! Shapes:
//!
//! - [`CheckerReentryStack`] — the ONE shared re-entry / cycle-id space. At
//!   U2 only `Relate` is wired onto it (the `Instantiate` Skeleton BFS reuse
//!   is RI-8; `FlowReturn` / `ResolveCall` land at U6). Each node is keyed by
//!   its full normalized identity (a `Relate` node by the full §2.7 key).
//! - [`RelationAssumptionStack`] — the typed VIEW over the reentry stack:
//!   assumption-edge recording plus the lowlink (min open-target) tracking
//!   the coinductive SCC discharge consumes. It cannot diverge from the
//!   reentry stack because it IS the same storage.
//! - [`InferenceSession`] / [`SessionAdmissionLedger`] — the in-flight
//!   inference substrate (RI-6 scope): a binding-producing relation opens a
//!   session whose SETUP is fully determined by the infer pattern it serves
//!   (see [`InferenceSession`]), so the content-free [`InferenceContextKey`]
//!   fingerprint is well-defined at session OPEN — the transient `SessionId`
//!   stand-in of design §2.2 is not needed for this subset (the setup never
//!   mutates mid-flight; fixation is a single deterministic pass).
//! - [`SccLedger`] — popped-but-unpublished SCC members awaiting their SCC
//!   root's close (PROVISIONAL verdicts — caller-return values + deferral
//!   metadata, NEVER the published payload).
//!
//! Execution model (single-threaded per transaction): frames nest strictly,
//! so assumption edges ALWAYS point from a deeper frame to an ancestor on the
//! current stack. The SCC of the frame being popped is therefore the
//! contiguous stack suffix from the minimum open-assumption target — the
//! Tarjan lowlink specialised to a path graph (design §2.3 step 1 "Tarjan
//! over the assumption edges"). Discharge (§2.3 step 3): a member decided
//! with all non-assumptive obligations positive closes POSITIVE
//! (`Assignable` + `CoinductiveCycle`); a member with a negative
//! non-assumptive obligation publishes `NotAssignable` (final); any
//! `Unknown` / budget edge anywhere in the component routes the WHOLE SCC
//! through `ReturnOnly` (nothing publishes).

use std::sync::Arc;

use rustc_hash::FxHashMap;

use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, InferBinding, InferableParamSetId,
    InferenceCandidatePriority, InferenceContextKey, NoInferMask, RecursionOrBudgetCap,
    RelateMemoKey, RelationPayload, SemanticNodeId, VariancePhase, VariancePolicy,
};

/// Transient per-transaction session token. Content-free; NEVER enters a
/// published key, a `ReadSetSignature.facts` observation, or any fact
/// signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SessionId(pub(crate) u64);

/// Normalized typed strict-family configuration threaded into the
/// transaction (RI-10): the reducer BRANCHES on it, and it folds into the
/// relation key's `type_env_hash` so a strict-on judgement can never
/// warm-hit a strict-off request (design obligation 3 — behavioral branch
/// AND key isolation, never hash-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct StrictFamilyConfig {
    /// `strictNullChecks`: when OFF, `null` / `undefined` are assignable to
    /// every target except `never`.
    pub(crate) strict_null_checks: bool,
    /// `strictFunctionTypes`: when ON, function-type parameters relate
    /// strictly contravariantly; when OFF they relate bivariantly (the
    /// non-strict function rule — either direction suffices).
    pub(crate) strict_function_types: bool,
}

impl StrictFamilyConfig {
    /// The default regime — matches the pre-activation engine's behavior
    /// (null/undefined isolated; contravariant function parameters).
    pub(crate) const TS_STRICT: Self = Self {
        strict_null_checks: true,
        strict_function_types: true,
    };

    /// The parameter-variance regime this configuration selects — folded
    /// into the key's [`crate::semantic_query::RelationPolicy`] so the
    /// behavioral branch and the key discriminator never diverge.
    pub(crate) fn variance_policy(self) -> VariancePolicy {
        if self.strict_function_types {
            VariancePolicy::StrictContravariance
        } else {
            VariancePolicy::MethodParameterBivariance
        }
    }

    /// Fold the strict family into a base `type_env_hash` (the `T`
    /// dimension of the relation key's env, R21 split). Deterministic,
    /// injective over the four configurations, and deliberately NOT the
    /// identity fold for any non-default configuration so strict-on and
    /// strict-off relations occupy distinct slots.
    pub(crate) fn mix_into_type_env_hash(self, base: [u8; 16]) -> [u8; 16] {
        if self == Self::TS_STRICT {
            return base;
        }
        let mut out = base;
        // Domain-separate marker byte plus the two config bits, mixed into
        // every lane so any base hash stays collision-free per config.
        let marker: u8 =
            0x5C ^ (self.strict_null_checks as u8) ^ ((self.strict_function_types as u8) << 1);
        for (i, b) in out.iter_mut().enumerate() {
            *b = b.wrapping_add(marker.rotate_left(i as u32));
        }
        out
    }
}

/// The engine-internal step result of one [`super::relation`] authority
/// dispatch (`execute_relate`). NEVER cached as-is; the admission boundary
/// maps it onto the public payload + the admission table.
#[derive(Debug, Clone)]
pub(crate) enum RelationStep {
    /// The source relates to the target (with the inference bindings a
    /// binding-producing judgement fixed at session close).
    Assignable { bindings: Arc<[InferBinding]> },
    /// The source provably does NOT relate to the target.
    NotAssignable,
    /// The judgement could not be decided (deferred / opaque / an
    /// undischargeable SCC edge). No public value-domain form; ReturnOnly.
    Unknown,
    /// A budget cap stopped the relate. PUBLIC-but-never-warm: expressible
    /// on the payload, ReturnOnly at the admission gate.
    BudgetExceeded(RecursionOrBudgetCap),
    /// The scoped coinductive assumption sentinel: the queried full
    /// identity is already on the reentry stack, so the relation is
    /// ASSUMED to hold for this SCC and the caller's frame recorded the
    /// assumption edge. NEVER warm-admitted, NEVER the published proof.
    Assumed,
}

/// One in-flight relation frame on the reentry stack — a full-identity
/// `Relate` node plus the coinductive bookkeeping its SCC discharge needs.
#[derive(Debug)]
pub(crate) struct ReentryFrame {
    /// The full §2.7 identity this frame computes.
    pub(crate) key: RelateMemoKey,
    /// Assumption edges recorded by this frame's subtree: stack indices of
    /// the frames this subtree ASSUMED hold (back-edges).
    pub(crate) assumption_targets: Vec<usize>,
    /// The Tarjan lowlink: the minimum stack index any open assumption in
    /// this frame's subtree targets. `Some(own)` or `None` at pop ⇒ this
    /// frame is its SCC's root.
    pub(crate) min_open_target: Option<usize>,
    /// This frame deposited inference candidates into the active session
    /// (a session-local delta — admission row 7: ReturnOnly, never
    /// published).
    pub(crate) session_delta: bool,
    /// This frame's reducer consumed a budget edge — the typed cap that
    /// stopped the relate. Poisons the whole SCC (ReturnOnly); the ROOT
    /// surfaces the public `BudgetExceeded` payload.
    pub(crate) budget_cap: Option<RecursionOrBudgetCap>,
    /// The session this frame OPENED (it is the binding root), if any.
    pub(crate) opened_session: Option<SessionId>,
    /// The `SccLedger` pending length at this frame's PUSH — the drain
    /// watermark. Everything deposited at `pending[watermark..]` was
    /// deposited by THIS frame's subtree (frames nest strictly), so an
    /// SCC-root close drains exactly its own suffix. Stack indices
    /// recycle after pops; this watermark does not, so a sibling frame
    /// that reuses a popped member's stack index can never steal that
    /// member from a still-open outer SCC.
    pub(crate) pending_watermark: usize,
}

/// The ONE shared re-entry / cycle-id space (design §2.1). Heap-backed,
/// per-`CheckerTransaction`, keyed by full normalized identity.
#[derive(Debug, Default)]
pub(crate) struct CheckerReentryStack {
    frames: Vec<ReentryFrame>,
    index: FxHashMap<RelateMemoKey, usize>,
}

impl CheckerReentryStack {
    /// The stack index of `key` when its full identity is already in
    /// flight on THIS transaction.
    pub(crate) fn find(&self, key: &RelateMemoKey) -> Option<usize> {
        self.index.get(key).copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub(crate) fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Push a fresh frame for `key` with the SCC ledger's current pending
    /// length as its drain watermark; returns its stack index.
    pub(crate) fn push(&mut self, key: RelateMemoKey, pending_watermark: usize) -> usize {
        let idx = self.frames.len();
        self.frames.push(ReentryFrame {
            key: key.clone(),
            assumption_targets: Vec::new(),
            min_open_target: None,
            session_delta: false,
            budget_cap: None,
            opened_session: None,
            pending_watermark,
        });
        self.index.insert(key, idx);
        idx
    }

    /// Pop the top frame. Callers uphold strict LIFO nesting (the
    /// transaction's execution model).
    pub(crate) fn pop(&mut self) -> ReentryFrame {
        let frame = self.frames.pop().expect("reentry stack underflow");
        self.index.remove(&frame.key);
        frame
    }

    pub(crate) fn top_mut(&mut self) -> Option<&mut ReentryFrame> {
        self.frames.last_mut()
    }

    /// Record a budget edge on the frame at `idx` (poisons its SCC).
    pub(crate) fn note_budget_edge(&mut self, idx: usize, cap: RecursionOrBudgetCap) {
        if let Some(frame) = self.frames.get_mut(idx) {
            if frame.budget_cap.is_none() {
                frame.budget_cap = Some(cap);
            }
        }
    }

    /// The top frame's key (the current frame's relation axes template
    /// for sub-relation key construction).
    pub(crate) fn frames_top_key(&self) -> Option<&RelateMemoKey> {
        self.frames.last().map(|frame| &frame.key)
    }

    /// The session the frame at `idx` opened, if any.
    pub(crate) fn frame_opened_session(&self, idx: usize) -> Option<SessionId> {
        self.frames.get(idx).and_then(|frame| frame.opened_session)
    }

    /// Mark the frame at `idx` as having opened session `session`.
    pub(crate) fn note_opened_session(&mut self, idx: usize, session: SessionId) {
        if let Some(frame) = self.frames.get_mut(idx) {
            frame.opened_session = Some(session);
        }
    }

    /// Mark the frame at `idx` as a session-local delta (row 7).
    pub(crate) fn note_session_delta(&mut self, idx: usize) {
        if let Some(frame) = self.frames.get_mut(idx) {
            frame.session_delta = true;
        }
    }
}

/// The typed VIEW over the reentry stack that records coinductive
/// assumption edges and tracks the discharge lowlink (design §2.1: the
/// relation assumption stack is a projection of the one reentry stack —
/// same storage, so the per-engine cycle spaces cannot diverge).
#[derive(Debug, Default)]
pub(crate) struct RelationAssumptionStack {
    reentry: CheckerReentryStack,
}

impl RelationAssumptionStack {
    pub(crate) fn reentry(&self) -> &CheckerReentryStack {
        &self.reentry
    }

    pub(crate) fn reentry_mut(&mut self) -> &mut CheckerReentryStack {
        &mut self.reentry
    }

    /// Record an assumption edge `top → target` (the coinductive "assume
    /// it holds" step, design §2.2): the caller's accumulator is marked
    /// `OpenAssumption(target)` — transient, NEVER written to a published
    /// `ReadSetSignature.facts`.
    pub(crate) fn record_assumption(&mut self, target: usize) {
        if let Some(frame) = self.reentry.top_mut() {
            frame.assumption_targets.push(target);
            frame.min_open_target = Some(
                frame
                    .min_open_target
                    .map_or(target, |current| current.min(target)),
            );
        }
    }

    /// Fold a popped child's still-open lowlink into the (new) top frame:
    /// an assumption the child recorded against a frame BELOW it stays
    /// open against the parent after the child pops.
    pub(crate) fn propagate_lowlink(&mut self, child_min_open: Option<usize>) {
        let Some(child_min_open) = child_min_open else {
            return;
        };
        if let Some(frame) = self.reentry.top_mut() {
            frame.min_open_target = Some(
                frame
                    .min_open_target
                    .map_or(child_min_open, |current| current.min(child_min_open)),
            );
        }
    }
}

/// Lifecycle of an in-flight inference session (design Decision 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferenceSessionState {
    /// Still collecting candidates — NOT converged (ReturnOnly).
    InProgress,
    /// Fixation completed deterministically: every inferable param is
    /// fixed-or-deterministically-defaulted and the final bindings are
    /// immutable. The ONLY state that admits.
    CompletedDeterministic,
    /// Cancel / budget-exceeded / superseded / non-deterministic — the
    /// deferred batch releases WITHOUT publish (ReturnOnly).
    Abandoned,
}

/// A rollback point over an [`InferenceSession`]'s per-parameter candidate
/// lists (see [`InferenceSession::checkpoint`]). Transient, alternative-
/// scoped — never stored beyond the alternative it brackets.
#[derive(Debug)]
pub(crate) struct SessionCheckpoint {
    /// `candidates.len()` per [`InferenceInfo`], in info order.
    candidate_lens: Vec<usize>,
}

/// One inference candidate deposited for a type parameter (design §4.2).
#[derive(Debug, Clone)]
pub(crate) struct InferenceCandidate {
    /// The bound node.
    pub(crate) node: SemanticNodeId,
    /// The priority-ladder rung this candidate was deposited under.
    pub(crate) priority: InferenceCandidatePriority,
    /// The variance of the POSITION this candidate was deposited from —
    /// drives the per-rung combination (covariant candidates union,
    /// contravariant candidates intersect).
    pub(crate) variance: VariancePhase,
}

/// Per-parameter inference state (design §4.2 `InferenceInfo`). The
/// `// setup_axis`-tagged fields feed the GENERATED
/// [`InferenceContextKey`] projection (R-b): the guard
/// `inference_context_key_projects_every_session_setup_axis` diffs the
/// tagged field set against the projection and fails on any
/// untagged-or-unprojected axis.
#[derive(Debug)]
pub(crate) struct InferenceInfo {
    /// The content-free identity of the parameter (its `Infer` node).
    pub(crate) param_node: SemanticNodeId,
    /// The parameter's display name (bindings surface by name).
    pub(crate) param_name: Arc<str>,
    // setup_axis: candidate priority ladder rung this info collects under.
    pub(crate) priority: InferenceCandidatePriority,
    // setup_axis: `<const T>` const-ness propagation policy for this param.
    #[allow(dead_code)] // projected into the fingerprint; read by the R-b guard
    pub(crate) const_param_policy: ConstParamPolicy,
    /// Deposited candidates (session-local deltas — ReturnOnly, never
    /// published as-is).
    pub(crate) candidates: Vec<InferenceCandidate>,
}

/// The mutable inference session — cold-compute STATE of `execute`
/// (design Decision 3 / §3.4), never a standalone engine, TRANSIENT, never
/// a cache key, never admitted. For the in-scope conditional-`infer`
/// cases (object property, tuple head/tail, function inference) the
/// session's SETUP is fully determined by the pattern it serves: the
/// inferable params are the pattern's `Infer` nodes, the variance pass is
/// covariant, the priority rung is the pattern's highest, and the masks
/// are empty — so the completed [`InferenceContextKey`] fingerprint is
/// well-defined at session OPEN (the design §2.2 `SessionId` stand-in for
/// a not-yet-knowable fingerprint does not arise in this subset).
#[derive(Debug)]
pub(crate) struct InferenceSession {
    /// The transient per-transaction token (content-free, never a key).
    #[allow(dead_code)] // identity for ledger keying; retained for the session stack
    pub(crate) id: SessionId,
    // setup_axis: which type parameters are open / inferable (the pattern's
    // `Infer` nodes — NOT their bound bodies; bodies live in the VALUE).
    pub(crate) inferable_params: Vec<SemanticNodeId>,
    // setup_axis: the variance MEASUREMENT pass (conditional `infer`
    // extraction runs the covariant pass).
    pub(crate) variance_phase: VariancePhase,
    // setup_axis: the highest candidate-priority rung the pattern contains.
    pub(crate) candidate_priority: InferenceCandidatePriority,
    // setup_axis: the occurrence-local `NoInfer` suppression mask in effect.
    pub(crate) no_infer_mask: NoInferMask,
    // setup_axis: whether / how a contextual target drives inference.
    pub(crate) contextual_inference_mode: ContextualInferenceMode,
    /// Per-parameter candidate state.
    pub(crate) infos: Vec<InferenceInfo>,
    /// Session lifecycle.
    pub(crate) state: InferenceSessionState,
}

impl InferenceSession {
    /// The content-free projection of the session's SETUP — the
    /// [`InferenceContextKey`] fingerprint (R-b GENERATED: one line per
    /// `// setup_axis`-tagged field across `InferenceSession` and
    /// `InferenceInfo`, in declaration order; the diff guard fails on any
    /// missing axis).
    pub(crate) fn context_key(&self) -> InferenceContextKey {
        // The session's ladder rung must equal the pattern's highest
        // per-info rung (both are setup projections — the guard
        // `inference_context_key_projects_every_session_setup_axis`
        // fails a drift).
        let highest_info_rung = self.infos.iter().map(|info| info.priority).fold(
            InferenceCandidatePriority::Argument,
            |acc, rung| match (acc, rung) {
                (InferenceCandidatePriority::NakedTypeParameter, _)
                | (_, InferenceCandidatePriority::NakedTypeParameter) => {
                    InferenceCandidatePriority::NakedTypeParameter
                }
                (InferenceCandidatePriority::ReturnType, _)
                | (_, InferenceCandidatePriority::ReturnType) => {
                    InferenceCandidatePriority::ReturnType
                }
                _ => InferenceCandidatePriority::Argument,
            },
        );
        debug_assert_eq!(
            self.candidate_priority, highest_info_rung,
            "the session ladder rung must equal the pattern's highest per-info rung"
        );
        InferenceContextKey {
            inferable_params: InferableParamSetId::new(Arc::from(
                self.inferable_params.clone().into_boxed_slice(),
            )),
            variance_phase: self.variance_phase,
            candidate_priority: self.candidate_priority,
            no_infer_mask: self.no_infer_mask,
            const_param_policy: self
                .infos
                .iter()
                .find(|info| info.const_param_policy == ConstParamPolicy::Const)
                .map_or(ConstParamPolicy::NonConst, |info| info.const_param_policy),
            contextual_inference_mode: self.contextual_inference_mode,
        }
    }

    /// A rollback point over the session's per-parameter candidate lists.
    /// Deposits strictly APPEND onto `InferenceInfo::candidates` (the info
    /// set itself is fixed at session open), so a checkpoint is the ordered
    /// list of candidate lengths and rollback truncates back to it — the
    /// alternative-scoping primitive: a LOSING overload / signature-group
    /// alternative's deposits must not survive into fixation.
    pub(crate) fn checkpoint(&self) -> SessionCheckpoint {
        SessionCheckpoint {
            candidate_lens: self
                .infos
                .iter()
                .map(|info| info.candidates.len())
                .collect(),
        }
    }

    /// Truncate every parameter's candidate list back to `checkpoint`
    /// (discarding the deposits a failed alternative made). A checkpoint
    /// taken on THIS session always matches the info count; a mismatched
    /// checkpoint (foreign session) is ignored rather than corrupting
    /// state.
    pub(crate) fn rollback_to(&mut self, checkpoint: &SessionCheckpoint) {
        if checkpoint.candidate_lens.len() != self.infos.len() {
            debug_assert!(
                false,
                "session checkpoint info-count mismatch: checkpoint {} vs session {}",
                checkpoint.candidate_lens.len(),
                self.infos.len()
            );
            return;
        }
        for (info, len) in self.infos.iter_mut().zip(checkpoint.candidate_lens.iter()) {
            info.candidates.truncate(*len);
        }
    }

    /// Deposit a candidate for `param` under `priority`, tagged with the
    /// variance of the position it came from.
    pub(crate) fn deposit(
        &mut self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
        variance: VariancePhase,
    ) {
        if let Some(info) = self.infos.iter_mut().find(|i| i.param_node == param_node) {
            info.candidates.push(InferenceCandidate {
                node: candidate,
                priority,
                variance,
            });
        }
    }

    /// Fixation (design §4.2): combine each parameter's candidates into its
    /// final binding through the closed priority ladder — the HIGHEST rung
    /// with candidates wins. Within the chosen rung the combination
    /// variance is PER-CANDIDATE: when any candidate came from a
    /// contravariant position, the contravariant candidates win and
    /// INTERSECT (the TS contravariant-inference rule); otherwise the
    /// covariant candidates union (deduplicated). Every parameter fixes
    /// (unfixed parameters default to `unknown`), so the session always
    /// reaches `CompletedDeterministic`.
    pub(crate) fn fixate<F>(&mut self, mut combine: F) -> Vec<InferBinding>
    where
        F: FnMut(&[SemanticNodeId], VariancePhase) -> SemanticNodeId,
    {
        let mut bindings = Vec::with_capacity(self.infos.len());
        let infos = std::mem::take(&mut self.infos);
        for info in &infos {
            let mut rungs = [
                info.candidates
                    .iter()
                    .filter(|c| c.priority == InferenceCandidatePriority::NakedTypeParameter)
                    .collect::<Vec<_>>(),
                info.candidates
                    .iter()
                    .filter(|c| c.priority == InferenceCandidatePriority::ReturnType)
                    .collect::<Vec<_>>(),
                info.candidates
                    .iter()
                    .filter(|c| c.priority == InferenceCandidatePriority::Argument)
                    .collect::<Vec<_>>(),
            ];
            let chosen = rungs
                .iter_mut()
                .find(|rung| !rung.is_empty())
                .map(std::mem::take)
                .unwrap_or_default();
            let contravariant: Vec<SemanticNodeId> = chosen
                .iter()
                .filter(|c| c.variance == VariancePhase::Contravariant)
                .map(|c| c.node)
                .collect();
            let bound = if contravariant.is_empty() {
                let covariant: Vec<SemanticNodeId> = chosen.iter().map(|c| c.node).collect();
                combine(&covariant, VariancePhase::Covariant)
            } else {
                combine(&contravariant, VariancePhase::Contravariant)
            };
            bindings.push(InferBinding {
                name: Arc::clone(&info.param_name),
                bound,
            });
        }
        self.infos = infos;
        self.state = InferenceSessionState::CompletedDeterministic;
        bindings
    }
}

/// A popped SCC member awaiting its SCC root's close — the PROVISIONAL
/// deferral record (design §2.3 step 4): a caller-return value plus
/// deferral metadata, NEVER the published payload. The published payload
/// is produced at the batched-publish instant by the discharge against
/// converged state.
#[derive(Debug)]
pub(crate) struct PendingSccMember {
    /// The member's full §2.7 identity.
    pub(crate) key: RelateMemoKey,
    /// The member's provisional discharged verdict at pop.
    pub(crate) verdict: PendingVerdict,
    /// Session-local delta (row 7) — never publishes.
    pub(crate) session_delta: bool,
    /// The member opened session `Some(..)` (a binding member).
    pub(crate) opened_session: Option<SessionId>,
}

/// The decided provisional verdict of a popped member.
#[derive(Debug, Clone)]
pub(crate) enum PendingVerdict {
    Assignable { bindings: Arc<[InferBinding]> },
    NotAssignable,
    Unknown,
    BudgetExceeded(RecursionOrBudgetCap),
}

/// The per-`CheckerTransaction` SCC ledger (design §2.3 step 4 R-a):
/// accumulates popped-but-unpublished members; the SCC root's close
/// computes each member's published outcome and routes the batch.
#[derive(Debug, Default)]
pub(crate) struct SccLedger {
    pending: Vec<PendingSccMember>,
}

impl SccLedger {
    pub(crate) fn deposit(&mut self, member: PendingSccMember) {
        self.pending.push(member);
    }

    /// The current pending length — recorded as a frame's drain watermark
    /// at push.
    pub(crate) fn pending_len(&self) -> usize {
        self.pending.len()
    }

    /// Drain every member deposited at or after `watermark` — exactly the
    /// closing frame's own subtree deposits (frames nest strictly and
    /// deposits append, so the suffix from the frame's push-time watermark
    /// IS its SCC membership; stack indices recycle and MUST NOT identify
    /// membership). The drained members are in pop order: deepest-popped
    /// first.
    pub(crate) fn drain_scc(&mut self, watermark: usize) -> Vec<PendingSccMember> {
        let split = watermark.min(self.pending.len());
        self.pending.split_off(split)
    }
}

/// The per-session deferred-admission ledger (design §2.3 step 4 /
/// §3.3): binding members of a not-yet-closed SCC record here at
/// SCC-close; the drain at the relevant session's `CompletedDeterministic`
/// close re-discharges against the converged state and publishes ONLY a
/// stable determined outcome (flip / abandonment publishes nothing).
#[derive(Debug, Default)]
pub(crate) struct SessionAdmissionLedger {
    deferred: std::collections::BTreeMap<SessionId, Vec<RelateMemoKey>>,
}

impl SessionAdmissionLedger {
    /// Record `key` as deferred on session `session`'s close.
    pub(crate) fn defer(&mut self, session: SessionId, key: RelateMemoKey) {
        self.deferred.entry(session).or_default().push(key);
    }

    /// Drain every key deferred on `session` (at that session's close).
    pub(crate) fn drain(&mut self, session: SessionId) -> Vec<RelateMemoKey> {
        self.deferred.remove(&session).unwrap_or_default()
    }
}

/// A member whose SCC closed cleanly, queued for the batched publish the
/// relation ROOT performs after its family-memo publish lands (the member
/// entries ride the ROOT's SCC-union carrier — design §2.3 step 3: the
/// published fact set is never the bare per-member set).
#[derive(Debug)]
pub(crate) struct CompletedSccMember {
    pub(crate) key: RelateMemoKey,
    pub(crate) payload: RelationPayload,
}

/// The per-relation-root cold-compute frame (design §2.1 /
/// `native-typeinfo-parity.md` §4.2). Transient; NEVER a cache key.
#[derive(Debug, Default)]
pub(crate) struct CheckerTransaction {
    /// The ONE shared re-entry / cycle-id space (only `Relate` wired at
    /// U2) with its relation-assumption typed view.
    pub(crate) assumptions: RelationAssumptionStack,
    /// Popped-but-unpublished SCC members awaiting their root's close.
    pub(crate) scc_ledger: SccLedger,
    /// The active inference-session stack.
    pub(crate) sessions: Vec<InferenceSession>,
    /// Per-session deferred-admission ledger.
    pub(crate) session_admission: SessionAdmissionLedger,
    /// SCC-closed members queued for the root's batched publish drain.
    pub(crate) completed_members: Vec<CompletedSccMember>,
    /// The normalized strict-family configuration in force (RI-10).
    pub(crate) strict: Option<StrictFamilyConfig>,
    /// The discharge substitution table of an in-flight re-discharge
    /// (design §2.3 step 4 — the converged verdicts a re-running member
    /// consults instead of re-entering the SCC).
    pub(crate) discharge_substitution: FxHashMap<RelateMemoKey, RelationStep>,
    /// Per-target-node memo of the `infer`-pattern detection (a pure
    /// function of the pattern; avoids rescanning per ask).
    pub(crate) pattern_cache: FxHashMap<SemanticNodeId, Option<super::relation::InferPatternInfo>>,
    next_session_id: u64,
}

impl CheckerTransaction {
    pub(crate) fn reentry(&self) -> &CheckerReentryStack {
        self.assumptions.reentry()
    }

    pub(crate) fn reentry_mut(&mut self) -> &mut CheckerReentryStack {
        self.assumptions.reentry_mut()
    }

    pub(crate) fn alloc_session_id(&mut self) -> SessionId {
        self.next_session_id += 1;
        SessionId(self.next_session_id)
    }

    /// The active (innermost `InProgress`) session, if any.
    pub(crate) fn active_session_mut(&mut self) -> Option<&mut InferenceSession> {
        self.sessions
            .iter_mut()
            .rev()
            .find(|s| s.state == InferenceSessionState::InProgress)
    }

    pub(crate) fn active_session(&self) -> Option<&InferenceSession> {
        self.sessions
            .iter()
            .rev()
            .find(|s| s.state == InferenceSessionState::InProgress)
    }
}
