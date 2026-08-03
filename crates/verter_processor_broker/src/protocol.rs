use std::path::PathBuf;

use crate::attestation::{CanonicalModuleGraph, LaunchEvidenceError, ProcessorBrokerInstanceId};
use crate::policy::{ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};

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

#[derive(Clone, Debug)]
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
    #[cfg(test)]
    pub fn encode(&self) -> Vec<u8> {
        let mut output = Vec::new();
        match self {
            Self::ReadOutsideGrant(path) => {
                output.push(1);
                encode_bytes(path.to_string_lossy().as_bytes(), &mut output);
            }
            Self::Network => output.push(2),
            Self::ChildProcess => output.push(3),
            Self::Environment => output.push(4),
            #[cfg(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))]
            Self::DirectOpen => output.push(7),
            #[cfg(target_os = "linux")]
            Self::OpenAt2 => output.push(8),
            Self::Hang => output.push(5),
            Self::Crash => output.push(6),
        }
        output
    }

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
