use crate::correlation::{
    BlockContentResolveContextTokenV1, BlockContentWorkTokenV1, CorrelationError,
    DependencyRequestIdV1,
};
use crate::policy::DependencyReadKind;

/// Maximum encoded processor-step descriptor accepted by one work submission.
pub const MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES: usize = 8 * 1024 * 1024;
pub(crate) const MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES: usize = 64 * 1024 * 1024;
pub(crate) const MAX_DEPENDENCY_BYTES_PER_WORK: usize = 64 * 1024 * 1024;
pub(crate) const MAX_DEPENDENCY_READS_PER_WORK: usize = 256;
pub(crate) const MAX_DEPENDENCY_REQUEST_DESCRIPTOR_BYTES: usize = 16 * 1024;

/// A bounded, byte-neutral request for one denied worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedBrokerWork {
    resolve_context: BlockContentResolveContextTokenV1,
    work: BlockContentWorkTokenV1,
    processor_step_descriptor: Vec<u8>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrustedBrokerWorkError {
    DescriptorTooLarge,
}

impl TrustedBrokerWork {
    pub fn new(
        resolve_context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
        processor_step_descriptor: Vec<u8>,
    ) -> Result<Self, TrustedBrokerWorkError> {
        if processor_step_descriptor.len() > MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES {
            return Err(TrustedBrokerWorkError::DescriptorTooLarge);
        }
        Ok(Self {
            resolve_context,
            work,
            processor_step_descriptor,
        })
    }

    #[must_use]
    pub const fn resolve_context(&self) -> BlockContentResolveContextTokenV1 {
        self.resolve_context
    }

    #[must_use]
    pub const fn work(&self) -> BlockContentWorkTokenV1 {
        self.work
    }

    #[must_use]
    pub fn processor_step_descriptor(&self) -> &[u8] {
        &self.processor_step_descriptor
    }
}

/// Bytes returned by an authenticated worker after result admission.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedBrokerWorkOutput(Vec<u8>);

impl TrustedBrokerWorkOutput {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    #[must_use]
    pub fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// A terminal work result from the authenticated worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedBrokerWorkResult {
    Success(TrustedBrokerWorkOutput),
    Failed(TrustedBrokerProcessingFailure),
}

/// Closed worker-side processing failures, disjoint from broker lifecycle errors.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum TrustedBrokerProcessingFailure {
    MalformedDescriptor,
    UnknownDescriptor,
    DependencyDenied(DependencyReadDenial),
    DependencyCorrelationRejected(CorrelationError),
    ProtocolRejected(WorkerFrameRejection),
}

/// Typed rejection of a decoded but invalid application frame.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkerFrameRejection {
    Malformed,
    UnknownFrame,
    OutOfWindow,
    ContextMismatch,
    WorkMismatch,
    DependencyMismatch,
    PayloadTooLarge,
    TruncatedPayload,
}

/// Authority-mediated dependency-read request from the suspended worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DependencyReadRequest {
    resolve_context: BlockContentResolveContextTokenV1,
    work: BlockContentWorkTokenV1,
    id: DependencyRequestIdV1,
    kind: DependencyReadKind,
    descriptor: Vec<u8>,
}

impl DependencyReadRequest {
    pub(crate) fn new(
        resolve_context: BlockContentResolveContextTokenV1,
        work: BlockContentWorkTokenV1,
        id: DependencyRequestIdV1,
        kind: DependencyReadKind,
        descriptor: Vec<u8>,
    ) -> Self {
        Self {
            resolve_context,
            work,
            id,
            kind,
            descriptor,
        }
    }

    #[must_use]
    pub const fn resolve_context(&self) -> BlockContentResolveContextTokenV1 {
        self.resolve_context
    }

    #[must_use]
    pub const fn work(&self) -> BlockContentWorkTokenV1 {
        self.work
    }

    #[must_use]
    pub const fn id(&self) -> DependencyRequestIdV1 {
        self.id
    }

    #[must_use]
    pub const fn kind(&self) -> DependencyReadKind {
        self.kind
    }

    #[must_use]
    pub fn descriptor(&self) -> &[u8] {
        &self.descriptor
    }
}

/// Capability-scoped dependency bytes returned only through an authority callback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CapabilityScopedDependencyBytes(Vec<u8>);

impl CapabilityScopedDependencyBytes {
    #[must_use]
    pub fn bytes(&self) -> &[u8] {
        &self.0
    }

    pub(crate) fn into_bytes(self) -> Vec<u8> {
        self.0
    }
}

/// Closed authority decision for one exact dependency-read correlation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DependencyReadDecision {
    Resolved(CapabilityScopedDependencyBytes),
    Denied(DependencyReadDenial),
}

impl DependencyReadDecision {
    #[must_use]
    pub fn resolved(bytes: Vec<u8>) -> Self {
        Self::Resolved(CapabilityScopedDependencyBytes(bytes))
    }

    #[must_use]
    pub const fn denied(reason: DependencyReadDenial) -> Self {
        Self::Denied(reason)
    }
}

/// Closed authority-side denial sum for a dependency read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DependencyReadDenial {
    NotFound,
    ScopeDenied,
    Cycle,
    BudgetExceeded,
    Stale,
    Cancelled,
    Failed,
    KindNotPermitted,
}

mod sealed {
    pub trait Sealed {}
}

/// Sealed authority callback invoked only for a registered pending dependency ID.
pub trait DependencyReadAuthority: sealed::Sealed {
    fn read_dependency(&mut self, request: &DependencyReadRequest) -> DependencyReadDecision;
}

pub struct DependencyReadAuthorityFn<F> {
    callback: F,
}

impl<F> sealed::Sealed for DependencyReadAuthorityFn<F> {}

impl<F> DependencyReadAuthority for DependencyReadAuthorityFn<F>
where
    F: FnMut(&DependencyReadRequest) -> DependencyReadDecision,
{
    fn read_dependency(&mut self, request: &DependencyReadRequest) -> DependencyReadDecision {
        (self.callback)(request)
    }
}

/// Wraps an authority callback in the broker-owned sealed interface.
#[must_use]
pub fn dependency_read_authority<F>(callback: F) -> DependencyReadAuthorityFn<F>
where
    F: FnMut(&DependencyReadRequest) -> DependencyReadDecision,
{
    DependencyReadAuthorityFn { callback }
}
