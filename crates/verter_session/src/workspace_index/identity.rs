//! Slot, basis and observation identities.
//!
//! Every identity is a tagged, length-delimited canonical encoding. The slot
//! ([`ContributionId`]) names what is being replaced and never carries a
//! revision; the [`SourceBasis`] beside it names which version a value holds,
//! so an edit changes the value without changing the unit's lineage.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, InputBasisId, QueryIdentity, SourceRevision, SourceUnitId,
};
use verter_identity::successor::{
    AttachmentId, CarrierProfileId, ConfiguredProjectId, FrameworkProfileId, ProjectProfileId,
    RegionId,
};

verter_identity::digest_identity!(
    /// Logical replacement slot of one contribution.
    ContributionId
);
verter_identity::digest_identity!(
    /// Registered descriptor of the producer that owns a slot.
    ProducerDescriptorId
);
verter_identity::digest_identity!(
    /// Immutable catalog revision a contribution was produced under.
    CatalogRevisionId
);
verter_identity::digest_identity!(
    /// Owner snapshot a producer captured its facts from.
    OwnerSnapshotId
);
verter_identity::digest_identity!(
    /// One declared enumeration population.
    ScopeId
);
verter_identity::digest_identity!(
    /// Project identity the project model minted for an inferred project.
    /// The index stores it; it never derives one from a path.
    InferredProjectId
);
verter_identity::digest_identity!(
    /// Attributed identity of an extension producer.
    ExtensionId
);

/// Selection epoch captured with the view a contribution was derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SelectionEpoch(pub u64);

/// Demand epoch captured with the view a contribution was derived from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DemandEpoch(pub u64);

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

/// A declared, bounded fact family inside one source unit. Never a thread,
/// timestamp or execution limit.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PartitionId(pub Arc<str>);

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
    pub partition: PartitionId,
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
        encoder.field_str(10, &self.partition.0);
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
    pub fn id(&self) -> ContributionId {
        ContributionId::from_canonical(self)
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
    pub catalog: CatalogRevisionId,
    pub selection_epoch: SelectionEpoch,
    pub demand_epoch: DemandEpoch,
    pub owner_snapshot: OwnerSnapshotId,
}
