//! The inference owner's session state: the per-parameter candidate
//! journals one collecting session accumulates, its reverse-projection
//! journals, the checkpoint an alternative rolls back to, and the
//! one-way lifecycle from collection to a committed fixation. The
//! transaction holds the session stack; only this module mutates a
//! session.

use std::sync::Arc;

use rustc_hash::FxHashSet;

use super::super::dispatch_txn::SessionId;
use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, IndexSignature, InferBinding, InferableParamSetId,
    InferenceCandidatePriority, InferenceContextKey, InferencePassKind, NoInferMask,
    SemanticNodeId, SurfaceMember, TupleElement, VariancePhase,
};

/// Lifecycle of an in-flight inference session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InferenceSessionState {
    /// Still collecting candidates — NOT converged (ReturnOnly).
    Collecting,
    /// Fixation completed deterministically. The binding snapshot is
    /// immutable and the session is inactive for deposits, but it has not
    /// crossed the atomic publication boundary.
    StagedDeterministic,
    /// The staged snapshot crossed its stability gate. The ONLY state that
    /// admits when its ledger is atomically drained.
    CommittedDeterministic,
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
    /// Registered reverse-projection targets are append-only.
    projection_info_len: usize,
    /// `candidates.len()` per existing reverse projection target.
    projection_candidate_lens: Vec<usize>,
    /// Recovered reverse members accumulated so far.
    recovered_len: usize,
    /// The reverse pass's full/partial status at the checkpoint.
    reverse_partial: bool,
    /// Aggregate candidates already deposited by the reverse pass.
    aggregate_candidate_len: usize,
}

/// One inference candidate deposited for a type parameter (design §4.2).
#[derive(Debug, Clone)]
pub struct InferenceCandidate {
    /// The bound node.
    pub node: SemanticNodeId,
    /// The priority-ladder rung this candidate was deposited under.
    pub priority: InferenceCandidatePriority,
    /// The variance of the POSITION this candidate was deposited from —
    /// drives the per-rung combination (covariant candidates union,
    /// contravariant candidates intersect).
    pub variance: VariancePhase,
}

/// One registered indexed-access projection and the ordinary inference
/// candidates deposited when relation descent reaches it.
#[derive(Debug)]
struct ProjectionInferenceInfo {
    target_node: SemanticNodeId,
    candidates: Vec<InferenceCandidate>,
}

/// Recovered source-shape entries accumulated by the reverse pass.
#[derive(Debug, Clone)]
pub(crate) enum ReverseRecoveredEntry {
    ObjectMember { member: SurfaceMember },
    ArrayElement { value: SemanticNodeId },
    TupleElement { element: TupleElement },
    IndexSignature { signature: IndexSignature },
}

/// Session-owned journals for exact reverse-homomorphic mapped inference.
#[derive(Debug)]
pub struct ReverseProjectionState {
    spec: super::super::relation::ReverseHomomorphicSpec,
    projection_infos: Vec<ProjectionInferenceInfo>,
    recovered: Vec<ReverseRecoveredEntry>,
    partial: bool,
    aggregate_candidates: Vec<InferenceCandidate>,
}

impl ReverseProjectionState {
    pub(crate) fn new(spec: super::super::relation::ReverseHomomorphicSpec) -> Self {
        Self {
            spec,
            projection_infos: Vec::new(),
            recovered: Vec::new(),
            partial: false,
            aggregate_candidates: Vec::new(),
        }
    }
}

/// Immutable setup for one inferable parameter. Candidate vectors deliberately
/// do not live here: setup is frozen before the relation key is built.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceInfoSetup {
    /// The content-free identity of the parameter (its `Infer` node).
    pub(in crate::project_semantic_dispatch) param_node: SemanticNodeId,
    /// The parameter's display name (bindings surface by name).
    pub(in crate::project_semantic_dispatch) param_name: Arc<str>,
    /// Call inference owns const policy per declaration parameter. Other
    /// inference domains use the neutral ordinary policy.
    pub(in crate::project_semantic_dispatch) const_policy: ConstParamPolicy,
    /// Whether the declared parameter carries a constraint. A FRESH
    /// primitive-literal candidate stays fresh only for an UNCONSTRAINED
    /// parameter (a constrained parameter's preserved literal is regular
    /// — the upper-bound check regularizes it).
    pub(in crate::project_semantic_dispatch) has_constraint: bool,
}

impl InferenceInfoSetup {
    pub(crate) fn new(param_node: SemanticNodeId, param_name: Arc<str>) -> Self {
        Self {
            param_node,
            param_name,
            const_policy: ConstParamPolicy::NonConst,
            has_constraint: false,
        }
    }

    pub(crate) fn for_call(
        param_node: SemanticNodeId,
        param_name: Arc<str>,
        const_policy: ConstParamPolicy,
        has_constraint: bool,
    ) -> Self {
        Self {
            param_node,
            param_name,
            const_policy,
            has_constraint,
        }
    }
}

/// The single immutable authority for inference-session setup. The frozen
/// context key and the parameter setup records are constructed together once;
/// both relation-key construction and session opening consume this same value.
/// Mutable candidates and reverse-projection journals live only on
/// [`InferenceSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InferenceSessionSetup {
    pub(in crate::project_semantic_dispatch) context_key: InferenceContextKey,
    pub(in crate::project_semantic_dispatch) infos: Arc<[InferenceInfoSetup]>,
}

impl InferenceSessionSetup {
    pub fn new(
        infos: Arc<[InferenceInfoSetup]>,
        variance_phase: VariancePhase,
        pass_kind: InferencePassKind,
        candidate_priority: InferenceCandidatePriority,
        no_infer_mask: NoInferMask,
        const_param_policy: ConstParamPolicy,
        contextual_inference_mode: ContextualInferenceMode,
    ) -> Self {
        let mut seen = FxHashSet::default();
        let infos: Arc<[InferenceInfoSetup]> = Arc::from(
            infos
                .iter()
                .filter(|info| seen.insert(info.param_node))
                .cloned()
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        );
        let inferable_params = InferableParamSetId::new(Arc::from(
            infos
                .iter()
                .map(|info| info.param_node)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        ));
        Self {
            context_key: InferenceContextKey {
                inferable_params,
                variance_phase,
                pass_kind,
                candidate_priority,
                no_infer_mask,
                const_param_policy,
                contextual_inference_mode,
            },
            infos,
        }
    }

    pub(crate) fn context_key(&self) -> &InferenceContextKey {
        &self.context_key
    }
}

/// Mutable per-parameter inference state. Every setup-affecting field lives in
/// [`InferenceInfoSetup`]; this state owns candidate deltas only.
#[derive(Debug)]
struct InferenceInfo {
    param_node: SemanticNodeId,
    param_name: Arc<str>,
    const_policy: ConstParamPolicy,
    has_constraint: bool,
    /// Deposited candidates (session-local deltas — ReturnOnly, never
    /// published as-is).
    candidates: Vec<InferenceCandidate>,
    /// The arity a bare type-parameter rest (`...args: A`) takes from the
    /// call's arguments (the checker's `impliedArity`), set before any
    /// inference runs; a tuple inference splitting `[...A, ...B]` reads it.
    implied_arity: Option<usize>,
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
pub struct InferenceSession {
    /// FRESH primitive-literal deposits accepted at a NAKED top-level
    /// position of an UNCONSTRAINED parameter: `(param, literal)` pairs.
    /// Freshness provenance consumed at fixation to mark a naked declared
    /// return as a fresh literal the caller's return position widens.
    fresh_literal_deposits: Vec<(SemanticNodeId, SemanticNodeId)>,
    /// The transient per-transaction token (content-free, never a key).
    #[allow(dead_code)] // identity for ledger keying; retained for the session stack
    pub id: SessionId,
    /// Frozen setup shared with the relation key that opened this session.
    setup: InferenceSessionSetup,
    /// Per-parameter candidate state.
    infos: Vec<InferenceInfo>,
    /// Reverse-projection journals, present only for a reverse-homomorphic
    /// session selected at open.
    reverse_projection: Option<ReverseProjectionState>,
    /// Immutable fixed bindings retained across staging and commit.
    staged_bindings: Option<Arc<[InferBinding]>>,
    /// The reentry-stack depth when this session opened. Every frame
    /// below it was already open and encloses the session — whichever
    /// frame opened it, a relation root or a call executor — so only a
    /// frame at or above this depth mutates an outer session when it
    /// deposits.
    pub(crate) opened_at_depth: usize,
    /// Session lifecycle.
    pub state: InferenceSessionState,
}

impl InferenceSession {
    pub(crate) fn new(
        id: SessionId,
        setup: InferenceSessionSetup,
        reverse_projection: Option<ReverseProjectionState>,
    ) -> Self {
        let infos = setup
            .infos
            .iter()
            .map(|info| InferenceInfo {
                param_node: info.param_node,
                param_name: Arc::clone(&info.param_name),
                const_policy: info.const_policy,
                has_constraint: info.has_constraint,
                candidates: Vec::new(),
                implied_arity: None,
            })
            .collect();
        Self {
            fresh_literal_deposits: Vec::new(),
            id,
            setup,
            infos,
            reverse_projection,
            staged_bindings: None,
            opened_at_depth: 0,
            state: InferenceSessionState::Collecting,
        }
    }

    /// The exact frozen setup key used by both the enclosing relation key and
    /// this session. Candidate collection cannot mutate it.
    pub(crate) fn context_key(&self) -> &InferenceContextKey {
        self.setup.context_key()
    }

    pub(crate) fn reverse_spec(&self) -> Option<&super::super::relation::ReverseHomomorphicSpec> {
        self.reverse_projection
            .as_ref()
            .map(|reverse| &reverse.spec)
    }

    /// A rollback point over the session's per-parameter candidate lists.
    /// Deposits strictly APPEND onto `InferenceInfo::candidates` (the info
    /// set itself is fixed at session open), so a checkpoint is the ordered
    /// list of candidate lengths and rollback truncates back to it — the
    /// alternative-scoping primitive: a LOSING overload / signature-group
    /// alternative's deposits must not survive into fixation.
    pub(crate) fn checkpoint(&self) -> SessionCheckpoint {
        let reverse = self.reverse_projection.as_ref();
        SessionCheckpoint {
            candidate_lens: self
                .infos
                .iter()
                .map(|info| info.candidates.len())
                .collect(),
            projection_info_len: reverse.map_or(0, |state| state.projection_infos.len()),
            projection_candidate_lens: reverse
                .map(|state| {
                    state
                        .projection_infos
                        .iter()
                        .map(|info| info.candidates.len())
                        .collect()
                })
                .unwrap_or_default(),
            recovered_len: reverse.map_or(0, |state| state.recovered.len()),
            reverse_partial: reverse.is_some_and(|state| state.partial),
            aggregate_candidate_len: reverse.map_or(0, |state| state.aggregate_candidates.len()),
        }
    }

    /// Truncate every parameter's candidate list back to `checkpoint`
    /// (discarding the deposits a failed alternative made). A checkpoint
    /// taken on THIS session always matches the info count; a mismatched
    /// checkpoint (foreign session) is ignored rather than corrupting
    /// state.
    pub(crate) fn rollback_to(&mut self, checkpoint: &SessionCheckpoint) {
        if self.state != InferenceSessionState::Collecting {
            return;
        }
        if checkpoint.candidate_lens.len() != self.infos.len() {
            verter_debug_assert!(
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
        let Some(reverse) = self.reverse_projection.as_mut() else {
            return;
        };
        if checkpoint.projection_candidate_lens.len() != checkpoint.projection_info_len
            || checkpoint.projection_info_len > reverse.projection_infos.len()
        {
            verter_debug_assert!(
                false,
                "reverse projection checkpoint does not match the active session"
            );
            return;
        }
        for (info, len) in reverse
            .projection_infos
            .iter_mut()
            .take(checkpoint.projection_info_len)
            .zip(checkpoint.projection_candidate_lens.iter())
        {
            info.candidates.truncate(*len);
        }
        reverse
            .projection_infos
            .truncate(checkpoint.projection_info_len);
        reverse.recovered.truncate(checkpoint.recovered_len);
        reverse.partial = checkpoint.reverse_partial;
        reverse
            .aggregate_candidates
            .truncate(checkpoint.aggregate_candidate_len);
    }

    /// Register canonical indexed-access nodes for the current reverse
    /// projection. A fresh journal entry is appended even when an older
    /// projection used the same canonical node; deposits select the newest
    /// registration, so nested checkpoints can remove it without mutating an
    /// earlier alternative's state.
    pub(crate) fn register_projection_targets(&mut self, targets: &[SemanticNodeId]) -> bool {
        if self.state != InferenceSessionState::Collecting {
            return false;
        }
        let Some(reverse) = self.reverse_projection.as_mut() else {
            return false;
        };
        for (position, target) in targets.iter().enumerate() {
            if targets[..position].contains(target) {
                continue;
            }
            reverse.projection_infos.push(ProjectionInferenceInfo {
                target_node: *target,
                candidates: Vec::new(),
            });
        }
        true
    }

    /// Whether `node` is registered as a projection target in this session.
    pub(crate) fn is_projection_target(&self, node: SemanticNodeId) -> bool {
        self.reverse_projection.as_ref().is_some_and(|reverse| {
            reverse
                .projection_infos
                .iter()
                .any(|info| info.target_node == node)
        })
    }

    /// Deposit into the newest registration for `target`.
    pub(crate) fn deposit_projection(
        &mut self,
        target: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
        variance: VariancePhase,
    ) -> bool {
        if self.state != InferenceSessionState::Collecting {
            return false;
        }
        let Some(info) = self.reverse_projection.as_mut().and_then(|reverse| {
            reverse
                .projection_infos
                .iter_mut()
                .rev()
                .find(|info| info.target_node == target)
        }) else {
            return false;
        };
        info.candidates.push(InferenceCandidate {
            node: candidate,
            priority,
            variance,
        });
        true
    }

    /// Projection candidates deposited since `checkpoint`.
    pub(crate) fn projection_candidates_since(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Vec<InferenceCandidate> {
        self.reverse_projection
            .as_ref()
            .map(|reverse| {
                reverse.projection_infos[checkpoint.projection_info_len..]
                    .iter()
                    .flat_map(|info| info.candidates.iter().cloned())
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(crate) fn push_recovered(&mut self, recovered: ReverseRecoveredEntry) {
        if self.state != InferenceSessionState::Collecting {
            return;
        }
        if let Some(reverse) = self.reverse_projection.as_mut() {
            reverse.recovered.push(recovered);
        }
    }

    pub(crate) fn mark_reverse_partial(&mut self) {
        if self.state != InferenceSessionState::Collecting {
            return;
        }
        if let Some(reverse) = self.reverse_projection.as_mut() {
            reverse.partial = true;
        }
    }

    pub(crate) fn reverse_is_partial(&self) -> bool {
        self.reverse_projection
            .as_ref()
            .is_some_and(|reverse| reverse.partial)
    }

    pub(crate) fn recovered_since(
        &self,
        checkpoint: &SessionCheckpoint,
    ) -> Vec<ReverseRecoveredEntry> {
        self.reverse_projection
            .as_ref()
            .map(|reverse| reverse.recovered[checkpoint.recovered_len..].to_vec())
            .unwrap_or_default()
    }

    pub(crate) fn deposit_reverse_aggregate(
        &mut self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
    ) -> bool {
        if self.state != InferenceSessionState::Collecting {
            return false;
        }
        let Some(info_index) = self
            .infos
            .iter()
            .position(|info| info.param_node == param_node)
        else {
            return false;
        };
        let Some(reverse) = self.reverse_projection.as_mut() else {
            return false;
        };
        let aggregate = InferenceCandidate {
            node: candidate,
            priority,
            variance: VariancePhase::Covariant,
        };
        reverse.aggregate_candidates.push(aggregate.clone());
        self.infos[info_index].candidates.push(aggregate);
        true
    }

    /// Deposit a candidate for `param` under `priority`, tagged with the
    /// variance of the position it came from. Returns `false` when `param`
    /// is absent from the frozen setup; callers must propagate `Unknown`
    /// rather than treating an inactive declaration as a successful bind.
    pub(crate) fn deposit(
        &mut self,
        param_node: SemanticNodeId,
        candidate: SemanticNodeId,
        priority: InferenceCandidatePriority,
        variance: VariancePhase,
    ) -> bool {
        if self.state != InferenceSessionState::Collecting {
            return false;
        }
        let Some(info) = self
            .infos
            .iter_mut()
            .find(|info| info.param_node == param_node)
        else {
            return false;
        };
        info.candidates.push(InferenceCandidate {
            node: candidate,
            priority,
            variance,
        });
        true
    }

    /// Record a FRESH primitive-literal deposit for `param_node`. A
    /// constrained parameter regularizes its preserved literal, so the
    /// note is a no-op there.
    pub(crate) fn note_fresh_literal_deposit(
        &mut self,
        param_node: SemanticNodeId,
        literal: SemanticNodeId,
    ) {
        let unconstrained = self
            .infos
            .iter()
            .any(|info| info.param_node == param_node && !info.has_constraint);
        if unconstrained {
            self.fresh_literal_deposits.push((param_node, literal));
        }
    }

    /// Whether `param_node` accepted a FRESH deposit of exactly `literal`.
    pub(crate) fn fresh_literal_deposit(
        &self,
        param_node: SemanticNodeId,
        literal: SemanticNodeId,
    ) -> bool {
        self.fresh_literal_deposits.contains(&(param_node, literal))
    }

    /// Whether this session infers `param_node`.
    pub(crate) fn infers(&self, param_node: SemanticNodeId) -> bool {
        self.infos.iter().any(|info| info.param_node == param_node)
    }

    /// Record the arity the call's arguments imply for the bare rest type
    /// parameter `param_node`.
    pub(crate) fn set_implied_arity(&mut self, param_node: SemanticNodeId, arity: usize) {
        if let Some(info) = self
            .infos
            .iter_mut()
            .find(|info| info.param_node == param_node)
        {
            info.implied_arity = Some(arity);
        }
    }

    /// The arity the call's arguments imply for `param_node`, when it is
    /// the call's bare rest type parameter.
    pub(crate) fn implied_arity(&self, param_node: SemanticNodeId) -> Option<usize> {
        self.infos
            .iter()
            .find(|info| info.param_node == param_node)
            .and_then(|info| info.implied_arity)
    }

    pub(crate) fn call_const_policy(&self, param_node: SemanticNodeId) -> Option<ConstParamPolicy> {
        (self.context_key().pass_kind == InferencePassKind::CallApplicability)
            .then(|| {
                self.infos
                    .iter()
                    .find(|info| info.param_node == param_node)
                    .map(|info| info.const_policy)
            })
            .flatten()
    }

    /// Stage deterministic fixation: combine each parameter's candidates into its
    /// final binding through the closed priority ladder — the HIGHEST rung
    /// with candidates wins. Within the chosen rung the combination
    /// variance is PER-CANDIDATE: when any candidate came from a
    /// contravariant position, the contravariant candidates win and
    /// INTERSECT (the TS contravariant-inference rule); otherwise the
    /// covariant candidates union (deduplicated). Every parameter fixes
    /// (unfixed parameters default to `unknown`). Only a collecting session
    /// may stage, and staging makes every candidate journal deposit-inactive.
    pub fn stage_fixation<F>(&mut self, mut combine: F) -> Option<Arc<[InferBinding]>>
    where
        F: FnMut(&[SemanticNodeId], VariancePhase) -> SemanticNodeId,
    {
        if self.state != InferenceSessionState::Collecting {
            return None;
        }
        let mut bindings = Vec::with_capacity(self.infos.len());
        let infos = std::mem::take(&mut self.infos);
        for info in &infos {
            let winning = super::winning_candidates(&info.candidates);
            let (candidates, variance) = winning.inferred_from();
            let bound = combine(candidates, variance);
            bindings.push(InferBinding {
                param: info.param_node,
                name: Arc::clone(&info.param_name),
                bound,
            });
        }
        self.infos = infos;
        let bindings = Arc::from(bindings.into_boxed_slice());
        self.staged_bindings = Some(Arc::clone(&bindings));
        self.state = InferenceSessionState::StagedDeterministic;
        Some(bindings)
    }

    /// The per-parameter fixation inputs of a COLLECTING session, in
    /// declaration order: the parameter node, its display name, the winning
    /// candidate rung, and that rung's combination variance.
    ///
    /// Fixation itself runs OUTSIDE the transaction borrow, because
    /// TypeScript's `getInferredType` needs a relation (an uninferred
    /// parameter falls back to its CONSTRAINT when the default-or-`unknown`
    /// fallback does not satisfy it) and a relation re-enters the
    /// transaction. The computed bindings are staged back through
    /// [`Self::stage_fixation_bindings`].
    pub(crate) fn fixation_inputs(&self) -> Option<Vec<FixationInput>> {
        if self.state != InferenceSessionState::Collecting {
            return None;
        }
        Some(
            self.infos
                .iter()
                .map(|info| FixationInput {
                    param: info.param_node,
                    name: Arc::clone(&info.param_name),
                    candidates: super::winning_candidates(&info.candidates),
                })
                .collect(),
        )
    }

    /// Stage an immutable fixation snapshot computed from
    /// [`Self::fixation_inputs`]. Only a collecting session may stage, and
    /// staging makes every candidate journal deposit-inactive.
    pub(crate) fn stage_fixation_bindings(
        &mut self,
        bindings: Vec<InferBinding>,
    ) -> Option<Arc<[InferBinding]>> {
        if self.state != InferenceSessionState::Collecting {
            return None;
        }
        let bindings = Arc::from(bindings.into_boxed_slice());
        self.staged_bindings = Some(Arc::clone(&bindings));
        self.state = InferenceSessionState::StagedDeterministic;
        Some(bindings)
    }

    /// Commit an immutable staged snapshot after its stability gate. The
    /// caller must immediately drain/publish the owning ledger boundary.
    pub fn commit_completed(&mut self) -> bool {
        if self.state != InferenceSessionState::StagedDeterministic
            || self.staged_bindings.is_none()
        {
            return false;
        }
        self.state = InferenceSessionState::CommittedDeterministic;
        true
    }

    /// Abandon a collecting or staged session. A committed snapshot cannot be
    /// rolled back after publication admission begins.
    pub(crate) fn abandon(&mut self) -> bool {
        if !matches!(
            self.state,
            InferenceSessionState::Collecting | InferenceSessionState::StagedDeterministic
        ) {
            return false;
        }
        for info in &mut self.infos {
            info.candidates.clear();
        }
        if let Some(reverse) = self.reverse_projection.as_mut() {
            reverse.projection_infos.clear();
            reverse.recovered.clear();
            reverse.partial = false;
            reverse.aggregate_candidates.clear();
        }
        self.staged_bindings = None;
        self.state = InferenceSessionState::Abandoned;
        true
    }
}

/// One parameter's fixation inputs — see
/// [`InferenceSession::fixation_inputs`].
pub(crate) struct FixationInput {
    /// The exact declaration node whose binder fixes.
    pub(crate) param: SemanticNodeId,
    /// The binder's display name.
    pub(crate) name: Arc<str>,
    /// The strongest-priority candidates (empty when the parameter is
    /// uninferred).
    pub(crate) candidates: super::WinningCandidates,
}
