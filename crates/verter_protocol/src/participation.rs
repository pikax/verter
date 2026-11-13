//! Participation wire inputs.
//!
//! A host sends its participation scope as normalized data: resolved
//! presets, validated competing-tool ownership cells, the optional visual
//! layers its user turned off, and the operations its client cannot serve.
//! Editor extension ids, extension-name lists and file-extension ownership
//! rules stay in the host: a profile crosses as its kind and identity
//! digest, a competing tool only as an opaque evidence digest. Rust decodes
//! the input into [`ParticipationInputs`] and computes every effective mode.

use serde::{Deserialize, Serialize};
use verter_identity::encoding::CanonicalDigest;
use verter_identity::identity::SourceUnitId;
use verter_language::participation::{
    CapabilityId, CapabilityOperation, CapabilityTarget, OwnershipDeclaration, OwnershipEvidence,
    ParticipationInputs, ParticipationPreset, PresetDeclaration, ProfileRef, SelectorScope,
};

/// The only input schema version this decoder accepts.
pub const PARTICIPATION_INPUT_VERSION: u32 = 1;

/// One complete participation input of a host scope. Every update replaces
/// the previous input whole.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ParticipationInput {
    pub version: u32,
    #[serde(default)]
    pub presets: Vec<PresetInput>,
    #[serde(default)]
    pub ownership: Vec<OwnershipInput>,
    #[serde(default)]
    pub disabled_presentations: Vec<TargetInput>,
    #[serde(default)]
    pub unsupported_client_operations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PresetInput {
    pub target: TargetInput,
    pub selector: SelectorInput,
    /// `auto`, `disabled`, `workspace` or `full`.
    pub preset: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OwnershipInput {
    pub profile: ProfileInput,
    pub operation: String,
    pub selector: SelectorInput,
    /// Lower-hex digest (64 characters) of the validated declaration row.
    pub evidence: String,
}

/// A catalog profile by kind and identity digest (lower hex).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileInput {
    pub kind: ProfileKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ProfileKind {
    Carrier,
    Framework,
    Project,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum TargetInput {
    /// A struct variant, so an unknown field is refused rather than ignored.
    All {},
    Profile {
        profile: ProfileInput,
    },
    Operation {
        operation: String,
    },
    Capability {
        profile: ProfileInput,
        operation: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum SelectorInput {
    All {},
    RegionRole {
        role: String,
    },
    /// One document, by the URI the host addresses it with.
    Document {
        uri: String,
    },
    DocumentRegion {
        uri: String,
        role: String,
    },
}

/// What the decoder needs from the host's own authorities: its catalog
/// profiles and its source-unit identities. Neither is guessed.
pub trait ParticipationResolver {
    fn profile(&self, kind: ProfileKind, id: &str) -> Option<ProfileRef>;
    fn unit(&self, uri: &str) -> Option<SourceUnitId>;
}

/// A refused input: nothing of it is published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParticipationInputError {
    UnsupportedVersion(u32),
    InvalidPreset { index: usize, value: String },
    UnknownProfile { index: usize },
    UnknownOperation { index: usize, operation: String },
    UnknownDocument { index: usize },
}

/// An ownership or client-support entry that established nothing. It is
/// reported as invalid evidence and never widens into a broad withdrawal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidEvidence {
    Ownership { index: usize, cause: &'static str },
    UnknownClientOperation(String),
}

/// A decoded input: the validated mask inputs and the evidence that was set
/// aside.
#[derive(Debug, Clone)]
pub struct DecodedParticipation {
    pub inputs: ParticipationInputs,
    pub invalid: Vec<InvalidEvidence>,
}

impl ParticipationInput {
    /// Decode against the host's authorities. Presets are configuration:
    /// any invalid one refuses the whole input. Ownership and client-support
    /// entries are observations: an invalid one is reported and dropped.
    pub fn decode(
        &self,
        resolver: &dyn ParticipationResolver,
    ) -> Result<DecodedParticipation, ParticipationInputError> {
        if self.version != PARTICIPATION_INPUT_VERSION {
            return Err(ParticipationInputError::UnsupportedVersion(self.version));
        }
        let mut invalid = Vec::new();

        let mut presets = Vec::with_capacity(self.presets.len());
        for (index, preset) in self.presets.iter().enumerate() {
            let value = ParticipationPreset::from_key(&preset.preset).ok_or_else(|| {
                ParticipationInputError::InvalidPreset {
                    index,
                    value: preset.preset.clone(),
                }
            })?;
            presets.push(PresetDeclaration {
                target: decode_target(&preset.target, resolver, index)?,
                selector: decode_selector(&preset.selector, resolver)
                    .ok_or(ParticipationInputError::UnknownDocument { index })?,
                preset: value,
            });
        }

        let mut ownership = Vec::with_capacity(self.ownership.len());
        for (index, cell) in self.ownership.iter().enumerate() {
            let rejected = |cause| InvalidEvidence::Ownership { index, cause };
            let Some(profile) = resolver.profile(cell.profile.kind, &cell.profile.id) else {
                invalid.push(rejected("unknown profile"));
                continue;
            };
            let Some(operation) = CapabilityOperation::from_key(&cell.operation) else {
                invalid.push(rejected("unknown operation"));
                continue;
            };
            let Some(selector) = decode_selector(&cell.selector, resolver) else {
                invalid.push(rejected("unknown document"));
                continue;
            };
            if !is_digest_hex(&cell.evidence) {
                invalid.push(rejected("malformed evidence"));
                continue;
            }
            ownership.push(OwnershipDeclaration {
                capability: CapabilityId::new(profile, operation),
                selector,
                evidence: OwnershipEvidence(CanonicalDigest::of_bytes(cell.evidence.as_bytes())),
            });
        }

        let mut disabled_presentations = Vec::with_capacity(self.disabled_presentations.len());
        for (index, target) in self.disabled_presentations.iter().enumerate() {
            disabled_presentations.push(decode_target(target, resolver, index)?);
        }

        let mut unsupported_client_operations = Vec::new();
        for key in &self.unsupported_client_operations {
            match CapabilityOperation::from_key(key) {
                Some(operation) => unsupported_client_operations.push(operation),
                None => invalid.push(InvalidEvidence::UnknownClientOperation(key.clone())),
            }
        }

        Ok(DecodedParticipation {
            inputs: ParticipationInputs {
                presets,
                ownership,
                disabled_presentations,
                unsupported_client_operations,
                ..ParticipationInputs::default()
            },
            invalid,
        })
    }
}

fn decode_operation(
    operation: &str,
    index: usize,
) -> Result<CapabilityOperation, ParticipationInputError> {
    CapabilityOperation::from_key(operation).ok_or_else(|| {
        ParticipationInputError::UnknownOperation {
            index,
            operation: operation.to_owned(),
        }
    })
}

fn decode_profile(
    profile: &ProfileInput,
    resolver: &dyn ParticipationResolver,
    index: usize,
) -> Result<ProfileRef, ParticipationInputError> {
    resolver
        .profile(profile.kind, &profile.id)
        .ok_or(ParticipationInputError::UnknownProfile { index })
}

fn decode_target(
    target: &TargetInput,
    resolver: &dyn ParticipationResolver,
    index: usize,
) -> Result<CapabilityTarget, ParticipationInputError> {
    Ok(match target {
        TargetInput::All {} => CapabilityTarget::AllCapabilities,
        TargetInput::Profile { profile } => {
            CapabilityTarget::Profile(decode_profile(profile, resolver, index)?)
        }
        TargetInput::Operation { operation } => {
            CapabilityTarget::Operation(decode_operation(operation, index)?)
        }
        TargetInput::Capability { profile, operation } => {
            CapabilityTarget::Capability(CapabilityId::new(
                decode_profile(profile, resolver, index)?,
                decode_operation(operation, index)?,
            ))
        }
    })
}

fn decode_selector(
    selector: &SelectorInput,
    resolver: &dyn ParticipationResolver,
) -> Option<SelectorScope> {
    Some(match selector {
        SelectorInput::All {} => SelectorScope::AllSources,
        SelectorInput::RegionRole { role } => SelectorScope::RegionRole(role.as_str().into()),
        SelectorInput::Document { uri } => SelectorScope::Unit(resolver.unit(uri)?),
        SelectorInput::DocumentRegion { uri, role } => SelectorScope::UnitRegion {
            unit: resolver.unit(uri)?,
            role: role.as_str().into(),
        },
    })
}

fn is_digest_hex(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
