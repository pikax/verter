//! # `type_info` — the sealed non-flow semantic gateway (C2)
//!
//! [`TypeInfoCore::attempt`](core::TypeInfoCore::attempt) is the only
//! C2-accessible non-flow semantic gateway: a pure kernel that projects
//! Vue macro semantics over immutable observation snapshots. The kernel
//! owns imported-component lookup, macro type-query inventory derivation,
//! and props/emits/model/expose projection shaping — deterministic
//! identity, ordering, and provenance decisions that must not depend on
//! which session or compiler route asked.
//!
//! The kernel never names a host, scheduler, session type, or callback:
//! every input arrives as staged immutable observation data
//! ([`core::NonFlowObservationSnapshot`]), every question it cannot yet
//! answer from that data returns
//! [`NonFlowOutcome::NeedInputs`](non_flow::NonFlowOutcome::NeedInputs)
//! carrying a [`NonFlowLoadSet`](non_flow::NonFlowLoadSet) — the exact
//! unstaged observation slots the route reads, bound to the resolution
//! basis the retry runs under (the operation's stable `C2-GAP3-MISSING-*`
//! proof id lives on
//! [`NonFlowOperation::missing_input_proof_id`](non_flow::NonFlowOperation::missing_input_proof_id))
//! — and a request outside the operation's domain returns
//! [`NonFlowOutcome::Terminal`](non_flow::NonFlowOutcome::Terminal).
//! Driving I/O (loading the demanded inputs, re-attempting, discarding
//! on invalidation) belongs to the compile-transaction driver in
//! `verter_compiler`, never to this crate.

pub mod core;
pub mod non_flow;

pub use core::{
    ImportedComponentResolution, NonFlowObservation, NonFlowObservationKey,
    NonFlowObservationSnapshot, ObservedMacroSurface, ObservedSurfaceMember, TypeInfoCore,
};
pub use non_flow::{
    ExposeSurfaceProjection, ImportedComponentSurface, MacroSemanticLane, NonFlowLoadSet,
    NonFlowOperation, NonFlowOutcome, NonFlowPayload, NonFlowTerminal, ProjectedExposeRow,
    ProjectedRuntimePropRow, RuntimeEmitsProjection, RuntimeModelProjection,
    RuntimePropsProjection, VueMacroMissingRoot, VueMacroSemanticDemand, VueMacroSemanticInput,
    MISSING_PROOF_EMITS, MISSING_PROOF_EXPOSE, MISSING_PROOF_IMPORTED_COMPONENT,
    MISSING_PROOF_MODEL, MISSING_PROOF_PROPS, MISSING_PROOF_VUE_MACRO,
};
