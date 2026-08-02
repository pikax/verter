//! `CheckerDispatchTransaction` — the transient per-obligation-root
//! cold-compute frame of the ONE resolver (design `docs/arch/u2-relation-infer-design.md`
//! §2.1), laid out as ONE tagged obligation runtime plus per-domain
//! runtimes:
//!
//! ```text
//! CheckerDispatchTransaction
//! ├── ObligationRuntime          (tagged identities, generic frames/
//! │   │                            backedges/lowlinks, the generic pending
//! │   │                            ledger + watermarks, the tagged
//! │   │                            provisional substitution table)
//! │   ├── ObligationIdentity::{Relate}
//! │   ├── ObligationReentryStack (frames + tagged index)
//! │   └── ObligationPendingLedger
//! └── RelationDomainRuntime      (inference sessions, relation
//!                                 provisional payloads, relation
//!                                 redischarge/fixation state)
//! ```
//!
//! The persistent relation cache lives in the family memo's `Relate` family,
//! keyed by the full §2.7 identity; EVERYTHING in this module is TRANSIENT
//! per-`CheckerDispatchTransaction` state and is NEVER a cache key, NEVER
//! thread-local, NEVER process-wide. The transaction rides the dispatch
//! ([`crate::project_semantic_dispatch::ProjectSemanticDispatch`]) as a
//! `RefCell`, exactly like the dispatch's other cold-compute cycle guards
//! (`instantiate_active`, `carrier_normalizing`, `build_local_taint`).
//!
//! Shapes:
//!
//! - [`ObligationReentryStack`] — the ONE shared re-entry / cycle-id space.
//!   Each node is keyed by its full normalized tagged identity (a `Relate`
//!   node by the full §2.7 key plus its transient inference occurrence).
//! - Assumption-edge recording plus the lowlink (min open-target) tracking
//!   lives on the GENERIC frame — a coinductive SCC whose members span
//!   domains discharges through the same storage, so the per-engine cycle
//!   spaces cannot diverge.
//! - [`InferenceSession`] / [`SessionAdmissionLedger`] — the in-flight
//!   relation inference substrate: a binding-producing relation opens a
//!   session whose SETUP is fully determined by the infer pattern it serves
//!   (see [`InferenceSession`]), so the content-free [`InferenceContextKey`]
//!   fingerprint is well-defined at session OPEN — the transient `SessionId`
//!   stand-in of design §2.2 is not needed for this subset (the setup never
//!   mutates mid-flight; fixation is a single deterministic pass).
//! - [`ObligationPendingLedger`] — popped-but-unpublished SCC members
//!   awaiting their SCC root's close (PROVISIONAL verdicts — caller-return
//!   values + deferral metadata, NEVER the published payload).
//!
//! Execution model (single-threaded per transaction): frames nest strictly,
//! so assumption edges ALWAYS point from a deeper frame to an ancestor on
//! the current stack. The SCC of the frame being popped is therefore the
//! contiguous stack suffix from the minimum open-assumption target — the
//! Tarjan lowlink specialised to a path graph (design §2.3 step 1 "Tarjan
//! over the assumption edges"). Discharge (§2.3 step 3): a member decided
//! with all non-assumptive obligations positive closes POSITIVE
//! (`Assignable` + `CoinductiveCycle`); a member with a negative
//! non-assumptive obligation publishes `NotAssignable` (final); any
//! `Unknown` / budget edge anywhere in the component routes the WHOLE SCC
//! through `ReturnOnly` (nothing publishes).

use std::sync::Arc;

use rustc_hash::{FxHashMap, FxHashSet};

use crate::semantic_query::{
    ConstParamPolicy, ContextualInferenceMode, FlowReturnFailure, FlowReturnKey, FlowReturnResult,
    IndexSignature, InferBinding, InferableParamSetId, InferenceCandidatePriority,
    InferenceContextKey, InferencePassKind, NoInferMask, RecursionOrBudgetCap, RelateMemoKey,
    RelationPayload, SemanticNodeId, SurfaceMember, TupleElement, VariancePhase, VariancePolicy,
};
use crate::semantic_query_memo::InlineRelationFlight;

/// Transient per-transaction session token. Content-free; NEVER enters a
/// published key, a `ReadSetSignature.facts` observation, or any fact
/// signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SessionId(pub(crate) u64);

/// Transient inference occurrence carried by one in-flight relation frame.
/// It affects session-local candidate deposits, but never the persistent
/// `RelateMemoKey` or a published payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct InferenceOccurrence {
    pub(crate) priority: InferenceCandidatePriority,
    pub(crate) variance: VariancePhase,
}

impl InferenceOccurrence {
    pub(crate) const ARGUMENT_COVARIANT: Self = Self {
        priority: InferenceCandidatePriority::Argument,
        variance: VariancePhase::Covariant,
    };
}

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
    /// `exactOptionalPropertyTypes`: when ON, an authored optional write
    /// keeps its explicit `undefined` in the present value (Disabled drops
    /// it). Not part of the `strict` family — threaded along the same host
    /// knob so every consumer reads one configuration.
    pub(crate) exact_optional_property_types: bool,
}

impl StrictFamilyConfig {
    /// The default regime — matches the pre-activation engine's behavior
    /// (null/undefined isolated; contravariant function parameters).
    pub(crate) const TS_STRICT: Self = Self {
        strict_null_checks: true,
        strict_function_types: true,
        exact_optional_property_types: false,
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

// ---------------------------------------------------------------------------
// Tagged obligation identity + generic frame/pending machinery
// ---------------------------------------------------------------------------

/// The tagged full identity of one in-flight obligation on the shared
/// reentry stack. Reentry identity IS this value exactly: a `Relate`
/// obligation is the full §2.7 key plus its transient inference occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ObligationIdentity {
    /// A relation judgement in flight.
    Relate {
        /// The full §2.7 identity.
        key: RelateMemoKey,
        /// The transient occurrence axis (session-local orientation).
        occurrence: InferenceOccurrence,
    },
    /// A whole-function `FlowReturn` evaluation in flight. Reentry
    /// identity IS the `FlowReturnKey` exactly.
    FlowReturn(FlowReturnKey),
}

impl ObligationIdentity {
    /// The relation identity parts, when this obligation is a relation.
    pub(crate) fn as_relate(&self) -> Option<(&RelateMemoKey, InferenceOccurrence)> {
        match self {
            Self::Relate { key, occurrence } => Some((key, *occurrence)),
            Self::FlowReturn(_) => None,
        }
    }

    /// The flow-return key, when this obligation is a flow evaluation.
    pub(crate) fn as_flow_return(&self) -> Option<&FlowReturnKey> {
        match self {
            Self::Relate { .. } => None,
            Self::FlowReturn(key) => Some(key),
        }
    }

    /// The relation identity parts. Panics when the obligation is not a
    /// relation — callers on a relation-only code path uphold that the
    /// frames they pop are relation frames.
    pub(crate) fn expect_relate(&self) -> (&RelateMemoKey, InferenceOccurrence) {
        self.as_relate()
            .expect("relation code path popped a non-relation obligation frame")
    }
}

/// The relation-domain payload of one in-flight frame.
#[derive(Debug)]
pub(crate) struct RelationFrameState {
    /// This frame deposited inference candidates into the active session
    /// (a session-local delta — admission row 7: ReturnOnly, never
    /// published).
    pub(crate) session_delta: bool,
    /// The session this frame OPENED (it is the binding root), if any.
    pub(crate) opened_session: Option<SessionId>,
    /// Store-owned family admission claimed for a non-binding inline
    /// relation. It follows the member through SCC deferral and is either
    /// completed by the root's batched publish or explicitly aborted.
    pub(crate) inline_flight: Option<InlineRelationFlight>,
}

impl RelationFrameState {
    fn new() -> Self {
        Self {
            session_delta: false,
            opened_session: None,
            inline_flight: None,
        }
    }
}

/// The flow-return-domain payload of one in-flight frame. The ordered
/// return-site contributor map is evaluated inside the frame's compute
/// and decided at pop: a recursive same-slot edge records as a
/// coinductive hold (never a contributor, never a failure), so the
/// outcome is final when the frame closes.
#[derive(Debug, Default)]
pub(crate) struct FlowReturnFrameState {
    /// Store-owned family admission claimed for a non-root inline flow
    /// evaluation. It follows the member through SCC deferral and is
    /// either completed by the root's batched publish or explicitly
    /// aborted.
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
}

/// The domain payload of one in-flight frame.
#[derive(Debug)]
pub(crate) enum ObligationFrameDomain {
    /// Relation frame state.
    Relate(RelationFrameState),
    /// Flow-return frame state.
    FlowReturn(FlowReturnFrameState),
}

/// One in-flight obligation frame on the reentry stack — a tagged
/// full identity plus the GENERIC coinductive bookkeeping its SCC
/// discharge needs (assumption edges + lowlink + drain watermark) and its
/// domain payload.
#[derive(Debug)]
pub(crate) struct ObligationFrame {
    /// The tagged full identity this frame computes.
    pub(crate) identity: ObligationIdentity,
    /// Assumption edges recorded by this frame's subtree: stack indices of
    /// the frames this subtree ASSUMED hold (back-edges).
    pub(crate) assumption_targets: Vec<usize>,
    /// The Tarjan lowlink: the minimum stack index any open assumption in
    /// this frame's subtree targets. `Some(own)` or `None` at pop ⇒ this
    /// frame is its SCC's root.
    pub(crate) min_open_target: Option<usize>,
    /// This frame's reducer consumed a budget edge — the typed cap that
    /// stopped the obligation. Poisons the whole SCC (ReturnOnly); the ROOT
    /// surfaces the public `BudgetExceeded` payload.
    pub(crate) budget_cap: Option<RecursionOrBudgetCap>,
    /// The `ObligationPendingLedger` pending length at this frame's PUSH —
    /// the drain watermark. Everything deposited at `pending[watermark..]`
    /// was deposited by THIS frame's subtree (frames nest strictly), so an
    /// SCC-root close drains exactly its own suffix. Stack indices
    /// recycle after pops; this watermark does not, so a sibling frame
    /// that reuses a popped member's stack index can never steal that
    /// member from a still-open outer SCC.
    pub(crate) pending_watermark: usize,
    /// The domain payload.
    pub(crate) domain: ObligationFrameDomain,
}

impl ObligationFrame {
    /// The relation frame state, when this is a relation frame.
    pub(crate) fn relation(&self) -> Option<&RelationFrameState> {
        match &self.domain {
            ObligationFrameDomain::Relate(state) => Some(state),
            ObligationFrameDomain::FlowReturn(_) => None,
        }
    }

    /// The relation frame state mutably, when this is a relation frame.
    pub(crate) fn relation_mut(&mut self) -> Option<&mut RelationFrameState> {
        match &mut self.domain {
            ObligationFrameDomain::Relate(state) => Some(state),
            ObligationFrameDomain::FlowReturn(_) => None,
        }
    }

    /// The flow-return frame state mutably, when this is a flow frame.
    pub(crate) fn flow_return_mut(&mut self) -> Option<&mut FlowReturnFrameState> {
        match &mut self.domain {
            ObligationFrameDomain::Relate(_) => None,
            ObligationFrameDomain::FlowReturn(state) => Some(state),
        }
    }
}

/// The ONE shared re-entry / cycle-id space (design §2.1). Heap-backed,
/// per-`CheckerDispatchTransaction`, keyed by tagged full identity.
#[derive(Debug, Default)]
pub(crate) struct ObligationReentryStack {
    frames: Vec<ObligationFrame>,
    index: FxHashMap<ObligationIdentity, usize>,
}

impl ObligationReentryStack {
    /// The stack index of `identity` when that tagged obligation is already
    /// in flight on THIS transaction.
    pub(crate) fn find(&self, identity: &ObligationIdentity) -> Option<usize> {
        self.index.get(identity).copied()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.frames.is_empty()
    }

    pub(crate) fn depth(&self) -> usize {
        self.frames.len()
    }

    /// Push a fresh RELATION frame for `(key, occurrence)` with the pending
    /// ledger's current length as its drain watermark; returns its stack
    /// index.
    pub(crate) fn push_relate(
        &mut self,
        key: RelateMemoKey,
        occurrence: InferenceOccurrence,
        pending_watermark: usize,
    ) -> usize {
        let identity = ObligationIdentity::Relate { key, occurrence };
        let idx = self.frames.len();
        self.frames.push(ObligationFrame {
            identity: identity.clone(),
            assumption_targets: Vec::new(),
            min_open_target: None,
            budget_cap: None,
            pending_watermark,
            domain: ObligationFrameDomain::Relate(RelationFrameState::new()),
        });
        self.index.insert(identity, idx);
        idx
    }

    /// Push a fresh FLOW-RETURN frame for `key` with the pending ledger's
    /// current length as its drain watermark; returns its stack index.
    pub(crate) fn push_flow_return(
        &mut self,
        key: FlowReturnKey,
        pending_watermark: usize,
    ) -> usize {
        let identity = ObligationIdentity::FlowReturn(key);
        let idx = self.frames.len();
        self.frames.push(ObligationFrame {
            identity: identity.clone(),
            assumption_targets: Vec::new(),
            min_open_target: None,
            budget_cap: None,
            pending_watermark,
            domain: ObligationFrameDomain::FlowReturn(FlowReturnFrameState::default()),
        });
        self.index.insert(identity, idx);
        idx
    }

    /// Pop the top frame. Callers uphold strict LIFO nesting (the
    /// transaction's execution model).
    pub(crate) fn pop(&mut self) -> ObligationFrame {
        let frame = self.frames.pop().expect("reentry stack underflow");
        self.index.remove(&frame.identity);
        frame
    }

    pub(crate) fn top_mut(&mut self) -> Option<&mut ObligationFrame> {
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

    /// The frame at `idx`, when in range.
    pub(crate) fn frame(&self, idx: usize) -> Option<&ObligationFrame> {
        self.frames.get(idx)
    }

    /// The frame at `idx` mutably, when in range.
    pub(crate) fn frame_mut_for_update(&mut self, idx: usize) -> Option<&mut ObligationFrame> {
        self.frames.get_mut(idx)
    }

    /// The nearest open RELATION frame's identity parts, walking from the
    /// top of the GENERIC stack down. Relation subkeys inherit their axes
    /// from the nearest open `Relate` ancestor — never the untyped top of
    /// a mixed stack (a non-relation frame between two relation frames
    /// carries no relation axes to inherit).
    pub(crate) fn nearest_relate(&self) -> Option<(&RelateMemoKey, InferenceOccurrence)> {
        self.frames
            .iter()
            .rev()
            .find_map(|frame| frame.identity.as_relate())
    }
}

/// A popped SCC member awaiting its SCC root's close — the PROVISIONAL
/// deferral record (design §2.3 step 4): a caller-return value plus
/// deferral metadata, NEVER the published payload. The published payload
/// is produced at the batched-publish instant by the discharge against
/// converged state.
#[derive(Debug)]
pub(crate) struct PendingObligation {
    /// The member's tagged full identity.
    pub(crate) identity: ObligationIdentity,
    /// The domain deferral payload.
    pub(crate) domain: PendingObligationDomain,
}

/// The relation-domain deferral payload of a popped member.
#[derive(Debug)]
pub(crate) struct RelationPendingState {
    /// The member's provisional discharged verdict at pop.
    pub(crate) verdict: PendingVerdict,
    /// Session-local delta (row 7) — never publishes.
    pub(crate) session_delta: bool,
    /// The member opened session `Some(..)` (a binding member).
    pub(crate) opened_session: Option<SessionId>,
    /// Store-owned admission for this inline non-binding member.
    pub(crate) inline_flight: Option<InlineRelationFlight>,
}

/// The decided outcome of a popped flow-return member. Decided at pop:
/// a same-slot recursive backedge is a coinductive hold, so the
/// contributor set is complete when the frame closes — the seed check
/// runs once, at pop.
#[derive(Debug, Clone)]
pub(crate) enum FlowReturnPendingOutcome {
    /// Complete evaluation (the admitted shape).
    Complete(FlowReturnResult),
    /// Typed failure — `ReturnOnly`, never admitted.
    Degraded(FlowReturnFailure),
}

/// The flow-return-domain deferral payload of a popped member.
#[derive(Debug)]
pub(crate) struct FlowReturnPendingState {
    /// The member's decided outcome at pop.
    pub(crate) outcome: FlowReturnPendingOutcome,
    /// Store-owned admission for this inline member.
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
    /// The coinductive hold targets the member's evaluation met (in-flight
    /// callees and direct self-calls) — the SCC close discharges an
    /// empty-cycle member on its targets' admitted returns.
    pub(crate) holds: Vec<FlowReturnKey>,
    /// The member's own file roots — the published component's self-roots
    /// are the UNION of every drained member's roots, so a cross-file edit
    /// invalidates the whole component.
    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
}

/// The domain deferral payload of a popped member.
#[derive(Debug)]
pub(crate) enum PendingObligationDomain {
    /// Relation deferral state.
    Relate(RelationPendingState),
    /// Flow-return deferral state.
    FlowReturn(FlowReturnPendingState),
}

/// The per-`CheckerDispatchTransaction` pending ledger (design §2.3 step 4
/// R-a): accumulates popped-but-unpublished TAGGED members; the SCC root's
/// close computes each member's published outcome and routes the batch.
#[derive(Debug, Default)]
pub(crate) struct ObligationPendingLedger {
    pending: Vec<PendingObligation>,
}

impl ObligationPendingLedger {
    pub(crate) fn deposit(&mut self, member: PendingObligation) {
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
    pub(crate) fn drain_scc(&mut self, watermark: usize) -> Vec<PendingObligation> {
        let split = watermark.min(self.pending.len());
        self.pending.split_off(split)
    }
}

/// One entry of the tagged provisional substitution table an SCC close
/// installs for its members' re-discharge (design §2.3 step 4 — the
/// converged verdicts a re-running member consults instead of re-entering
/// the SCC).
#[derive(Debug, Clone)]
pub(crate) enum ProvisionalVerdict {
    /// A relation step verdict.
    Relate(RelationStep),
}

/// The ONE tagged provisional substitution table: SCC members re-discharge
/// deepest-first/root-last against it across domains.
pub(crate) type ProvisionalSubstitution = FxHashMap<ObligationIdentity, ProvisionalVerdict>;

/// Read a RELATION verdict from the tagged table.
pub(crate) fn provisional_relate_step<'a>(
    substitution: &'a ProvisionalSubstitution,
    key: &RelateMemoKey,
    occurrence: InferenceOccurrence,
) -> Option<&'a RelationStep> {
    match substitution.get(&ObligationIdentity::Relate {
        key: key.clone(),
        occurrence,
    }) {
        Some(ProvisionalVerdict::Relate(step)) => Some(step),
        None => None,
    }
}

/// The generic obligation runtime: tagged identities, generic frames /
/// backedges / lowlinks, the generic pending ledger + watermarks, and the
/// tagged provisional substitution table. Domain runtimes own their
/// verdict algebra; this runtime owns the SCC topology.
#[derive(Debug, Default)]
pub(crate) struct ObligationRuntime {
    stack: ObligationReentryStack,
    pending: ObligationPendingLedger,
    substitution: ProvisionalSubstitution,
}

impl ObligationRuntime {
    pub(crate) fn stack(&self) -> &ObligationReentryStack {
        &self.stack
    }

    pub(crate) fn stack_mut(&mut self) -> &mut ObligationReentryStack {
        &mut self.stack
    }

    pub(crate) fn pending(&self) -> &ObligationPendingLedger {
        &self.pending
    }

    pub(crate) fn pending_mut(&mut self) -> &mut ObligationPendingLedger {
        &mut self.pending
    }

    pub(crate) fn substitution(&self) -> &ProvisionalSubstitution {
        &self.substitution
    }

    /// Whether the next obligation push is a ROOT push (the generic stack
    /// is empty). Root versus inline is decided HERE, at the generic
    /// transaction — a nested obligation of any domain under an open frame
    /// is inline because the generic root owns its eventual drain.
    pub(crate) fn decides_root(&self) -> bool {
        self.stack.is_empty()
    }

    /// Record an assumption edge `top → target` (the coinductive "assume
    /// it holds" step, design §2.2): the caller's accumulator is marked
    /// `OpenAssumption(target)` — transient, NEVER written to a published
    /// `ReadSetSignature.facts`.
    pub(crate) fn record_assumption(&mut self, target: usize) {
        if let Some(frame) = self.stack.top_mut() {
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
    /// open against the parent after the child pops. This folds through
    /// EVERY generic frame, including non-relation frames between two
    /// relation frames.
    pub(crate) fn propagate_lowlink(&mut self, child_min_open: Option<usize>) {
        let Some(child_min_open) = child_min_open else {
            return;
        };
        if let Some(frame) = self.stack.top_mut() {
            frame.min_open_target = Some(
                frame
                    .min_open_target
                    .map_or(child_min_open, |current| current.min(child_min_open)),
            );
        }
    }

    /// Install one SCC re-discharge context (the tagged substitution table)
    /// and return the complete previous context so a nested re-discharge
    /// can restore its caller exactly. The relation occurrence rail rides
    /// the relation domain runtime; this installs the tagged table only.
    pub(crate) fn replace_substitution(
        &mut self,
        substitution: ProvisionalSubstitution,
    ) -> ProvisionalSubstitution {
        std::mem::replace(&mut self.substitution, substitution)
    }

    /// Restore a previously saved substitution table.
    pub(crate) fn restore_substitution(&mut self, saved: ProvisionalSubstitution) {
        self.substitution = saved;
    }
}

// ---------------------------------------------------------------------------
// Relation domain runtime
// ---------------------------------------------------------------------------

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
pub(crate) struct ReverseProjectionState {
    spec: super::relation::ReverseHomomorphicSpec,
    projection_infos: Vec<ProjectionInferenceInfo>,
    recovered: Vec<ReverseRecoveredEntry>,
    partial: bool,
    aggregate_candidates: Vec<InferenceCandidate>,
}

impl ReverseProjectionState {
    pub(crate) fn new(spec: super::relation::ReverseHomomorphicSpec) -> Self {
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
pub(crate) struct InferenceInfoSetup {
    /// The content-free identity of the parameter (its `Infer` node).
    param_node: SemanticNodeId,
    /// The parameter's display name (bindings surface by name).
    param_name: Arc<str>,
}

impl InferenceInfoSetup {
    pub(crate) fn new(param_node: SemanticNodeId, param_name: Arc<str>) -> Self {
        Self {
            param_node,
            param_name,
        }
    }
}

/// The single immutable authority for inference-session setup. The frozen
/// context key and the parameter setup records are constructed together once;
/// both relation-key construction and session opening consume this same value.
/// Mutable candidates and reverse-projection journals live only on
/// [`InferenceSession`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InferenceSessionSetup {
    context_key: InferenceContextKey,
    infos: Arc<[InferenceInfoSetup]>,
}

impl InferenceSessionSetup {
    pub(crate) fn new(
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
    /// Deposited candidates (session-local deltas — ReturnOnly, never
    /// published as-is).
    candidates: Vec<InferenceCandidate>,
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
    /// Frozen setup shared with the relation key that opened this session.
    setup: InferenceSessionSetup,
    /// Per-parameter candidate state.
    infos: Vec<InferenceInfo>,
    /// Reverse-projection journals, present only for a reverse-homomorphic
    /// session selected at open.
    reverse_projection: Option<ReverseProjectionState>,
    /// Session lifecycle.
    pub(crate) state: InferenceSessionState,
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
                candidates: Vec::new(),
            })
            .collect();
        Self {
            id,
            setup,
            infos,
            reverse_projection,
            state: InferenceSessionState::InProgress,
        }
    }

    /// The exact frozen setup key used by both the enclosing relation key and
    /// this session. Candidate collection cannot mutate it.
    pub(crate) fn context_key(&self) -> &InferenceContextKey {
        self.setup.context_key()
    }

    pub(crate) fn reverse_spec(&self) -> Option<&super::relation::ReverseHomomorphicSpec> {
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
        let Some(reverse) = self.reverse_projection.as_mut() else {
            return;
        };
        if checkpoint.projection_candidate_lens.len() != checkpoint.projection_info_len
            || checkpoint.projection_info_len > reverse.projection_infos.len()
        {
            debug_assert!(
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
        if let Some(reverse) = self.reverse_projection.as_mut() {
            reverse.recovered.push(recovered);
        }
    }

    pub(crate) fn mark_reverse_partial(&mut self) {
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
            let (candidates, variance) = select_inference_candidates(&info.candidates);
            let bound = combine(&candidates, variance);
            bindings.push(InferBinding {
                param: info.param_node,
                name: Arc::clone(&info.param_name),
                bound,
            });
        }
        self.infos = infos;
        self.state = InferenceSessionState::CompletedDeterministic;
        bindings
    }
}

/// Select the winning priority rung and combination variance for a candidate
/// list. This is shared by top-level fixation and reverse-projection recovery.
pub(crate) fn select_inference_candidates(
    candidates: &[InferenceCandidate],
) -> (Vec<SemanticNodeId>, VariancePhase) {
    let Some(priority) = candidates
        .iter()
        .map(|candidate| candidate.priority)
        .max_by_key(|priority| crate::semantic_query::inference_candidate_precedence(*priority))
    else {
        return (Vec::new(), VariancePhase::Covariant);
    };
    let chosen: Vec<&InferenceCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.priority == priority)
        .collect();
    let contravariant: Vec<SemanticNodeId> = chosen
        .iter()
        .filter(|candidate| candidate.variance == VariancePhase::Contravariant)
        .map(|candidate| candidate.node)
        .collect();
    if contravariant.is_empty() {
        (
            chosen.iter().map(|candidate| candidate.node).collect(),
            VariancePhase::Covariant,
        )
    } else {
        (contravariant, VariancePhase::Contravariant)
    }
}

/// The decided provisional verdict of a popped member.
#[derive(Debug, Clone)]
pub(crate) enum PendingVerdict {
    Assignable { bindings: Arc<[InferBinding]> },
    NotAssignable,
    Unknown,
    BudgetExceeded(RecursionOrBudgetCap),
}

/// Session-close publication stability gate. The SCC-close snapshot is
/// provisional: redischarge may publish only when its polarity and complete
/// fixed-binding snapshot are unchanged. Proof shape is then deterministic
/// from that verdict plus the unchanged SCC key set; `Unknown`/budget states
/// are never stable publication candidates.
pub(crate) fn redischarge_is_stable(
    provisional: &PendingVerdict,
    redischarge: &PendingVerdict,
) -> bool {
    match (provisional, redischarge) {
        (
            PendingVerdict::Assignable {
                bindings: provisional,
            },
            PendingVerdict::Assignable {
                bindings: redischarge,
            },
        ) => provisional == redischarge,
        (PendingVerdict::NotAssignable, PendingVerdict::NotAssignable) => true,
        _ => false,
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
    pub(crate) inline_flight: Option<InlineRelationFlight>,
}

/// The relation domain runtime: inference sessions, relation provisional
/// payloads, and relation redischarge/fixation state. The SCC topology it
/// runs on lives in the generic [`ObligationRuntime`].
#[derive(Debug, Default)]
pub(crate) struct RelationDomainRuntime {
    /// The active inference-session stack.
    pub(crate) sessions: Vec<InferenceSession>,
    /// Per-session deferred-admission ledger.
    pub(crate) session_admission: SessionAdmissionLedger,
    /// SCC-closed members queued for the root's batched publish drain.
    pub(crate) completed_members: Vec<CompletedSccMember>,
    /// The normalized strict-family configuration in force (RI-10).
    pub(crate) strict: Option<StrictFamilyConfig>,
    /// Virtual root occurrence used while an SCC member re-discharges after
    /// its real frame has been popped. The recorded stack depth lets nested
    /// frames take over normally while preserving the popped member's
    /// orientation at the virtual root.
    pub(crate) redischarge_occurrence: Option<(usize, InferenceOccurrence)>,
    /// Per-target-node memo of the `infer`-pattern detection (a pure
    /// function of the pattern; avoids rescanning per ask).
    pub(crate) pattern_cache: FxHashMap<SemanticNodeId, Option<super::relation::InferPatternInfo>>,
    next_session_id: u64,
}

/// Saved transient state for a nested SCC re-discharge. Persistent relation
/// identity is unaffected; this restores only the virtual occurrence and
/// substitution rails used by the enclosing re-discharge.
pub(crate) struct SavedRedischargeContext {
    substitution: ProvisionalSubstitution,
    occurrence: Option<(usize, InferenceOccurrence)>,
}

/// The per-obligation-root cold-compute frame (design §2.1 /
/// `native-typeinfo-parity.md` §4.2): ONE tagged obligation runtime plus
/// the per-domain runtimes. Transient; NEVER a cache key.
/// A flow member whose SCC closed cleanly, queued for the batched
/// publish the relation ROOT performs after its family-memo publish
/// lands (the member entries ride the ROOT's SCC-union carrier — the
/// published fact set is the UNION of all SCC members' observed facts).
#[derive(Debug)]
pub(crate) struct CompletedFlowReturnMember {
    pub(crate) key: FlowReturnKey,
    pub(crate) result: FlowReturnResult,
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
    /// The member's own file roots (the SCC-union carrier's self-roots
    /// include them even when the ROOT is a relation obligation).
    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
}

/// The flow-return domain runtime: the completed flow members queued
/// for the relation root's batched publish. Contributor maps ride the
/// in-flight frames and the tagged pending ledger (a popped member's
/// decided outcome is final at pop — same-slot recursive edges are
/// coinductive holds, never unresolved failures), so the domain owns
/// no parallel contributor ledger.
#[derive(Debug, Default)]
pub(crate) struct FlowReturnDomainRuntime {
    /// SCC-closed flow members queued for the root's batched publish
    /// drain.
    pub(crate) completed_members: Vec<CompletedFlowReturnMember>,
    /// The typed failure of the in-flight machinery ROOT's close — the
    /// caller-return payload channel: the family memo admits only COMPLETE
    /// values, so a degraded root's typed failure rides the transaction to
    /// the demanding caller (never admitted, never a cache value).
    pub(crate) last_root_failure: Option<crate::semantic_query::FlowReturnFailure>,
}

/// The per-obligation-root cold-compute frame (design §2.1 /
/// `native-typeinfo-parity.md` §4.2): ONE tagged obligation runtime plus
/// the per-domain runtimes. Transient; NEVER a cache key.
#[derive(Debug, Default)]
pub(crate) struct CheckerDispatchTransaction {
    /// The generic obligation runtime (tagged identities, frames, pending
    /// ledger, watermarks, the tagged provisional substitution table).
    pub(crate) obligations: ObligationRuntime,
    /// The relation domain runtime.
    pub(crate) relation: RelationDomainRuntime,
    /// The flow-return domain runtime.
    pub(crate) flow: FlowReturnDomainRuntime,
}

/// One entry of a tagged flow component awaiting its equation fixed
/// point: the member's current outcome (a Complete outcome IS its
/// concrete seed join; a hold-only EmptyCycle has no seed) and the
/// coinductive hold targets its evaluation met.
#[derive(Debug)]
pub(super) struct FlowDischargeEntry {
    /// The member's flow identity.
    pub(super) key: crate::semantic_query::FlowReturnKey,
    /// The member's outcome (updated in place by the discharge).
    pub(super) outcome: FlowReturnPendingOutcome,
    /// The member's coinductive hold targets.
    pub(super) holds: Vec<crate::semantic_query::FlowReturnKey>,
}

impl CheckerDispatchTransaction {
    pub(crate) fn reentry(&self) -> &ObligationReentryStack {
        self.obligations.stack()
    }

    pub(crate) fn reentry_mut(&mut self) -> &mut ObligationReentryStack {
        self.obligations.stack_mut()
    }

    pub(crate) fn alloc_session_id(&mut self) -> SessionId {
        self.relation.next_session_id += 1;
        SessionId(self.relation.next_session_id)
    }

    /// The active (innermost `InProgress`) session, if any.
    pub(crate) fn active_session_mut(&mut self) -> Option<&mut InferenceSession> {
        self.relation
            .sessions
            .iter_mut()
            .rev()
            .find(|s| s.state == InferenceSessionState::InProgress)
    }

    pub(crate) fn active_session(&self) -> Option<&InferenceSession> {
        self.relation
            .sessions
            .iter()
            .rev()
            .find(|s| s.state == InferenceSessionState::InProgress)
    }

    /// The session the frame at `idx` opened, if any.
    pub(crate) fn frame_opened_session(&self, idx: usize) -> Option<SessionId> {
        self.reentry()
            .frame(idx)
            .and_then(|frame| frame.relation())
            .and_then(|state| state.opened_session)
    }

    /// Mark the frame at `idx` as having opened session `session`.
    pub(crate) fn note_opened_session(&mut self, idx: usize, session: SessionId) {
        if let Some(state) = self
            .reentry_mut()
            .frame_mut_for_update(idx)
            .and_then(ObligationFrame::relation_mut)
        {
            state.opened_session = Some(session);
        }
    }

    pub(crate) fn note_inline_flight(&mut self, idx: usize, flight: Option<InlineRelationFlight>) {
        if let Some(state) = self
            .reentry_mut()
            .frame_mut_for_update(idx)
            .and_then(ObligationFrame::relation_mut)
        {
            state.inline_flight = flight;
        }
    }

    pub(crate) fn note_session_delta_range(&mut self, start: usize, end: usize) {
        for idx in start..end {
            if let Some(state) = self
                .reentry_mut()
                .frame_mut_for_update(idx)
                .and_then(ObligationFrame::relation_mut)
            {
                state.session_delta = true;
            }
        }
    }

    /// Mark every active non-owner frame when an accepted candidate write
    /// mutates an outer session.
    pub(crate) fn note_candidate_write(&mut self, active_id: Option<SessionId>) {
        let depth = self.reentry().depth();
        if depth == 0 {
            return;
        }
        let owner = (0..depth).rev().find(|index| {
            self.frame_opened_session(*index)
                .is_some_and(|opened| Some(opened) == active_id)
        });
        let first_non_owner = owner.map_or(0, |index| index + 1);
        self.note_session_delta_range(first_non_owner, depth);
    }

    /// Install one SCC re-discharge context and return the complete previous
    /// context so a nested re-discharge can restore its caller exactly.
    pub(crate) fn replace_redischarge_context(
        &mut self,
        substitution: ProvisionalSubstitution,
        occurrence: InferenceOccurrence,
    ) -> SavedRedischargeContext {
        let previous_substitution = self.obligations.replace_substitution(substitution);
        let depth = self.reentry().depth();
        let previous_occurrence = self
            .relation
            .redischarge_occurrence
            .replace((depth, occurrence));
        SavedRedischargeContext {
            substitution: previous_substitution,
            occurrence: previous_occurrence,
        }
    }

    pub(crate) fn restore_redischarge_context(&mut self, saved: SavedRedischargeContext) {
        self.obligations.restore_substitution(saved.substitution);
        self.relation.redischarge_occurrence = saved.occurrence;
    }
}

#[cfg(test)]
#[path = "dispatch_txn_tests.rs"]
mod dispatch_txn_tests;
