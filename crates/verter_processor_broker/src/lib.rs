//! Native broker substrate for fresh denied trusted-processor workers.

mod attestation;
mod channel;
mod correlation;
mod lifecycle;
mod platform;
mod policy;
mod protocol;

pub use attestation::{
    CanonicalModuleGraph, LaunchEvidenceError, ModuleGraphEntry, ProcessorBrokerInstanceId,
    TrustedProcessorAttestation,
};
pub use channel::{ChannelError, TrustedBrokerChannelBindingV1, ValidatedBrokerChannel};
pub use correlation::{
    BlockContentResolveContextTokenV1, BlockContentWorkTokenV1, CorrelationError,
    CorrelationRegistry, DependencyRequestIdV1,
};
pub use lifecycle::{
    AttestedDeniedWorker, BrokerError, DeniedWorkerBroker, DeniedWorkerLaunch, DeniedWorkerSession,
    SandboxUnavailableEvidence,
};
pub use policy::{DependencyReadKind, ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};

#[doc(hidden)]
#[must_use]
pub fn worker_main() -> i32 {
    lifecycle::worker_entry()
}

#[cfg(test)]
mod tests;
