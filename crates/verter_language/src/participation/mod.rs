//! Per-profile participation: whether Verter serves one capability cell for
//! one profile, document selector and demand purpose (COX0; contract
//! `docs/arch/kernel/cox0d-participation.md`).
//!
//! This module is pure data: the preset and mode vocabulary, the
//! profile-qualified [`CapabilityId`], and the immutable
//! [`CapabilityOwnershipMask`] with its admission decision. It holds no
//! clock, host, editor identity or file-extension heuristic. The live
//! service that publishes one mask with its demand epoch is
//! `verter_session::participation`; hosts reach it through the
//! `verter_protocol::participation` inputs, which carry capability masks and
//! normalized evidence but never extension ids.

mod capability;
mod mask;

pub use capability::{CapabilityId, CapabilityOperation, ProfileRef};
pub use mask::{
    AdmissionRequest, AdmittedCell, CapabilityOwnershipMask, CapabilityTarget, CatalogSupport,
    ClientSupport, DemandPurpose, EffectiveMode, InteractiveOwnership, MaskConstructionError,
    MaskIdentity, NonExecution, NonExecutionReason, OptionalPresentation, OwnershipDeclaration,
    OwnershipEvidence, ParticipationCell, ParticipationInputs, ParticipationPreset,
    PresetDeclaration, ProviderAvailability, ProviderRequirement, ProviderState, RequestSite,
    Restrictions, SelectorScope,
};
