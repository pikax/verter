//! `CheckerDispatchTransaction` — the transient per-obligation-root
//! cold-compute frame of the ONE resolver (design `.claude/skills/type-resolution/SKILL.md`
//! §2.1), laid out as ONE tagged obligation runtime plus per-domain
//! runtimes:
//!
//! ```text
//! CheckerDispatchTransaction
//! ├── ObligationRuntime          (tagged identities, generic frames/
//! │   │                            backedges/lowlinks, the generic pending
//! │   │                            ledger + watermarks, the tagged
//! │   │                            provisional substitution table)
//! │   ├── ObligationIdentity::{Relate, FlowReturn, ResolveCall}
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
    CanonicalTypeSubstitution, ConstParamPolicy, ContextualInferenceMode, FlowReturnFailure,
    FlowReturnKey, FlowReturnResult, IndexSignature, InferBinding, InferableParamSetId,
    InferenceCandidatePriority, InferenceContextKey, InferencePassKind, NoInferMask,
    RecursionOrBudgetCap, RelateMemoKey, RelationPayload, ResolveCallFailure, ResolveCallKey,
    ResolvedCallResult, SemanticNodeId, SignatureCandidateOrigin, SurfaceMember, TupleElement,
    VariancePhase, VariancePolicy,
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
    Assumed(RelationAssumptionEvidence),
}

/// Exact transient dependency evidence carried by a coinductive relation
/// assumption. The suffix starts at the intercepted ancestor and ends at the
/// current demander; it is never admitted into a memo value.
#[derive(Debug, Clone)]
pub(crate) struct RelationAssumptionEvidence {
    closure: Arc<[ObligationIdentity]>,
}

impl RelationAssumptionEvidence {
    pub(crate) fn reaches_flow_function(
        &self,
        function: &crate::semantic_query::FlowFunctionSlotIdentity,
    ) -> bool {
        self.closure.iter().any(|identity| {
            matches!(identity, ObligationIdentity::FlowReturn(key) if &key.function == function)
        })
    }

    #[cfg(test)]
    pub(crate) fn empty_for_tests() -> Self {
        Self {
            closure: Arc::from([]),
        }
    }
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
    /// A call-resolution execution. Its transparent generic-runtime frame
    /// owns no pending/publication payload, but makes every relation opened by
    /// the executor structurally inline.
    ResolveCall(ResolveCallKey),
}

impl ObligationIdentity {
    /// The relation identity parts, when this obligation is a relation.
    pub(crate) fn as_relate(&self) -> Option<(&RelateMemoKey, InferenceOccurrence)> {
        match self {
            Self::Relate { key, occurrence } => Some((key, *occurrence)),
            Self::FlowReturn(_) | Self::ResolveCall(_) => None,
        }
    }

    /// The flow-return key, when this obligation is a flow evaluation.
    pub(crate) fn as_flow_return(&self) -> Option<&FlowReturnKey> {
        match self {
            Self::Relate { .. } => None,
            Self::FlowReturn(key) => Some(key),
            Self::ResolveCall(_) => None,
        }
    }

    /// The call-resolution key, when this obligation is a call.
    pub(crate) fn as_resolve_call(&self) -> Option<&ResolveCallKey> {
        match self {
            Self::Relate { .. } | Self::FlowReturn(_) => None,
            Self::ResolveCall(key) => Some(key),
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
    /// Tagged return dependencies discovered by indexed call evaluation
    /// while this flow frame is active.
    pub(crate) holds: Vec<ReturnObligationIdentity>,
    /// The frame's own installed flow demand carrier (handle + plan +
    /// provenance), installed by the production demand-preparation step
    /// (`prepare_flow_return_demand`) after the frame opens. `None` when
    /// the demand could not be planned — the evaluation still runs, but
    /// no proof can mint, so the close finalizes unproven.
    pub(crate) flow_demand: Option<flow_obligation_state::FlowDemandCarrier>,
}

/// The call-resolution-domain payload of one in-flight frame.
#[derive(Debug, Default)]
pub(crate) struct ResolveCallFrameState {
    /// Store-owned family admission claimed for a non-root inline call.
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
}

/// The domain payload of one in-flight frame.
#[derive(Debug)]
pub(crate) enum ObligationFrameDomain {
    /// Relation frame state.
    Relate(RelationFrameState),
    /// Flow-return frame state.
    FlowReturn(FlowReturnFrameState),
    /// Call-resolution frame state.
    ResolveCall(ResolveCallFrameState),
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
            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => None,
        }
    }

    /// The relation frame state mutably, when this is a relation frame.
    pub(crate) fn relation_mut(&mut self) -> Option<&mut RelationFrameState> {
        match &mut self.domain {
            ObligationFrameDomain::Relate(state) => Some(state),
            ObligationFrameDomain::FlowReturn(_) | ObligationFrameDomain::ResolveCall(_) => None,
        }
    }

    /// The flow-return frame state mutably, when this is a flow frame.
    pub(crate) fn flow_return_mut(&mut self) -> Option<&mut FlowReturnFrameState> {
        match &mut self.domain {
            ObligationFrameDomain::Relate(_) => None,
            ObligationFrameDomain::FlowReturn(state) => Some(state),
            ObligationFrameDomain::ResolveCall(_) => None,
        }
    }

    /// The call-resolution frame state mutably, when this is a call frame.
    pub(crate) fn resolve_call_mut(&mut self) -> Option<&mut ResolveCallFrameState> {
        match &mut self.domain {
            ObligationFrameDomain::Relate(_) | ObligationFrameDomain::FlowReturn(_) => None,
            ObligationFrameDomain::ResolveCall(state) => Some(state),
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

    /// Push a transparent RESOLVE-CALL executor frame. The frame exists before
    /// candidate sessions or argument relations are opened, so the generic
    /// transaction — not a relation-domain special case — classifies those
    /// relations as inline.
    pub(crate) fn push_resolve_call(
        &mut self,
        key: ResolveCallKey,
        pending_watermark: usize,
    ) -> usize {
        let identity = ObligationIdentity::ResolveCall(key);
        let idx = self.frames.len();
        self.frames.push(ObligationFrame {
            identity: identity.clone(),
            assumption_targets: Vec::new(),
            min_open_target: None,
            budget_cap: None,
            pending_watermark,
            domain: ObligationFrameDomain::ResolveCall(ResolveCallFrameState::default()),
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

    pub(crate) fn assumption_evidence(&self, target: usize) -> RelationAssumptionEvidence {
        let closure = self.frames[target..]
            .iter()
            .map(|frame| frame.identity.clone())
            .collect::<Vec<_>>();
        RelationAssumptionEvidence {
            closure: Arc::from(closure.into_boxed_slice()),
        }
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

    /// Attach a resolved-call hold to the nearest active flow frame.
    /// Returns false when the caller is not executing inside FlowReturn,
    /// or when the identity is a bare flow return (a flow hold without
    /// its instantiation clause cannot be transferred — the evaluator's
    /// own callee gate records those).
    pub(crate) fn record_nearest_flow_hold(&mut self, hold: ReturnObligationIdentity) -> bool {
        if matches!(hold, ReturnObligationIdentity::FlowReturn(_)) {
            return false;
        }
        let Some(state) = self
            .frames
            .iter_mut()
            .rev()
            .find_map(|frame| match &mut frame.domain {
                ObligationFrameDomain::FlowReturn(state) => Some(state),
                ObligationFrameDomain::Relate(_) | ObligationFrameDomain::ResolveCall(_) => None,
            })
        else {
            return false;
        };
        if !state.holds.contains(&hold) {
            state.holds.push(hold);
        }
        true
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
///
/// `EvaluatedValue` is the PRE-PROOF value arm: the evaluator's computed
/// value, never a completeness claim. Warm admission is claimed ONLY by
/// the finalizer's proof token, minted at the component close from this
/// arm's value.
#[derive(Debug, Clone)]
pub(crate) enum FlowReturnPendingOutcome {
    /// The evaluated value (possibly a DEGRADED success — usable, but a
    /// degraded value can never seal, so it never warms).
    EvaluatedValue(FlowReturnResult),
    /// Typed failure — `ReturnOnly`, never admitted.
    NoValue {
        /// The typed no-value failure.
        failure: FlowReturnFailure,
        /// The degradation the FAILED evaluation had already observed
        /// before it failed.
        ///
        /// This field is not optional decoration: a hold-only
        /// [`FlowReturnFailure::EmptyCycle`] member is RESURRECTED by the
        /// component discharge (its value is the join of its hold
        /// targets'), so a degradation observed on the way to the empty
        /// cycle must ride the failure into the fixed point. Dropping it
        /// launders a degraded evaluation into a clean, WARM-admissible
        /// result — and, because only the non-root member takes the
        /// resurrection path, it does so in exactly one of the two demand
        /// orders. Naming the field at every construction site is what
        /// makes "a Degraded outcome without its degradation"
        /// unrepresentable.
        degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    },
}

impl FlowReturnPendingOutcome {
    /// The outcome's OWN degradation, whichever arm carries it.
    pub(crate) fn degradation(&self) -> Option<crate::semantic_query::FlowReturnDegradation> {
        match self {
            Self::EvaluatedValue(result) => result.degradation(),
            Self::NoValue { degradation, .. } => *degradation,
        }
    }
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
    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
    /// The member's own file roots — the published component's self-roots
    /// are the UNION of every drained member's roots, so a cross-file edit
    /// invalidates the whole component.
    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The materialised point set the member's compute ACTUALLY produced
    /// (§3.4 — recorded by the compute, never re-derived from the nominal
    /// key at publish).
    pub(crate) materialized: crate::semantic_query::demand::MaterializedSet,
    /// Whether every one of the member's OWN return contributors was a
    /// FRESH literal (and no bare-return / fallthrough arm joined). The
    /// component-wide literal-widening decision is made after the
    /// equation fixed point converges, so the bit must survive the pop.
    pub(crate) fresh_seed: bool,
    /// The member's own installed flow demand carrier, surviving the pop
    /// so the deferred member discharges and finalizes against EXACTLY its
    /// own demand at the component close. `None` when the demand could not
    /// be planned (an unproven member never publishes).
    pub(crate) flow_demand: Option<flow_obligation_state::FlowDemandCarrier>,
    /// The member's typed discharge report — which planned obligations
    /// its evaluation actually completed. Produced ONCE by the private
    /// evaluation outcome; applied centrally at the component close.
    pub(crate) discharge: Option<flow_obligation_state::FlowDischargeReport>,
}

/// The winning candidate's signature while the shared return equation is
/// still running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SelectedSignature {
    /// A general signature node.
    General(SemanticNodeId),
    /// The sealed index-composed carrier. Its general signature is minted
    /// only once the equation resolves the call's return, so a deferred
    /// return is never observable as a failed one.
    Deferred(Box<crate::semantic_query::DeferredCallable>),
}

/// Stable winner metadata retained while the shared return equation resolves
/// the call's return node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ResolveCallSelection {
    Selected {
        selected: SignatureCandidateOrigin,
        selected_signature: SelectedSignature,
        substitution: CanonicalTypeSubstitution,
        /// The FRESH primitive-literal return candidates this winner may
        /// close on: a naked declared return of an unconstrained parameter
        /// fixed to a bare-literal argument. Consulted at close — a final
        /// return equal to one of these is a fresh literal the caller's
        /// return position widens.
        fresh_literal_returns: Vec<SemanticNodeId>,
    },
    /// A UNION callee's per-arm winners: one first-applicable signature in
    /// EVERY callable arm; the close unions the arm returns.
    UnionSelected {
        arms: Vec<ResolveCallUnionArmSelection>,
    },
    DynamicAny,
}

/// One union-callee arm's staged winner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ResolveCallUnionArmSelection {
    pub(crate) selected: SignatureCandidateOrigin,
    /// The arm winner's signature node (the sealed deferred carrier node
    /// when the arm's return deferred — the general signature of a lone
    /// winner is minted at close, but a union close has no per-arm return
    /// to mint with, so the carrier node itself is the arm's signature).
    pub(crate) selected_signature: SemanticNodeId,
    pub(crate) substitution: CanonicalTypeSubstitution,
}

impl ResolveCallSelection {
    /// The winner's fresh primitive-literal return candidates (empty for a
    /// dynamic-`any` selection).
    pub(crate) fn fresh_literal_returns(&self) -> &[SemanticNodeId] {
        match self {
            Self::Selected {
                fresh_literal_returns,
                ..
            } => fresh_literal_returns,
            Self::UnionSelected { .. } | Self::DynamicAny => &[],
        }
    }

    pub(crate) fn with_return_type(
        &self,
        dispatch: &super::ProjectSemanticDispatch<'_>,
        return_type: SemanticNodeId,
    ) -> ResolvedCallResult {
        match self {
            Self::Selected {
                selected,
                selected_signature,
                substitution,
                fresh_literal_returns,
            } => ResolvedCallResult::Selected {
                selected: selected.clone(),
                selected_signature: match selected_signature {
                    SelectedSignature::General(node) => *node,
                    SelectedSignature::Deferred(callable) => dispatch
                        .graph()
                        .intern_node(callable.clone().into_general_signature(return_type)),
                },
                substitution: substitution.clone(),
                return_type,
                fresh_literal_return: fresh_literal_returns.contains(&return_type),
            },
            Self::UnionSelected { arms } => ResolvedCallResult::UnionSelected {
                selections: Arc::from(
                    arms.iter()
                        .map(|arm| crate::semantic_query::ResolvedUnionArm {
                            selected: arm.selected.clone(),
                            selected_signature: arm.selected_signature,
                            substitution: arm.substitution.clone(),
                        })
                        .collect::<Vec<_>>()
                        .into_boxed_slice(),
                ),
                return_type,
            },
            Self::DynamicAny => ResolvedCallResult::DynamicAny { return_type },
        }
    }
}

/// The call-resolution-domain deferral payload of a popped member.
#[derive(Debug, Clone)]
pub(crate) struct ResolveCallPendingState {
    /// The fixed winning occurrence/substitution, without a pre-equation
    /// return node.
    pub(crate) selection: ResolveCallSelection,
    /// Concrete return seeds owned by the call (declared return or dynamic
    /// `any`).
    pub(crate) concrete_seeds: Vec<SemanticNodeId>,
    /// Tagged return dependencies (a body-derived winner holds FlowReturn).
    pub(crate) holds: Vec<ReturnObligationIdentity>,
    /// Candidate session staged by this winner, committed only after the
    /// mixed component is stable.
    pub(crate) staged_session: Option<SessionId>,
    /// Relation-only assumptions require a fresh applicability replay at the
    /// component root before the return equation runs.
    pub(crate) replay_applicability: bool,
    /// Store-owned admission for this inline member.
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
    /// The call site's own file roots.
    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
}

/// The domain deferral payload of a popped member.
#[derive(Debug)]
pub(crate) enum PendingObligationDomain {
    /// Relation deferral state.
    Relate(RelationPendingState),
    /// Flow-return deferral state.
    FlowReturn(FlowReturnPendingState),
    /// ResolveCall deferral state (boxed: the union-selection payload
    /// makes this by far the largest domain).
    ResolveCall(Box<ResolveCallPendingState>),
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

    /// Whether `identity` is an OPEN member of the enclosing component —
    /// deposited here and not yet drained by an SCC close. A hold edge is
    /// only meaningful against such a member; anything else has already
    /// converged to a value.
    pub(crate) fn contains(&self, identity: &ObligationIdentity) -> bool {
        self.pending
            .iter()
            .any(|member| &member.identity == identity)
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
    /// A converged call result used while relation members re-discharge.
    ResolveCall(ResolvedCallResult),
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
        Some(ProvisionalVerdict::ResolveCall(_)) | None => None,
    }
}

/// Read a RESOLVE-CALL result from the tagged table.
pub(crate) fn provisional_resolve_call_result<'a>(
    substitution: &'a ProvisionalSubstitution,
    key: &ResolveCallKey,
) -> Option<&'a ResolvedCallResult> {
    match substitution.get(&ObligationIdentity::ResolveCall(key.clone())) {
        Some(ProvisionalVerdict::ResolveCall(result)) => Some(result),
        _ => None,
    }
}

/// The generic obligation runtime: tagged identities, generic frames /
/// backedges / lowlinks, the generic pending ledger + watermarks, and the
/// tagged provisional substitution table. Domain runtimes own their
/// verdict algebra; this runtime owns the SCC topology. The flow-solve
/// ledger is the completeness-proof layer's typed obligation state, ON
/// this runtime (never a peer ledger): production-compiled but publicly
/// unreachable, one installed demand per [`FlowDemandHandle`](flow_obligation_state::FlowDemandHandle)
/// so nested flow frames and deferred SCC members each hold their own
/// demand. A default `Vec` reserves no heap storage — the no-flow path
/// allocates nothing.
#[derive(Debug)]
pub struct ObligationRuntime {
    /// The instance identity minted at construction: every
    /// [`FlowDemandHandle`](flow_obligation_state::FlowDemandHandle) this
    /// runtime mints carries it, and every resolution verifies it, so a
    /// handle of one runtime fails closed on another — even a populated
    /// one. A plain `Copy` scalar: the no-flow path allocates nothing.
    instance_identity: u64,
    stack: ObligationReentryStack,
    pending: ObligationPendingLedger,
    substitution: ProvisionalSubstitution,
    flow_demands: Vec<flow_obligation_state::InstalledFlowDemand>,
}

/// The identity-nonce minter for [`ObligationRuntime`] instances: every
/// construction mints a DISTINCT identity, so a flow-demand handle's
/// runtime axis is unique per runtime. Content-free and transient — never
/// a cache key, never a fact, never persisted.
static OBLIGATION_RUNTIME_IDENTITY: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl Default for ObligationRuntime {
    fn default() -> Self {
        Self {
            instance_identity: OBLIGATION_RUNTIME_IDENTITY
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            stack: ObligationReentryStack::default(),
            pending: ObligationPendingLedger::default(),
            substitution: ProvisionalSubstitution::default(),
            flow_demands: Vec::new(),
        }
    }
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

/// The typed flow-solve obligation state of the ONE obligation runtime:
/// the completeness-proof layer's state machine and evidence carriers —
/// records ON [`ObligationRuntime`] itself, never a peer ledger.
/// Production-live: the flow evaluator's demand-preparation step installs
/// one demand per cold flow frame, and the component close finalizes
/// against it.
///
/// Every installed demand runs the one-shot lifecycle
/// `Discharging → ExpansionClosed → Converging → Converged → Sealed` on
/// its OWN [`InstalledFlowDemand`]: obligations transition only before
/// convergence begins, convergence may be observed only once the
/// expansion frontier is closed AND every required obligation is
/// Discharged, a dependent obligation discharges only after its exact
/// dependencies are themselves Discharged, sealing takes `&mut self` and
/// mints the ONE artifact (a repeat is `AlreadySealed`), and every
/// post-seal transition fails. One demand's lifecycle never gates
/// another's — nested flow frames and deferred SCC members hold distinct
/// handles.
// The whole module is the completeness-proof substrate: production-live
// (the evaluator's demand preparation installs demands and the component
// close drives them), with the test surface exercising the same items
// through `crate::for_tests`.
pub(crate) mod flow_obligation_state {
    use std::sync::Arc;

    use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
    use verter_identity::identity::{InputBasisId, ResultContractId};
    use verter_semantic::analysis::flow::flow_graph::{FlowEdgeClass, FlowNodeId, FlowNodeKind};
    use verter_semantic::analysis::flow::{
        SkeletonBindingId, SkeletonBindingKind, SkeletonExprSiteId, SkeletonRegionId,
    };
    use verter_semantic::analysis::function_program::FlowBindingIdentity;

    use super::super::flow_solve::{
        flow_family_route, flow_operation_contract, require_registered_flow_requirement,
        FlowConvergencePolicy, FlowDemandBasis, FlowDemandPlan, FlowDemandSubject,
        FlowExpansionRule, FlowFactFamily, FlowFailure, FlowOperationRole, FlowRequirement,
    };
    use crate::semantic_query::{FlowGap, FlowReturnResult, SemanticQueryKeyTag};

    /// The plan-local identity of one flow-solve obligation (work order).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct FlowObligationId(pub u32);
    /// Where an obligation came from: a contract-required domain, a
    /// registered expansion rule, or a caller-asserted requirement.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FlowObligationOrigin { ContractDomain, Expansion(FlowExpansionRule), Additional }
    /// The binding-slot identity of one binding obligation: the lexical
    /// slot plus the cross-frame binding identity — never a fresh
    /// identity. The planner populates both at plan time by resolving the
    /// skeleton's binding index against the frame's binding inventory
    /// (the ONE cross-frame authority whose slots ARE the
    /// `FlowBindingIdentity.binding_slot` domain).
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlowBindingBasis { pub binding: SkeletonBindingId, pub identity: FlowBindingIdentity }
    /// The mandatory closed semantic identity of one planned obligation:
    /// every obligation names exactly the semantic subject it proves
    /// something about — the demand root and its derived program point,
    /// a family-coverage enumeration, one graph site, one binding hub,
    /// one call occurrence, one guard, one dynamic fact of a call
    /// expansion event, one capture subject, or one full selected edge.
    /// `UnmodeledBinding` and `Capture` are fail-closed arms: they install
    /// directly in `Gap` — never with a fabricated identity, never an
    /// omission.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum FlowObligationBasis {
        /// Anchored at the demand root: the program point derived from
        /// the query's own demand axis.
        DemandRoot { subject: FlowDemandSubject },
        /// The family-coverage obligation of one required fact family:
        /// discharging it asserts the family's subject enumeration was
        /// EXHAUSTIVE — "proved empty" when the family has no concrete
        /// instances, never "planner forgot the family".
        FamilyCoverage { family: FlowFactFamily },
        /// A graph site node (a return site).
        Site { node: FlowNodeId, kind: FlowNodeKind },
        /// A binding hub: the graph node plus the real slot identity.
        Binding { node: FlowNodeId, slot: FlowBindingBasis },
        /// A binding hub the cross-frame vocabulary cannot name.
        UnmodeledBinding { node: FlowNodeId, binding: SkeletonBindingId, kind: SkeletonBindingKind },
        /// One full selected edge: endpoints, class, and source ordinal.
        Edge { from: FlowNodeId, to: FlowNodeId, class: FlowEdgeClass, ordinal: u32 },
        /// ONE call occurrence: the expression site plus the call ordinal
        /// within it — every concrete occurrence has its own identity.
        CallSite { node: FlowNodeId, site: SkeletonExprSiteId, call_ordinal: u32 },
        /// A guard predicate anchored on exactly (region, control input).
        Guard { node: FlowNodeId, region: SkeletonRegionId, control_input: SkeletonExprSiteId },
        /// The contextual target of one expression site.
        ContextualTarget { node: FlowNodeId, site: SkeletonExprSiteId },
        /// A dynamic semantic relation anchored on the registered
        /// call-expansion event that produced it (the call occurrence).
        SemanticRelation { node: FlowNodeId, site: SkeletonExprSiteId, call_ordinal: u32 },
        /// A capture subject the structural authority cannot name: the
        /// nested function DECLARATION's binding identity (the capture
        /// SET of a nested declaration body is beyond this skeleton's
        /// authority — nested bodies carry no reads here), or a closure
        /// expression's captured binding the cross-frame inventory cannot
        /// name (`identity: None`, e.g. a destructured parameter). The
        /// obligation installs directly in the family's accepted typed
        /// gap.
        Capture { node: FlowNodeId, binding: SkeletonBindingId, identity: Option<FlowBindingIdentity> },
        /// A concrete capture subject: ONE binding the closure expression
        /// at this graph node captures, with its real cross-frame identity
        /// — one obligation per (closure site, captured binding). The
        /// structural authority named the subject exactly, so the
        /// obligation is dischargeable, never a gap.
        CapturedBinding { node: FlowNodeId, site: SkeletonExprSiteId, identity: FlowBindingIdentity },
    }
    /// Evidence that one live semantic suboperation was consumed. This is
    /// a discharge INPUT: the runtime validates it against the specific
    /// obligation's declared suboperations at mint time.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlowSuboperationEvidence { pub operation: SemanticQueryKeyTag, pub result_contract: ResultContractId }
    /// The sealed evidence of one discharge. Fields are private and the
    /// only construction is inside
    /// [`super::ObligationRuntime::discharge_flow_obligation`], which
    /// validates the presented claims against THAT obligation's spec
    /// (its exact declared dependencies and suboperations) before
    /// sealing — there is no externally callable path that constructs
    /// arbitrary discharge evidence.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct DischargeEvidence {
        input_basis: InputBasisId, result_contract: ResultContractId,
        dependencies: Arc<[FlowObligationId]>, suboperations: Arc<[FlowSuboperationEvidence]>,
    }
    /// The runtime-OBSERVED convergence of one solve: the policy the
    /// demand was installed under, the iterations the runtime counted,
    /// and the observed stable point. Fields are private and the only
    /// construction is inside
    /// [`super::ObligationRuntime::seal_flow_completion`], minted from the
    /// runtime's own observation log — never caller-authored.
    #[rustfmt::skip]
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FlowConvergenceEvidence { policy: FlowConvergencePolicy, iterations: u32, stable: bool }

    impl FlowConvergenceEvidence {
        /// A test-only mint: the fenced-publish machinery tests stage
        /// members whose proofs need a convergence payload without a full
        /// solve drive. Production evidence is minted ONLY by
        /// `seal_flow_completion` from the runtime's observation log.
        #[cfg(test)]
        pub(crate) fn for_tests(
            policy: FlowConvergencePolicy,
            iterations: u32,
            stable: bool,
        ) -> Self {
            Self {
                policy,
                iterations,
                stable,
            }
        }

        /// The policy the solve converged under.
        #[must_use]
        pub fn policy(&self) -> FlowConvergencePolicy {
            self.policy
        }
        /// The iterations the runtime counted up to the stable point.
        /// Read by the test surface; the finalizer consumes the policy.
        #[allow(dead_code)]
        #[must_use]
        pub fn iterations(&self) -> u32 {
            self.iterations
        }
        /// Whether the runtime observed a stable final iteration.
        /// Read by the test surface; the runtime's own log gates the seal.
        #[allow(dead_code)]
        #[must_use]
        pub fn stable(&self) -> bool {
            self.stable
        }
    }

    /// The runtime's own convergence observation log for the installed
    /// demand: the installed policy, the counted iterations, and the
    /// observed stable point. Private to this module; mutated ONLY by
    /// `observe_flow_iteration`.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct FlowConvergenceObservation {
        pub(super) policy: FlowConvergencePolicy,
        pub(super) iterations: u32,
        pub(super) stable: bool,
    }

    /// The one-shot lifecycle phase of one installed flow demand:
    /// `Discharging` (obligation transitions allowed; the expansion
    /// frontier is open) → `ExpansionClosed` (every obligation terminal)
    /// → `Converging` (the runtime observed at least one fixed-point
    /// iteration) → `Converged` (the stable point was observed) →
    /// `Sealed` (the completion artifact was minted — the demand is
    /// frozen). Phases advance only forward.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(super) enum FlowDemandPhase {
        /// Installed; obligations may start / discharge / gap / fail.
        Discharging,
        /// Every obligation is terminal; the expansion frontier is closed.
        ExpansionClosed,
        /// At least one fixed-point iteration observed; no obligation
        /// transition is legal from here on.
        Converging,
        /// The stable point was observed; sealing is now legal.
        Converged,
        /// The ONE completion artifact was minted; every further
        /// transition fails.
        Sealed,
    }

    /// The unforgeable handle of one installed flow demand: an index into
    /// the runtime's demand ledger plus the owning runtime's instance
    /// identity, minted ONLY by
    /// [`super::ObligationRuntime::install_flow_demand`]. The fields are
    /// private, so no caller fabricates a handle; a handle of one runtime
    /// fails closed on another — the identity is verified at EVERY
    /// resolution, so a foreign handle is `NoDemandInstalled` even against
    /// a populated runtime whose slot index matches.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FlowDemandHandle {
        /// The demand's slot in the owning runtime's ledger.
        index: u32,
        /// The owning runtime's instance identity.
        identity: u64,
    }

    /// The opaque evaluation provenance binding one flow demand to the
    /// evaluation that serves it: the semantic store's instance identity
    /// plus the request's project generation. Minted by the production
    /// dispatch at demand-preparation time and carried — atomically, in
    /// [`FlowDemandCarrier`] — with the demand's handle, plan, value, and
    /// discharge report. The finalization driver accepts evidence only
    /// when the carried provenance IS the dispatch's current mint: a value
    /// or report from another demand, another store, or a stale generation
    /// is a typed partial, never a proof.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct FlowEvaluationProvenance {
        store_identity: u64,
        request_generation: u64,
    }

    impl FlowEvaluationProvenance {
        /// Mint a provenance token. The mint inputs are owned by the
        /// caller (the production dispatch derives them from its own
        /// store and the live project generation); the token is opaque
        /// identity data, not a capability.
        pub(crate) fn new(store_identity: u64, request_generation: u64) -> Self {
            Self {
                store_identity,
                request_generation,
            }
        }
    }

    impl CanonicalEncode for FlowEvaluationProvenance {
        const DOMAIN_TAG: &'static str = "verter.session.flow.evaluation_provenance.v1";

        fn encode_fields(&self, e: &mut CanonicalEncoder) {
            e.field_u64(1, self.store_identity);
            e.field_u64(2, self.request_generation);
        }
    }

    /// The per-demand proof carrier: the installed demand's handle, its
    /// plan, and the evaluation provenance, bound atomically at
    /// preparation time. Carried by the in-flight frame state and — for a
    /// deferred member — the pending ledger, so the SCC close finalizes
    /// the member against EXACTLY its own demand.
    #[derive(Debug, Clone)]
    pub(crate) struct FlowDemandCarrier {
        /// The installed demand's unforgeable handle.
        pub(crate) handle: FlowDemandHandle,
        /// The demand plan the runtime installed.
        pub(crate) plan: Arc<FlowDemandPlan>,
        /// The evaluation provenance minted at preparation.
        pub(crate) provenance: FlowEvaluationProvenance,
    }

    /// The convergence the component fixed point actually RAN: the
    /// iteration count (including the final stable pass) and whether a
    /// stable point was reached. Produced by the discharge loop, replayed
    /// into each demand's runtime-observed convergence log by the
    /// finalization driver — never minted into evidence directly.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub(crate) struct ObservedFlowConvergence {
        /// The fixed-point passes the discharge ran (≥ 1 when it ran).
        pub(crate) iterations: u32,
        /// Whether the fixed point reached a stable pass.
        pub(crate) stable: bool,
    }

    /// One installed flow demand: its basis, its obligation records, its
    /// convergence observation log, and its one-shot lifecycle phase.
    /// Per-demand — the runtime holds a `Vec` of these, so a nested flow
    /// frame's or a deferred SCC member's demand never collides with its
    /// enclosing demand.
    #[derive(Debug)]
    pub(super) struct InstalledFlowDemand {
        basis: FlowDemandBasis,
        obligations: Vec<FlowObligationRecord>,
        convergence: FlowConvergenceObservation,
        phase: FlowDemandPhase,
    }

    impl InstalledFlowDemand {
        /// The phase gate for obligation transitions: once convergence has
        /// begun (or the demand is sealed), no obligation expansion or
        /// transition is legal.
        fn ensure_obligations_mutable(&self) -> Result<(), FlowTransitionError> {
            match self.phase {
                FlowDemandPhase::Converging
                | FlowDemandPhase::Converged
                | FlowDemandPhase::Sealed => Err(FlowTransitionError::IllegalTransition),
                FlowDemandPhase::Discharging | FlowDemandPhase::ExpansionClosed => Ok(()),
            }
        }
        fn record(
            &self,
            id: FlowObligationId,
        ) -> Result<&FlowObligationRecord, FlowTransitionError> {
            self.obligations
                .iter()
                .find(|r| r.spec.id == id)
                .ok_or(FlowTransitionError::UnknownObligation)
        }
        fn record_mut(
            &mut self,
            id: FlowObligationId,
        ) -> Result<&mut FlowObligationRecord, FlowTransitionError> {
            self.obligations
                .iter_mut()
                .find(|r| r.spec.id == id)
                .ok_or(FlowTransitionError::UnknownObligation)
        }
        /// Advance `Discharging` → `ExpansionClosed` once every obligation
        /// is terminal (the expansion frontier is closed).
        fn refresh_expansion_closure(&mut self) {
            if self.phase == FlowDemandPhase::Discharging
                && !self.obligations.is_empty()
                && self.obligations.iter().all(|record| {
                    !matches!(
                        record.state,
                        ObligationState::Pending | ObligationState::Running
                    )
                })
            {
                self.phase = FlowDemandPhase::ExpansionClosed;
            }
        }
    }

    /// The typed state of one flow-solve obligation.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum ObligationState { Pending, Running, Discharged(DischargeEvidence), Gap(FlowGap), Failed(FlowFailure) }
    /// The specification of one planned obligation: its identity, its
    /// closed semantic basis, and the EXACT evidence contract a discharge
    /// must satisfy — the declared dependencies and suboperations this
    /// specific obligation requires (per-spec, never a global subset).
    ///
    /// SEALED: every field is private and the sole constructor is visible
    /// only inside the planner/runtime boundary (the
    /// `project_semantic_dispatch` module tree) — no external struct
    /// literal, no setters, immutable views only.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlowObligationSpec {
        id: FlowObligationId, requirement: FlowRequirement,
        origin: FlowObligationOrigin, basis: FlowObligationBasis,
        expected_dependencies: Arc<[FlowObligationId]>,
        expected_suboperations: Arc<[SemanticQueryKeyTag]>,
    }

    #[rustfmt::skip]
    impl FlowObligationSpec {
        /// The sole constructor — restricted to the planner/runtime
        /// boundary (the `project_semantic_dispatch` module tree).
        pub(in crate::project_semantic_dispatch) fn new(
            id: FlowObligationId, requirement: FlowRequirement,
            origin: FlowObligationOrigin, basis: FlowObligationBasis,
            expected_dependencies: Arc<[FlowObligationId]>,
            expected_suboperations: Arc<[SemanticQueryKeyTag]>,
        ) -> Self {
            Self { id, requirement, origin, basis, expected_dependencies, expected_suboperations }
        }
        /// The plan-local identity of this obligation (work order).
        pub fn id(&self) -> FlowObligationId { self.id }
        /// The requirement this obligation proves.
        pub fn requirement(&self) -> &FlowRequirement { &self.requirement }
        /// Where this obligation came from. Read by the test surface;
        /// discharge validation consults the evidence-contract fields.
        #[allow(dead_code)]
        pub fn origin(&self) -> &FlowObligationOrigin { &self.origin }
        /// The closed semantic identity of this obligation's subject.
        pub fn basis(&self) -> &FlowObligationBasis { &self.basis }
        /// The EXACT declared dependencies a discharge must present.
        pub fn expected_dependencies(&self) -> &[FlowObligationId] { &self.expected_dependencies }
        /// The EXACT declared suboperations a discharge must present.
        pub fn expected_suboperations(&self) -> &[SemanticQueryKeyTag] { &self.expected_suboperations }
    }
    /// One installed obligation: its spec plus its current state.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlowObligationRecord { pub spec: FlowObligationSpec, pub state: ObligationState }
    /// One completed obligation of a discharge report: the obligation the
    /// evaluation ACTUALLY completed plus the evidence it claims. A claim,
    /// not a proof — the runtime re-validates every entry against the
    /// obligation's spec (exact dependencies, exact same-contract
    /// suboperations, dependency readiness) at application time.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct FlowDischargeEntry {
        pub obligation: FlowObligationId,
        pub dependencies: Arc<[FlowObligationId]>,
        pub suboperations: Arc<[FlowSuboperationEvidence]>,
    }
    /// The evaluator's typed discharge report for one flow demand: which
    /// planned obligations the evaluation ACTUALLY completed (domains,
    /// graph facts, calls, relations). The runtime applies it centrally,
    /// in the plan's deterministic work order — never through scattered
    /// mark-complete calls.
    #[rustfmt::skip]
    #[derive(Debug, Clone, PartialEq, Eq, Default)]
    pub struct FlowDischargeReport { entries: Arc<[FlowDischargeEntry]> }

    impl FlowDischargeReport {
        /// A report over `entries` — a claim; application re-validates
        /// each entry against the obligation's spec.
        #[must_use]
        pub fn new(entries: Vec<FlowDischargeEntry>) -> Self {
            Self {
                entries: Arc::from(entries.into_boxed_slice()),
            }
        }
        /// The report's entries (order-free; application iterates the
        /// plan's work order, not this order).
        #[must_use]
        pub fn entries(&self) -> &[FlowDischargeEntry] {
            &self.entries
        }
    }
    /// Why a flow-obligation transition was refused.
    #[rustfmt::skip]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlowTransitionError {
        NoDemandInstalled, UnknownObligation, IllegalTransition,
        UnplannedDependency, UndischargedDependency, NonSuboperationEvidence, ConvergenceBudget,
        /// The caller-supplied plan's basis is not the installed demand's
        /// basis — a report built for another demand is refused before
        /// any obligation is touched.
        BasisMismatch,
    }
    /// Why the runtime refuses to seal a completion artifact: the demand
    /// was already sealed (the artifact is one-shot), no demand is
    /// installed, obligations are still undischarged, no fixed point was
    /// observed, or the value payload is degraded.
    #[rustfmt::skip]
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum FlowSealError { AlreadySealed, NoDemandInstalled, UndischargedObligations, NonConverged, DegradedValue }
    /// The ONE sealed completion artifact of a flow solve: the
    /// solve-derived value, the exact installed basis, the per-spec
    /// discharge proofs (a snapshot of the runtime's records at seal
    /// time), and the runtime-observed convergence. Fields are private,
    /// the type is deliberately NOT `Clone`, and the only construction is
    /// inside [`super::ObligationRuntime::seal_flow_completion`], which
    /// mints it exactly once per demand (Converged → Sealed) — the
    /// finalizer consumes this artifact and nothing else.
    #[derive(Debug)]
    pub struct SealedFlowCompletion {
        basis: FlowDemandBasis,
        value: FlowReturnResult,
        convergence: FlowConvergenceEvidence,
        proofs: Arc<[FlowObligationRecord]>,
    }

    impl SealedFlowCompletion {
        /// The solve-derived value sealed into this artifact.
        #[must_use]
        pub fn value(&self) -> &FlowReturnResult {
            &self.value
        }
        /// The exact demand basis the solve ran under.
        #[must_use]
        pub fn basis(&self) -> &FlowDemandBasis {
            &self.basis
        }
        /// The runtime-observed convergence.
        #[must_use]
        pub fn convergence(&self) -> &FlowConvergenceEvidence {
            &self.convergence
        }
        /// The per-spec discharge proofs (the sealed record snapshot).
        #[must_use]
        pub fn proofs(&self) -> &[FlowObligationRecord] {
            &self.proofs
        }
    }

    #[rustfmt::skip]
    impl super::ObligationRuntime {
        /// Install one demand plan: its basis, its convergence policy, and
        /// one record per planned spec (an undeclared requirement — or a
        /// subject the structural authority cannot name — installs
        /// directly in `Gap`). The demand enters the `Discharging` phase;
        /// when every obligation already installed terminal, the expansion
        /// frontier is closed at install. Returns the demand's unforgeable
        /// handle. Installation never refuses a second demand: nested
        /// flow frames and deferred SCC members hold distinct demands, and
        /// every later operation names its demand by handle.
        pub fn install_flow_demand(&mut self, plan: &FlowDemandPlan) -> FlowDemandHandle {
            let mut records = Vec::with_capacity(plan.obligation_specs().len());
            for spec in plan.obligation_specs() {
                let gap = match spec.basis() {
                    FlowObligationBasis::UnmodeledBinding { .. } => Some(FlowGap::UnmodeledExpression),
                    // A capture subject's identity is real but its capture
                    // set is beyond the structural authority: the family's
                    // registered accepted gap, never a fabricated discharge.
                    FlowObligationBasis::Capture { .. } => Some(flow_family_route(&FlowFactFamily::Capture).accepted_gap),
                    _ => None,
                };
                let registered = require_registered_flow_requirement(spec.requirement().operation, &spec.requirement().requirement).is_ok();
                let state = match gap {
                    Some(gap) => ObligationState::Gap(gap),
                    None if registered => ObligationState::Pending,
                    None => ObligationState::Gap(FlowGap::UnmodeledExpression),
                };
                records.push(FlowObligationRecord { spec: spec.clone(), state });
            }
            let phase = if !records.is_empty() && records.iter().all(|record| !matches!(record.state, ObligationState::Pending | ObligationState::Running)) {
                FlowDemandPhase::ExpansionClosed
            } else {
                FlowDemandPhase::Discharging
            };
            let handle = FlowDemandHandle {
                index: u32::try_from(self.flow_demands.len()).unwrap_or(u32::MAX),
                identity: self.instance_identity,
            };
            self.flow_demands.push(InstalledFlowDemand {
                basis: plan.basis().clone(),
                obligations: records,
                convergence: FlowConvergenceObservation { policy: plan.convergence(), iterations: 0, stable: false },
                phase,
            });
            handle
        }

        /// Transition Pending → Running.
        pub fn start_flow_obligation(&mut self, handle: FlowDemandHandle, id: FlowObligationId) -> Result<(), FlowTransitionError> {
            self.transition(handle, id, |state| matches!(state, ObligationState::Pending).then_some(ObligationState::Running))
        }

        /// Transition Running → Discharged, minting the SEALED evidence:
        /// the presented claims are validated against THIS obligation's
        /// spec — the exact declared dependencies (each planned, none
        /// self, and each itself already DISCHARGED) and the exact
        /// declared same-contract suboperations — and the basis fields are
        /// taken from the installed demand, never from the caller. Empty
        /// claims cannot discharge a spec that declares required evidence,
        /// and a refused mint leaves the obligation Running.
        pub fn discharge_flow_obligation(
            &mut self,
            handle: FlowDemandHandle,
            id: FlowObligationId,
            dependencies: Arc<[FlowObligationId]>,
            suboperations: Arc<[FlowSuboperationEvidence]>,
        ) -> Result<(), FlowTransitionError> {
            let Some(index) = self.flow_demand_index(handle) else { return Err(FlowTransitionError::NoDemandInstalled) };
            let demand = &self.flow_demands[index];
            demand.ensure_obligations_mutable()?;
            let basis = demand.basis.clone();
            let record = demand.record(id)?;
            if !matches!(record.state, ObligationState::Running) {
                return Err(FlowTransitionError::IllegalTransition);
            }
            if dependencies != record.spec.expected_dependencies {
                return Err(FlowTransitionError::UnplannedDependency);
            }
            let subs_exact = suboperations.len() == record.spec.expected_suboperations.len()
                && suboperations.iter().zip(record.spec.expected_suboperations.iter()).all(|(sub, expected)| {
                    sub.operation == *expected
                        && sub.result_contract == basis.result_contract
                        && flow_operation_contract(sub.operation).is_some_and(|c| c.role == FlowOperationRole::SemanticSuboperation)
                });
            if !subs_exact {
                return Err(FlowTransitionError::NonSuboperationEvidence);
            }

            // Dependency readiness: a dependent obligation discharges only
            // after EVERY exact dependency is itself Discharged.
            for dependency in dependencies.iter() {
                let dependency_record = demand.record(*dependency)?;
                if !matches!(dependency_record.state, ObligationState::Discharged(_)) {
                    return Err(FlowTransitionError::UndischargedDependency);
                }
            }
            let evidence = DischargeEvidence {
                input_basis: basis.input_basis, result_contract: basis.result_contract,
                dependencies, suboperations,
            };
            let demand = &mut self.flow_demands[index];
            demand.record_mut(id)?.state = ObligationState::Discharged(evidence);
            demand.refresh_expansion_closure();
            Ok(())
        }
        /// Transition Pending|Running → Gap. Test surface: production
        /// gaps install at plan-install time, never mid-solve.
        #[allow(dead_code)]
        pub fn gap_flow_obligation(&mut self, handle: FlowDemandHandle, id: FlowObligationId, gap: FlowGap) -> Result<(), FlowTransitionError> {
            self.transition(handle, id, |state| matches!(state, ObligationState::Pending | ObligationState::Running).then_some(ObligationState::Gap(gap)))
        }
        /// Transition Pending|Running → Failed.
        pub fn fail_flow_obligation(&mut self, handle: FlowDemandHandle, id: FlowObligationId, failure: FlowFailure) -> Result<(), FlowTransitionError> {
            self.transition(handle, id, |state| matches!(state, ObligationState::Pending | ObligationState::Running).then_some(ObligationState::Failed(failure)))
        }

        /// Apply one evaluator discharge report CENTRALLY, in the plan's
        /// deterministic work order: for every obligation the report claims
        /// (in work order, never in the report's own order), start it and
        /// discharge it against the exact evidence the claim carries. The
        /// plan must carry the INSTALLED demand's exact basis — a report
        /// iterated under a foreign basis is refused before any obligation
        /// is touched, so two demands with matching local obligation shapes
        /// can never accept each other's report. Every
        /// entry is re-validated against the obligation's spec by
        /// [`Self::discharge_flow_obligation`] — a claim naming an
        /// obligation this demand never installed, or carrying wrong
        /// dependencies, wrong suboperations, or an undischarged
        /// dependency, fails closed and stops the application. Obligations
        /// the report does not claim stay untouched (a partial report is
        /// not a completion).
        pub fn apply_flow_discharge_report(&mut self, handle: FlowDemandHandle, plan: &FlowDemandPlan, report: &FlowDischargeReport) -> Result<(), FlowTransitionError> {
            let by_obligation: rustc_hash::FxHashMap<FlowObligationId, &FlowDischargeEntry> =
                report.entries().iter().map(|entry| (entry.obligation, entry)).collect();
            // The report applies to the INSTALLED demand only: the plan
            // the caller iterates must carry the installed demand's exact
            // basis — two demands with matching local obligation shapes
            // can never accept each other's report. And every claim must
            // name an obligation installed for THIS demand — a foreign
            // claim fails closed instead of being silently dropped.
            {
                let Some(demand) = self.flow_demand(handle) else { return Err(FlowTransitionError::NoDemandInstalled) };
                if demand.basis != *plan.basis() { return Err(FlowTransitionError::BasisMismatch); }
                for entry in report.entries() {
                    demand.record(entry.obligation)?;
                }
            }
            for id in plan.work_order() {
                let Some(entry) = by_obligation.get(id) else { continue };
                self.start_flow_obligation(handle, *id)?;
                self.discharge_flow_obligation(handle, *id, Arc::clone(&entry.dependencies), Arc::clone(&entry.suboperations))?;
            }
            Ok(())
        }

        /// Record one fixed-point iteration the solve ran: `changed` is
        /// whether the iteration observed a fact-set change. The FIRST
        /// observation is gated: the expansion frontier must be closed AND
        /// every required obligation Discharged — convergence is never
        /// observed over an open or partially discharged universe. The
        /// first iteration observed with `changed == false` closes
        /// convergence (`Converged`); observing past the stable point or
        /// past the seal is an illegal transition, and an iteration beyond
        /// the installed policy's cap is refused as budget exhaustion.
        /// Convergence enters the sealed artifact ONLY from this log — it
        /// is runtime-observed, never caller-authored.
        pub fn observe_flow_iteration(&mut self, handle: FlowDemandHandle, changed: bool) -> Result<(), FlowTransitionError> {
            let Some(demand) = self.flow_demand_mut(handle) else { return Err(FlowTransitionError::NoDemandInstalled) };
            match demand.phase {
                FlowDemandPhase::Converged | FlowDemandPhase::Sealed => return Err(FlowTransitionError::IllegalTransition),
                FlowDemandPhase::Discharging | FlowDemandPhase::ExpansionClosed => {
                    if !demand.obligations.iter().all(|record| matches!(record.state, ObligationState::Discharged(_))) {
                        return Err(FlowTransitionError::IllegalTransition);
                    }
                }
                FlowDemandPhase::Converging => {}
            }
            if demand.convergence.iterations >= demand.convergence.policy.max_iterations { return Err(FlowTransitionError::ConvergenceBudget); }
            demand.convergence.iterations += 1;
            if !changed { demand.convergence.stable = true; }
            demand.phase = if demand.convergence.stable { FlowDemandPhase::Converged } else { FlowDemandPhase::Converging };
            Ok(())
        }

        /// Mint the ONE sealed completion artifact of this solve
        /// (Converged → Sealed). Mints ONLY when every installed
        /// obligation is `Discharged` (each with spec-validated evidence),
        /// the runtime observed convergence, and the value payload carries
        /// no degradation. The artifact binds the installed basis, the
        /// value, the per-spec discharge proofs (a record snapshot), and
        /// the observed convergence into one unforgeable carrier. Sealing
        /// takes `&mut self` and is ONE-SHOT per demand: a repeated seal
        /// is `AlreadySealed`, and the sealed demand rejects every further
        /// transition (its siblings are unaffected).
        pub fn seal_flow_completion(&mut self, handle: FlowDemandHandle, value: FlowReturnResult) -> Result<SealedFlowCompletion, FlowSealError> {
            let Some(demand) = self.flow_demand_mut(handle) else { return Err(FlowSealError::NoDemandInstalled) };
            if demand.phase == FlowDemandPhase::Sealed { return Err(FlowSealError::AlreadySealed); }
            if !demand.obligations.iter().all(|record| matches!(record.state, ObligationState::Discharged(_))) {
                return Err(FlowSealError::UndischargedObligations);
            }
            let observation = demand.convergence;
            if !observation.stable { return Err(FlowSealError::NonConverged); }
            if value.degradation().is_some() { return Err(FlowSealError::DegradedValue); }
            demand.phase = FlowDemandPhase::Sealed;
            Ok(SealedFlowCompletion {
                basis: demand.basis.clone(),
                value,
                convergence: FlowConvergenceEvidence {
                    policy: observation.policy, iterations: observation.iterations, stable: observation.stable,
                },
                proofs: Arc::from(demand.obligations.clone().into_boxed_slice()),
            })
        }

        /// The installed demand's records, in plan work order.
        pub fn flow_obligations(&self, handle: FlowDemandHandle) -> Option<&[FlowObligationRecord]> {
            self.flow_demand(handle).map(|demand| demand.obligations.as_slice())
        }

        /// The installed demand's basis.
        pub fn flow_basis(&self, handle: FlowDemandHandle) -> Option<&FlowDemandBasis> {
            self.flow_demand(handle).map(|demand| &demand.basis)
        }

        /// The number of installed flow demands. Test surface (the
        /// no-flow allocation contract asserts on it).
        #[allow(dead_code)]
        pub fn flow_demand_count(&self) -> usize { self.flow_demands.len() }

        /// The reserved capacity of the DEMAND storage — the
        /// no-reservation probe for a runtime that never served a demand.
        #[allow(dead_code)] // test surface (the no-flow allocation contract)
        pub fn flow_demand_storage_capacity(&self) -> usize { self.flow_demands.capacity() }

        /// Resolve a handle to its demand's ledger slot: the handle must
        /// carry THIS runtime's instance identity and an in-range index —
        /// a foreign or out-of-range handle resolves to nothing, so every
        /// operation on it fails closed.
        fn flow_demand_index(&self, handle: FlowDemandHandle) -> Option<usize> {
            if handle.identity != self.instance_identity {
                return None;
            }
            let index = handle.index as usize;
            (index < self.flow_demands.len()).then_some(index)
        }
        fn flow_demand(&self, handle: FlowDemandHandle) -> Option<&InstalledFlowDemand> {
            self.flow_demand_index(handle)
                .map(|index| &self.flow_demands[index])
        }
        fn flow_demand_mut(&mut self, handle: FlowDemandHandle) -> Option<&mut InstalledFlowDemand> {
            self.flow_demand_index(handle)
                .map(|index| &mut self.flow_demands[index])
        }

        fn transition(&mut self, handle: FlowDemandHandle, id: FlowObligationId, next: impl FnOnce(&ObligationState) -> Option<ObligationState>) -> Result<(), FlowTransitionError> {
            let Some(demand) = self.flow_demand_mut(handle) else { return Err(FlowTransitionError::NoDemandInstalled) };
            demand.ensure_obligations_mutable()?;
            let record = demand.record_mut(id)?;
            let Some(state) = next(&record.state) else { return Err(FlowTransitionError::IllegalTransition) };
            record.state = state;
            demand.refresh_expansion_closure();
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// Relation domain runtime
// ---------------------------------------------------------------------------

/// Lifecycle of an in-flight inference session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InferenceSessionState {
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
    /// Call inference owns const policy per declaration parameter. Other
    /// inference domains use the neutral ordinary policy.
    const_policy: ConstParamPolicy,
    /// Whether the declared parameter carries a constraint. A FRESH
    /// primitive-literal candidate stays fresh only for an UNCONSTRAINED
    /// parameter (a constrained parameter's preserved literal is regular
    /// — the upper-bound check regularizes it).
    has_constraint: bool,
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
    const_policy: ConstParamPolicy,
    has_constraint: bool,
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
    /// FRESH primitive-literal deposits accepted at a NAKED top-level
    /// position of an UNCONSTRAINED parameter: `(param, literal)` pairs.
    /// Freshness provenance consumed at fixation to mark a naked declared
    /// return as a fresh literal the caller's return position widens.
    fresh_literal_deposits: Vec<(SemanticNodeId, SemanticNodeId)>,
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
    /// Immutable fixed bindings retained across staging and commit.
    staged_bindings: Option<Arc<[InferBinding]>>,
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
                const_policy: info.const_policy,
                has_constraint: info.has_constraint,
                candidates: Vec::new(),
            })
            .collect();
        Self {
            fresh_literal_deposits: Vec::new(),
            id,
            setup,
            infos,
            reverse_projection,
            staged_bindings: None,
            state: InferenceSessionState::Collecting,
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

    /// Whether `param_node` is one of this session's frozen inference
    /// declarations — a deposit target the forward relation arm binds.
    pub(crate) fn declares(&self, param_node: SemanticNodeId) -> bool {
        self.infos.iter().any(|info| info.param_node == param_node)
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
    pub(crate) fn stage_fixation<F>(&mut self, mut combine: F) -> Option<Arc<[InferBinding]>>
    where
        F: FnMut(&[SemanticNodeId], VariancePhase) -> SemanticNodeId,
    {
        if self.state != InferenceSessionState::Collecting {
            return None;
        }
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
                .map(|info| {
                    let (candidates, variance) = select_inference_candidates(&info.candidates);
                    FixationInput {
                        param: info.param_node,
                        name: Arc::clone(&info.param_name),
                        candidates,
                        variance,
                    }
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
    pub(crate) fn commit_completed(&mut self) -> bool {
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
    /// The winning candidate rung (empty when the parameter is uninferred).
    pub(crate) candidates: Vec<SemanticNodeId>,
    /// That rung's combination variance.
    pub(crate) variance: VariancePhase,
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
/// SCC-close; the drain at the relevant session's `CommittedDeterministic`
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

    /// Validate a deferred member without consuming the ledger. Mixed
    /// return components perform every fallible check before committing
    /// call sessions, then drain in the no-semantic-work publication tail.
    pub(crate) fn contains(&self, session: SessionId, key: &RelateMemoKey) -> bool {
        self.deferred
            .get(&session)
            .is_some_and(|keys| keys.iter().any(|candidate| candidate == key))
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
    /// Nestable call-applicability final-check barriers. Sessions below the
    /// newest length watermark are inactive; a genuinely nested call may push
    /// a fresh session above it and infer normally.
    binding_disabled_session_barriers: Vec<usize>,
    /// Literal interpretation for the current call argument relation. Empty
    /// outside call-owned collection, so ordinary relation inference is
    /// unchanged.
    call_argument_literal_modes: Vec<CallArgumentLiteralPolicy>,
    /// Monotonic count of ACCEPTED session deposits (ordinary, reverse
    /// aggregate, and projection), bumped at each acceptance site. The
    /// call executor charges its `inference_deposits` fuse from deltas of
    /// this counter, so the fuse's unit is the accepted deposit itself —
    /// never one unit per top-level argument.
    pub(crate) accepted_inference_deposits: u64,
    next_session_id: u64,
}

/// One in-flight call-argument relation's literal policy: the argument's
/// authored literal mode plus the parameter positions its declared TARGET
/// exposes at TOP LEVEL (the naked type-parameter set — the parameter
/// itself or a union / intersection arm). A deposit into a top-level
/// position preserves a primitive-literal candidate (TypeScript's naked
/// inference, constrained or not); a nested deposit widens under the
/// parameter's const policy.
#[derive(Debug)]
struct CallArgumentLiteralPolicy {
    literal_mode: Option<crate::semantic_query::ArgumentLiteralMode>,
    top_level_infer_targets: Vec<SemanticNodeId>,
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
    /// The member's PROOF — the sole warm-admission authority. The
    /// published payload is extracted from the token at the fenced batch
    /// publish; a member whose finalization did not complete never enters
    /// this queue at all.
    pub(crate) result: super::flow_solve::CompleteFlowResult,
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineFlowReturnFlight>,
    /// The member's own file roots (the SCC-union carrier's self-roots
    /// include them even when the ROOT is a relation obligation).
    pub(crate) self_roots: Vec<crate::semantic_query_memo::ObservedGraphSelfRoot>,
    /// The materialised point set the member's compute ACTUALLY produced
    /// (§3.4) — carried to the fenced member publish.
    pub(crate) materialized: crate::semantic_query::demand::MaterializedSet,
}

/// A call member whose mixed component closed cleanly, queued for the
/// root's completed-member drain — fenced backfill behind the root's
/// committing admission, never a second commit boundary.
#[derive(Debug)]
pub(crate) struct CompletedResolveCallMember {
    pub(crate) key: ResolveCallKey,
    /// The admitted result. A rootless winner cannot be represented here,
    /// so it never reaches the shared cache.
    pub(crate) result: crate::semantic_query::AdmissibleCallResult,
    pub(crate) inline_flight: Option<crate::semantic_query_memo::InlineResolveCallFlight>,
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
    /// The VALUE channel for closed flow members: every member (and every
    /// inline SCC root) whose close produced an evaluated value records it
    /// here — proven or not. The shared return equation reads closed
    /// members' values as its just-discharged override source for hold
    /// targets outside the solving component (never the store). This is a
    /// VALUE-computation input, not an admission channel: admission is
    /// `completed_members`' proof typing alone.
    pub(crate) closed_values: Vec<(FlowReturnKey, FlowReturnResult)>,
    /// The typed failure of the in-flight machinery ROOT's close — the
    /// caller-return payload channel: the family memo admits only COMPLETE
    /// values, so a degraded root's typed failure rides the transaction to
    /// the demanding caller (never admitted, never a cache value).
    pub(crate) last_root_failure: Option<crate::semantic_query::FlowReturnFailure>,
}

/// The call-resolution domain runtime.
#[derive(Debug, Default)]
pub(crate) struct ResolveCallDomainRuntime {
    /// Mixed-component members awaiting the root carrier's drain (fenced
    /// backfill behind the root's committing admission).
    pub(crate) completed_members: Vec<CompletedResolveCallMember>,
    /// Typed failure channel for a machinery-root call whose family value is
    /// suppressed.
    pub(crate) last_root_failure: Option<ResolveCallFailure>,
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
    /// The call-resolution domain runtime.
    pub(crate) call: ResolveCallDomainRuntime,
}

/// One entry of a tagged flow component awaiting its equation fixed
/// point: the member's current outcome (a Complete outcome IS its
/// concrete seed join; a hold-only EmptyCycle has no seed) and the
/// coinductive hold targets its evaluation met.
#[derive(Debug, Clone)]
pub(super) struct FlowDischargeEntry {
    /// The member's flow identity.
    pub(super) key: crate::semantic_query::FlowReturnKey,
    /// The member's outcome (updated in place by the discharge).
    pub(super) outcome: FlowReturnPendingOutcome,
    /// The member's coinductive hold targets, each carrying the
    /// instantiation obligation the fixed point owes its callee.
    pub(super) holds: Vec<super::flow_return_callee::HeldCallee>,
    /// Whether the member's own contributors were all FRESH literals —
    /// the post-convergence literal-widening input.
    pub(super) fresh_seed: bool,
}

/// Identity of a member in the shared return equation. This is deliberately
/// separate from [`ObligationIdentity`]: relations share SCC topology but do
/// not inhabit the return lattice.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum ReturnObligationIdentity {
    FlowReturn(FlowReturnKey),
    ResolveCall(ResolveCallKey),
}

/// Domain-specific metadata retained beside the shared return lattice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReturnDomainMetadata {
    // Exercised by the solver's own contract tests: production flow members
    // discharge through the callee-clause fixed point and never enter the
    // call equation.
    #[allow(dead_code)]
    FlowReturn {
        can_fall_through: bool,
    },
    ResolveCall,
}

/// One member of the multi-domain return equation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ReturnEquationMember {
    /// The member's FRESH primitive-literal return candidates (ResolveCall
    /// members only; a flow member's own position already widened its
    /// fresh leaves). A FLOW-domain consumer widens a contributed leaf
    /// equal to one of these.
    pub(crate) fresh_literal_returns: Vec<SemanticNodeId>,
    pub(crate) identity: ReturnObligationIdentity,
    pub(crate) concrete_seeds: Vec<SemanticNodeId>,
    pub(crate) holds: Vec<ReturnObligationIdentity>,
    pub(crate) domain: ReturnDomainMetadata,
}

/// Failure of the shared equation. Both cases poison the whole mixed
/// component and admit nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReturnEquationFailure {
    EmptyCycle,
    UnresolvedOutsideHold,
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

    /// Push a fresh collecting session unconditionally. Call-owned candidate
    /// execution uses this path even when an outer collector exists; relation
    /// roots retain their separate admission predicate.
    pub(crate) fn push_collecting_session(
        &mut self,
        setup: InferenceSessionSetup,
        reverse_projection: Option<ReverseProjectionState>,
    ) -> SessionId {
        let id = self.alloc_session_id();
        self.relation
            .sessions
            .push(InferenceSession::new(id, setup, reverse_projection));
        id
    }

    /// The active (innermost `Collecting`) session, if any.
    pub(crate) fn active_session_mut(&mut self) -> Option<&mut InferenceSession> {
        let start = self
            .relation
            .binding_disabled_session_barriers
            .last()
            .copied()
            .unwrap_or(0);
        self.relation
            .sessions
            .get_mut(start..)?
            .iter_mut()
            .rev()
            .find(|s| s.state == InferenceSessionState::Collecting)
    }

    pub(crate) fn active_session(&self) -> Option<&InferenceSession> {
        let start = self
            .relation
            .binding_disabled_session_barriers
            .last()
            .copied()
            .unwrap_or(0);
        self.relation
            .sessions
            .get(start..)?
            .iter()
            .rev()
            .find(|s| s.state == InferenceSessionState::Collecting)
    }

    pub(crate) fn binding_is_disabled(&self) -> bool {
        !self.relation.binding_disabled_session_barriers.is_empty()
    }

    pub(crate) fn begin_binding_disabled(&mut self) {
        self.relation
            .binding_disabled_session_barriers
            .push(self.relation.sessions.len());
    }

    pub(crate) fn end_binding_disabled(&mut self) {
        self.relation
            .binding_disabled_session_barriers
            .pop()
            .expect("binding-disabled barrier underflow");
    }

    pub(crate) fn begin_call_argument(
        &mut self,
        literal_mode: Option<crate::semantic_query::ArgumentLiteralMode>,
        top_level_infer_targets: Vec<SemanticNodeId>,
    ) {
        self.relation
            .call_argument_literal_modes
            .push(CallArgumentLiteralPolicy {
                literal_mode,
                top_level_infer_targets,
            });
    }

    pub(crate) fn end_call_argument(&mut self) {
        self.relation
            .call_argument_literal_modes
            .pop()
            .expect("call-argument literal-mode stack underflow");
    }

    pub(crate) fn call_argument_literal_mode(
        &self,
    ) -> Option<crate::semantic_query::ArgumentLiteralMode> {
        self.relation
            .call_argument_literal_modes
            .last()
            .and_then(|policy| policy.literal_mode)
    }

    /// Whether the CURRENT call argument's declared TARGET exposes
    /// `param_node` at top level (a naked type-parameter position — the
    /// parameter itself, or a union / intersection arm of it). A deposit
    /// into a top-level position preserves a primitive-literal candidate;
    /// a nested deposit widens it.
    pub(crate) fn call_argument_target_is_top_level(&self, param_node: SemanticNodeId) -> bool {
        self.relation
            .call_argument_literal_modes
            .last()
            .is_some_and(|policy| policy.top_level_infer_targets.contains(&param_node))
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
