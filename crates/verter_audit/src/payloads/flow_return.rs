#![deny(missing_docs)]
//! [`FlowReturnInferencePayload`] — typed payload for
//! [`crate::record::RequestKind::FlowReturnInference`] records.
//!
//! Populated by the session-side flow-return audited entry-point from
//! per-request counters bumped at the cold-path emission sites, plus
//! the typed partiality reason read off the outcome the evaluator
//! already produced. Every field is producer-populated per request;
//! none is a reserved slot.
//!
//! The tag enums below are CLOSED MIRRORS of the session-side flow
//! vocabulary (`FlowGap`, `FlowReturnDegradation`,
//! `FlowReturnFailure`). They live here so the leaf audit substrate
//! stays free of a back-edge to `verter_session`; the producer maps its
//! domain enums onto them at the one audit emission boundary
//! (`VerterHost::get_flow_return_type_with_audit`) through exhaustive
//! matches, so a new domain variant is a compile error rather than a
//! silently collapsed reason.

use serde::{Deserialize, Serialize};

/// Why a flow-return request was not a clean, complete value.
///
/// The two arms mirror the audited entry-point's split result/carrier
/// contract exactly: a DEGRADED SUCCESS is a usable value that refuses
/// warm admission and rides the carrier's `Ok` arm; a NO-VALUE outcome
/// rides the `Err` arm. A request that produced a complete, non-degraded
/// value carries no partiality at all
/// ([`FlowReturnInferencePayload::partiality`] is `None`).
///
/// Exactly ONE reason is reported, never a set. The producer's typed
/// outcome already reduced every observed gap to the FIRST one in
/// source order, so a function carrying several distinct gaps still
/// names only the earliest. Read the tag as "the reason this request
/// was partial", never as "the complete inventory of what is missing".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FlowPartialityTag {
    /// The evaluation produced a USABLE value that is nonetheless
    /// incomplete — the typed degradation reason attached to the `Ok`
    /// outcome. Never warm-admitted.
    Degraded(FlowDegradationTag),
    /// The evaluation produced NO value — the typed reason on the `Err`
    /// outcome.
    NoValue(FlowFailureTag),
}

/// Closed mirror of the session's `FlowReturnDegradation` — the reason a
/// usable flow-return value is incomplete.
///
/// The domain's `FlowGap(_)` arm is reduced through its own gap variant,
/// so each detected flow-model gap keeps a distinct wire spelling
/// instead of collapsing into one "gap" bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FlowDegradationTag {
    /// A guard's subject arms could not be enumerated or classified, so
    /// the narrow retained a superset (`FlowGap::GuardNarrowing`).
    GapGuardNarrowing,
    /// A nominal relation the flow model does not decide
    /// (`FlowGap::NominalRelation`).
    GapNominalRelation,
    /// A captured binding whose post-capture writes the model does not
    /// track (`FlowGap::ClosureCapture`).
    GapClosureCapture,
    /// An abrupt completion crossing the modeled region
    /// (`FlowGap::AbruptCompletion`).
    GapAbruptCompletion,
    /// An expression form the flow model does not represent
    /// (`FlowGap::UnmodeledExpression`).
    GapUnmodeledExpression,
    /// A call on a binding that is neither callable nor `any` evaluated
    /// to `any`.
    NonCallableBinding,
    /// A symbolic call carrier whose callee could not be represented or
    /// resolved to a signature evaluated to `any`.
    UnrepresentableCallee,
    /// A binding whose initializer failed with a typed flow failure was
    /// observed; the observation evaluated to `any`.
    FailedBindingInitializer,
    /// A whole-slot write effect the evaluator did not apply, so the
    /// value may miss the assignment's narrowing.
    UnappliedWriteEffect,
    /// A function-scoped (`var`) binding defined inside a conditional
    /// arm was observed after the arms rejoin, without branch-join
    /// algebra over it.
    ConditionalVarDefinition,
    /// An annotated declarator's declared union could not be reduced to
    /// the constituents its initializer selects; the binding holds the
    /// whole declared union.
    UnreducedDeclaredUnion,
    /// The evaluated value reaches a semantic-miss carrier — an honest
    /// local answer that is not a complete result.
    UnresolvedValue,
    /// A sub-expression position contributed the typed unresolved
    /// marker and the enclosing structure composed around it.
    UnmodeledPosition,
}

/// Closed mirror of the session's no-value flow-return reasons — the
/// `FlowReturnFailure` class plus the host's own unstable-view refusal.
///
/// The three `FlowReturnFailure` variants that nest a further closed
/// enum (`Unsupported`, `CallResolution`, `Budget`) are reduced through
/// that inner variant, so every distinct no-value reason keeps its own
/// wire spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum FlowFailureTag {
    /// The function position has no served body.
    Missing,
    /// A loop whose statement subtree contains a `return`.
    UnsupportedLoop,
    /// A `break` / `continue` crossing the modeled region.
    UnsupportedJump,
    /// A directly invoked closure statement whose captured flow effect
    /// the sequential statement evaluator does not represent.
    UnsupportedInvokedClosureEffect,
    /// A `with` statement.
    UnsupportedWith,
    /// A module-level statement inside the body.
    UnsupportedModuleDeclaration,
    /// An in-flight dependency could not be decided (a torn view, a
    /// missing non-cycle edge, or a nonconverging recursive component).
    Unresolved,
    /// The recursive component has no concrete semantic seed.
    EmptyCycle,
    /// The demanded `(demand, input)` point is beyond the whole-return /
    /// empty-input point the evaluation models.
    UnmodeledDemandPoint,
    /// A call in the body had no signature in the requested bucket.
    CallNotCallable,
    /// Every visible call candidate was definitely inapplicable.
    CallNoApplicableOverload,
    /// Call applicability depended on unsupported or unresolved work.
    CallUndecidable,
    /// The call-resolution work envelope tripped.
    CallBudget,
    /// A depth budget stopped the evaluation.
    BudgetDepthExceeded,
    /// A work budget stopped the evaluation (the shallow expression
    /// lowering's work budget, or the obligation runtime's
    /// connected-demand cap surfaced as the work budget).
    BudgetWorkExceeded,
    /// The host could not pin a proven-current store view within the
    /// bounded retry window, so the query was not resolved against
    /// superseded state.
    UnstableState,
}

/// Per-request counters and the typed partiality reason for one
/// flow-return inference request.
///
/// The three counters mirror the cold-path structured events one to
/// one: each cold whole-function evaluation bumps `cold_computes`
/// (paired with `FlowReturnStarted`), each flow-slice budget refusal
/// bumps `budget_exceeded_events` (paired with
/// `FlowSliceBudgetExceeded`), and each coinductive re-entry hold on
/// the obligation runtime bumps `cycle_reentry_holds` (paired with
/// `FlowCycleSentinelHit`). A warm family hit bumps nothing — the
/// cold-vs-warm audit contract's counter-side witness is
/// `cold_computes == 0`.
///
/// The counters report THAT a request did cold work, hit a budget, or
/// held on a cycle; [`Self::partiality`] reports WHY the request came
/// back degraded or with no value at all. It is read-only telemetry
/// derived from the outcome the evaluator already produced — no
/// admission decision, warm/cold classification, or cache identity
/// consults it.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct FlowReturnInferencePayload {
    /// Demanded function's symbol name (display attribution only —
    /// the record's `target_identity` carries the canonical file).
    pub function_symbol: String,
    /// Number of cold whole-function flow evaluations this request
    /// ran (root plus nested inline frames). `0` on a pure warm hit.
    pub cold_computes: u32,
    /// Number of flow-slice budget refusals observed (each one is a
    /// typed `Budget` failure routed through `ReturnOnly`).
    pub budget_exceeded_events: u32,
    /// Number of coinductive re-entry holds recorded on the shared
    /// obligation runtime (the flow-cycle sentinel).
    pub cycle_reentry_holds: u32,
    /// Why the request was partial: the typed degradation reason on a
    /// degraded-but-usable value, or the typed no-value reason on a
    /// refusal. `None` is the complete, warm-admissible outcome — and
    /// also the default-filled value on the filtered / audit-disabled
    /// record, where no payload was collected at all.
    pub partiality: Option<FlowPartialityTag>,
}
