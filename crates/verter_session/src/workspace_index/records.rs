//! Typed rows of the registered tables.
//!
//! A node carries its typed subject and slot; an edge carries typed endpoints,
//! a relation and a role. Spellings are candidate keys only: nothing here can
//! turn a name into a semantic target, and no row can hold a checker handle,
//! a callback or an evaluated component copy.

use std::sync::Arc;

use verter_identity::identity::{
    ContentId, InputBasisId, QueryIdentity, ResultContractId, SourceRevision, SourceUnitId,
};
use verter_identity::mapping::MapRevision;
use verter_identity::successor::ReleaseId;
use verter_span::{EmbeddedByteRange, GeneratedByteRange, SourceByteRange};

use super::identity::{
    ExtensionId, ObservationRef, ObservedFact, PmProjectId, ProducerDescriptorId, TableKind,
};
use super::membership::{MembershipStatus, PreferredOwner, ProjectMember};

/// Version of the lexical search projection stored in posting keys.
pub const LEXICAL_PROJECTION_VERSION: u32 = 1;

/// Which family of candidate key a posting belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum KeyFamily {
    /// Exact authored spelling.
    Exact,
    /// Versioned case-folded search projection.
    Lexical,
    /// Forward adjacency out of a unit.
    Forward,
    /// Reverse adjacency into an endpoint or project.
    Reverse,
}

/// One candidate lookup key. A spelling here finds possible candidates; it is
/// never a target identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PostingKey {
    pub table: TableKind,
    pub family: KeyFamily,
    /// Version of the key's projection; zero for an unprojected key.
    pub version: u32,
    pub text: Arc<str>,
}

impl PostingKey {
    #[must_use]
    pub fn exact(table: TableKind, spelling: &str) -> Self {
        Self {
            table,
            family: KeyFamily::Exact,
            version: 0,
            text: Arc::from(spelling),
        }
    }

    /// The case-folded search key of an authored spelling.
    #[must_use]
    pub fn lexical(table: TableKind, spelling: &str) -> Self {
        Self {
            table,
            family: KeyFamily::Lexical,
            version: LEXICAL_PROJECTION_VERSION,
            text: Arc::from(spelling.to_lowercase()),
        }
    }

    /// Adjacency key out of an endpoint with this canonical text.
    #[must_use]
    pub fn forward(table: TableKind, endpoint: &str) -> Self {
        Self {
            table,
            family: KeyFamily::Forward,
            version: 0,
            text: Arc::from(endpoint),
        }
    }

    /// Adjacency key into an endpoint with this canonical text.
    #[must_use]
    pub fn reverse(table: TableKind, endpoint: &str) -> Self {
        Self {
            table,
            family: KeyFamily::Reverse,
            version: 0,
            text: Arc::from(endpoint),
        }
    }
}

/// What kind of slot a producer assigned to a record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SlotKind {
    Declaration,
    /// Revision-local: meaningful only under its contribution's basis.
    Occurrence,
    Relation,
    Membership,
}

/// Slot of one record inside its contribution. Duplicate names, overloads and
/// multiple origins stay distinct records.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RecordSlot {
    pub kind: SlotKind,
    pub ordinal: u32,
}

/// An authored origin in checked source bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceAnchor {
    pub unit: SourceUnitId,
    pub revision: SourceRevision,
    pub content: ContentId,
    pub range: SourceByteRange,
}

/// A range in the coordinate domain a projection produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectedRange {
    Generated(GeneratedByteRange),
    Embedded(EmbeddedByteRange),
}

/// A projected origin with the map chain that produced it. Without an
/// `authored` origin it cannot become an authored occurrence or edit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedAnchor {
    pub range: ProjectedRange,
    pub maps: Vec<MapRevision>,
    pub authored: Option<SourceAnchor>,
}

/// Why a record has no anchor. There is no zero-range stand-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OriginUnavailable {
    /// The owning fact records no span for this row.
    OwnerRecordsNoSpan,
    /// The owner's span does not fit the checked source domain.
    InvalidOwnerSpan,
    /// A map could not carry the range back to authored source.
    Unmapped,
}

/// One origin of a record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordOrigin {
    Authored(SourceAnchor),
    Projected(ProjectedAnchor),
    Unavailable(OriginUnavailable),
}

/// How far the owner guarantees a fact. Independent of enumeration status: an
/// exact enumeration of unresolved relations does not resolve them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactQuality {
    Exact,
    Approximate,
    Unresolved,
}

/// A machine cause carried with a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FactCause {
    /// The specifier was not resolved by the producing fact.
    SpecifierUnresolved,
    /// The relation depends on a runtime value.
    DynamicValue,
    /// The target owner has not produced the endpoint.
    OwnerUnavailable,
    /// Producer-declared cause.
    Declared(Arc<str>),
}

/// Producer, contract and evidence of one contribution. Display strings and
/// optional traces are never validity evidence, so none appear here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    pub producer: ProducerDescriptorId,
    pub result_contract: ResultContractId,
    pub release: Option<ReleaseId>,
    pub consumed: Vec<ObservationRef>,
}

/// What a posting is about. Part of the canonical posting order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TypedSubject {
    /// A name inside one source unit.
    Name {
        unit: SourceUnitId,
        spelling: Arc<str>,
    },
    /// A demanded view of one component subject.
    Component {
        unit: SourceUnitId,
        export: Arc<str>,
    },
    /// A relation from one unit, ordered by relation then target spelling.
    Link {
        unit: SourceUnitId,
        relation: LinkRelation,
        target: Arc<str>,
    },
    /// An owner-qualified domain registration.
    Registration {
        owner: ProducerDescriptorId,
        key: RegistrationKey,
    },
    /// A stylesheet symbol.
    StyleSymbol {
        kind: StyleSymbolKind,
        name: Arc<str>,
    },
    /// A configuration key or environment reference.
    ConfigEnv { kind: ConfigEnvKind, name: Arc<str> },
    /// A runner-qualified test declaration.
    Test { runner: Arc<str>, id: Arc<str> },
    /// A registered extension fact's subject.
    Extension {
        extension: ExtensionId,
        kind: Arc<str>,
        subject: Arc<str>,
    },
    /// One unit's project-membership set, or one member of it.
    Membership {
        unit: SourceUnitId,
        member: Option<PmProjectId>,
    },
}

/// Lexical role of a name record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NameRole {
    Declaration,
    Occurrence,
    Import,
    Export,
}

/// Declaration and occurrence headers, and import/export postings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameRow {
    pub spelling: Arc<str>,
    pub role: NameRole,
    /// The owner's declaration-kind label, for presentation.
    pub declaration_kind: Option<Arc<str>>,
}

/// View references and usage links, keyed by the owner's query identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComponentRow {
    /// A reference to a demanded view. It holds no facet payload: the
    /// TypeInfo owner answers. A composed view keeps its ordered children
    /// and carries no basis of its own.
    View {
        query: QueryIdentity<ObservedFact>,
        native_basis: Option<InputBasisId>,
        composed_children: Vec<ObservationRef>,
        /// Display-only spelling of the subject.
        display: Arc<str>,
    },
    /// A component use in markup.
    Usage { tag: Arc<str>, target: Endpoint },
}

/// A relation endpoint. External endpoints are owner-qualified references,
/// and unresolved or dynamic endpoints stay explicit with their cause.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Endpoint {
    Unit(SourceUnitId),
    External {
        owner: ProducerDescriptorId,
        observation: ObservationRef,
    },
    Unresolved {
        spelling: Arc<str>,
        cause: FactCause,
    },
    Dynamic {
        cause: FactCause,
    },
}

/// Relation kind of a link edge.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LinkRelation {
    Import,
    Reexport,
    Include,
    ComponentUse,
    Registered(Arc<str>),
}

/// How certain a link is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRole {
    Static,
    Conditional,
    Dynamic,
    Unresolved,
}

/// An authored relation. Package and module resolution proof is a
/// project-model reference, never derived here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkRow {
    pub relation: LinkRelation,
    pub role: LinkRole,
    pub source: Endpoint,
    pub target: Endpoint,
    pub resolution: Option<ObservationRef>,
}

/// A registration key. String and symbol keys are distinguishable.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RegistrationKey {
    String(Arc<str>),
    Symbol(Arc<str>),
}

/// A source-backed domain registration or its use. It names a delivered
/// operation of the producing vertical and never holds a callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegistrationRow {
    pub domain: Arc<str>,
    pub key: RegistrationKey,
    pub is_usage: bool,
    pub operation: Arc<str>,
}

/// Which stylesheet symbol family a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StyleSymbolKind {
    Class,
    Id,
    CustomProperty,
}

/// What the row does to the symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StyleRole {
    Definition,
    Reference,
    Manipulation,
}

/// Where the row was authored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StyleSurface {
    /// One style block, by its position in the owner's analysis.
    Style {
        block: u32,
    },
    Template,
    Script,
}

/// Scope reported by the owner. A fact of the producer, not an index deduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StyleScope {
    Scoped,
    Module,
    Global,
    /// The owning surface has no stylesheet scope.
    NotApplicable,
}

/// A stylesheet symbol definition, reference or manipulation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StylesheetRow {
    pub kind: StyleSymbolKind,
    pub name: Arc<str>,
    pub role: StyleRole,
    pub surface: StyleSurface,
    pub scope: StyleScope,
}

/// Which configuration family a row belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConfigEnvKind {
    ConfigKey,
    EnvironmentReference,
}

/// A reference and its provenance. There is no value field: the index cannot
/// hold a secret or claim a deployment lacks one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigEnvRow {
    pub kind: ConfigEnvKind,
    pub name: Arc<str>,
    pub realm: Option<Arc<str>>,
    pub condition: Option<Arc<str>>,
    pub uncertainty: Option<FactCause>,
}

/// How a test declaration was discovered. It says nothing about results.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestDiscovery {
    Static,
    Dynamic(FactCause),
    Unknown(FactCause),
}

/// A runner-qualified test declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestDeclarationRow {
    pub runner: Arc<str>,
    pub id: Arc<str>,
    pub discovery: TestDiscovery,
}

/// A typed field value of a registered extension schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionFieldValue {
    Text(Arc<str>),
    Integer(i64),
    Flag(bool),
    Endpoint(Endpoint),
}

/// One named field of a registered extension schema.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionField {
    pub name: Arc<str>,
    pub value: ExtensionFieldValue,
}

/// A registered extension fact with validated attribution. Payloads are typed
/// fields under a registered kind and version, never opaque bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtensionRow {
    pub extension: ExtensionId,
    pub extension_version: Arc<str>,
    pub permissions: Vec<Arc<str>>,
    pub kind: Arc<str>,
    pub kind_version: u32,
    pub fields: Vec<ExtensionField>,
}

/// One unit's membership header, or one member of its set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembershipRow {
    /// The project model's complete answer for the unit.
    Set {
        status: MembershipStatus,
        member_count: u32,
        preferred: PreferredOwner,
        observation: ObservationRef,
    },
    Member(ProjectMember),
}

/// The typed payload of one record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexRow {
    Name(NameRow),
    Component(ComponentRow),
    Link(LinkRow),
    Registration(RegistrationRow),
    Stylesheet(StylesheetRow),
    ConfigEnv(ConfigEnvRow),
    Test(TestDeclarationRow),
    Extension(ExtensionRow),
    Membership(MembershipRow),
}

impl IndexRow {
    /// The one table this row may be published in.
    #[must_use]
    pub fn table(&self) -> TableKind {
        match self {
            Self::Name(_) => TableKind::Names,
            Self::Component(_) => TableKind::Components,
            Self::Link(_) => TableKind::Links,
            Self::Registration(_) => TableKind::Registrations,
            Self::Stylesheet(_) => TableKind::StylesheetSymbols,
            Self::ConfigEnv(_) => TableKind::ConfigEnvReferences,
            Self::Test(_) => TableKind::TestDeclarations,
            Self::Extension(_) => TableKind::ExtensionFacts,
            Self::Membership(_) => TableKind::ProjectMemberships,
        }
    }
}

/// One record of a contribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexRecord {
    pub slot: RecordSlot,
    pub subject: TypedSubject,
    pub keys: Vec<PostingKey>,
    pub row: IndexRow,
    /// Every origin, in the producer's order. Never empty.
    pub origins: Vec<RecordOrigin>,
    pub quality: FactQuality,
    pub causes: Vec<FactCause>,
}

impl IndexRecord {
    /// Estimated heap bytes, for retention admission only. Each key also
    /// costs one posting entry in the snapshot.
    pub(super) fn estimated_bytes(&self) -> usize {
        let key_bytes: usize = self
            .keys
            .iter()
            .map(|key| 3 * std::mem::size_of::<PostingKey>() + key.text.len())
            .sum();
        std::mem::size_of::<Self>()
            + key_bytes
            + self.origins.len() * std::mem::size_of::<RecordOrigin>()
            + subject_text_len(&self.subject)
    }
}

fn subject_text_len(subject: &TypedSubject) -> usize {
    match subject {
        TypedSubject::Name { spelling, .. } => spelling.len(),
        TypedSubject::Component { export, .. } => export.len(),
        TypedSubject::Link { target, .. } => target.len(),
        TypedSubject::Registration { key, .. } => match key {
            RegistrationKey::String(text) | RegistrationKey::Symbol(text) => text.len(),
        },
        TypedSubject::StyleSymbol { name, .. } | TypedSubject::ConfigEnv { name, .. } => name.len(),
        TypedSubject::Test { runner, id } => runner.len() + id.len(),
        TypedSubject::Extension { kind, subject, .. } => kind.len() + subject.len(),
        TypedSubject::Membership { .. } => 0,
    }
}
