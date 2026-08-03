use std::fmt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::attestation::{
    config_hash, executable_hash, manifest_hash, AttestationFields, CanonicalModuleGraph,
    LaunchEvidenceError, ModuleGraphEntry, ProcessorBrokerInstanceId, TrustedProcessorAttestation,
};
use crate::channel::{
    build_handshake, generate_ephemeral_keypair, read_handshake_message, write_handshake_message,
    ChannelBindingInputs, ChannelError, TrustedBrokerChannelBindingV1, ValidatedBrokerChannel,
};
use crate::platform::{self, PlatformChild, PlatformStream};
use crate::policy::{ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};
use crate::protocol::{Bootstrap, WorkerProbe, BOOTSTRAP_MAX};

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
    WorkerTimeout,
    WorkerCrashed(Option<i32>),
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
        let mut session = DeniedWorkerSession {
            worker,
            channel,
            _stream: spawned.stream,
            launch,
            launched_executable: spawned.executable,
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
        if current != self.worker.attestation {
            return Err(LaunchEvidenceError::ExecutableHashMismatch.into());
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn probe_for_test(
        &mut self,
        probe: WorkerProbe,
        timeout: Duration,
    ) -> Result<bool, BrokerError> {
        self.recheck_evidence()?;
        self.channel
            .write_frame(&mut self._stream, &probe.encode())?;
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
        match response.as_slice() {
            [0] => Ok(false),
            [1] => Ok(true),
            _ => Err(BrokerError::Protocol("invalid probe response")),
        }
    }
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
        let request = match channel.read_frame(stream) {
            Ok(request) => request,
            Err(ChannelError::Io(_)) => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        match WorkerProbe::decode(&request)? {
            WorkerProbe::Hang => loop {
                std::thread::park();
            },
            WorkerProbe::Crash => std::process::abort(),
            other => {
                channel.write_frame(stream, &[u8::from(run_probe(other))])?;
            }
        }
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
