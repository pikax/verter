use std::path::PathBuf;

use crate::attestation::{CanonicalModuleGraph, LaunchEvidenceError, ProcessorBrokerInstanceId};
use crate::correlation::{
    BlockContentResolveContextTokenV1, BlockContentWorkTokenV1, CorrelationError,
    DependencyRequestIdV1,
};
use crate::policy::{ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};
use crate::work::{
    DependencyReadDenial, TrustedBrokerProcessingFailure, WorkerFrameRejection,
    MAX_DEPENDENCY_REQUEST_DESCRIPTOR_BYTES, MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES,
    MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES,
};

pub(crate) const BOOTSTRAP_MAX: usize = 8 * 1024 * 1024;
const BOOTSTRAP_MAGIC: &[u8; 16] = b"VERTER-BROKER-1\0";

#[derive(Clone, Debug)]
pub(crate) struct Bootstrap {
    pub broker_instance: ProcessorBrokerInstanceId,
    pub launch_nonce: [u8; 16],
    pub launch_secret: [u8; 32],
    pub broker_public_key: [u8; 32],
    pub executable_hash: [u8; 32],
    pub canonical_config: Vec<u8>,
    pub module_graph: CanonicalModuleGraph,
    pub sandbox_kind: ProcessorSandboxKindV1,
    pub sandbox_profile_hash: [u8; 32],
    pub manifest: TrustedProcessorCapabilityManifest,
}

impl Bootstrap {
    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::with_capacity(256 + self.canonical_config.len());
        output.extend_from_slice(BOOTSTRAP_MAGIC);
        output.extend_from_slice(self.broker_instance.as_bytes());
        output.extend_from_slice(&self.launch_nonce);
        output.extend_from_slice(&self.launch_secret);
        output.extend_from_slice(&self.broker_public_key);
        output.extend_from_slice(&self.executable_hash);
        encode_bytes(&self.canonical_config, &mut output);
        self.module_graph.encode(&mut output);
        output.push(self.sandbox_kind as u8);
        output.extend_from_slice(&self.sandbox_profile_hash);
        self.manifest.encode_canonical(&mut output);
        output
    }

    pub fn decode(mut input: &[u8]) -> Result<Self, LaunchEvidenceError> {
        if read_array::<16>(&mut input)? != *BOOTSTRAP_MAGIC {
            return Err(LaunchEvidenceError::Io("invalid bootstrap magic".into()));
        }
        let broker_instance = ProcessorBrokerInstanceId::from_bytes(read_array(&mut input)?);
        let launch_nonce = read_array(&mut input)?;
        let launch_secret = read_array(&mut input)?;
        let broker_public_key = read_array(&mut input)?;
        let executable_hash = read_array(&mut input)?;
        let canonical_config = read_bytes(&mut input)?.to_vec();
        let module_graph = CanonicalModuleGraph::decode(&mut input)?;
        let sandbox_kind = match read_array::<1>(&mut input)?[0] {
            1 => ProcessorSandboxKindV1::LinuxNamespaceSeccomp,
            2 => ProcessorSandboxKindV1::MacSandbox,
            3 => ProcessorSandboxKindV1::WindowsAppContainer,
            _ => return Err(LaunchEvidenceError::Io("unknown sandbox kind".into())),
        };
        let sandbox_profile_hash = read_array(&mut input)?;
        let manifest = TrustedProcessorCapabilityManifest::decode_canonical(&mut input)?;
        if !input.is_empty() {
            return Err(LaunchEvidenceError::Io(
                "trailing bootstrap evidence".into(),
            ));
        }
        Ok(Self {
            broker_instance,
            launch_nonce,
            launch_secret,
            broker_public_key,
            executable_hash,
            canonical_config,
            module_graph,
            sandbox_kind,
            sandbox_profile_hash,
            manifest,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerProbe {
    ReadOutsideGrant(PathBuf),
    Network,
    ChildProcess,
    Environment,
    #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
    DirectOpen,
    #[cfg(target_os = "linux")]
    OpenAt2,
    Hang,
    Crash,
}

impl WorkerProbe {
    pub fn decode(mut input: &[u8]) -> Result<Self, LaunchEvidenceError> {
        let tag = read_array::<1>(&mut input)?[0];
        let probe = match tag {
            1 => Self::ReadOutsideGrant(PathBuf::from(
                String::from_utf8(read_bytes(&mut input)?.to_vec())
                    .map_err(|error| LaunchEvidenceError::Io(error.to_string()))?,
            )),
            2 => Self::Network,
            3 => Self::ChildProcess,
            4 => Self::Environment,
            #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
            7 => Self::DirectOpen,
            #[cfg(target_os = "linux")]
            8 => Self::OpenAt2,
            5 => Self::Hang,
            6 => Self::Crash,
            _ => return Err(LaunchEvidenceError::Io("unknown worker probe".into())),
        };
        if !input.is_empty() {
            return Err(LaunchEvidenceError::Io("trailing worker probe".into()));
        }
        Ok(probe)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WorkScope {
    pub context: BlockContentResolveContextTokenV1,
    pub work: BlockContentWorkTokenV1,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum BrokerToWorkerFrame {
    Probe(WorkerProbe),
    WorkStart {
        scope: WorkScope,
        total: u64,
    },
    WorkChunk {
        scope: WorkScope,
        offset: u64,
        bytes: Vec<u8>,
    },
    WorkEnd {
        scope: WorkScope,
    },
    DependencyResolvedStart {
        scope: WorkScope,
        id: DependencyRequestIdV1,
        total: u64,
    },
    DependencyResolvedChunk {
        scope: WorkScope,
        id: DependencyRequestIdV1,
        offset: u64,
        bytes: Vec<u8>,
    },
    DependencyResolvedEnd {
        scope: WorkScope,
        id: DependencyRequestIdV1,
    },
    DependencyDenied {
        scope: WorkScope,
        id: DependencyRequestIdV1,
        denial: DependencyReadDenial,
    },
    DependencyCorrelationRejected {
        scope: WorkScope,
        id: DependencyRequestIdV1,
        error: CorrelationError,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum WorkerToBrokerFrame {
    ProbeResult(bool),
    DependencyRequest {
        scope: WorkScope,
        id: DependencyRequestIdV1,
        kind: crate::policy::DependencyReadKind,
        descriptor: Vec<u8>,
    },
    WorkSuccessStart {
        scope: WorkScope,
        total: u64,
    },
    WorkSuccessChunk {
        scope: WorkScope,
        offset: u64,
        bytes: Vec<u8>,
    },
    WorkSuccessEnd {
        scope: WorkScope,
    },
    WorkFailure {
        scope: WorkScope,
        failure: TrustedBrokerProcessingFailure,
    },
    FrameRejected(WorkerFrameRejection),
}

impl BrokerToWorkerFrame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        match self {
            Self::Probe(probe) => {
                output.push(1);
                encode_bytes(&probe.encode_wire(), &mut output);
            }
            Self::WorkStart { scope, total } => {
                output.push(2);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&total.to_be_bytes());
            }
            Self::WorkChunk {
                scope,
                offset,
                bytes,
            } => {
                output.push(3);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&offset.to_be_bytes());
                encode_bytes(bytes, &mut output);
            }
            Self::WorkEnd { scope } => {
                output.push(4);
                encode_scope(*scope, &mut output);
            }
            Self::DependencyResolvedStart { scope, id, total } => {
                output.push(5);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
                output.extend_from_slice(&total.to_be_bytes());
            }
            Self::DependencyResolvedChunk {
                scope,
                id,
                offset,
                bytes,
            } => {
                output.push(6);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
                output.extend_from_slice(&offset.to_be_bytes());
                encode_bytes(bytes, &mut output);
            }
            Self::DependencyResolvedEnd { scope, id } => {
                output.push(7);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
            }
            Self::DependencyDenied { scope, id, denial } => {
                output.push(8);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
                output.push(encode_dependency_denial(*denial));
            }
            Self::DependencyCorrelationRejected { scope, id, error } => {
                output.push(9);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
                output.push(encode_correlation_error(*error));
            }
        }
        output
    }

    pub(crate) fn decode(input: &[u8]) -> Result<Self, WorkerFrameRejection> {
        let mut reader = WireReader::new(input);
        let tag = reader.byte()?;
        let frame = match tag {
            1 => Self::Probe(WorkerProbe::decode_wire(reader.bytes()?)?),
            2 => {
                let scope = reader.scope()?;
                let total = reader.u64()?;
                if total > MAX_TRUSTED_BROKER_WORK_DESCRIPTOR_BYTES as u64 {
                    return Err(WorkerFrameRejection::PayloadTooLarge);
                }
                Self::WorkStart { scope, total }
            }
            3 => Self::WorkChunk {
                scope: reader.scope()?,
                offset: reader.u64()?,
                bytes: reader.bytes()?.to_vec(),
            },
            4 => Self::WorkEnd {
                scope: reader.scope()?,
            },
            5 => Self::DependencyResolvedStart {
                scope: reader.scope()?,
                id: reader.dependency_id()?,
                total: reader.u64()?,
            },
            6 => Self::DependencyResolvedChunk {
                scope: reader.scope()?,
                id: reader.dependency_id()?,
                offset: reader.u64()?,
                bytes: reader.bytes()?.to_vec(),
            },
            7 => Self::DependencyResolvedEnd {
                scope: reader.scope()?,
                id: reader.dependency_id()?,
            },
            8 => Self::DependencyDenied {
                scope: reader.scope()?,
                id: reader.dependency_id()?,
                denial: decode_dependency_denial(reader.byte()?)?,
            },
            9 => Self::DependencyCorrelationRejected {
                scope: reader.scope()?,
                id: reader.dependency_id()?,
                error: decode_correlation_error(reader.byte()?)?,
            },
            _ => return Err(WorkerFrameRejection::UnknownFrame),
        };
        reader.finish()?;
        Ok(frame)
    }
}

impl WorkerToBrokerFrame {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        match self {
            Self::ProbeResult(value) => {
                output.extend_from_slice(&[1, u8::from(*value)]);
            }
            Self::DependencyRequest {
                scope,
                id,
                kind,
                descriptor,
            } => {
                output.push(2);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&id.as_bytes());
                output.push(*kind as u8);
                encode_bytes(descriptor, &mut output);
            }
            Self::WorkSuccessStart { scope, total } => {
                output.push(3);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&total.to_be_bytes());
            }
            Self::WorkSuccessChunk {
                scope,
                offset,
                bytes,
            } => {
                output.push(4);
                encode_scope(*scope, &mut output);
                output.extend_from_slice(&offset.to_be_bytes());
                encode_bytes(bytes, &mut output);
            }
            Self::WorkSuccessEnd { scope } => {
                output.push(5);
                encode_scope(*scope, &mut output);
            }
            Self::WorkFailure { scope, failure } => {
                output.push(6);
                encode_scope(*scope, &mut output);
                encode_processing_failure(failure, &mut output);
            }
            Self::FrameRejected(rejection) => {
                output.extend_from_slice(&[7, encode_frame_rejection(*rejection)]);
            }
        }
        output
    }

    pub(crate) fn decode(input: &[u8]) -> Result<Self, WorkerFrameRejection> {
        let mut reader = WireReader::new(input);
        let frame = match reader.byte()? {
            1 => Self::ProbeResult(match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(WorkerFrameRejection::Malformed),
            }),
            2 => {
                let scope = reader.scope()?;
                let id = reader.dependency_id()?;
                let kind = crate::policy::DependencyReadKind::from_wire(reader.byte()?)
                    .ok_or(WorkerFrameRejection::Malformed)?;
                let descriptor = reader.bytes()?.to_vec();
                if descriptor.len() > MAX_DEPENDENCY_REQUEST_DESCRIPTOR_BYTES {
                    return Err(WorkerFrameRejection::PayloadTooLarge);
                }
                Self::DependencyRequest {
                    scope,
                    id,
                    kind,
                    descriptor,
                }
            }
            3 => {
                let scope = reader.scope()?;
                let total = reader.u64()?;
                if total > MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES as u64 {
                    return Err(WorkerFrameRejection::PayloadTooLarge);
                }
                Self::WorkSuccessStart { scope, total }
            }
            4 => Self::WorkSuccessChunk {
                scope: reader.scope()?,
                offset: reader.u64()?,
                bytes: reader.bytes()?.to_vec(),
            },
            5 => Self::WorkSuccessEnd {
                scope: reader.scope()?,
            },
            6 => Self::WorkFailure {
                scope: reader.scope()?,
                failure: decode_processing_failure(&mut reader)?,
            },
            7 => Self::FrameRejected(decode_frame_rejection(reader.byte()?)?),
            _ => return Err(WorkerFrameRejection::UnknownFrame),
        };
        reader.finish()?;
        Ok(frame)
    }
}

impl WorkerProbe {
    fn encode_wire(&self) -> Vec<u8> {
        let mut output = Vec::new();
        match self {
            Self::ReadOutsideGrant(path) => {
                output.push(1);
                encode_bytes(path.to_string_lossy().as_bytes(), &mut output);
            }
            Self::Network => output.push(2),
            Self::ChildProcess => output.push(3),
            Self::Environment => output.push(4),
            Self::Hang => output.push(5),
            Self::Crash => output.push(6),
            #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
            Self::DirectOpen => output.push(7),
            #[cfg(target_os = "linux")]
            Self::OpenAt2 => output.push(8),
        }
        output
    }

    fn decode_wire(input: &[u8]) -> Result<Self, WorkerFrameRejection> {
        Self::decode(input).map_err(|_| WorkerFrameRejection::Malformed)
    }
}

fn encode_scope(scope: WorkScope, output: &mut Vec<u8>) {
    output.extend_from_slice(&scope.context.as_bytes());
    output.extend_from_slice(&scope.work.as_bytes());
}

struct WireReader<'a> {
    remaining: &'a [u8],
}

impl<'a> WireReader<'a> {
    const fn new(input: &'a [u8]) -> Self {
        Self { remaining: input }
    }

    fn byte(&mut self) -> Result<u8, WorkerFrameRejection> {
        Ok(self.array::<1>()?[0])
    }

    fn u64(&mut self) -> Result<u64, WorkerFrameRejection> {
        Ok(u64::from_be_bytes(self.array()?))
    }

    fn array<const N: usize>(&mut self) -> Result<[u8; N], WorkerFrameRejection> {
        if self.remaining.len() < N {
            return Err(WorkerFrameRejection::TruncatedPayload);
        }
        let (head, tail) = self.remaining.split_at(N);
        self.remaining = tail;
        head.try_into()
            .map_err(|_| WorkerFrameRejection::TruncatedPayload)
    }

    fn bytes(&mut self) -> Result<&'a [u8], WorkerFrameRejection> {
        let length = u32::from_be_bytes(self.array()?) as usize;
        if self.remaining.len() < length {
            return Err(WorkerFrameRejection::TruncatedPayload);
        }
        let (head, tail) = self.remaining.split_at(length);
        self.remaining = tail;
        Ok(head)
    }

    fn scope(&mut self) -> Result<WorkScope, WorkerFrameRejection> {
        Ok(WorkScope {
            context: BlockContentResolveContextTokenV1::from_bytes(self.array()?),
            work: BlockContentWorkTokenV1::from_bytes(self.array()?),
        })
    }

    fn dependency_id(&mut self) -> Result<DependencyRequestIdV1, WorkerFrameRejection> {
        DependencyRequestIdV1::from_bytes(self.array()?)
            .map_err(|_| WorkerFrameRejection::Malformed)
    }

    fn finish(self) -> Result<(), WorkerFrameRejection> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(WorkerFrameRejection::Malformed)
        }
    }
}

fn encode_processing_failure(failure: &TrustedBrokerProcessingFailure, output: &mut Vec<u8>) {
    match failure {
        TrustedBrokerProcessingFailure::MalformedDescriptor => output.push(1),
        TrustedBrokerProcessingFailure::UnknownDescriptor => output.push(2),
        TrustedBrokerProcessingFailure::DependencyDenied(denial) => {
            output.extend_from_slice(&[3, encode_dependency_denial(*denial)]);
        }
        TrustedBrokerProcessingFailure::DependencyCorrelationRejected(error) => {
            output.extend_from_slice(&[4, encode_correlation_error(*error)]);
        }
        TrustedBrokerProcessingFailure::ProtocolRejected(rejection) => {
            output.extend_from_slice(&[5, encode_frame_rejection(*rejection)]);
        }
    }
}

fn decode_processing_failure(
    reader: &mut WireReader<'_>,
) -> Result<TrustedBrokerProcessingFailure, WorkerFrameRejection> {
    Ok(match reader.byte()? {
        1 => TrustedBrokerProcessingFailure::MalformedDescriptor,
        2 => TrustedBrokerProcessingFailure::UnknownDescriptor,
        3 => TrustedBrokerProcessingFailure::DependencyDenied(decode_dependency_denial(
            reader.byte()?,
        )?),
        4 => TrustedBrokerProcessingFailure::DependencyCorrelationRejected(
            decode_correlation_error(reader.byte()?)?,
        ),
        5 => TrustedBrokerProcessingFailure::ProtocolRejected(decode_frame_rejection(
            reader.byte()?,
        )?),
        _ => return Err(WorkerFrameRejection::Malformed),
    })
}

const fn encode_dependency_denial(value: DependencyReadDenial) -> u8 {
    match value {
        DependencyReadDenial::NotFound => 1,
        DependencyReadDenial::ScopeDenied => 2,
        DependencyReadDenial::Cycle => 3,
        DependencyReadDenial::BudgetExceeded => 4,
        DependencyReadDenial::Stale => 5,
        DependencyReadDenial::Cancelled => 6,
        DependencyReadDenial::Failed => 7,
        DependencyReadDenial::KindNotPermitted => 8,
    }
}

fn decode_dependency_denial(value: u8) -> Result<DependencyReadDenial, WorkerFrameRejection> {
    match value {
        1 => Ok(DependencyReadDenial::NotFound),
        2 => Ok(DependencyReadDenial::ScopeDenied),
        3 => Ok(DependencyReadDenial::Cycle),
        4 => Ok(DependencyReadDenial::BudgetExceeded),
        5 => Ok(DependencyReadDenial::Stale),
        6 => Ok(DependencyReadDenial::Cancelled),
        7 => Ok(DependencyReadDenial::Failed),
        8 => Ok(DependencyReadDenial::KindNotPermitted),
        _ => Err(WorkerFrameRejection::Malformed),
    }
}

const fn encode_frame_rejection(value: WorkerFrameRejection) -> u8 {
    match value {
        WorkerFrameRejection::Malformed => 1,
        WorkerFrameRejection::UnknownFrame => 2,
        WorkerFrameRejection::OutOfWindow => 3,
        WorkerFrameRejection::ContextMismatch => 4,
        WorkerFrameRejection::WorkMismatch => 5,
        WorkerFrameRejection::DependencyMismatch => 6,
        WorkerFrameRejection::PayloadTooLarge => 7,
        WorkerFrameRejection::TruncatedPayload => 8,
    }
}

fn decode_frame_rejection(value: u8) -> Result<WorkerFrameRejection, WorkerFrameRejection> {
    match value {
        1 => Ok(WorkerFrameRejection::Malformed),
        2 => Ok(WorkerFrameRejection::UnknownFrame),
        3 => Ok(WorkerFrameRejection::OutOfWindow),
        4 => Ok(WorkerFrameRejection::ContextMismatch),
        5 => Ok(WorkerFrameRejection::WorkMismatch),
        6 => Ok(WorkerFrameRejection::DependencyMismatch),
        7 => Ok(WorkerFrameRejection::PayloadTooLarge),
        8 => Ok(WorkerFrameRejection::TruncatedPayload),
        _ => Err(WorkerFrameRejection::Malformed),
    }
}

const fn encode_correlation_error(value: CorrelationError) -> u8 {
    match value {
        CorrelationError::MalformedRequestId => 1,
        CorrelationError::AllZeroRequestId => 2,
        CorrelationError::DuplicatePending => 3,
        CorrelationError::ReplayConsumed => 4,
        CorrelationError::UnknownRequest => 5,
        CorrelationError::ContextMismatch => 6,
        CorrelationError::WorkMismatch => 7,
        CorrelationError::ChannelMismatch => 8,
    }
}

fn decode_correlation_error(value: u8) -> Result<CorrelationError, WorkerFrameRejection> {
    match value {
        1 => Ok(CorrelationError::MalformedRequestId),
        2 => Ok(CorrelationError::AllZeroRequestId),
        3 => Ok(CorrelationError::DuplicatePending),
        4 => Ok(CorrelationError::ReplayConsumed),
        5 => Ok(CorrelationError::UnknownRequest),
        6 => Ok(CorrelationError::ContextMismatch),
        7 => Ok(CorrelationError::WorkMismatch),
        8 => Ok(CorrelationError::ChannelMismatch),
        _ => Err(WorkerFrameRejection::Malformed),
    }
}

fn encode_bytes(bytes: &[u8], output: &mut Vec<u8>) {
    output.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    output.extend_from_slice(bytes);
}

fn read_bytes<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], LaunchEvidenceError> {
    let length = u32::from_be_bytes(read_array(input)?) as usize;
    if input.len() < length {
        return Err(LaunchEvidenceError::Io("truncated protocol bytes".into()));
    }
    let (head, tail) = input.split_at(length);
    *input = tail;
    Ok(head)
}

fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], LaunchEvidenceError> {
    if input.len() < N {
        return Err(LaunchEvidenceError::Io("truncated protocol bytes".into()));
    }
    let (head, tail) = input.split_at(N);
    *input = tail;
    Ok(head.try_into().expect("length checked"))
}
