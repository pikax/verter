//! Typed, snapshot-qualified candidate storage and bounded enumeration.
//!
//! A posting says where an owning operation may find a fact. It never
//! resolves a symbol, decides a framework relationship, establishes project
//! membership or certifies reachability: those answers stay with the producer
//! named by each row's provenance, and consumers validate candidates through
//! that owner before returning semantic answers or authored edits. See
//! `docs/arch/kernel/idx0d-contributions.md` for the contract.
//!
//! The module has one write path. A producer seals a complete enumeration
//! into a [`CompletePartition`] (a partial or cancelled walk has no such
//! constructor), composes partitions into a [`ContributionDelta`], and
//! [`WorkspaceIndex::publish`] revalidates the delta's fence and swaps one
//! immutable [`IndexSnapshot`]. Readers hold whichever whole snapshot they
//! captured.

mod delta;
mod host_projection;
mod identity;
mod membership;
mod query;
mod records;
mod snapshot;

pub use delta::{
    CompletePartition, ContributionDelta, ExpectedPrior, IngressRejection, ObservationOracle,
    PartialCause, PartialEnumeration, PartitionDraft, PublishOutcome, PublishRefusal,
    RetirementEvidence, SchemaAdmission, SlotChange,
};
pub use host_projection::{
    CssVarCandidates, IndexDemand, IndexDemandPartition, IndexReconcileWork, ReconcileStatus,
};
pub use identity::{
    CatalogRevisionId, ContributionId, ContributionKey, DemandEpoch, ExtensionId,
    InferredProjectId, ObservationKey, ObservationOwner, ObservationRef, ObservedFact,
    OwnerSnapshotId, PartitionId, PmProjectId, ProducerDescriptorId, ProfileClaim, ReadSet,
    ReadWitness, ScopeId, SelectionEpoch, SourceBasis, TableKind, TableSchema,
};
pub use membership::{
    project_units, publishable_membership, unit_members, MembershipDecline, MembershipObservation,
    MembershipPage, MembershipStatus, PreferredOwner, ProjectMember, ProjectUnitsPage,
};
pub use query::{
    query_candidates, Candidate, CandidatePage, CoveredScope, EnumerationStatus, IndexContinuation,
    IndexQuery, Lookup, QueryBudgets, QueryWork,
};
pub use records::{
    ComponentRow, ConfigEnvKind, ConfigEnvRow, Endpoint, ExtensionField, ExtensionFieldValue,
    ExtensionRow, FactCause, FactQuality, IndexRecord, IndexRow, KeyFamily, LinkRelation, LinkRole,
    LinkRow, MembershipRow, NameRole, NameRow, OriginUnavailable, PostingKey, ProjectedAnchor,
    ProjectedRange, Provenance, RecordOrigin, RecordSlot, RegistrationKey, RegistrationRow,
    SlotKind, SourceAnchor, StyleRole, StyleScope, StyleSurface, StyleSymbolKind, StylesheetRow,
    TestDeclarationRow, TestDiscovery, TypedSubject, LEXICAL_PROJECTION_VERSION,
};
pub use snapshot::{Contribution, CoverageRecord, IndexSnapshot, InvalidationWork, WorkspaceIndex};
