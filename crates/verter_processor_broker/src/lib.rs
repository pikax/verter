//! Native broker substrate for fresh denied trusted-processor workers.

mod attestation;
mod channel;
mod correlation;
mod execution;
mod lifecycle;
mod platform;
mod policy;
mod protocol;
mod work;

pub use attestation::{
    CanonicalModuleGraph, LaunchEvidenceError, ModuleGraphEntry, ProcessorBrokerInstanceId,
    TrustedProcessorAttestation,
};
pub use channel::{ChannelError, TrustedBrokerChannelBindingV1, ValidatedBrokerChannel};
pub use correlation::{
    BlockContentResolveContextTokenV1, BlockContentWorkTokenV1, CorrelationAuditEvent,
    CorrelationError, CorrelationRegistry, DependencyRequestIdV1, CONSUMED_CORRELATION_TTL,
    MAX_CORRELATION_ENTRIES,
};
pub use lifecycle::{
    AttestedDeniedWorker, BrokerError, DeniedWorkerBroker, DeniedWorkerLaunch, DeniedWorkerSession,
    SandboxUnavailableEvidence,
};
pub use policy::{DependencyReadKind, ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};
pub use work::{
    dependency_read_authority, CapabilityScopedDependencyBytes, DependencyReadAuthority,
    DependencyReadAuthorityFn, DependencyReadDecision, DependencyReadDenial, DependencyReadRequest,
    TrustedBrokerProcessingFailure, TrustedBrokerWork, TrustedBrokerWorkError,
    TrustedBrokerWorkOutput, TrustedBrokerWorkResult, WorkerFrameRejection,
    MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES,
};

#[cfg(test)]
pub(crate) use lifecycle::EvidenceMutationPoint;

#[doc(hidden)]
#[must_use]
pub fn worker_main() -> i32 {
    lifecycle::worker_entry()
}

#[cfg(test)]
mod hardening_tests;
#[cfg(test)]
mod tests;
