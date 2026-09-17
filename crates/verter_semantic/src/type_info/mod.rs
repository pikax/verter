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
//! ([`core::NonFlowObservationSnapshot`]), and every question it cannot
//! answer from that data returns
//! [`AttemptOutcome::NeedInputs`](crate::resolver_core::AttemptOutcome)
//! with the operation's missing-input proof id. Driving I/O (loading the
//! demanded inputs, re-attempting, discarding on invalidation) belongs to
//! the compile-transaction driver in `verter_compiler`, never to this
//! crate.

pub mod core;
pub mod non_flow;

pub use core::{
    ImportedComponentResolution, NonFlowObservation, NonFlowObservationKey,
    NonFlowObservationSnapshot, ObservedMacroSurface, ObservedSurfaceMember, TypeInfoCore,
};
pub use non_flow::{
    ExposeSurfaceProjection, ImportedComponentSurface, MacroSemanticLane, NonFlowLoadSet,
    NonFlowOperation, NonFlowOutcome, NonFlowPayload, ProjectedExposeRow, ProjectedRuntimePropRow,
    RuntimeEmitsProjection, RuntimeModelProjection, RuntimePropsProjection, VueMacroMissingRoot,
    VueMacroSemanticDemand, VueMacroSemanticInput, MISSING_PROOF_EMITS, MISSING_PROOF_EXPOSE,
    MISSING_PROOF_IMPORTED_COMPONENT, MISSING_PROOF_MODEL, MISSING_PROOF_PROPS,
    MISSING_PROOF_VUE_MACRO,
};
