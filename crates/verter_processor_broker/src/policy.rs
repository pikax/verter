use std::sync::Arc;

use crate::attestation::LaunchEvidenceError;

/// Closed dependency kinds that a denied worker may request from its broker.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
#[repr(u8)]
pub enum DependencyReadKind {
    Source = 1,
    Config = 2,
    Plugin = 3,
    MapSource = 4,
}

impl DependencyReadKind {
    pub(crate) const fn from_wire(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Source),
            2 => Some(Self::Config),
            3 => Some(Self::Plugin),
            4 => Some(Self::MapSource),
            _ => None,
        }
    }
}

/// The platform sandbox named by a trusted-processor attestation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum ProcessorSandboxKindV1 {
    LinuxNamespaceSeccomp = 1,
    MacSandbox = 2,
    WindowsAppContainer = 3,
}

impl ProcessorSandboxKindV1 {
    #[must_use]
    pub const fn current() -> Self {
        #[cfg(target_os = "linux")]
        {
            Self::LinuxNamespaceSeccomp
        }
        #[cfg(target_os = "macos")]
        {
            Self::MacSandbox
        }
        #[cfg(windows)]
        {
            Self::WindowsAppContainer
        }
        #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
        compile_error!("verter_processor_broker supports only Windows, Linux, and macOS");
    }
}

/// The complete denied-by-default capability policy for one processor binary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TrustedProcessorCapabilityManifest {
    schema_version: u32,
    processor_binary_hash: [u8; 32],
    sandbox_profile_hash: [u8; 32],
    permitted_dependency_kinds: Arc<[DependencyReadKind]>,
    denied: DeniedCapabilities,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DeniedCapabilities {
    ambient_filesystem: Denied,
    ambient_network: Denied,
    child_process: Denied,
    native_addon_loading: Denied,
    environment_access: Denied,
    ambient_package_resolution: Denied,
    dynamic_module_loading: DependencyReadOnly,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Denied {
    Denied,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DependencyReadOnly {
    DependencyReadOnly,
}

impl TrustedProcessorCapabilityManifest {
    pub const SCHEMA_VERSION: u32 = 1;

    #[must_use]
    pub fn denied(
        processor_binary_hash: [u8; 32],
        sandbox_profile_hash: [u8; 32],
        permitted_dependency_kinds: impl IntoIterator<Item = DependencyReadKind>,
    ) -> Self {
        let mut permitted_dependency_kinds: Vec<_> =
            permitted_dependency_kinds.into_iter().collect();
        permitted_dependency_kinds.sort_unstable();
        permitted_dependency_kinds.dedup();
        Self {
            schema_version: Self::SCHEMA_VERSION,
            processor_binary_hash,
            sandbox_profile_hash,
            permitted_dependency_kinds: permitted_dependency_kinds.into(),
            denied: DeniedCapabilities {
                ambient_filesystem: Denied::Denied,
                ambient_network: Denied::Denied,
                child_process: Denied::Denied,
                native_addon_loading: Denied::Denied,
                environment_access: Denied::Denied,
                ambient_package_resolution: Denied::Denied,
                dynamic_module_loading: DependencyReadOnly::DependencyReadOnly,
            },
        }
    }

    #[must_use]
    pub const fn schema_version(&self) -> u32 {
        self.schema_version
    }

    #[must_use]
    pub const fn processor_binary_hash(&self) -> [u8; 32] {
        self.processor_binary_hash
    }

    #[must_use]
    pub const fn sandbox_profile_hash(&self) -> [u8; 32] {
        self.sandbox_profile_hash
    }

    #[must_use]
    pub fn permitted_dependency_kinds(&self) -> &[DependencyReadKind] {
        &self.permitted_dependency_kinds
    }

    #[must_use]
    pub const fn ambient_filesystem_denied(&self) -> bool {
        matches!(self.denied.ambient_filesystem, Denied::Denied)
    }

    #[must_use]
    pub const fn ambient_network_denied(&self) -> bool {
        matches!(self.denied.ambient_network, Denied::Denied)
    }

    #[must_use]
    pub const fn child_process_denied(&self) -> bool {
        matches!(self.denied.child_process, Denied::Denied)
    }

    #[must_use]
    pub const fn native_addon_loading_denied(&self) -> bool {
        matches!(self.denied.native_addon_loading, Denied::Denied)
    }

    #[must_use]
    pub const fn environment_access_denied(&self) -> bool {
        matches!(self.denied.environment_access, Denied::Denied)
    }

    #[must_use]
    pub const fn ambient_package_resolution_denied(&self) -> bool {
        matches!(self.denied.ambient_package_resolution, Denied::Denied)
    }

    #[must_use]
    pub const fn dynamic_module_loading_is_dependency_only(&self) -> bool {
        matches!(
            self.denied.dynamic_module_loading,
            DependencyReadOnly::DependencyReadOnly
        )
    }

    pub(crate) fn encode_canonical(&self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.schema_version.to_be_bytes());
        out.extend_from_slice(&self.processor_binary_hash);
        out.extend_from_slice(&self.sandbox_profile_hash);
        out.extend_from_slice(&(self.permitted_dependency_kinds.len() as u32).to_be_bytes());
        out.extend(
            self.permitted_dependency_kinds
                .iter()
                .copied()
                .map(|kind| kind as u8),
        );
        out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 1]);
    }

    pub(crate) fn decode_canonical(input: &mut &[u8]) -> Result<Self, LaunchEvidenceError> {
        let schema_version = read_u32(input)?;
        if schema_version != Self::SCHEMA_VERSION {
            return Err(LaunchEvidenceError::Io(
                "unsupported capability manifest schema".into(),
            ));
        }
        let processor_binary_hash = read_array(input)?;
        let sandbox_profile_hash = read_array(input)?;
        let count = read_u32(input)? as usize;
        if input.len() < count + 7 {
            return Err(LaunchEvidenceError::Io(
                "truncated capability manifest".into(),
            ));
        }
        let mut kinds = Vec::with_capacity(count);
        for encoded in &input[..count] {
            kinds.push(match *encoded {
                1 => DependencyReadKind::Source,
                2 => DependencyReadKind::Config,
                3 => DependencyReadKind::Plugin,
                4 => DependencyReadKind::MapSource,
                _ => {
                    return Err(LaunchEvidenceError::Io(
                        "unknown dependency read kind".into(),
                    ));
                }
            });
        }
        *input = &input[count..];
        if input[..7] != [0, 0, 0, 0, 0, 0, 1] {
            return Err(LaunchEvidenceError::Io(
                "capability manifest is not deny-default".into(),
            ));
        }
        *input = &input[7..];
        let manifest = Self::denied(processor_binary_hash, sandbox_profile_hash, kinds);
        if manifest.permitted_dependency_kinds.len() != count {
            return Err(LaunchEvidenceError::Io(
                "dependency read kinds are not canonical".into(),
            ));
        }
        Ok(manifest)
    }
}

fn read_u32(input: &mut &[u8]) -> Result<u32, LaunchEvidenceError> {
    Ok(u32::from_be_bytes(read_array(input)?))
}

fn read_array<const N: usize>(input: &mut &[u8]) -> Result<[u8; N], LaunchEvidenceError> {
    if input.len() < N {
        return Err(LaunchEvidenceError::Io(
            "truncated capability manifest".into(),
        ));
    }
    let (head, tail) = input.split_at(N);
    *input = tail;
    Ok(head.try_into().expect("length checked"))
}
