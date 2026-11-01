//! Set-valued project membership, stored for the project model.
//!
//! The project model owns membership. This module stores its complete answer
//! for a unit as one contribution and derives only the mechanical forward and
//! reverse postings. It performs no glob, root, import or precedence inference,
//! never picks a sole owner, and a preferred owner never deletes other members.

use std::sync::Arc;

use verter_identity::identity::SourceUnitId;

use super::delta::{
    CompletePartition, IngressRejection, PartialCause, PartitionDraft, SchemaAdmission,
};
use super::identity::{
    ContributionKey, ObservationRef, PmProjectId, ProducerDescriptorId, ProfileClaim, ReadSet,
    ScopeId, SourceBasis, TableKind, TableSchema,
};
use super::query::{
    query_candidates, CoveredScope, EnumerationStatus, IndexContinuation, IndexQuery, Lookup,
    QueryBudgets,
};
use super::records::{
    FactQuality, IndexRecord, IndexRow, MembershipRow, OriginUnavailable, PostingKey, Provenance,
    RecordOrigin, RecordSlot, SlotKind, TypedSubject,
};
use super::snapshot::IndexSnapshot;

/// The project model's answer for one unit on one snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MembershipStatus {
    /// The whole set is proved for the unit and scope.
    Complete,
    /// Known positive members only.
    Partial,
    NeedInputs,
    Unsupported,
    Cancelled,
    Stale,
}

/// The project model's separate preferred-owner decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreferredOwner {
    None,
    Selected(PmProjectId),
    /// Ambiguity between preferred owners. The proved set is unaffected.
    Ambiguous,
}

/// One project a unit belongs to, with its environment reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectMember {
    pub project: PmProjectId,
    pub environment: ObservationRef,
}

/// One membership observation as the project model reports it.
#[derive(Debug, Clone)]
pub struct MembershipObservation {
    pub unit: SourceUnitId,
    pub status: MembershipStatus,
    pub members: Vec<ProjectMember>,
    pub preferred: PreferredOwner,
    pub observation: ObservationRef,
    pub basis: SourceBasis,
    pub read_set: ReadSet,
    pub producer: ProducerDescriptorId,
    pub claim: ProfileClaim,
    pub provenance: Provenance,
}

/// Why an observation was not stored. Its known members may still answer the
/// request that asked, but they prove no excluded project.
#[derive(Debug)]
pub enum MembershipDecline {
    /// Not a complete observation: nothing is published.
    NotComplete {
        status: MembershipStatus,
        positive_members: Vec<ProjectMember>,
    },
    Rejected(IngressRejection),
}

/// The table schema membership contributions are written under.
pub(super) const MEMBERSHIP_SCHEMA: TableSchema = TableSchema {
    table: TableKind::ProjectMemberships,
    version: 1,
};

fn unit_key(unit: &SourceUnitId) -> PostingKey {
    PostingKey::forward(TableKind::ProjectMemberships, &unit.digest().to_hex())
}

fn project_key(project: &PmProjectId) -> PostingKey {
    let digest = match project {
        PmProjectId::Configured(id) => id.digest(),
        PmProjectId::Inferred(id) => id.digest(),
    };
    PostingKey::reverse(TableKind::ProjectMemberships, &digest.to_hex())
}

/// Seal a complete observation as the unit's one membership contribution. A
/// changed set replaces the whole contribution and its inverse postings.
///
/// # Errors
///
/// [`MembershipDecline::NotComplete`] for every status but `Complete`, or the
/// ingress rejection.
pub fn publishable_membership(
    observation: MembershipObservation,
    admission: &SchemaAdmission,
    account: &Arc<verter_session_query::retention::SemanticRetentionAccount>,
) -> Result<CompletePartition, MembershipDecline> {
    if observation.status != MembershipStatus::Complete {
        return Err(MembershipDecline::NotComplete {
            status: observation.status,
            positive_members: observation.members,
        });
    }
    let unit = observation.unit.clone();
    let origin = || {
        vec![RecordOrigin::Unavailable(
            OriginUnavailable::OwnerRecordsNoSpan,
        )]
    };
    let mut members = observation.members;
    members.sort_by(|left, right| left.project.cmp(&right.project));
    members.dedup_by(|left, right| left.project == right.project);

    let mut records = Vec::with_capacity(members.len() + 1);
    records.push(IndexRecord {
        slot: RecordSlot {
            kind: SlotKind::Membership,
            ordinal: 0,
        },
        subject: TypedSubject::Membership {
            unit: unit.clone(),
            member: None,
        },
        keys: vec![unit_key(&unit)],
        row: IndexRow::Membership(MembershipRow::Set {
            status: MembershipStatus::Complete,
            member_count: u32::try_from(members.len()).unwrap_or(u32::MAX),
            preferred: observation.preferred,
            observation: observation.observation,
        }),
        origins: origin(),
        quality: FactQuality::Exact,
        causes: Vec::new(),
    });
    for (index, member) in members.into_iter().enumerate() {
        records.push(IndexRecord {
            slot: RecordSlot {
                kind: SlotKind::Membership,
                ordinal: u32::try_from(index + 1).unwrap_or(u32::MAX),
            },
            subject: TypedSubject::Membership {
                unit: unit.clone(),
                member: Some(member.project.clone()),
            },
            keys: vec![unit_key(&unit), project_key(&member.project)],
            row: IndexRow::Membership(MembershipRow::Member(member)),
            origins: origin(),
            quality: FactQuality::Exact,
            causes: Vec::new(),
        });
    }

    PartitionDraft {
        key: ContributionKey {
            producer: observation.producer,
            schema: MEMBERSHIP_SCHEMA,
            unit,
            claim: observation.claim,
            project: None,
            partition: super::identity::PartitionId(Arc::from("project-membership")),
        },
        basis: observation.basis,
        provenance: observation.provenance,
        read_set: observation.read_set,
        records,
    }
    .seal(admission, account)
    .map_err(MembershipDecline::Rejected)
}

/// A unit's stored membership set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipPage {
    /// Members in canonical project order. All of them when `status` is
    /// `Complete`; a truncated prefix never reports complete.
    pub members: Vec<ProjectMember>,
    pub preferred: PreferredOwner,
    pub status: EnumerationStatus,
    /// Set entries inspected, bounded by the member budget.
    pub entries_inspected: u32,
}

/// Read one unit's set, inspecting at most `max_members` entries.
///
/// A stored empty set is `Complete`: the project model proved no member. An
/// unobserved unit is `Partial`, never empty, and an oversized set reports
/// `Partial` rather than truncating as complete.
#[must_use]
pub fn unit_members(
    snapshot: &IndexSnapshot,
    unit: &SourceUnitId,
    max_members: u32,
) -> MembershipPage {
    let query = IndexQuery {
        lookup: Lookup::Keys(vec![unit_key(unit)]),
        // The set header is one extra entry beyond the members it describes.
        budgets: QueryBudgets {
            postings: max_members.saturating_add(1),
            records: max_members.saturating_add(1),
        },
        scope: None,
        continuation: None,
    };
    let page = query_candidates(snapshot, &query, None, &|| false);

    let mut members = Vec::new();
    let mut declared = None;
    let mut preferred = PreferredOwner::None;
    for candidate in &page.candidates {
        match candidate.record().map(|record| &record.row) {
            Some(IndexRow::Membership(MembershipRow::Set {
                member_count,
                preferred: stored,
                ..
            })) => {
                declared = Some(*member_count);
                preferred = stored.clone();
            }
            Some(IndexRow::Membership(MembershipRow::Member(member))) => {
                members.push(member.clone());
            }
            _ => {}
        }
    }
    let partial = |cause| EnumerationStatus::Partial {
        cause,
        covered: CoveredScope {
            keys_exhausted: 0,
            postings_visited: page.work.posting_visits,
        },
        continuation: None,
    };
    let status = match declared {
        None => partial(PartialCause::UnavailableInput),
        Some(count) if usize::try_from(count).is_ok_and(|count| count == members.len()) => {
            EnumerationStatus::Complete
        }
        Some(_) => partial(PartialCause::BudgetExhausted),
    };
    MembershipPage {
        members,
        preferred,
        status,
        entries_inspected: page.work.posting_visits,
    }
}

/// Units with a stored membership in one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectUnitsPage {
    pub units: Vec<SourceUnitId>,
    pub status: EnumerationStatus,
    pub continuation: Option<IndexContinuation>,
}

/// Enumerate one project's known units, visiting at most `max_postings`.
///
/// Forward-set proof for the units seen does not prove every unit of the
/// project was discovered: without the project model's population `witness`
/// recorded under `scope` the answer stays `Partial`.
#[must_use]
pub fn project_units(
    snapshot: &IndexSnapshot,
    project: &PmProjectId,
    scope: Option<ScopeId>,
    max_postings: u32,
) -> ProjectUnitsPage {
    let query = IndexQuery {
        lookup: Lookup::Keys(vec![project_key(project)]),
        budgets: QueryBudgets {
            postings: max_postings,
            records: max_postings,
        },
        scope,
        continuation: None,
    };
    let page = query_candidates(snapshot, &query, None, &|| false);
    let units = page
        .candidates
        .iter()
        .map(|candidate| candidate.contribution.basis.unit.clone())
        .collect();
    let continuation = match &page.status {
        EnumerationStatus::Partial { continuation, .. } => continuation.clone(),
        _ => None,
    };
    ProjectUnitsPage {
        units,
        status: page.status,
        continuation,
    }
}
