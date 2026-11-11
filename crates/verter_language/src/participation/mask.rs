//! The immutable [`CapabilityOwnershipMask`] and its admission decision.

use std::collections::BTreeMap;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncoder};
use verter_identity::identity::SourceUnitId;
use verter_identity::successor::RegionId;

use super::capability::{CapabilityId, CapabilityOperation, ProfileRef};

/// The public preset vocabulary: exactly these four. TypeScript provider
/// selection (`auto`, `tsserver`, …) is a separate fact.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ParticipationPreset {
    Auto,
    Disabled,
    Workspace,
    Full,
}

impl ParticipationPreset {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Disabled => "disabled",
            Self::Workspace => "workspace",
            Self::Full => "full",
        }
    }

    /// Decode a preset; an invalid value is a validation error for the
    /// caller, never `auto`.
    pub fn from_key(key: &str) -> Option<Self> {
        match key {
            "auto" => Some(Self::Auto),
            "disabled" => Some(Self::Disabled),
            "workspace" => Some(Self::Workspace),
            "full" => Some(Self::Full),
            _ => None,
        }
    }

    const fn discriminant(self) -> u32 {
        match self {
            Self::Auto => 1,
            Self::Disabled => 2,
            Self::Workspace => 3,
            Self::Full => 4,
        }
    }
}

/// The effective mode of one cell. Modes belong to cells, never to a
/// process-wide boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EffectiveMode {
    Disabled,
    WorkspaceOnly,
    Full,
}

impl EffectiveMode {
    /// Whether this mode admits a demand of `purpose`. `WorkspaceOnly` admits
    /// only bounded workspace semantics (`IndexContribution`) and a
    /// caller-only `BatchCheck`; interactive presentation, diagnostic
    /// publication, compile and explain requests need their own Full cell.
    pub const fn admits(self, purpose: DemandPurpose) -> bool {
        match self {
            Self::Disabled => false,
            Self::WorkspaceOnly => matches!(
                purpose,
                DemandPurpose::IndexContribution | DemandPurpose::BatchCheck
            ),
            Self::Full => true,
        }
    }
}

/// The closed demand purpose (DEM0 `DR17`). It selects admission and budget,
/// never a different semantic answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DemandPurpose {
    Interactive,
    DiagnosticsPublication,
    BatchCheck,
    IndexContribution,
    Compile,
    Explain,
}

/// Who owns a cell's interactive presentation. This selects participation,
/// never a semantic engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InteractiveOwnership {
    Verter,
    /// A competing tool proved overlap; the evidence is on the cell.
    OtherTool,
}

/// Host-normalized, identity-free ownership evidence: the digest of one
/// validated coexistence declaration row. Never an extension id, a display
/// name or a file suffix; it does not enter the mask identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct OwnershipEvidence(pub CanonicalDigest);

/// Whether an admitted producer needs the selected TypeScript authority.
/// The producer declares it; the mask never guesses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderRequirement {
    NotRequired,
    TypeScript,
}

/// Availability of the one selected TypeScript authority, as its Rust owner
/// reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderAvailability {
    Available,
    Unavailable,
}

/// A cell's provider fact for one requirement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ProviderState {
    NotRequired,
    Available,
    ProviderUnavailable,
}

/// Client support for an operation on this transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClientSupport {
    Supported,
    UnsupportedClientFeature,
}

/// The optional-visual-layer fact of a cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OptionalPresentation {
    NotOptional,
    Enabled,
    DisabledOptionalVisualLayer,
}

/// Catalog support for a capability, checked by the catalog owner and passed
/// in at admission; the mask never widens it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CatalogSupport {
    Supported,
    UnsupportedCapability,
}

/// The document selector scope of a declaration. Region selectors keep
/// their typed source/region identity, so a stylesheet-region conflict
/// cannot withdraw template hover. No file extension is a selector.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SelectorScope {
    AllSources,
    /// Every region with this carrier-minted role (`"style"`, `"template"`).
    RegionRole(Box<str>),
    Unit(SourceUnitId),
    /// One addressed region: [`RegionId::from_region_role`] of the pair.
    UnitRegion {
        unit: SourceUnitId,
        role: Box<str>,
    },
}

impl SelectorScope {
    /// Fixed precedence: an addressed region over a unit over a role over
    /// every source. Never arrival or registration order.
    const fn specificity(&self) -> u8 {
        match self {
            Self::AllSources => 0,
            Self::RegionRole(_) => 1,
            Self::Unit(_) => 2,
            Self::UnitRegion { .. } => 3,
        }
    }

    /// The typed region this selector addresses, when it addresses one.
    pub fn region_id(&self) -> Option<RegionId> {
        match self {
            Self::UnitRegion { unit, role } => Some(RegionId::from_region_role(unit, role)),
            _ => None,
        }
    }

    fn matches(&self, site: &RequestSite<'_>) -> bool {
        match self {
            Self::AllSources => true,
            Self::RegionRole(role) => site.region_role == Some(&**role),
            Self::Unit(unit) => site.unit == Some(unit),
            Self::UnitRegion { unit, role } => {
                site.unit == Some(unit) && site.region_role == Some(&**role)
            }
        }
    }

    fn encoded(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("verter.language.participation.selector.v1");
        match self {
            Self::AllSources => {
                encoder.field_enum_discriminant(1, 1);
            }
            Self::RegionRole(role) => {
                encoder.field_enum_discriminant(1, 2).field_str(2, role);
            }
            Self::Unit(unit) => {
                encoder
                    .field_enum_discriminant(1, 3)
                    .field_bytes(3, unit.canonical_bytes());
            }
            Self::UnitRegion { unit, role } => {
                encoder
                    .field_enum_discriminant(1, 4)
                    .field_bytes(3, unit.canonical_bytes())
                    .field_str(2, role);
            }
        }
        encoder.finish()
    }
}

/// Where one demand lands: its unit and, inside it, its region role. A
/// workspace-wide demand has neither and matches only [`SelectorScope::AllSources`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestSite<'a> {
    pub unit: Option<&'a SourceUnitId>,
    pub region_role: Option<&'a str>,
}

/// Which capabilities a preset or presentation declaration covers.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum CapabilityTarget {
    AllCapabilities,
    Profile(ProfileRef),
    Operation(CapabilityOperation),
    Capability(CapabilityId),
}

impl CapabilityTarget {
    /// Fixed precedence: one capability over one facet over one profile over
    /// every capability.
    const fn specificity(&self) -> u8 {
        match self {
            Self::AllCapabilities => 0,
            Self::Profile(_) => 1,
            Self::Operation(_) => 2,
            Self::Capability(_) => 3,
        }
    }

    fn covers(&self, capability: &CapabilityId) -> bool {
        self.covers_parts(capability.profile(), capability.operation())
    }

    fn covers_parts(&self, profile: &ProfileRef, operation: CapabilityOperation) -> bool {
        match self {
            Self::AllCapabilities => true,
            Self::Profile(covered) => covered == profile,
            Self::Operation(covered) => *covered == operation,
            Self::Capability(exact) => exact.profile() == profile && exact.operation() == operation,
        }
    }

    fn encoded(&self) -> Vec<u8> {
        let mut encoder = CanonicalEncoder::new("verter.language.participation.target.v1");
        match self {
            Self::AllCapabilities => {
                encoder.field_enum_discriminant(1, 1);
            }
            Self::Profile(profile) => {
                encoder
                    .field_enum_discriminant(1, 2)
                    .field_bytes(2, &profile.encoded());
            }
            Self::Operation(operation) => {
                encoder
                    .field_enum_discriminant(1, 3)
                    .field_str(3, operation.key());
            }
            Self::Capability(capability) => {
                encoder
                    .field_enum_discriminant(1, 4)
                    .field_bytes(4, capability.digest().as_bytes());
            }
        }
        encoder.finish()
    }
}

/// One resolved preset declaration of the configuration authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetDeclaration {
    pub target: CapabilityTarget,
    pub selector: SelectorScope,
    pub preset: ParticipationPreset,
}

/// One validated competing-tool declaration: it names exactly one
/// capability cell. A broad target is not accepted, so an unknown or
/// malformed observation can never withdraw broadly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipDeclaration {
    pub capability: CapabilityId,
    pub selector: SelectorScope,
    pub evidence: OwnershipEvidence,
}

/// Everything a mask is built from, already validated against the catalog
/// and normalized by the host. Enumeration order never matters.
#[derive(Debug, Clone)]
pub struct ParticipationInputs {
    pub presets: Vec<PresetDeclaration>,
    pub ownership: Vec<OwnershipDeclaration>,
    /// Optional visual layers the user turned off.
    pub disabled_presentations: Vec<CapabilityTarget>,
    /// Operations this client and transport cannot serve.
    pub unsupported_client_operations: Vec<CapabilityOperation>,
    pub typescript_provider: ProviderAvailability,
}

impl Default for ParticipationInputs {
    fn default() -> Self {
        Self {
            presets: Vec::new(),
            ownership: Vec::new(),
            disabled_presentations: Vec::new(),
            unsupported_client_operations: Vec::new(),
            typescript_provider: ProviderAvailability::Available,
        }
    }
}

/// Why a mask was refused before publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MaskConstructionError {
    /// Two declarations for one target and selector resolve to different
    /// presets; arrival order cannot break the tie.
    ContradictoryPreset {
        target: Box<CapabilityTarget>,
        selector: Box<SelectorScope>,
    },
    /// A disabled-presentation target names a facet that is not an optional
    /// visual layer, which would disable semantic facts.
    NotAnOptionalPresentation(Box<CapabilityTarget>),
}

verter_identity::sealed_digest_identity!(
    /// Semantic identity of one mask: its presets, ownership cells and the
    /// three availability dimensions. Ownership evidence, raw editor
    /// identity and observation timestamps never enter it, so equivalent
    /// observations in any order mint the same identity.
    MaskIdentity
);

#[derive(Debug, Clone)]
struct OwnershipCell {
    selector: SelectorScope,
    evidence: Vec<OwnershipEvidence>,
}

/// Immutable captured participation data, indexed by profile-qualified
/// capability and document selector scope.
#[derive(Debug, Clone)]
pub struct CapabilityOwnershipMask {
    /// Highest precedence first, so the first covering match wins.
    presets: Vec<PresetDeclaration>,
    ownership: BTreeMap<CapabilityId, Vec<OwnershipCell>>,
    disabled_presentations: Vec<CapabilityTarget>,
    unsupported_client_operations: Vec<CapabilityOperation>,
    typescript_provider: ProviderAvailability,
    identity: MaskIdentity,
}

/// All facts of one cell at one site, kept independent: a withdrawn
/// capability is not an empty answer, provider absence is not tool
/// ownership, unsupported client support is not disabled configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParticipationCell {
    pub preset: ParticipationPreset,
    pub mode: EffectiveMode,
    pub ownership: InteractiveOwnership,
    /// Sorted, coalesced evidence of every overlapping declaration.
    pub evidence: Vec<OwnershipEvidence>,
    pub client: ClientSupport,
    pub presentation: OptionalPresentation,
}

/// One demand presented for admission.
#[derive(Debug, Clone, Copy)]
pub struct AdmissionRequest<'a> {
    pub capability: &'a CapabilityId,
    pub site: RequestSite<'a>,
    pub purpose: DemandPurpose,
    pub provider: ProviderRequirement,
    pub catalog: CatalogSupport,
}

/// A typed non-execution reason, in primary order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum NonExecutionReason {
    Disabled,
    PurposeForbiddenByWorkspaceOnly,
    UnsupportedCapability,
    UnsupportedClientFeature,
    DisabledOptionalVisualLayer,
    ProviderUnavailable,
}

impl NonExecutionReason {
    /// Every reason in primary order.
    pub const ORDER: [Self; 6] = [
        Self::Disabled,
        Self::PurposeForbiddenByWorkspaceOnly,
        Self::UnsupportedCapability,
        Self::UnsupportedClientFeature,
        Self::DisabledOptionalVisualLayer,
        Self::ProviderUnavailable,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::PurposeForbiddenByWorkspaceOnly => "workspaceOnly",
            Self::UnsupportedCapability => "unsupportedCapability",
            Self::UnsupportedClientFeature => "unsupportedClientFeature",
            Self::DisabledOptionalVisualLayer => "disabledOptionalVisualLayer",
            Self::ProviderUnavailable => "providerUnavailable",
        }
    }

    const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Every restriction that applied, preserved beside the primary reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Restrictions(u8);

impl Restrictions {
    pub fn contains(self, reason: NonExecutionReason) -> bool {
        self.0 & reason.bit() != 0
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The applied reasons in primary order.
    pub fn iter(self) -> impl Iterator<Item = NonExecutionReason> {
        NonExecutionReason::ORDER
            .into_iter()
            .filter(move |reason| self.contains(*reason))
    }

    fn insert(&mut self, reason: NonExecutionReason) {
        self.0 |= reason.bit();
    }
}

/// A refused demand: the primary reason and every restriction that applied.
/// The producer was never started to discover it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NonExecution {
    pub primary: NonExecutionReason,
    pub restrictions: Restrictions,
}

/// An admitted demand's cell facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AdmittedCell {
    pub mode: EffectiveMode,
    pub ownership: InteractiveOwnership,
    pub provider: ProviderState,
}

impl CapabilityOwnershipMask {
    const DOMAIN_TAG: &'static str = "verter.language.participation.mask.v1";

    /// Build the canonical mask: duplicate identical declarations coalesce,
    /// contradictory ones are refused, and the result is independent of the
    /// order the inputs were enumerated in.
    pub fn build(inputs: ParticipationInputs) -> Result<Self, MaskConstructionError> {
        let ParticipationInputs {
            presets,
            ownership,
            disabled_presentations,
            unsupported_client_operations,
            typescript_provider,
        } = inputs;

        let mut by_key: BTreeMap<(Vec<u8>, Vec<u8>), PresetDeclaration> = BTreeMap::new();
        for declaration in presets {
            let key = (declaration.target.encoded(), declaration.selector.encoded());
            match by_key.get(&key) {
                Some(existing) if existing.preset != declaration.preset => {
                    return Err(MaskConstructionError::ContradictoryPreset {
                        target: Box::new(declaration.target),
                        selector: Box::new(declaration.selector),
                    });
                }
                Some(_) => {}
                None => {
                    by_key.insert(key, declaration);
                }
            }
        }
        // The implicit default restated explicitly is the same mask.
        by_key.retain(|_, declaration| {
            !(declaration.preset == ParticipationPreset::Auto
                && declaration.target == CapabilityTarget::AllCapabilities
                && declaration.selector == SelectorScope::AllSources)
        });
        let preset_encodings: Vec<Vec<u8>> = by_key
            .iter()
            .map(|((target, selector), declaration)| {
                let mut row = Vec::with_capacity(target.len() + selector.len() + 20);
                put_framed(&mut row, target);
                put_framed(&mut row, selector);
                row.extend_from_slice(&declaration.preset.discriminant().to_le_bytes());
                row
            })
            .collect();
        let mut presets: Vec<PresetDeclaration> = by_key.into_values().collect();
        // Stable over the canonical key order, so equal-precedence rows keep
        // a deterministic order (they can never both cover one site).
        presets.sort_by_key(|declaration| {
            std::cmp::Reverse((
                declaration.selector.specificity(),
                declaration.target.specificity(),
            ))
        });

        let mut cells: BTreeMap<CapabilityId, BTreeMap<Vec<u8>, OwnershipCell>> = BTreeMap::new();
        for declaration in ownership {
            let cell = cells
                .entry(declaration.capability)
                .or_default()
                .entry(declaration.selector.encoded())
                .or_insert_with(|| OwnershipCell {
                    selector: declaration.selector,
                    evidence: Vec::new(),
                });
            cell.evidence.push(declaration.evidence);
        }
        let mut ownership_encodings = Vec::new();
        let ownership: BTreeMap<CapabilityId, Vec<OwnershipCell>> = cells
            .into_iter()
            .map(|(capability, by_selector)| {
                let cells = by_selector
                    .into_iter()
                    .map(|(selector, mut cell)| {
                        let mut row = Vec::with_capacity(32 + selector.len() + 8);
                        row.extend_from_slice(capability.digest().as_bytes());
                        put_framed(&mut row, &selector);
                        ownership_encodings.push(row);
                        cell.evidence.sort();
                        cell.evidence.dedup();
                        cell
                    })
                    .collect();
                (capability, cells)
            })
            .collect();

        let mut presentations: BTreeMap<Vec<u8>, CapabilityTarget> = BTreeMap::new();
        for target in disabled_presentations {
            let optional = match &target {
                CapabilityTarget::Operation(operation) => operation.is_optional_presentation(),
                CapabilityTarget::Capability(capability) => {
                    capability.operation().is_optional_presentation()
                }
                CapabilityTarget::AllCapabilities | CapabilityTarget::Profile(_) => false,
            };
            if !optional {
                return Err(MaskConstructionError::NotAnOptionalPresentation(Box::new(
                    target,
                )));
            }
            presentations.insert(target.encoded(), target);
        }

        let mut unsupported = unsupported_client_operations;
        unsupported.sort();
        unsupported.dedup();

        let mut encoder = CanonicalEncoder::new(Self::DOMAIN_TAG);
        encoder
            .field_sorted_set(1, &preset_encodings)
            .field_sorted_set(2, &ownership_encodings)
            .field_sorted_set(3, presentations.keys())
            .field_sorted_set(4, unsupported.iter().map(|operation| operation.key()))
            .field_enum_discriminant(
                5,
                match typescript_provider {
                    ProviderAvailability::Available => 1,
                    ProviderAvailability::Unavailable => 2,
                },
            );

        Ok(Self {
            presets,
            ownership,
            disabled_presentations: presentations.into_values().collect(),
            unsupported_client_operations: unsupported,
            typescript_provider,
            identity: MaskIdentity(Canonical::from_encoder(&encoder)),
        })
    }

    /// The mask every capability gets with no declaration: `auto`, no
    /// observed overlap, every client feature supported, provider available.
    pub fn unrestricted() -> Self {
        Self::build(ParticipationInputs::default())
            .expect("empty participation inputs are always valid")
    }

    pub fn identity(&self) -> &MaskIdentity {
        &self.identity
    }

    pub fn typescript_provider(&self) -> ProviderAvailability {
        self.typescript_provider
    }

    /// The effective preset of one capability at one site; omitted
    /// participation is `auto`.
    pub fn preset(&self, capability: &CapabilityId, site: RequestSite<'_>) -> ParticipationPreset {
        self.preset_of(capability.profile(), capability.operation(), site)
    }

    fn preset_of(
        &self,
        profile: &ProfileRef,
        operation: CapabilityOperation,
        site: RequestSite<'_>,
    ) -> ParticipationPreset {
        self.presets
            .iter()
            .find(|declaration| {
                declaration.target.covers_parts(profile, operation)
                    && declaration.selector.matches(&site)
            })
            .map_or(ParticipationPreset::Auto, |declaration| declaration.preset)
    }

    /// Whether no facet of `profile` is `Full` at `site`. Such a scope admits
    /// at most explicitly demanded, bounded workspace semantics, so a host owes
    /// it no eager interactive, provider or publication work.
    pub fn admits_no_full_cell(&self, profile: &ProfileRef, site: RequestSite<'_>) -> bool {
        CapabilityOperation::ALL.iter().all(|&operation| {
            match self.preset_of(profile, operation, site) {
                ParticipationPreset::Disabled | ParticipationPreset::Workspace => true,
                ParticipationPreset::Full => false,
                ParticipationPreset::Auto => self
                    .overlapping(&CapabilityId::new(profile.clone(), operation), site)
                    .next()
                    .is_some(),
            }
        })
    }

    fn overlapping<'m>(
        &'m self,
        capability: &CapabilityId,
        site: RequestSite<'m>,
    ) -> impl Iterator<Item = &'m OwnershipCell> + 'm {
        self.ownership
            .get(capability)
            .into_iter()
            .flatten()
            .filter(move |cell| cell.selector.matches(&site))
    }

    fn mode_and_ownership(
        preset: ParticipationPreset,
        overlap: bool,
    ) -> (EffectiveMode, InteractiveOwnership) {
        match preset {
            ParticipationPreset::Disabled => {
                (EffectiveMode::Disabled, InteractiveOwnership::Verter)
            }
            ParticipationPreset::Workspace => {
                (EffectiveMode::WorkspaceOnly, InteractiveOwnership::Verter)
            }
            // An explicit `full` wins over competing-tool observations.
            ParticipationPreset::Full => (EffectiveMode::Full, InteractiveOwnership::Verter),
            ParticipationPreset::Auto if overlap => (
                EffectiveMode::WorkspaceOnly,
                InteractiveOwnership::OtherTool,
            ),
            ParticipationPreset::Auto => (EffectiveMode::Full, InteractiveOwnership::Verter),
        }
    }

    fn client(&self, operation: CapabilityOperation) -> ClientSupport {
        if self
            .unsupported_client_operations
            .binary_search(&operation)
            .is_ok()
        {
            ClientSupport::UnsupportedClientFeature
        } else {
            ClientSupport::Supported
        }
    }

    fn presentation(&self, capability: &CapabilityId) -> OptionalPresentation {
        if !capability.operation().is_optional_presentation() {
            OptionalPresentation::NotOptional
        } else if self
            .disabled_presentations
            .iter()
            .any(|target| target.covers(capability))
        {
            OptionalPresentation::DisabledOptionalVisualLayer
        } else {
            OptionalPresentation::Enabled
        }
    }

    /// Every independent fact of one cell, for explanation.
    pub fn cell(&self, capability: &CapabilityId, site: RequestSite<'_>) -> ParticipationCell {
        let preset = self.preset(capability, site);
        let mut evidence: Vec<OwnershipEvidence> = self
            .overlapping(capability, site)
            .flat_map(|cell| cell.evidence.iter().copied())
            .collect();
        evidence.sort();
        evidence.dedup();
        let (mode, ownership) = Self::mode_and_ownership(preset, !evidence.is_empty());
        ParticipationCell {
            preset,
            mode,
            ownership,
            evidence,
            client: self.client(capability.operation()),
            presentation: self.presentation(capability),
        }
    }

    /// Admit or refuse one demand without allocating. Every applicable
    /// restriction is recorded; the primary reason follows the fixed order.
    pub fn admit(&self, request: &AdmissionRequest<'_>) -> Result<AdmittedCell, NonExecution> {
        let capability = request.capability;
        let preset = self.preset(capability, request.site);
        let overlap = self.overlapping(capability, request.site).next().is_some();
        let (mode, ownership) = Self::mode_and_ownership(preset, overlap);

        let mut restrictions = Restrictions::default();
        if mode == EffectiveMode::Disabled {
            restrictions.insert(NonExecutionReason::Disabled);
        } else if !mode.admits(request.purpose) {
            restrictions.insert(NonExecutionReason::PurposeForbiddenByWorkspaceOnly);
        }
        if request.catalog == CatalogSupport::UnsupportedCapability {
            restrictions.insert(NonExecutionReason::UnsupportedCapability);
        }
        if self.client(capability.operation()) == ClientSupport::UnsupportedClientFeature {
            restrictions.insert(NonExecutionReason::UnsupportedClientFeature);
        }
        if self.presentation(capability) == OptionalPresentation::DisabledOptionalVisualLayer {
            restrictions.insert(NonExecutionReason::DisabledOptionalVisualLayer);
        }
        let provider = match (request.provider, self.typescript_provider) {
            (ProviderRequirement::NotRequired, _) => ProviderState::NotRequired,
            (ProviderRequirement::TypeScript, ProviderAvailability::Available) => {
                ProviderState::Available
            }
            (ProviderRequirement::TypeScript, ProviderAvailability::Unavailable) => {
                restrictions.insert(NonExecutionReason::ProviderUnavailable);
                ProviderState::ProviderUnavailable
            }
        };

        match restrictions.iter().next() {
            Some(primary) => Err(NonExecution {
                primary,
                restrictions,
            }),
            None => Ok(AdmittedCell {
                mode,
                ownership,
                provider,
            }),
        }
    }
}

fn put_framed(out: &mut Vec<u8>, bytes: &[u8]) {
    out.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
    out.extend_from_slice(bytes);
}
