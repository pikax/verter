use std::fs::File;
use std::io::{self, Read};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::policy::{ProcessorSandboxKindV1, TrustedProcessorCapabilityManifest};

const HASH_DOMAIN: &[u8] = b"verter.processor-broker.sha256.v1\0";

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct ProcessorBrokerInstanceId([u8; 16]);

impl ProcessorBrokerInstanceId {
    #[must_use]
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleGraphEntry {
    module_id: String,
    content_hash: [u8; 32],
    dependencies: Vec<String>,
}

impl ModuleGraphEntry {
    #[must_use]
    pub fn new(
        module_id: impl Into<String>,
        content_hash: [u8; 32],
        dependencies: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            module_id: module_id.into(),
            content_hash,
            dependencies: dependencies.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CanonicalModuleGraph {
    entries: Vec<ModuleGraphEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LaunchEvidenceError {
    NonCanonicalModuleOrder,
    NonCanonicalDependencyOrder { module_id: String },
    Io(String),
    ExecutableHashMismatch,
    ConfigHashMismatch,
    ModuleGraphHashMismatch,
    SandboxProfileHashMismatch,
    ManifestHashMismatch,
    BrokerInstanceMismatch,
    LaunchNonceMismatch,
    SandboxKindMismatch,
    AmbientEnvironmentInherited,
}

impl From<io::Error> for LaunchEvidenceError {
    fn from(value: io::Error) -> Self {
        Self::Io(value.to_string())
    }
}

impl CanonicalModuleGraph {
    pub fn new(
        entries: impl IntoIterator<Item = ModuleGraphEntry>,
    ) -> Result<Self, LaunchEvidenceError> {
        let entries: Vec<_> = entries.into_iter().collect();
        if !entries
            .windows(2)
            .all(|pair| pair[0].module_id < pair[1].module_id)
        {
            return Err(LaunchEvidenceError::NonCanonicalModuleOrder);
        }
        for entry in &entries {
            if !entry.dependencies.windows(2).all(|pair| pair[0] < pair[1]) {
                return Err(LaunchEvidenceError::NonCanonicalDependencyOrder {
                    module_id: entry.module_id.clone(),
                });
            }
        }
        Ok(Self { entries })
    }

    #[must_use]
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    #[must_use]
    pub fn hash(&self) -> [u8; 32] {
        let mut encoded = Vec::with_capacity(self.entries.len().saturating_mul(96));
        encoded.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for entry in &self.entries {
            encode_bytes(entry.module_id.as_bytes(), &mut encoded);
            encoded.extend_from_slice(&entry.content_hash);
            encoded.extend_from_slice(&(entry.dependencies.len() as u32).to_be_bytes());
            for dependency in &entry.dependencies {
                encode_bytes(dependency.as_bytes(), &mut encoded);
            }
        }
        domain_hash(b"module-graph\0", &[&encoded])
    }

    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&(self.entries.len() as u32).to_be_bytes());
        for entry in &self.entries {
            encode_bytes(entry.module_id.as_bytes(), out);
            out.extend_from_slice(&entry.content_hash);
            out.extend_from_slice(&(entry.dependencies.len() as u32).to_be_bytes());
            for dependency in &entry.dependencies {
                encode_bytes(dependency.as_bytes(), out);
            }
        }
    }

    pub(crate) fn decode(input: &mut &[u8]) -> Result<Self, LaunchEvidenceError> {
        let count = read_u32(input)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            let module_id = String::from_utf8(read_bytes(input)?.to_vec())
                .map_err(|error| LaunchEvidenceError::Io(error.to_string()))?;
            let content_hash = read_array(input)?;
            let dependency_count = read_u32(input)? as usize;
            let mut dependencies = Vec::with_capacity(dependency_count);
            for _ in 0..dependency_count {
                dependencies.push(
                    String::from_utf8(read_bytes(input)?.to_vec())
                        .map_err(|error| LaunchEvidenceError::Io(error.to_string()))?,
                );
            }
            entries.push(ModuleGraphEntry {
                module_id,
                content_hash,
                dependencies,
            });
        }
        Self::new(entries)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedProcessorAttestation {
    broker_instance: ProcessorBrokerInstanceId,
    launch_nonce: [u8; 16],
    executable_hash: [u8; 32],
    config_hash: [u8; 32],
    module_graph_hash: [u8; 32],
    os_sandbox_kind: ProcessorSandboxKindV1,
    sandbox_profile_hash: [u8; 32],
    manifest_hash: [u8; 32],
}

pub(crate) struct AttestationFields {
    pub broker_instance: ProcessorBrokerInstanceId,
    pub launch_nonce: [u8; 16],
    pub executable_hash: [u8; 32],
    pub config_hash: [u8; 32],
    pub module_graph_hash: [u8; 32],
    pub os_sandbox_kind: ProcessorSandboxKindV1,
    pub sandbox_profile_hash: [u8; 32],
    pub manifest_hash: [u8; 32],
}

impl TrustedProcessorAttestation {
    pub(crate) fn new(fields: AttestationFields) -> Self {
        Self {
            broker_instance: fields.broker_instance,
            launch_nonce: fields.launch_nonce,
            executable_hash: fields.executable_hash,
            config_hash: fields.config_hash,
            module_graph_hash: fields.module_graph_hash,
            os_sandbox_kind: fields.os_sandbox_kind,
            sandbox_profile_hash: fields.sandbox_profile_hash,
            manifest_hash: fields.manifest_hash,
        }
    }

    #[must_use]
    pub fn canonical_hash(&self) -> [u8; 32] {
        let mut encoded = Vec::with_capacity(193);
        encoded.extend_from_slice(self.broker_instance.as_bytes());
        encoded.extend_from_slice(&self.launch_nonce);
        encoded.extend_from_slice(&self.executable_hash);
        encoded.extend_from_slice(&self.config_hash);
        encoded.extend_from_slice(&self.module_graph_hash);
        encoded.push(self.os_sandbox_kind as u8);
        encoded.extend_from_slice(&self.sandbox_profile_hash);
        encoded.extend_from_slice(&self.manifest_hash);
        domain_hash(b"attestation\0", &[&encoded])
    }

    #[must_use]
    pub const fn broker_instance(&self) -> ProcessorBrokerInstanceId {
        self.broker_instance
    }

    #[must_use]
    pub const fn launch_nonce(&self) -> [u8; 16] {
        self.launch_nonce
    }

    #[must_use]
    pub const fn executable_hash(&self) -> [u8; 32] {
        self.executable_hash
    }

    #[must_use]
    pub const fn config_hash(&self) -> [u8; 32] {
        self.config_hash
    }

    #[must_use]
    pub const fn module_graph_hash(&self) -> [u8; 32] {
        self.module_graph_hash
    }

    #[must_use]
    pub const fn os_sandbox_kind(&self) -> ProcessorSandboxKindV1 {
        self.os_sandbox_kind
    }

    #[must_use]
    pub const fn sandbox_profile_hash(&self) -> [u8; 32] {
        self.sandbox_profile_hash
    }

    #[must_use]
    pub const fn manifest_hash(&self) -> [u8; 32] {
        self.manifest_hash
    }

    #[cfg(test)]
    pub(crate) fn new_for_test(fields: AttestationFields) -> Self {
        Self::new(fields)
    }

    #[cfg(test)]
    pub(crate) fn single_field_mutations_for_test(&self) -> Vec<Self> {
        let mut mutations = Vec::with_capacity(8);
        macro_rules! mutate {
            ($field:ident, $value:expr) => {{
                let mut changed = self.clone();
                changed.$field = $value;
                mutations.push(changed);
            }};
        }
        mutate!(broker_instance, ProcessorBrokerInstanceId([9; 16]));
        mutate!(launch_nonce, [9; 16]);
        mutate!(executable_hash, [9; 32]);
        mutate!(config_hash, [9; 32]);
        mutate!(module_graph_hash, [9; 32]);
        mutate!(
            os_sandbox_kind,
            match self.os_sandbox_kind {
                ProcessorSandboxKindV1::LinuxNamespaceSeccomp => {
                    ProcessorSandboxKindV1::MacSandbox
                }
                ProcessorSandboxKindV1::MacSandbox => {
                    ProcessorSandboxKindV1::WindowsAppContainer
                }
                ProcessorSandboxKindV1::WindowsAppContainer => {
                    ProcessorSandboxKindV1::LinuxNamespaceSeccomp
                }
            }
        );
        mutate!(sandbox_profile_hash, [9; 32]);
        mutate!(manifest_hash, [9; 32]);
        mutations
    }
}

pub(crate) fn config_hash(config: &[u8]) -> [u8; 32] {
    domain_hash(b"canonical-config\0", &[config])
}

pub(crate) fn manifest_hash(manifest: &TrustedProcessorCapabilityManifest) -> [u8; 32] {
    let mut encoded = Vec::with_capacity(112);
    manifest.encode_canonical(&mut encoded);
    domain_hash(b"capability-manifest\0", &[&encoded])
}

pub(crate) fn executable_hash(path: &Path) -> Result<[u8; 32], LaunchEvidenceError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    hasher.update(b"executable\0");
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher.finalize().into())
}

pub(crate) fn domain_hash(domain: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(HASH_DOMAIN);
    hasher.update((domain.len() as u32).to_be_bytes());
    hasher.update(domain);
    for part in parts {
        hasher.update((part.len() as u64).to_be_bytes());
        hasher.update(part);
    }
    hasher.finalize().into()
}

fn encode_bytes(bytes: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&(bytes.len() as u32).to_be_bytes());
    out.extend_from_slice(bytes);
}

fn read_u32(input: &mut &[u8]) -> Result<u32, LaunchEvidenceError> {
    Ok(u32::from_be_bytes(read_array(input)?))
}

fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], LaunchEvidenceError> {
    if input.len() < N {
        return Err(LaunchEvidenceError::Io(
            "truncated canonical evidence".into(),
        ));
    }
    let (head, tail) = input.split_at(N);
    *input = tail;
    Ok(head.try_into().expect("length checked"))
}

fn read_bytes<'a>(input: &mut &'a [u8]) -> Result<&'a [u8], LaunchEvidenceError> {
    let length = read_u32(input)? as usize;
    if input.len() < length {
        return Err(LaunchEvidenceError::Io(
            "truncated canonical evidence".into(),
        ));
    }
    let (head, tail) = input.split_at(length);
    *input = tail;
    Ok(head)
}
