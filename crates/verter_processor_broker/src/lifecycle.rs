use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use crate::attestation::{
    config_hash, executable_hash, manifest_hash, AttestationFields, CanonicalModuleGraph,
    LaunchEvidenceError, ModuleGraphEntry, ProcessorBrokerInstanceId, TrustedProcessorAttestation,
};
use crate::channel::{
    build_handshake, generate_ephemeral_keypair, read_handshake_message, write_handshake_message,
    ChannelBindingInputs, ChannelError, TrustedBrokerChannelBindingV1, ValidatedBrokerChannel,
};
use crate::correlation::{CorrelationError, CorrelationRegistry, DependencyRequestIdV1};
use crate::execution::{WorkerExecutionEnvelope, WorkerExecutionEvent, WorkerExecutionMachine};
use crate::platform::{self, PlatformChild, PlatformStream};
use crate::policy::{ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};
use crate::protocol::{
    Bootstrap, BrokerToWorkerFrame, WorkScope, WorkerProbe, WorkerToBrokerFrame, BOOTSTRAP_MAX,
};
use crate::work::{
    DependencyReadAuthority, DependencyReadDecision, DependencyReadDenial, DependencyReadRequest,
    TrustedBrokerProcessingFailure, TrustedBrokerWork, TrustedBrokerWorkOutput,
    TrustedBrokerWorkResult, WorkerFrameRejection, MAX_DEPENDENCY_BYTES_PER_WORK,
    MAX_DEPENDENCY_READS_PER_WORK, MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES,
};

const APPLICATION_CHUNK_BYTES: usize = 48 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SandboxUnavailableEvidence {
    sandbox_kind: ProcessorSandboxKindV1,
    operation: &'static str,
    os_error: Option<i32>,
}

impl SandboxUnavailableEvidence {
    pub(crate) const fn new(
        sandbox_kind: ProcessorSandboxKindV1,
        operation: &'static str,
        os_error: Option<i32>,
    ) -> Self {
        Self {
            sandbox_kind,
            operation,
            os_error,
        }
    }

    #[must_use]
    pub const fn sandbox_kind(&self) -> ProcessorSandboxKindV1 {
        self.sandbox_kind
    }

    #[must_use]
    pub const fn operation(&self) -> &'static str {
        self.operation
    }

    #[must_use]
    pub const fn os_error(&self) -> Option<i32> {
        self.os_error
    }

    #[must_use]
    pub const fn is_typed_and_fail_closed(&self) -> bool {
        !self.operation.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BrokerError {
    SandboxUnavailable(SandboxUnavailableEvidence),
    LaunchEvidence(LaunchEvidenceError),
    Channel(ChannelError),
    Correlation(CorrelationError),
    WorkerTimeout,
    WorkerCrashed(Option<i32>),
    WorkerFrameRejected(WorkerFrameRejection),
    Protocol(&'static str),
    Io(String),
}

impl From<LaunchEvidenceError> for BrokerError {
    fn from(value: LaunchEvidenceError) -> Self {
        Self::LaunchEvidence(value)
    }
}

impl From<ChannelError> for BrokerError {
    fn from(value: ChannelError) -> Self {
        Self::Channel(value)
    }
}

impl From<CorrelationError> for BrokerError {
    fn from(value: CorrelationError) -> Self {
        Self::Correlation(value)
    }
}

impl From<WorkerFrameRejection> for BrokerError {
    fn from(value: WorkerFrameRejection) -> Self {
        Self::WorkerFrameRejected(value)
    }
}

impl From<std::io::Error> for BrokerError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

#[derive(Clone)]
pub struct DeniedWorkerLaunch {
    executable: PathBuf,
    canonical_config: Vec<u8>,
    module_graph: CanonicalModuleGraph,
    manifest: TrustedProcessorCapabilityManifest,
}

impl fmt::Debug for DeniedWorkerLaunch {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DeniedWorkerLaunch")
            .field("executable", &self.executable)
            .field("canonical_config_len", &self.canonical_config.len())
            .field("module_graph", &self.module_graph)
            .field("manifest", &self.manifest)
            .finish()
    }
}

impl DeniedWorkerLaunch {
    pub fn new(
        executable: impl Into<PathBuf>,
        canonical_config: Vec<u8>,
        module_graph: CanonicalModuleGraph,
        manifest: TrustedProcessorCapabilityManifest,
    ) -> Result<Self, BrokerError> {
        let executable = executable.into();
        let actual = executable_hash(&executable)?;
        if actual != manifest.processor_binary_hash() {
            return Err(LaunchEvidenceError::ExecutableHashMismatch.into());
        }
        if platform::sandbox_profile_hash() != manifest.sandbox_profile_hash() {
            return Err(LaunchEvidenceError::SandboxProfileHashMismatch.into());
        }
        Ok(Self {
            executable,
            canonical_config,
            module_graph,
            manifest,
        })
    }

    pub fn from_worker(
        executable: impl Into<PathBuf>,
        canonical_config: Vec<u8>,
        module_graph: impl IntoIterator<Item = ModuleGraphEntry>,
    ) -> Result<Self, BrokerError> {
        let executable = executable.into();
        let manifest = TrustedProcessorCapabilityManifest::denied(
            executable_hash(&executable)?,
            platform::sandbox_profile_hash(),
            [],
        );
        Self::new(
            executable,
            canonical_config,
            CanonicalModuleGraph::new(module_graph)?,
            manifest,
        )
    }
}

pub struct DeniedWorkerBroker {
    instance: ProcessorBrokerInstanceId,
}

impl DeniedWorkerBroker {
    pub fn new() -> Result<Self, BrokerError> {
        let mut instance = [0_u8; 16];
        platform::random_fill(&mut instance)?;
        Ok(Self {
            instance: ProcessorBrokerInstanceId::from_bytes(instance),
        })
    }

    pub fn launch(
        &self,
        launch: DeniedWorkerLaunch,
        timeout: Duration,
    ) -> Result<DeniedWorkerSession, BrokerError> {
        let mut launch_nonce = [0_u8; 16];
        let mut launch_secret = [0_u8; 32];
        platform::random_fill(&mut launch_nonce)?;
        platform::random_fill(&mut launch_secret)?;
        let broker_key = generate_ephemeral_keypair()?;
        let mut spawned = platform::spawn_denied_worker(&launch.executable, &launch_nonce)?;
        let attestation =
            build_attestation(self.instance, launch_nonce, &spawned.executable, &launch)?;
        let broker_public: [u8; 32] = broker_key
            .public
            .as_slice()
            .try_into()
            .map_err(|_| ChannelError::InvalidKey)?;
        let bootstrap = Bootstrap {
            broker_instance: self.instance,
            launch_nonce,
            launch_secret,
            broker_public_key: broker_public,
            executable_hash: attestation.executable_hash(),
            canonical_config: launch.canonical_config.clone(),
            module_graph: launch.module_graph.clone(),
            sandbox_kind: ProcessorSandboxKindV1::current(),
            sandbox_profile_hash: launch.manifest.sandbox_profile_hash(),
            manifest: launch.manifest.clone(),
        };
        write_bounded(&mut spawned.stream, &bootstrap.encode())?;
        platform::wait_readable(&mut spawned.stream, &mut spawned.child, timeout)?;
        let worker_hello = read_bounded(&mut spawned.stream, 4096)?;
        if let Some(message) = worker_hello.strip_prefix(b"ERROR:") {
            return Err(BrokerError::Io(
                String::from_utf8_lossy(message).into_owned(),
            ));
        }
        let worker_public: [u8; 32] = worker_hello
            .try_into()
            .map_err(|_| BrokerError::Protocol("invalid worker public key"))?;
        let attestation_hash = attestation.canonical_hash();
        let (binding, transcript) =
            TrustedBrokerChannelBindingV1::from_transcript(ChannelBindingInputs {
                broker_instance_token: self.instance,
                broker_ephemeral_public_key: broker_public,
                worker_ephemeral_public_key: worker_public,
                launch_nonce,
                broker_attestation_hash: attestation_hash,
                worker_attestation_hash: attestation_hash,
                manifest_hash: attestation.manifest_hash(),
                sandbox_profile_hash: attestation.sandbox_profile_hash(),
            });
        let mut handshake = build_handshake(
            true,
            &broker_key.private,
            &worker_public,
            &transcript,
            &launch_secret,
        )?;
        write_bounded(
            &mut spawned.stream,
            &write_handshake_message(&mut handshake, b"broker-attested")?,
        )?;
        platform::wait_readable(&mut spawned.stream, &mut spawned.child, timeout)?;
        let response =
            read_handshake_message(&mut handshake, &read_bounded(&mut spawned.stream, 65_535)?)?;
        if response != b"worker-attested" {
            return Err(BrokerError::Protocol("worker handshake payload mismatch"));
        }
        let transport = handshake
            .into_transport_mode()
            .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
        let mut channel = ValidatedBrokerChannel::new(binding, transport);
        platform::wait_readable(&mut spawned.stream, &mut spawned.child, timeout)?;
        let admission = channel.read_frame(&mut spawned.stream)?;
        if admission.as_slice() != binding_admission(&channel, &attestation) {
            return Err(BrokerError::Protocol("worker admission mismatch"));
        }
        let worker = AttestedDeniedWorker {
            child: spawned.child,
            attestation,
        };
        let correlation = CorrelationRegistry::new(channel.binding().handshake_transcript_hash());
        let mut session = DeniedWorkerSession {
            worker,
            channel,
            _stream: spawned.stream,
            launch,
            launched_executable: spawned.executable,
            correlation,
            #[cfg(test)]
            evidence_mutation_point: None,
            #[cfg(test)]
            original_config_for_test: None,
            #[cfg(test)]
            force_worker_frame_rejection_for_test: false,
        };
        session.recheck_evidence()?;
        Ok(session)
    }
}

/// A worker handle that is minted only after sandbox and attestation validation.
pub struct AttestedDeniedWorker {
    child: PlatformChild,
    attestation: TrustedProcessorAttestation,
}

impl AttestedDeniedWorker {
    #[must_use]
    pub fn attestation(&self) -> &TrustedProcessorAttestation {
        &self.attestation
    }

    #[must_use]
    pub fn pid(&self) -> u32 {
        self.child.pid()
    }
}

/// The paired sealed worker and mutually authenticated broker channel.
pub struct DeniedWorkerSession {
    worker: AttestedDeniedWorker,
    channel: ValidatedBrokerChannel,
    _stream: PlatformStream,
    launch: DeniedWorkerLaunch,
    launched_executable: PathBuf,
    correlation: CorrelationRegistry,
    #[cfg(test)]
    evidence_mutation_point: Option<EvidenceMutationPoint>,
    #[cfg(test)]
    original_config_for_test: Option<Vec<u8>>,
    #[cfg(test)]
    force_worker_frame_rejection_for_test: bool,
}

impl DeniedWorkerSession {
    #[must_use]
    pub const fn worker(&self) -> &AttestedDeniedWorker {
        &self.worker
    }

    #[must_use]
    pub const fn channel(&self) -> &ValidatedBrokerChannel {
        &self.channel
    }

    fn recheck_evidence(&mut self) -> Result<(), BrokerError> {
        let current = build_attestation(
            self.worker.attestation.broker_instance(),
            self.worker.attestation.launch_nonce(),
            &self.launched_executable,
            &self.launch,
        )?;
        let expected = &self.worker.attestation;
        let mismatch = if current.broker_instance() != expected.broker_instance() {
            Some(LaunchEvidenceError::BrokerInstanceMismatch)
        } else if current.launch_nonce() != expected.launch_nonce() {
            Some(LaunchEvidenceError::LaunchNonceMismatch)
        } else if current.executable_hash() != expected.executable_hash() {
            Some(LaunchEvidenceError::ExecutableHashMismatch)
        } else if current.config_hash() != expected.config_hash() {
            Some(LaunchEvidenceError::ConfigHashMismatch)
        } else if current.module_graph_hash() != expected.module_graph_hash() {
            Some(LaunchEvidenceError::ModuleGraphHashMismatch)
        } else if current.os_sandbox_kind() != expected.os_sandbox_kind() {
            Some(LaunchEvidenceError::SandboxKindMismatch)
        } else if current.sandbox_profile_hash() != expected.sandbox_profile_hash() {
            Some(LaunchEvidenceError::SandboxProfileHashMismatch)
        } else if current.manifest_hash() != expected.manifest_hash() {
            Some(LaunchEvidenceError::ManifestHashMismatch)
        } else {
            None
        };
        if let Some(mismatch) = mismatch {
            return Err(mismatch.into());
        }
        Ok(())
    }

    fn recheck_or_teardown(&mut self) -> Result<(), BrokerError> {
        if let Err(error) = self.recheck_evidence() {
            self.worker.child.kill_tree();
            self.worker.child.wait_bounded(Duration::from_secs(5));
            return Err(error);
        }
        Ok(())
    }

    /// Submits one bounded processor step over this session's authenticated worker stream.
    pub fn submit_work(
        &mut self,
        work: TrustedBrokerWork,
        authority: &mut impl DependencyReadAuthority,
        timeout: Duration,
    ) -> Result<TrustedBrokerWorkResult, BrokerError> {
        let result = self.submit_work_inner(work, authority, timeout);
        if matches!(result, Err(BrokerError::WorkerFrameRejected(_))) {
            self.worker.child.kill_tree();
            self.worker.child.wait_bounded(Duration::from_secs(5));
        }
        result
    }

    fn submit_work_inner(
        &mut self,
        work: TrustedBrokerWork,
        authority: &mut impl DependencyReadAuthority,
        timeout: Duration,
    ) -> Result<TrustedBrokerWorkResult, BrokerError> {
        self.recheck_or_teardown()?;
        self.restore_after_dispatch_recheck_for_test();
        let started = Instant::now();
        let scope = WorkScope {
            context: work.resolve_context(),
            work: work.work(),
        };
        self.write_application_frame(&BrokerToWorkerFrame::WorkStart {
            scope,
            total: work.processor_step_descriptor().len() as u64,
        })?;
        if self.take_forced_worker_frame_rejection_for_test() {
            self.channel.write_frame(&mut self._stream, &[3])?;
        } else {
            for (index, chunk) in work
                .processor_step_descriptor()
                .chunks(APPLICATION_CHUNK_BYTES)
                .enumerate()
            {
                self.write_application_frame(&BrokerToWorkerFrame::WorkChunk {
                    scope,
                    offset: (index * APPLICATION_CHUNK_BYTES) as u64,
                    bytes: chunk.to_vec(),
                })?;
            }
            self.write_application_frame(&BrokerToWorkerFrame::WorkEnd { scope })?;
        }

        let mut dependency_reads = 0_usize;
        let mut dependency_bytes = 0_usize;
        loop {
            let frame = self.read_worker_application_frame(started, timeout)?;
            validate_worker_frame_window(WorkerFrameWindow::Submit, &frame)?;
            match frame {
                WorkerToBrokerFrame::DependencyRequest {
                    scope: response_scope,
                    id,
                    kind,
                    descriptor,
                } => {
                    validate_scope(scope, response_scope)?;
                    dependency_reads = dependency_reads.saturating_add(1);
                    let registration = self.correlation.register(id, scope.context, scope.work);
                    if let Err(error) = registration {
                        self.write_application_frame(
                            &BrokerToWorkerFrame::DependencyCorrelationRejected {
                                scope,
                                id,
                                error,
                            },
                        )?;
                        continue;
                    }

                    let decision = if dependency_reads > MAX_DEPENDENCY_READS_PER_WORK {
                        DependencyReadDecision::denied(DependencyReadDenial::BudgetExceeded)
                    } else if !self
                        .launch
                        .manifest
                        .permitted_dependency_kinds()
                        .contains(&kind)
                    {
                        DependencyReadDecision::denied(DependencyReadDenial::KindNotPermitted)
                    } else {
                        authority.read_dependency(&DependencyReadRequest::new(
                            scope.context,
                            scope.work,
                            id,
                            kind,
                            descriptor,
                        ))
                    };
                    self.correlation.consume(
                        id,
                        scope.context,
                        scope.work,
                        self.channel.binding().handshake_transcript_hash(),
                    )?;
                    match decision {
                        DependencyReadDecision::Resolved(bytes) => {
                            let bytes = bytes.into_bytes();
                            dependency_bytes = dependency_bytes.saturating_add(bytes.len());
                            if dependency_bytes > MAX_DEPENDENCY_BYTES_PER_WORK {
                                self.write_application_frame(
                                    &BrokerToWorkerFrame::DependencyDenied {
                                        scope,
                                        id,
                                        denial: DependencyReadDenial::BudgetExceeded,
                                    },
                                )?;
                            } else {
                                self.write_dependency_bytes(scope, id, &bytes)?;
                            }
                        }
                        DependencyReadDecision::Denied(denial) => {
                            self.write_application_frame(&BrokerToWorkerFrame::DependencyDenied {
                                scope,
                                id,
                                denial,
                            })?;
                        }
                    }
                }
                WorkerToBrokerFrame::WorkSuccessStart {
                    scope: response_scope,
                    total,
                } => {
                    validate_scope(scope, response_scope)?;
                    let output = self.read_work_output(scope, total, started, timeout)?;
                    self.mutate_before_admission_for_test(EvidenceMutationPoint::Success);
                    self.recheck_or_teardown()?;
                    return Ok(TrustedBrokerWorkResult::Success(
                        TrustedBrokerWorkOutput::new(output),
                    ));
                }
                WorkerToBrokerFrame::WorkFailure {
                    scope: response_scope,
                    failure,
                } => {
                    validate_scope(scope, response_scope)?;
                    self.mutate_before_admission_for_test(EvidenceMutationPoint::Failure);
                    self.recheck_or_teardown()?;
                    return Ok(TrustedBrokerWorkResult::Failed(failure));
                }
                WorkerToBrokerFrame::FrameRejected(rejection) => {
                    self.mutate_before_admission_for_test(EvidenceMutationPoint::FrameRejected);
                    self.recheck_or_teardown()?;
                    return Ok(TrustedBrokerWorkResult::Failed(
                        TrustedBrokerProcessingFailure::ProtocolRejected(rejection),
                    ));
                }
                WorkerToBrokerFrame::ProbeResult(_)
                | WorkerToBrokerFrame::WorkSuccessChunk { .. }
                | WorkerToBrokerFrame::WorkSuccessEnd { .. } => unreachable!("window validated"),
            }
        }
    }

    fn write_dependency_bytes(
        &mut self,
        scope: WorkScope,
        id: DependencyRequestIdV1,
        bytes: &[u8],
    ) -> Result<(), BrokerError> {
        self.write_application_frame(&BrokerToWorkerFrame::DependencyResolvedStart {
            scope,
            id,
            total: bytes.len() as u64,
        })?;
        for (index, chunk) in bytes.chunks(APPLICATION_CHUNK_BYTES).enumerate() {
            self.write_application_frame(&BrokerToWorkerFrame::DependencyResolvedChunk {
                scope,
                id,
                offset: (index * APPLICATION_CHUNK_BYTES) as u64,
                bytes: chunk.to_vec(),
            })?;
        }
        self.write_application_frame(&BrokerToWorkerFrame::DependencyResolvedEnd { scope, id })
    }

    fn read_work_output(
        &mut self,
        scope: WorkScope,
        total: u64,
        started: Instant,
        timeout: Duration,
    ) -> Result<Vec<u8>, BrokerError> {
        let total = usize::try_from(total)
            .map_err(|_| BrokerError::WorkerFrameRejected(WorkerFrameRejection::PayloadTooLarge))?;
        if total > MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES {
            return Err(WorkerFrameRejection::PayloadTooLarge.into());
        }
        let mut output = Vec::with_capacity(total);
        loop {
            let frame = self.read_worker_application_frame(started, timeout)?;
            validate_worker_frame_window(WorkerFrameWindow::Output, &frame)?;
            match frame {
                WorkerToBrokerFrame::WorkSuccessChunk {
                    scope: response_scope,
                    offset,
                    bytes,
                } => {
                    validate_scope(scope, response_scope)?;
                    if offset != output.len() as u64
                        || output.len().saturating_add(bytes.len()) > total
                    {
                        return Err(WorkerFrameRejection::OutOfWindow.into());
                    }
                    output.extend_from_slice(&bytes);
                }
                WorkerToBrokerFrame::WorkSuccessEnd {
                    scope: response_scope,
                } => {
                    validate_scope(scope, response_scope)?;
                    if output.len() != total {
                        return Err(WorkerFrameRejection::TruncatedPayload.into());
                    }
                    return Ok(output);
                }
                _ => unreachable!("window validated"),
            }
        }
    }

    fn write_application_frame(&mut self, frame: &BrokerToWorkerFrame) -> Result<(), BrokerError> {
        self.channel
            .write_frame(&mut self._stream, &frame.encode())?;
        Ok(())
    }

    fn read_worker_application_frame(
        &mut self,
        started: Instant,
        timeout: Duration,
    ) -> Result<WorkerToBrokerFrame, BrokerError> {
        let remaining = timeout
            .checked_sub(started.elapsed())
            .ok_or(BrokerError::WorkerTimeout)?;
        match platform::wait_readable(&mut self._stream, &mut self.worker.child, remaining) {
            Ok(()) => {}
            Err(BrokerError::WorkerTimeout) => {
                self.worker.child.kill_tree();
                self.worker.child.wait_bounded(Duration::from_secs(5));
                return Err(BrokerError::WorkerTimeout);
            }
            Err(error) => return Err(error),
        }
        let payload = match self.channel.read_frame(&mut self._stream) {
            Ok(payload) => payload,
            Err(ChannelError::Io(_)) => {
                let status = self.worker.child.wait_bounded(Duration::from_secs(5));
                return Err(BrokerError::WorkerCrashed(status));
            }
            Err(error) => return Err(error.into()),
        };
        decode_worker_application_payload(&payload)
    }

    #[cfg(test)]
    pub(crate) fn mutate_evidence_for_test(&mut self, point: EvidenceMutationPoint) {
        match point {
            EvidenceMutationPoint::Dispatch => {
                self.original_config_for_test = Some(self.launch.canonical_config.clone());
                self.launch.canonical_config.push(0xff);
            }
            EvidenceMutationPoint::Success
            | EvidenceMutationPoint::Failure
            | EvidenceMutationPoint::FrameRejected => {
                self.evidence_mutation_point = Some(point);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn force_worker_frame_rejection_for_test(&mut self) {
        self.force_worker_frame_rejection_for_test = true;
    }

    #[cfg(test)]
    fn take_forced_worker_frame_rejection_for_test(&mut self) -> bool {
        std::mem::take(&mut self.force_worker_frame_rejection_for_test)
    }

    #[cfg(not(test))]
    fn take_forced_worker_frame_rejection_for_test(&mut self) -> bool {
        false
    }

    #[cfg(test)]
    pub(crate) fn inject_worker_frame_for_test(
        &mut self,
        payload: Vec<u8>,
    ) -> Result<(), BrokerError> {
        let rejection = decode_worker_application_payload(&payload)
            .and_then(|frame| {
                validate_worker_frame_window(WorkerFrameWindow::Submit, &frame)
                    .map_err(BrokerError::WorkerFrameRejected)
            })
            .err();
        if let Some(error) = rejection {
            self.worker.child.kill_tree();
            self.worker.child.wait_bounded(Duration::from_secs(5));
            return Err(error);
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn replay_worker_dependency_id_for_test(
        &mut self,
        id: DependencyRequestIdV1,
        context: crate::correlation::BlockContentResolveContextTokenV1,
        work: crate::correlation::BlockContentWorkTokenV1,
    ) -> Result<(), CorrelationError> {
        self.correlation.register(id, context, work)
    }

    #[cfg(test)]
    pub(crate) fn raw_application_frame_for_test(
        &mut self,
        payload: Vec<u8>,
        timeout: Duration,
    ) -> Result<WorkerFrameRejection, BrokerError> {
        self.channel.write_frame(&mut self._stream, &payload)?;
        let frame = self.read_worker_application_frame(Instant::now(), timeout)?;
        match frame {
            WorkerToBrokerFrame::FrameRejected(rejection) => Ok(rejection),
            _ => Err(WorkerFrameRejection::OutOfWindow.into()),
        }
    }

    #[cfg(test)]
    fn mutate_before_admission_for_test(&mut self, point: EvidenceMutationPoint) {
        if self.evidence_mutation_point.take() == Some(point) {
            self.launch.canonical_config.push(0xff);
        }
    }

    #[cfg(not(test))]
    fn mutate_before_admission_for_test(&mut self, _point: EvidenceMutationPoint) {}

    #[cfg(test)]
    fn restore_after_dispatch_recheck_for_test(&mut self) {
        if let Some(config) = self.original_config_for_test.take() {
            self.launch.canonical_config = config;
        }
    }

    #[cfg(not(test))]
    fn restore_after_dispatch_recheck_for_test(&mut self) {}

    #[cfg(test)]
    pub(crate) fn probe_for_test(
        &mut self,
        probe: WorkerProbe,
        timeout: Duration,
    ) -> Result<bool, BrokerError> {
        self.recheck_evidence()?;
        self.write_application_frame(&BrokerToWorkerFrame::Probe(probe))?;
        match platform::wait_readable(&mut self._stream, &mut self.worker.child, timeout) {
            Ok(()) => {}
            Err(BrokerError::WorkerTimeout) => {
                self.worker.child.kill_tree();
                self.worker.child.wait_bounded(Duration::from_secs(5));
                return Err(BrokerError::WorkerTimeout);
            }
            Err(error) => return Err(error),
        }
        let response = match self.channel.read_frame(&mut self._stream) {
            Ok(response) => response,
            Err(ChannelError::Io(_)) => {
                let status = self.worker.child.wait_bounded(Duration::from_secs(5));
                return Err(BrokerError::WorkerCrashed(status));
            }
            Err(error) => return Err(error.into()),
        };
        self.recheck_evidence()?;
        match decode_worker_application_payload(&response)? {
            WorkerToBrokerFrame::ProbeResult(result) => Ok(result),
            _ => Err(WorkerFrameRejection::OutOfWindow.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum EvidenceMutationPoint {
    #[cfg(test)]
    Dispatch,
    Success,
    Failure,
    FrameRejected,
}

fn validate_scope(expected: WorkScope, received: WorkScope) -> Result<(), BrokerError> {
    if expected.context != received.context {
        return Err(WorkerFrameRejection::ContextMismatch.into());
    }
    if expected.work != received.work {
        return Err(WorkerFrameRejection::WorkMismatch.into());
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum WorkerFrameWindow {
    Submit,
    Output,
}

fn validate_worker_frame_window(
    window: WorkerFrameWindow,
    frame: &WorkerToBrokerFrame,
) -> Result<(), WorkerFrameRejection> {
    let allowed = match window {
        WorkerFrameWindow::Submit => matches!(
            frame,
            WorkerToBrokerFrame::DependencyRequest { .. }
                | WorkerToBrokerFrame::WorkSuccessStart { .. }
                | WorkerToBrokerFrame::WorkFailure { .. }
                | WorkerToBrokerFrame::FrameRejected(_)
        ),
        WorkerFrameWindow::Output => matches!(
            frame,
            WorkerToBrokerFrame::WorkSuccessChunk { .. }
                | WorkerToBrokerFrame::WorkSuccessEnd { .. }
        ),
    };
    if allowed {
        Ok(())
    } else {
        Err(WorkerFrameRejection::OutOfWindow)
    }
}

fn decode_worker_application_payload(payload: &[u8]) -> Result<WorkerToBrokerFrame, BrokerError> {
    WorkerToBrokerFrame::decode(payload).map_err(BrokerError::WorkerFrameRejected)
}

impl Drop for DeniedWorkerSession {
    fn drop(&mut self) {
        self.worker.child.kill_tree();
        self.worker.child.wait_bounded(Duration::from_secs(5));
    }
}

fn build_attestation(
    broker_instance: ProcessorBrokerInstanceId,
    launch_nonce: [u8; 16],
    executable: &Path,
    launch: &DeniedWorkerLaunch,
) -> Result<TrustedProcessorAttestation, BrokerError> {
    let executable_hash = executable_hash(executable)?;
    if executable_hash != launch.manifest.processor_binary_hash() {
        return Err(LaunchEvidenceError::ExecutableHashMismatch.into());
    }
    Ok(TrustedProcessorAttestation::new(AttestationFields {
        broker_instance,
        launch_nonce,
        executable_hash,
        config_hash: config_hash(&launch.canonical_config),
        module_graph_hash: launch.module_graph.hash(),
        os_sandbox_kind: ProcessorSandboxKindV1::current(),
        sandbox_profile_hash: launch.manifest.sandbox_profile_hash(),
        manifest_hash: manifest_hash(&launch.manifest),
    }))
}

fn binding_admission(
    channel: &ValidatedBrokerChannel,
    attestation: &TrustedProcessorAttestation,
) -> Vec<u8> {
    let mut admission = Vec::with_capacity(64);
    admission.extend_from_slice(&attestation.canonical_hash());
    admission.extend_from_slice(&channel.binding().handshake_transcript_hash());
    admission
}

fn write_bounded(writer: &mut impl Write, bytes: &[u8]) -> Result<(), BrokerError> {
    if bytes.len() > BOOTSTRAP_MAX {
        return Err(BrokerError::Protocol("bootstrap message too large"));
    }
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(bytes)?;
    writer.flush()?;
    Ok(())
}

fn read_bounded(reader: &mut impl Read, maximum: usize) -> Result<Vec<u8>, BrokerError> {
    let mut length = [0_u8; 4];
    reader.read_exact(&mut length)?;
    let length = u32::from_be_bytes(length) as usize;
    if length > maximum {
        return Err(BrokerError::Protocol("bounded message too large"));
    }
    let mut message = vec![0_u8; length];
    reader.read_exact(&mut message)?;
    Ok(message)
}

pub(crate) fn worker_run(
    stream: &mut PlatformStream,
    executable: &Path,
) -> Result<(), BrokerError> {
    if std::env::vars_os().next().is_some() {
        return Err(LaunchEvidenceError::AmbientEnvironmentInherited.into());
    }
    let worker_executable_hash = executable_hash(executable)?;
    platform::apply_worker_sandbox()?;
    let bootstrap = Bootstrap::decode(&read_bounded(stream, BOOTSTRAP_MAX)?)?;
    if bootstrap.sandbox_kind != ProcessorSandboxKindV1::current() {
        return Err(LaunchEvidenceError::SandboxKindMismatch.into());
    }
    if worker_executable_hash != bootstrap.executable_hash
        || bootstrap.manifest.processor_binary_hash() != bootstrap.executable_hash
    {
        return Err(LaunchEvidenceError::ExecutableHashMismatch.into());
    }
    if bootstrap.manifest.sandbox_profile_hash() != bootstrap.sandbox_profile_hash {
        return Err(LaunchEvidenceError::SandboxProfileHashMismatch.into());
    }
    let worker_key = generate_ephemeral_keypair()?;
    write_bounded(stream, &worker_key.public)?;
    let attestation = TrustedProcessorAttestation::new(AttestationFields {
        broker_instance: bootstrap.broker_instance,
        launch_nonce: bootstrap.launch_nonce,
        executable_hash: bootstrap.executable_hash,
        config_hash: config_hash(&bootstrap.canonical_config),
        module_graph_hash: bootstrap.module_graph.hash(),
        os_sandbox_kind: bootstrap.sandbox_kind,
        sandbox_profile_hash: bootstrap.sandbox_profile_hash,
        manifest_hash: manifest_hash(&bootstrap.manifest),
    });
    let worker_public: [u8; 32] = worker_key
        .public
        .as_slice()
        .try_into()
        .map_err(|_| ChannelError::InvalidKey)?;
    let (binding, transcript) =
        TrustedBrokerChannelBindingV1::from_transcript(ChannelBindingInputs {
            broker_instance_token: bootstrap.broker_instance,
            broker_ephemeral_public_key: bootstrap.broker_public_key,
            worker_ephemeral_public_key: worker_public,
            launch_nonce: bootstrap.launch_nonce,
            broker_attestation_hash: attestation.canonical_hash(),
            worker_attestation_hash: attestation.canonical_hash(),
            manifest_hash: attestation.manifest_hash(),
            sandbox_profile_hash: attestation.sandbox_profile_hash(),
        });
    let mut handshake = build_handshake(
        false,
        &worker_key.private,
        &bootstrap.broker_public_key,
        &transcript,
        &bootstrap.launch_secret,
    )?;
    let request = read_handshake_message(&mut handshake, &read_bounded(stream, 65_535)?)?;
    if request != b"broker-attested" {
        return Err(BrokerError::Protocol("broker handshake payload mismatch"));
    }
    write_bounded(
        stream,
        &write_handshake_message(&mut handshake, b"worker-attested")?,
    )?;
    let transport = handshake
        .into_transport_mode()
        .map_err(|_| ChannelError::HandshakeAuthenticationFailed)?;
    let mut channel = ValidatedBrokerChannel::new(binding, transport);
    channel.write_frame(stream, &binding_admission(&channel, &attestation))?;
    loop {
        let payload = match channel.read_frame(stream) {
            Ok(request) => request,
            Err(ChannelError::Io(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        let request = match BrokerToWorkerFrame::decode(&payload) {
            Ok(request) => request,
            Err(rejection) => {
                write_worker_application_frame(
                    &mut channel,
                    stream,
                    &WorkerToBrokerFrame::FrameRejected(rejection),
                )?;
                continue;
            }
        };
        match request {
            BrokerToWorkerFrame::Probe(WorkerProbe::Hang) => loop {
                std::thread::park();
            },
            BrokerToWorkerFrame::Probe(WorkerProbe::Crash) => std::process::abort(),
            BrokerToWorkerFrame::Probe(other) => write_worker_application_frame(
                &mut channel,
                stream,
                &WorkerToBrokerFrame::ProbeResult(run_probe(other)),
            )?,
            BrokerToWorkerFrame::WorkStart { scope, total } => {
                handle_worker_work(&mut channel, stream, scope, total)?;
            }
            _ => write_worker_application_frame(
                &mut channel,
                stream,
                &WorkerToBrokerFrame::FrameRejected(WorkerFrameRejection::OutOfWindow),
            )?,
        }
    }
}

fn handle_worker_work(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    total: u64,
) -> Result<(), BrokerError> {
    let descriptor = match read_worker_work_descriptor(channel, stream, scope, total)? {
        Some(descriptor) => descriptor,
        None => return Ok(()),
    };
    let result = dispatch_worker_descriptor(channel, stream, scope, &descriptor)?;
    match result {
        Ok(output) => write_worker_success(channel, stream, scope, &output),
        Err(failure) => write_worker_application_frame(
            channel,
            stream,
            &WorkerToBrokerFrame::WorkFailure { scope, failure },
        ),
    }
}

fn read_worker_work_descriptor(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    total: u64,
) -> Result<Option<Vec<u8>>, BrokerError> {
    let total = usize::try_from(total)
        .map_err(|_| BrokerError::Protocol("work descriptor length overflow"))?;
    let mut descriptor = Vec::with_capacity(total);
    loop {
        let payload = channel.read_frame(stream)?;
        let frame = match BrokerToWorkerFrame::decode(&payload) {
            Ok(frame) => frame,
            Err(rejection) => {
                write_worker_application_frame(
                    channel,
                    stream,
                    &WorkerToBrokerFrame::FrameRejected(rejection),
                )?;
                return Ok(None);
            }
        };
        match frame {
            BrokerToWorkerFrame::WorkChunk {
                scope: received,
                offset,
                bytes,
            } => {
                if let Some(rejection) = scope_rejection(scope, received) {
                    write_worker_failure(channel, stream, scope, rejection)?;
                    return Ok(None);
                }
                if offset != descriptor.len() as u64
                    || descriptor.len().saturating_add(bytes.len()) > total
                {
                    write_worker_failure(
                        channel,
                        stream,
                        scope,
                        WorkerFrameRejection::OutOfWindow,
                    )?;
                    return Ok(None);
                }
                descriptor.extend_from_slice(&bytes);
            }
            BrokerToWorkerFrame::WorkEnd { scope: received } => {
                if let Some(rejection) = scope_rejection(scope, received) {
                    write_worker_failure(channel, stream, scope, rejection)?;
                    return Ok(None);
                }
                if descriptor.len() != total {
                    write_worker_failure(
                        channel,
                        stream,
                        scope,
                        WorkerFrameRejection::TruncatedPayload,
                    )?;
                    return Ok(None);
                }
                return Ok(Some(descriptor));
            }
            _ => {
                write_worker_failure(channel, stream, scope, WorkerFrameRejection::OutOfWindow)?;
                return Ok(None);
            }
        }
    }
}

fn dispatch_worker_descriptor(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    descriptor: &[u8],
) -> Result<Result<Vec<u8>, TrustedBrokerProcessingFailure>, BrokerError> {
    let envelope = match WorkerExecutionEnvelope::decode(descriptor) {
        Ok(envelope) => envelope,
        Err(failure) => return Ok(Err(failure)),
    };
    let mut execution = WorkerExecutionMachine::new(envelope);
    loop {
        match execution.next_event() {
            WorkerExecutionEvent::Suspend(request) => {
                let id = mint_worker_dependency_id()?;
                write_worker_application_frame(
                    channel,
                    stream,
                    &WorkerToBrokerFrame::DependencyRequest {
                        scope,
                        id,
                        kind: request.kind,
                        descriptor: request.descriptor,
                    },
                )?;
                match read_worker_dependency_response(channel, stream, scope, id)? {
                    Ok(bytes) => execution.resume(bytes),
                    Err(failure) => return Ok(Err(failure)),
                }
            }
            WorkerExecutionEvent::Complete(output) => return Ok(Ok(output)),
        }
    }
}

fn mint_worker_dependency_id() -> Result<DependencyRequestIdV1, BrokerError> {
    const NONZERO_ATTEMPTS: usize = 8;
    for _ in 0..NONZERO_ATTEMPTS {
        let mut bytes = [0_u8; 16];
        platform::random_fill(&mut bytes)?;
        if let Ok(id) = DependencyRequestIdV1::from_bytes(bytes) {
            return Ok(id);
        }
    }
    Err(BrokerError::Io(
        "platform CSPRNG returned an all-zero dependency ID repeatedly".into(),
    ))
}

fn read_worker_dependency_response(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    id: DependencyRequestIdV1,
) -> Result<Result<Vec<u8>, TrustedBrokerProcessingFailure>, BrokerError> {
    let payload = channel.read_frame(stream)?;
    let frame = match BrokerToWorkerFrame::decode(&payload) {
        Ok(frame) => frame,
        Err(rejection) => {
            return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                rejection,
            )));
        }
    };
    match frame {
        BrokerToWorkerFrame::DependencyDenied {
            scope: received,
            id: received_id,
            denial,
        } => {
            if let Some(rejection) = dependency_scope_rejection(scope, received, id, received_id) {
                return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                    rejection,
                )));
            }
            Ok(Err(TrustedBrokerProcessingFailure::DependencyDenied(
                denial,
            )))
        }
        BrokerToWorkerFrame::DependencyCorrelationRejected {
            scope: received,
            id: received_id,
            error,
        } => {
            if let Some(rejection) = dependency_scope_rejection(scope, received, id, received_id) {
                return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                    rejection,
                )));
            }
            Ok(Err(
                TrustedBrokerProcessingFailure::DependencyCorrelationRejected(error),
            ))
        }
        BrokerToWorkerFrame::DependencyResolvedStart {
            scope: received,
            id: received_id,
            total,
        } => {
            if let Some(rejection) = dependency_scope_rejection(scope, received, id, received_id) {
                return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                    rejection,
                )));
            }
            read_worker_dependency_bytes(channel, stream, scope, id, total)
        }
        _ => Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
            WorkerFrameRejection::OutOfWindow,
        ))),
    }
}

fn read_worker_dependency_bytes(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    id: DependencyRequestIdV1,
    total: u64,
) -> Result<Result<Vec<u8>, TrustedBrokerProcessingFailure>, BrokerError> {
    let total = match usize::try_from(total) {
        Ok(total) if total <= MAX_DEPENDENCY_BYTES_PER_WORK => total,
        _ => {
            return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                WorkerFrameRejection::PayloadTooLarge,
            )));
        }
    };
    let mut output = Vec::with_capacity(total);
    loop {
        let payload = channel.read_frame(stream)?;
        let frame = match BrokerToWorkerFrame::decode(&payload) {
            Ok(frame) => frame,
            Err(rejection) => {
                return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                    rejection,
                )));
            }
        };
        match frame {
            BrokerToWorkerFrame::DependencyResolvedChunk {
                scope: received,
                id: received_id,
                offset,
                bytes,
            } => {
                if let Some(rejection) =
                    dependency_scope_rejection(scope, received, id, received_id)
                {
                    return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                        rejection,
                    )));
                }
                if offset != output.len() as u64 || output.len().saturating_add(bytes.len()) > total
                {
                    return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                        WorkerFrameRejection::OutOfWindow,
                    )));
                }
                output.extend_from_slice(&bytes);
            }
            BrokerToWorkerFrame::DependencyResolvedEnd {
                scope: received,
                id: received_id,
            } => {
                if let Some(rejection) =
                    dependency_scope_rejection(scope, received, id, received_id)
                {
                    return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                        rejection,
                    )));
                }
                if output.len() != total {
                    return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                        WorkerFrameRejection::TruncatedPayload,
                    )));
                }
                return Ok(Ok(output));
            }
            _ => {
                return Ok(Err(TrustedBrokerProcessingFailure::ProtocolRejected(
                    WorkerFrameRejection::OutOfWindow,
                )));
            }
        }
    }
}

fn dependency_scope_rejection(
    expected_scope: WorkScope,
    received_scope: WorkScope,
    expected_id: DependencyRequestIdV1,
    received_id: DependencyRequestIdV1,
) -> Option<WorkerFrameRejection> {
    if let Some(rejection) = scope_rejection(expected_scope, received_scope) {
        return Some(rejection);
    }
    if expected_id != received_id {
        return Some(WorkerFrameRejection::DependencyMismatch);
    }
    None
}

fn write_worker_success(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    output: &[u8],
) -> Result<(), BrokerError> {
    if output.len() > MAX_TRUSTED_BROKER_WORK_OUTPUT_BYTES {
        return write_worker_failure(
            channel,
            stream,
            scope,
            WorkerFrameRejection::PayloadTooLarge,
        );
    }
    write_worker_application_frame(
        channel,
        stream,
        &WorkerToBrokerFrame::WorkSuccessStart {
            scope,
            total: output.len() as u64,
        },
    )?;
    for (index, chunk) in output.chunks(APPLICATION_CHUNK_BYTES).enumerate() {
        write_worker_application_frame(
            channel,
            stream,
            &WorkerToBrokerFrame::WorkSuccessChunk {
                scope,
                offset: (index * APPLICATION_CHUNK_BYTES) as u64,
                bytes: chunk.to_vec(),
            },
        )?;
    }
    write_worker_application_frame(
        channel,
        stream,
        &WorkerToBrokerFrame::WorkSuccessEnd { scope },
    )
}

fn write_worker_failure(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    scope: WorkScope,
    rejection: WorkerFrameRejection,
) -> Result<(), BrokerError> {
    write_worker_application_frame(
        channel,
        stream,
        &WorkerToBrokerFrame::WorkFailure {
            scope,
            failure: TrustedBrokerProcessingFailure::ProtocolRejected(rejection),
        },
    )
}

fn write_worker_application_frame(
    channel: &mut ValidatedBrokerChannel,
    stream: &mut PlatformStream,
    frame: &WorkerToBrokerFrame,
) -> Result<(), BrokerError> {
    channel.write_frame(stream, &frame.encode())?;
    Ok(())
}

fn scope_rejection(expected: WorkScope, received: WorkScope) -> Option<WorkerFrameRejection> {
    if expected.context != received.context {
        Some(WorkerFrameRejection::ContextMismatch)
    } else if expected.work != received.work {
        Some(WorkerFrameRejection::WorkMismatch)
    } else {
        None
    }
}

fn run_probe(probe: WorkerProbe) -> bool {
    match probe {
        WorkerProbe::ReadOutsideGrant(path) => std::fs::read(path).is_ok(),
        WorkerProbe::Network => std::net::TcpStream::connect(("127.0.0.1", 9)).is_ok(),
        WorkerProbe::ChildProcess => platform::attempt_child_process(),
        WorkerProbe::Environment => std::env::var_os("PATH").is_some(),
        #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
        WorkerProbe::DirectOpen => platform::attempt_direct_open(),
        #[cfg(target_os = "linux")]
        WorkerProbe::OpenAt2 => platform::attempt_openat2(),
        WorkerProbe::Hang | WorkerProbe::Crash => false,
    }
}

pub(crate) fn worker_entry() -> i32 {
    let (mut stream, executable) = match platform::worker_stream_from_args() {
        Ok(parts) => parts,
        Err(_) => return 70,
    };
    match worker_run(&mut stream, &executable) {
        Ok(()) => 0,
        Err(error) => {
            let message = format!("ERROR:{error:?}");
            let _ = write_bounded(&mut stream, message.as_bytes());
            70
        }
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    pub(crate) use crate::protocol::WorkerProbe;

    use std::path::PathBuf;

    pub(crate) fn worker_executable() -> PathBuf {
        std::env::var_os("NEXTEST_BIN_EXE_verter-processor-worker")
            .or_else(|| std::env::var_os("CARGO_BIN_EXE_verter-processor-worker"))
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let mut path = std::env::current_exe().expect("current test executable");
                path.pop();
                if path.ends_with("deps") {
                    path.pop();
                }
                path.push(if cfg!(windows) {
                    "verter-processor-worker.exe"
                } else {
                    "verter-processor-worker"
                });
                path
            })
    }
}
