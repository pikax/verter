//! Slot, basis and observation identities.
//!
//! Every identity is a tagged, length-delimited canonical encoding. The slot
//! ([`ContributionId`]) names what is being replaced and never carries a
//! revision; the [`SourceBasis`] beside it names which version a value holds,
//! so an edit changes the value without changing the unit's lineage.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, InputBasisId, QueryIdentity, SourceRevision, SourceUnitId,
};
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, ConfiguredProjectId, FrameworkProfileId, ProjectProfileId,
    RegionId,
};

verter_identity::sealed_digest_identity!(
    /// Logical replacement slot of one contribution.
    ContributionId
);
verter_identity::sealed_digest_identity!(
    /// Registered descriptor of the producer that owns a slot.
    ProducerDescriptorId
);
verter_identity::sealed_digest_identity!(
    /// Immutable catalog revision a contribution was produced under.
    CatalogRevisionId
);
verter_identity::sealed_digest_identity!(
    /// Owner snapshot a producer captured its facts from.
    OwnerSnapshotId
);
verter_identity::sealed_digest_identity!(
    /// One declared enumeration population.
    ScopeId
);
verter_identity::sealed_digest_identity!(
    /// Project identity the project model minted for an inferred project.
    /// The index stores it; it never derives one from a path.
    InferredProjectId
);
verter_identity::sealed_digest_identity!(
    /// Attributed identity of an extension producer.
    ExtensionId
);

impl ProducerDescriptorId {
    /// The identity a producer inside this crate registers under.
    pub(crate) fn of_descriptor<T: CanonicalEncode>(descriptor: &T) -> Self {
        Self(Canonical::from_encodable(descriptor))
    }
}

impl CatalogRevisionId {
    /// The revision the catalog owner reports. Nothing here invents one: a
    /// contribution with no catalog owner records that absence in its basis.
    #[cfg_attr(not(test), expect(dead_code, reason = "no catalog owner reports one"))]
    pub(crate) fn from_catalog_owner<T: CanonicalEncode>(revision: &T) -> Self {
        Self(Canonical::from_encodable(revision))
    }
}

impl OwnerSnapshotId {
    /// The snapshot a producer captured from its owner.
    pub(crate) fn of_owner_snapshot<T: CanonicalEncode>(snapshot: &T) -> Self {
        Self(Canonical::from_encodable(snapshot))
    }
}

impl ScopeId {
    /// The identity of one population a producer declares.
    pub(crate) fn of_population<T: CanonicalEncode>(population: &T) -> Self {
        Self(Canonical::from_encodable(population))
    }
}

impl ExtensionId {
    /// The identity of an attributed extension producer.
    #[cfg_attr(not(test), expect(dead_code, reason = "no extension producer"))]
    pub(crate) fn of_extension<T: CanonicalEncode>(extension: &T) -> Self {
        Self(Canonical::from_encodable(extension))
    }
}

/// Selection epoch of the view a contribution was derived from. Only the
/// selection owner mints one; a host with no selection owner records the
/// absence, never a counter from another numbering space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectionEpoch(u64);

impl SelectionEpoch {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "no selection owner reports one")
    )]
    pub(crate) fn from_selection_owner(epoch: u64) -> Self {
        Self(epoch)
    }
}

/// Demand epoch of the view a contribution was derived from. Only a demand
/// plan mints one; a request with no plan records the absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DemandEpoch(u64);

impl DemandEpoch {
    #[cfg_attr(not(test), expect(dead_code, reason = "no demand plan reports one"))]
    pub(crate) fn from_demand_plan(epoch: u64) -> Self {
        Self(epoch)
    }
}

/// The registered tables. Each row variant belongs to exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TableKind {
    Names,
    Components,
    Links,
    Registrations,
    StylesheetSymbols,
    ConfigEnvReferences,
    TestDeclarations,
    ExtensionFacts,
    ProjectMemberships,
}

/// Table kind plus the schema contract its payload was written under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TableSchema {
    pub table: TableKind,
    pub version: u32,
}

/// A declared, bounded fact family. Never a thread, timestamp or execution
/// limit. A family belongs to exactly one table, and a lookup key names its
/// family, so a key can never read a neighbouring family of a shared table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PartitionFamily {
    /// CSS custom property definitions, references, bindings and manipulations.
    CssVariables,
    /// Stylesheet classes and IDs.
    StyleSelectors,
    /// Script declarations and imported local names.
    Bindings,
    /// Component uses and the availability of component views.
    Components,
    /// Authored import relations.
    Imports,
    /// Domain registrations.
    Registrations,
    /// Configuration keys and environment references.
    ConfigEnv,
    /// Test declarations.
    TestDeclarations,
    /// Registered extension facts.
    Extensions,
    /// A unit's project-membership set.
    ProjectMembership,
}

impl PartitionFamily {
    /// The one table this family's rows are published in.
    #[must_use]
    pub fn table(self) -> TableKind {
        match self {
            Self::CssVariables | Self::StyleSelectors => TableKind::StylesheetSymbols,
            Self::Bindings => TableKind::Names,
            Self::Components => TableKind::Components,
            Self::Imports => TableKind::Links,
            Self::Registrations => TableKind::Registrations,
            Self::ConfigEnv => TableKind::ConfigEnvReferences,
            Self::TestDeclarations => TableKind::TestDeclarations,
            Self::Extensions => TableKind::ExtensionFacts,
            Self::ProjectMembership => TableKind::ProjectMemberships,
        }
    }

    /// Stable discriminant in canonical encodings, independent of declaration
    /// order.
    pub(super) fn encoding(self) -> u32 {
        match self {
            Self::CssVariables => 1,
            Self::StyleSelectors => 2,
            Self::Bindings => 3,
            Self::Components => 4,
            Self::Imports => 5,
            Self::Registrations => 6,
            Self::ConfigEnv => 7,
            Self::TestDeclarations => 8,
            Self::Extensions => 9,
            Self::ProjectMembership => 10,
        }
    }
}

/// One producer's fact family. Two producers declaring the same family hold
/// distinct partitions.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartitionId {
    owner: ProducerDescriptorId,
    family: PartitionFamily,
}

impl PartitionId {
    pub(crate) fn new(owner: ProducerDescriptorId, family: PartitionFamily) -> Self {
        Self { owner, family }
    }

    /// The producer that owns this partition.
    #[must_use]
    pub fn owner(&self) -> &ProducerDescriptorId {
        &self.owner
    }

    /// The fact family.
    #[must_use]
    pub fn family(&self) -> PartitionFamily {
        self.family
    }
}

/// The exact profile a contribution claims.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ProfileClaim {
    /// Facts of the carrier alone. Never a fabricated framework.
    Carrier(CarrierProfileId),
    /// Facts of one framework profile, optionally narrowed to where it
    /// attached.
    Framework {
        profile: FrameworkProfileId,
        attachment: Option<AttachmentId>,
        region: Option<RegionId>,
    },
    /// Facts of a meta-framework project overlay.
    Project(ProjectProfileId),
}

/// A project as the project model identifies it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PmProjectId {
    Configured(ConfiguredProjectId),
    Inferred(InferredProjectId),
}

/// The fields a [`ContributionId`] is derived from.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContributionKey {
    pub producer: ProducerDescriptorId,
    pub schema: TableSchema,
    pub unit: SourceUnitId,
    pub claim: ProfileClaim,
    pub project: Option<PmProjectId>,
    pub partition: PartitionFamily,
}

impl CanonicalEncode for ContributionKey {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.contribution_id.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, self.producer.digest().as_bytes());
        encoder.field_enum_discriminant(2, self.schema.table as u32);
        encoder.field_u32(3, self.schema.version);
        encoder.field_bytes(4, self.unit.digest().as_bytes());
        encode_claim(encoder, &self.claim);
        match &self.project {
            None => {
                encoder.field_enum_discriminant(8, 0);
            }
            Some(PmProjectId::Configured(id)) => {
                encoder.field_enum_discriminant(8, 1);
                encoder.field_bytes(9, id.digest().as_bytes());
            }
            Some(PmProjectId::Inferred(id)) => {
                encoder.field_enum_discriminant(8, 2);
                encoder.field_bytes(9, id.digest().as_bytes());
            }
        }
        encoder.field_enum_discriminant(10, self.partition.encoding());
    }
}

fn encode_claim(encoder: &mut CanonicalEncoder, claim: &ProfileClaim) {
    match claim {
        ProfileClaim::Carrier(id) => {
            encoder.field_enum_discriminant(5, 0);
            encoder.field_bytes(6, id.digest().as_bytes());
        }
        ProfileClaim::Framework {
            profile,
            attachment,
            region,
        } => {
            encoder.field_enum_discriminant(5, 1);
            encoder.field_bytes(6, profile.digest().as_bytes());
            let attachment = attachment.as_ref().map(|id| *id.digest().as_bytes());
            encoder.field_option(7, attachment.as_ref().map(<[u8; 32]>::as_slice));
            let region = region.as_ref().map(|id| *id.digest().as_bytes());
            encoder.field_option(11, region.as_ref().map(<[u8; 32]>::as_slice));
        }
        ProfileClaim::Project(id) => {
            encoder.field_enum_discriminant(5, 2);
            encoder.field_bytes(6, id.digest().as_bytes());
        }
    }
}

impl ContributionKey {
    /// The slot this key names.
    #[must_use]
    pub(crate) fn id(&self) -> ContributionId {
        ContributionId(Canonical::from_encodable(self))
    }

    /// The partition this slot belongs to.
    #[must_use]
    pub fn partition_id(&self) -> PartitionId {
        PartitionId::new(self.producer.clone(), self.partition)
    }

    /// The slot with its schema version removed. Two current contributions
    /// sharing a lineage would be parallel layouts of one fact.
    pub(super) fn lineage(&self) -> ContributionKey {
        let mut lineage = self.clone();
        lineage.schema.version = 0;
        lineage
    }
}

/// Which owner a read observation belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObservationOwner {
    /// Source units, revisions and content.
    Source,
    /// Shallow header or declaration slots.
    Shallow,
    /// Projection map revisions.
    Map,
    /// TypeInfo child observations.
    TypeInfo,
    /// Project-model membership, environment, resolution or discovery.
    ProjectModel,
    /// Activation, demand or catalog identities.
    Activation,
    /// A declared population's enumeration witness.
    Population,
    /// Relation endpoints and contributing slots.
    Relation,
}

/// Address of one owner observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObservationKey {
    pub owner: ObservationOwner,
    pub subject: CanonicalDigest,
}

/// One observation a contribution consumed, with the fingerprint it had. An
/// absent input is witnessed by the owner's fingerprint for that absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ReadWitness {
    pub key: ObservationKey,
    pub fingerprint: CanonicalDigest,
}

/// The exact observations used to derive rows and coverage. It is independent
/// of optional traces, and an empty set cannot be published.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadSet(pub BTreeSet<ReadWitness>);

impl ReadSet {
    #[must_use]
    pub fn of(witnesses: impl IntoIterator<Item = ReadWitness>) -> Self {
        Self(witnesses.into_iter().collect())
    }
}

/// Marker for an owner child fact's query identity. The index never mints a
/// second observation family.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObservedFact;

/// An owner's child fact: `(QueryIdentity, InputBasisId)` and the role slot
/// inside it. A composed view keeps the ordered list of these and has no
/// single basis of its own.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ObservationRef {
    pub query: QueryIdentity<ObservedFact>,
    pub basis: InputBasisId,
    pub role_slot: Option<u32>,
}

/// What a contribution's value was derived from. It sits beside the slot,
/// never inside it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceBasis {
    pub unit: SourceUnitId,
    /// Host locator of the unit, for presenting a candidate. Not an identity.
    pub locator: Arc<str>,
    pub revision: SourceRevision,
    pub content: ContentId,
    /// The catalog revision, or `None` when no catalog owner covers this
    /// producer's facts.
    pub catalog: Option<CatalogRevisionId>,
    /// The selection epoch, or `None` when no selection owner applies.
    pub selection_epoch: Option<SelectionEpoch>,
    /// The demand epoch, or `None` for a request with no demand plan.
    pub demand_epoch: Option<DemandEpoch>,
    pub owner_snapshot: OwnerSnapshotId,
}
