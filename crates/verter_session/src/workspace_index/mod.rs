//! Typed, snapshot-qualified candidate storage and bounded enumeration.
//!
//! A posting says where an owning operation may find a fact. It never
//! resolves a symbol, decides a framework relationship, establishes project
//! membership or certifies reachability: those answers stay with the producer
//! named by each row's provenance, and consumers validate candidates through
//! that owner before returning semantic answers or authored edits. The
//! contract is the atomic semantic contributions decision in
//! `docs/arch/kernel/`.
//!
//! The read side and the host entry points are public. The one write path is
//! private to this crate: a producer seals a complete enumeration into a
//! `CompletePartition` (a partial or cancelled walk has no such constructor),
//! composes partitions into a `ContributionDelta`, and `WorkspaceIndex::publish`
//! revalidates the delta's fence and swaps one immutable [`IndexSnapshot`].
//! Readers hold whichever whole snapshot they captured.

mod delta;
mod facts;
mod host_projection;
mod identity;
mod membership;
mod population;
mod query;
mod records;
mod snapshot;

#[cfg(test)]
mod store_tests;

pub use delta::{ObservationOracle, PartialCause};
pub use facts::{
    DemandItem, FactCoverage, FactReceipt, IndexDemand, IndexDemandPartition, IndexSurface,
};
pub use host_projection::{
    registration_domain, registration_domain_lookup, registration_posting_key, CssVarCandidates,
    IndexReconcileWork, PartitionShortfall, ReconcileStatus,
};
pub use identity::{
    CatalogRevisionId, ContributionId, ContributionKey, DemandEpoch, ExtensionId,
    InferredProjectId, ObservationKey, ObservationOwner, ObservationRef, ObservedFact,
    OwnerSnapshotId, PartitionFamily, PartitionId, PmProjectId, ProducerDescriptorId, ProfileClaim,
    ReadSet, ReadWitness, ScopeId, SelectionEpoch, SourceBasis, TableKind, TableSchema,
};
pub use membership::{
    project_units, unit_members, MembershipPage, MembershipStatus, PreferredOwner, ProjectMember,
    ProjectUnitsPage,
};
pub use query::{
    query_candidates, query_units, Candidate, CandidatePage, CoveredScope, DeclaredScope,
    EnumerationStatus, IndexContinuation, IndexQuery, Lookup, QueryBudgets, QueryWork, UnitPage,
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
