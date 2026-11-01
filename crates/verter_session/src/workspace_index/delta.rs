//! Sealing complete enumerations and describing one atomic publication.
//!
//! [`CompletePartition`] is the only value a [`ContributionDelta`] can publish
//! and [`PartitionDraft::seal`] is the only way to mint one, so a partial or
//! cancelled walk has no route into persistent postings: its positive rows
//! stay in [`PartialEnumeration`], which can only answer the request that
//! produced it.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_identity::encoding::CanonicalDigest;
use verter_session_query::retention::{RetentionCharge, SemanticRetentionAccount};

use super::identity::{
    ContributionId, ContributionKey, DemandEpoch, ExtensionId, ObservationKey, ReadSet,
    ReadWitness, SelectionEpoch, SourceBasis, TableSchema,
};
use super::records::{ExtensionRow, FactQuality, IndexRecord, IndexRow, Provenance, RecordOrigin};
use super::snapshot::{Contribution, CoverageRecord};

/// The immutable catalog rows ingress validates against: registered table
/// schemas, extension kinds and delivered operations. The index reads this
/// value and never adds to it.
#[derive(Debug, Clone, Default)]
pub struct SchemaAdmission {
    pub table_schemas: BTreeSet<TableSchema>,
    /// Registered `(kind, version)` pairs of extension facts.
    pub extension_kinds: BTreeSet<(Arc<str>, u32)>,
    /// Attributed extensions with their declared permissions.
    pub extensions: BTreeSet<ExtensionId>,
    /// Operations the producing verticals actually delivered.
    pub delivered_operations: BTreeSet<Arc<str>>,
}

/// Why ingress refused a draft. Each is a rejection or an explicit unsupported
/// answer, never a complete-empty decode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IngressRejection {
    /// The table schema is not registered: unsupported, not empty.
    UnsupportedSchema(TableSchema),
    /// A row's variant does not belong to the slot's table.
    RowOutsideTable { slot_ordinal: u32 },
    /// The extension fact's kind or version is not registered.
    UnsupportedExtensionKind { kind: Arc<str>, version: u32 },
    /// The extension is not attributed to a registered producer.
    UnattributedExtension,
    /// A registration names an operation its producer did not deliver.
    UndeliveredOperation(Arc<str>),
    /// The read set has no evidence for the contribution's own source unit.
    MissingReadEvidence,
    /// A record has no origin, not even a typed unavailable one.
    MissingOrigin { slot_ordinal: u32 },
    /// A source anchor names a different revision or content than the basis.
    AnchorOutsideBasis { slot_ordinal: u32 },
    /// Two records claimed one slot.
    DuplicateRecordSlot { slot_ordinal: u32 },
    /// An exact fact carried no authored or projected evidence of its target.
    UnresolvedMarkedExact { slot_ordinal: u32 },
}

/// A producer's rows for one slot, before validation.
#[derive(Debug, Clone)]
pub struct PartitionDraft {
    pub key: ContributionKey,
    pub basis: SourceBasis,
    pub provenance: Provenance,
    pub read_set: ReadSet,
    pub records: Vec<IndexRecord>,
}

impl PartitionDraft {
    /// Validate a complete enumeration and seal it for publication.
    ///
    /// # Errors
    ///
    /// The first [`IngressRejection`], leaving nothing charged.
    pub fn seal(
        self,
        admission: &SchemaAdmission,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Result<CompletePartition, IngressRejection> {
        self.validate(admission)?;
        let bytes = self.estimated_bytes();
        Ok(CompletePartition {
            draft: self,
            charge: account.reserve_in_flight(bytes),
        })
    }

    fn validate(&self, admission: &SchemaAdmission) -> Result<(), IngressRejection> {
        let schema = self.key.schema;
        if !admission.table_schemas.contains(&schema) {
            return Err(IngressRejection::UnsupportedSchema(schema));
        }
        if !self.read_set.0.iter().any(|witness| {
            witness.key.subject == self.basis.unit.digest()
                && witness.key.owner == super::identity::ObservationOwner::Source
        }) {
            return Err(IngressRejection::MissingReadEvidence);
        }
        let mut slots = BTreeSet::new();
        for record in &self.records {
            let ordinal = record.slot.ordinal;
            if !slots.insert(record.slot) {
                return Err(IngressRejection::DuplicateRecordSlot {
                    slot_ordinal: ordinal,
                });
            }
            if record.row.table() != schema.table {
                return Err(IngressRejection::RowOutsideTable {
                    slot_ordinal: ordinal,
                });
            }
            if record.origins.is_empty() {
                return Err(IngressRejection::MissingOrigin {
                    slot_ordinal: ordinal,
                });
            }
            if !record.origins.iter().all(|origin| self.anchored(origin)) {
                return Err(IngressRejection::AnchorOutsideBasis {
                    slot_ordinal: ordinal,
                });
            }
            if record.quality == FactQuality::Exact && record_is_unresolved(&record.row) {
                return Err(IngressRejection::UnresolvedMarkedExact {
                    slot_ordinal: ordinal,
                });
            }
            match &record.row {
                IndexRow::Extension(row) => validate_extension(row, admission)?,
                IndexRow::Registration(row)
                    if !admission.delivered_operations.contains(&row.operation) =>
                {
                    return Err(IngressRejection::UndeliveredOperation(Arc::clone(
                        &row.operation,
                    )));
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// An authored anchor in the contribution's own unit must be this basis's
    /// revision and content; an imported origin names its own unit.
    fn anchored(&self, origin: &RecordOrigin) -> bool {
        let anchor = match origin {
            RecordOrigin::Authored(anchor) => Some(anchor),
            RecordOrigin::Projected(projected) => projected.authored.as_ref(),
            RecordOrigin::Unavailable(_) => None,
        };
        anchor.is_none_or(|anchor| {
            anchor.unit != self.basis.unit
                || (anchor.revision == self.basis.revision && anchor.content == self.basis.content)
        })
    }

    fn estimated_bytes(&self) -> usize {
        std::mem::size_of::<Contribution>()
            + self.basis.locator.len()
            + self.read_set.0.len() * 2 * std::mem::size_of::<ReadWitness>()
            + self
                .records
                .iter()
                .map(IndexRecord::estimated_bytes)
                .sum::<usize>()
    }
}

fn record_is_unresolved(row: &IndexRow) -> bool {
    matches!(
        row,
        IndexRow::Link(link) if matches!(
            link.target,
            super::records::Endpoint::Unresolved { .. } | super::records::Endpoint::Dynamic { .. }
        )
    )
}

fn validate_extension(
    row: &ExtensionRow,
    admission: &SchemaAdmission,
) -> Result<(), IngressRejection> {
    if !admission.extensions.contains(&row.extension) {
        return Err(IngressRejection::UnattributedExtension);
    }
    if !admission
        .extension_kinds
        .contains(&(Arc::clone(&row.kind), row.kind_version))
    {
        return Err(IngressRejection::UnsupportedExtensionKind {
            kind: Arc::clone(&row.kind),
            version: row.kind_version,
        });
    }
    Ok(())
}

/// A validated, complete enumeration of one slot. Its bytes are charged as
/// in-flight work until publication adopts them.
#[derive(Debug)]
pub struct CompletePartition {
    pub(super) draft: PartitionDraft,
    pub(super) charge: RetentionCharge,
}

impl CompletePartition {
    #[must_use]
    pub fn id(&self) -> ContributionId {
        self.draft.key.id()
    }
}

/// Why an enumeration or query did not cover its whole declared scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PartialCause {
    /// A posting, record, adjacency or member budget ran out first.
    BudgetExhausted,
    /// An input the owner needs is not available.
    UnavailableInput,
    /// The producer for a requested partition is not available.
    UnavailableProducer,
    /// No complete population has been proved for this scope.
    OpenDiscovery,
    /// The scope includes coverage the producers do not support.
    UnsupportedCoverage,
    /// A newer basis was observed while answering.
    Stale,
    /// A complete value was refused retention, so it was not cached.
    RetentionPressure,
}

/// Positive rows from an incomplete walk. They answer this request only: no
/// conversion to a [`CompletePartition`] exists.
#[derive(Debug)]
pub struct PartialEnumeration {
    pub draft: PartitionDraft,
    pub cause: PartialCause,
    pub(super) charge: RetentionCharge,
}

impl PartialEnumeration {
    /// Hold a partial walk's rows for its own request.
    #[must_use]
    pub fn new(
        draft: PartitionDraft,
        cause: PartialCause,
        account: &Arc<SemanticRetentionAccount>,
    ) -> Self {
        let charge = account.reserve_in_flight(draft.estimated_bytes());
        Self {
            draft,
            cause,
            charge,
        }
    }
}

/// What a writer believes a slot holds. The publication fence compares it, so
/// completion order cannot decide a winner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpectedPrior {
    Absent,
    Basis(Box<SourceBasis>),
}

/// An authoritative deletion or retirement observation. A retirement cannot be
/// expressed without one, so an incomplete scan cannot tombstone unseen rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetirementEvidence {
    pub witness: ReadWitness,
}

/// One slot change.
#[derive(Debug)]
pub enum SlotChange {
    Upsert {
        partition: CompletePartition,
        expected: ExpectedPrior,
    },
    Retire {
        id: ContributionId,
        expected: Box<SourceBasis>,
        evidence: RetirementEvidence,
    },
}

/// A finite transaction over contributions and all their postings, published
/// whole or not at all.
#[derive(Debug)]
pub struct ContributionDelta {
    pub changes: Vec<SlotChange>,
    pub coverage: Vec<CoverageRecord>,
    pub selection_epoch: SelectionEpoch,
    pub demand_epoch: DemandEpoch,
}

/// The owners' current answers, consulted at the publication fence. The index
/// reads witnesses through this and never probes the filesystem, runs config
/// or resolves a module itself.
pub trait ObservationOracle {
    /// The current fingerprint of an observation, or `None` when the owner
    /// cannot answer. `None` never validates a witness.
    fn current(&self, key: &ObservationKey) -> Option<CanonicalDigest>;

    /// The selection and demand epochs a publication must still match.
    fn current_epochs(&self) -> (SelectionEpoch, DemandEpoch);
}

/// Why a delta committed nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PublishRefusal {
    /// A slot no longer holds what the writer expected: replan on a fresh
    /// view.
    Conflict(ContributionId),
    /// Another value already occupies this exact slot and basis.
    ReplayMismatch(ContributionId),
    /// A consumed observation or epoch changed since capture.
    Stale,
    /// Old and new schema layouts of one slot would both be current.
    ParallelSchemaLayout(ContributionId),
    /// Coverage names a partition this snapshot would not contain.
    CoverageWithoutPartition(ContributionId),
    /// Publication was cancelled before the swap.
    Cancelled,
    /// The account refused long-lived retention; the value stays uncached.
    RetentionPressure,
}

/// The result of one publication attempt.
#[derive(Debug)]
pub enum PublishOutcome {
    Published {
        generation: u64,
    },
    /// Every change was already current with identical values.
    AlreadyCurrent {
        generation: u64,
    },
    Refused(PublishRefusal),
}
