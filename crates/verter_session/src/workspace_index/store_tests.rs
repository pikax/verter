//! The index's write side: atomic publication, the fence, ingress validation,
//! query walks over hand-built snapshots and set-valued membership. These reach
//! the crate-private writer directly; the host route's behavior is covered
//! through the public API in the integration suite.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, InputBasisId, QueryIdentity, ResultContractId, SourceId, SourceRevision,
    SourceUnitId,
};
use verter_identity::successor::{CarrierProfileId, ConfiguredProjectId, ProfileSchemaEpoch};
use verter_language::{FileLanguage, FrameworkAdapterId};
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};
use verter_span::{SourceByteOffset, SourceByteRange};

use super::delta::*;
use super::host_projection::*;
use super::identity::*;
use super::membership::*;
use super::query::*;
use super::records::*;
use super::snapshot::*;
use crate::{HostConfig, UpsertRequest, VerterHost};

struct Text(&'static str);

impl CanonicalEncode for Text {
    const DOMAIN_TAG: &'static str = "verter.test.workspace_index.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

fn digest(text: &str) -> CanonicalDigest {
    CanonicalDigest::of_bytes(text.as_bytes())
}

fn unit(name: &'static str) -> SourceUnitId {
    SourceUnitId::from_lineage(&SourceId::from_canonical(&Text(name)), "file")
}

fn claim() -> ProfileClaim {
    ProfileClaim::Carrier(CarrierProfileId::from_carrier(
        "test",
        &ProfileSchemaEpoch::for_schema_family("test", 1),
    ))
}

fn stylesheet_schema() -> TableSchema {
    TableSchema {
        table: TableKind::StylesheetSymbols,
        version: 1,
    }
}

fn catalog() -> CatalogRevisionId {
    CatalogRevisionId::from_catalog_owner(&Text("catalog"))
}

fn admission() -> SchemaAdmission {
    SchemaAdmission {
        catalog: Some(catalog()),
        table_schemas: [
            stylesheet_schema(),
            TableSchema {
                table: TableKind::StylesheetSymbols,
                version: 2,
            },
            TableSchema {
                table: TableKind::ProjectMemberships,
                version: 1,
            },
            TableSchema {
                table: TableKind::Links,
                version: 1,
            },
            TableSchema {
                table: TableKind::ExtensionFacts,
                version: 1,
            },
            TableSchema {
                table: TableKind::Registrations,
                version: 1,
            },
        ]
        .into_iter()
        .collect(),
        ..SchemaAdmission::default()
    }
}

fn account() -> Arc<SemanticRetentionAccount> {
    limited_account(1 << 30)
}

fn limited_account(ceiling: usize) -> Arc<SemanticRetentionAccount> {
    SemanticRetentionAccount::new(RetentionLimits {
        aggregate_ceiling_bytes: ceiling,
        max_entry_bytes: ceiling,
        pin_threshold_bytes: ceiling,
    })
}

fn source_key(unit: &SourceUnitId) -> ObservationKey {
    ObservationKey {
        owner: ObservationOwner::Source,
        subject: unit.digest(),
    }
}

/// Owners' answers as plain test state.
#[derive(Default)]
struct Owners {
    current: BTreeMap<ObservationKey, CanonicalDigest>,
}

impl Owners {
    fn with_source(unit: &SourceUnitId, content: &str) -> Self {
        let mut owners = Self::default();
        owners.set_source(unit, content);
        owners
    }

    fn set_source(&mut self, unit: &SourceUnitId, content: &str) {
        self.current.insert(source_key(unit), digest(content));
    }
}

impl ObservationOracle for Owners {
    fn current(&self, key: &ObservationKey) -> Option<CanonicalDigest> {
        self.current.get(key).copied()
    }

    fn current_epochs(&self) -> (Option<SelectionEpoch>, Option<DemandEpoch>) {
        (
            Some(SelectionEpoch::from_selection_owner(1)),
            Some(DemandEpoch::from_demand_plan(1)),
        )
    }
}

fn producer() -> ProducerDescriptorId {
    ProducerDescriptorId::of_descriptor(&Text("producer"))
}

fn key(unit: &SourceUnitId) -> ContributionKey {
    ContributionKey {
        producer: producer(),
        schema: stylesheet_schema(),
        unit: unit.clone(),
        claim: claim(),
        project: None,
        partition: PartitionFamily::StyleSelectors,
    }
}

fn basis(unit: &SourceUnitId, content: &str) -> SourceBasis {
    SourceBasis {
        unit: unit.clone(),
        locator: Arc::from("locator"),
        revision: SourceRevision::from_canonical(&ContentText(content.to_string())),
        content: ContentId::from_content_bytes(content.as_bytes()),
        catalog: Some(catalog()),
        selection_epoch: Some(SelectionEpoch::from_selection_owner(1)),
        demand_epoch: Some(DemandEpoch::from_demand_plan(1)),
        owner_snapshot: OwnerSnapshotId::of_owner_snapshot(&Text("owner")),
    }
}

struct ContentText(String);

impl CanonicalEncode for ContentText {
    const DOMAIN_TAG: &'static str = "verter.test.workspace_index.content.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, &self.0);
    }
}

fn class_record(ordinal: u32, name: &str) -> IndexRecord {
    IndexRecord {
        slot: RecordSlot {
            kind: SlotKind::Occurrence,
            ordinal,
        },
        subject: TypedSubject::StyleSymbol {
            kind: StyleSymbolKind::Class,
            name: Arc::from(name),
        },
        keys: vec![PostingKey::exact(PartitionFamily::StyleSelectors, name)],
        row: IndexRow::Stylesheet(StylesheetRow {
            kind: StyleSymbolKind::Class,
            name: Arc::from(name),
            role: StyleRole::Definition,
            surface: StyleSurface::Style { block: 0 },
            scope: StyleScope::Global,
        }),
        origins: vec![RecordOrigin::Unavailable(
            OriginUnavailable::OwnerRecordsNoSpan,
        )],
        quality: FactQuality::Exact,
        causes: Vec::new(),
    }
}

fn draft(unit: &SourceUnitId, content: &str, classes: &[&str]) -> PartitionDraft {
    PartitionDraft {
        key: key(unit),
        basis: basis(unit, content),
        provenance: Provenance {
            producer: producer(),
            result_contract: ResultContractId::from_canonical(&Text("rows")),
            release: None,
            consumed: Vec::new(),
        },
        read_set: ReadSet::of([ReadWitness {
            key: source_key(unit),
            fingerprint: digest(content),
        }]),
        records: classes
            .iter()
            .enumerate()
            .map(|(ordinal, name)| class_record(u32::try_from(ordinal).unwrap(), name))
            .collect(),
    }
}

fn sealed(draft: PartitionDraft, account: &Arc<SemanticRetentionAccount>) -> CompletePartition {
    draft.seal(&admission(), account).expect("valid draft")
}

fn delta(changes: Vec<SlotChange>) -> ContributionDelta {
    ContributionDelta {
        changes,
        coverage: Vec::new(),
        decertified: Vec::new(),
        selection_epoch: Some(SelectionEpoch::from_selection_owner(1)),
        demand_epoch: Some(DemandEpoch::from_demand_plan(1)),
    }
}

fn store(account: &Arc<SemanticRetentionAccount>) -> WorkspaceIndex {
    WorkspaceIndex::new(StoreAccount::new(Arc::clone(account)))
}

fn upsert(partition: CompletePartition, expected: ExpectedPrior) -> SlotChange {
    SlotChange::Upsert {
        partition: Box::new(partition),
        expected,
    }
}

fn scope_id(name: &'static str) -> ScopeId {
    ScopeId::of_population(&Text(name))
}

/// This test producer's partition of stylesheet classes.
fn selectors() -> BTreeSet<PartitionId> {
    [PartitionId::new(
        producer(),
        PartitionFamily::StyleSelectors,
    )]
    .into_iter()
    .collect()
}

fn declared(scope: ScopeId) -> DeclaredScope {
    DeclaredScope::new(scope, selectors())
}

fn never() -> impl Fn() -> bool {
    || false
}

fn lookup(name: &str, postings: u32, records: u32) -> IndexQuery {
    IndexQuery {
        lookup: Lookup::Keys(vec![PostingKey::exact(
            PartitionFamily::StyleSelectors,
            name,
        )]),
        budgets: QueryBudgets { postings, records },
        scope: None,
        continuation: None,
    }
}

fn found(snapshot: &IndexSnapshot, name: &str) -> usize {
    query_candidates(snapshot, &lookup(name, 100, 100), None, &never())
        .candidates
        .len()
}

#[test]
fn replacing_a_slot_moves_its_postings_in_one_snapshot_and_keeps_old_readers_whole() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let owners = Owners::with_source(&a, "v1");
    let published = index.publish(
        delta(vec![upsert(
            sealed(draft(&a, "v1", &["old"]), &account),
            ExpectedPrior::Absent,
        )]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));
    let old_reader = index.snapshot();

    let owners = Owners::with_source(&a, "v2");
    let published = index.publish(
        delta(vec![upsert(
            sealed(draft(&a, "v2", &["new"]), &account),
            ExpectedPrior::Basis(Box::new(basis(&a, "v1"))),
        )]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));

    let current = index.snapshot();
    assert_eq!(found(&current, "new"), 1);
    assert_eq!(
        found(&current, "old"),
        0,
        "no stale reverse posting survives"
    );
    assert_eq!(
        found(&old_reader, "old"),
        1,
        "the old reader keeps its snapshot"
    );
    assert_eq!(found(&old_reader, "new"), 0);
}

#[test]
fn one_failing_slot_commits_no_healthy_sibling() {
    let account = account();
    let index = store(&account);
    let (a, b) = (unit("a"), unit("b"));
    let mut owners = Owners::with_source(&a, "v1");
    owners.set_source(&b, "v1");

    // `b` expects a prior value it does not hold.
    let outcome = index.publish(
        delta(vec![
            upsert(
                sealed(draft(&a, "v1", &["healthy"]), &account),
                ExpectedPrior::Absent,
            ),
            upsert(
                sealed(draft(&b, "v1", &["doomed"]), &account),
                ExpectedPrior::Basis(Box::new(basis(&b, "v0"))),
            ),
        ]),
        &owners,
        &never(),
    );
    assert!(matches!(
        outcome,
        PublishOutcome::Refused(PublishRefusal::Conflict(_))
    ));
    let current = index.snapshot();
    assert_eq!(current.generation(), 0);
    assert_eq!(found(&current, "healthy"), 0);
    assert_eq!(current.contribution_count(), 0);
}

#[test]
fn a_stale_writer_conflicts_and_replay_is_idempotent_only_for_an_identical_value() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let write = |content: &str, classes: &[&str], expected| {
        let owners = Owners::with_source(&a, content);
        index.publish(
            delta(vec![upsert(
                sealed(draft(&a, content, classes), &account),
                expected,
            )]),
            &owners,
            &never(),
        )
    };
    assert!(matches!(
        write("v2", &["newer"], ExpectedPrior::Absent),
        PublishOutcome::Published
    ));
    // An older computation finishing last still expected the slot to be empty.
    assert!(matches!(
        write("v1", &["older"], ExpectedPrior::Absent),
        PublishOutcome::Refused(PublishRefusal::Conflict(_))
    ));
    assert_eq!(found(&index.snapshot(), "newer"), 1);
    assert_eq!(found(&index.snapshot(), "older"), 0);

    let generation = index.snapshot().generation();
    assert!(matches!(
        write("v2", &["newer"], ExpectedPrior::Absent),
        PublishOutcome::AlreadyCurrent
    ));
    assert_eq!(index.snapshot().generation(), generation);
    assert!(matches!(
        write("v2", &["different"], ExpectedPrior::Absent),
        PublishOutcome::Refused(PublishRefusal::ReplayMismatch(_))
    ));
    assert_eq!(index.snapshot().generation(), generation);
}

#[test]
fn a_changed_observation_or_epoch_refuses_publication_at_the_fence() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let moved_on = Owners::with_source(&a, "v2");
    assert!(matches!(
        index.publish(
            delta(vec![upsert(
                sealed(draft(&a, "v1", &["x"]), &account),
                ExpectedPrior::Absent
            )]),
            &moved_on,
            &never(),
        ),
        PublishOutcome::Refused(PublishRefusal::Stale)
    ));

    let owners = Owners::with_source(&a, "v1");
    let mut superseded = delta(vec![upsert(
        sealed(draft(&a, "v1", &["x"]), &account),
        ExpectedPrior::Absent,
    )]);
    superseded.selection_epoch = Some(SelectionEpoch::from_selection_owner(0));
    assert!(matches!(
        index.publish(superseded, &owners, &never()),
        PublishOutcome::Refused(PublishRefusal::Stale)
    ));

    // An absent plan is not a real epoch, whatever that epoch's number.
    for absent in [None, Some(DemandEpoch::from_demand_plan(0))] {
        let mut no_plan = delta(vec![upsert(
            sealed(draft(&a, "v1", &["x"]), &account),
            ExpectedPrior::Absent,
        )]);
        no_plan.demand_epoch = absent;
        assert!(matches!(
            index.publish(no_plan, &owners, &never()),
            PublishOutcome::Refused(PublishRefusal::Stale)
        ));
    }
    assert_eq!(index.snapshot().contribution_count(), 0);
}

#[test]
fn a_cancelled_publication_discards_the_delta_at_every_boundary() {
    let account = account();
    let a = unit("a");
    let owners = Owners::with_source(&a, "v1");
    // Boundary 0 is before fencing, boundary 1 is immediately before the swap.
    for boundary in 0..2 {
        let index = store(&account);
        let calls = Cell::new(0);
        let is_cancelled = || {
            let call = calls.get();
            calls.set(call + 1);
            call == boundary
        };
        let outcome = index.publish(
            delta(vec![upsert(
                sealed(draft(&a, "v1", &["x"]), &account),
                ExpectedPrior::Absent,
            )]),
            &owners,
            &is_cancelled,
        );
        assert!(
            matches!(outcome, PublishOutcome::Refused(PublishRefusal::Cancelled)),
            "boundary {boundary}"
        );
        assert_eq!(index.snapshot().contribution_count(), 0);
        assert_eq!(calls.get(), boundary + 1, "boundary {boundary} was reached");
    }
}

#[test]
fn old_and_new_schema_layouts_cannot_stay_current_together() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let owners = Owners::with_source(&a, "v1");
    let first = sealed(draft(&a, "v1", &["x"]), &account);
    let old_id = first.id();
    let published = index.publish(
        delta(vec![upsert(first, ExpectedPrior::Absent)]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));

    let upgraded = || {
        let mut upgraded = draft(&a, "v1", &["x"]);
        upgraded.key.schema.version = 2;
        sealed(upgraded, &account)
    };
    assert!(matches!(
        index.publish(
            delta(vec![upsert(upgraded(), ExpectedPrior::Absent)]),
            &owners,
            &never(),
        ),
        PublishOutcome::Refused(PublishRefusal::ParallelSchemaLayout(_))
    ));

    let retired = SlotChange::Retire {
        id: old_id,
        expected: Box::new(basis(&a, "v1")),
        evidence: RetirementEvidence {
            witness: ReadWitness {
                key: source_key(&a),
                fingerprint: digest("v1"),
            },
        },
    };
    let published = index.publish(
        delta(vec![retired, upsert(upgraded(), ExpectedPrior::Absent)]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));
    assert_eq!(index.snapshot().contribution_count(), 1);
    assert_eq!(found(&index.snapshot(), "x"), 1);
}

#[test]
fn a_retirement_with_a_stale_witness_tombstones_nothing() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let mut owners = Owners::with_source(&a, "v1");
    let first = sealed(draft(&a, "v1", &["x"]), &account);
    let id = first.id();
    let published = index.publish(
        delta(vec![upsert(first, ExpectedPrior::Absent)]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));

    // The deletion witness says "absent", but the source owner still has it.
    owners.set_source(&a, "v1");
    let outcome = index.publish(
        delta(vec![SlotChange::Retire {
            id,
            expected: Box::new(basis(&a, "v1")),
            evidence: RetirementEvidence {
                witness: ReadWitness {
                    key: source_key(&a),
                    fingerprint: digest("absent"),
                },
            },
        }]),
        &owners,
        &never(),
    );
    assert!(matches!(
        outcome,
        PublishOutcome::Refused(PublishRefusal::Stale)
    ));
    assert_eq!(found(&index.snapshot(), "x"), 1);
}

#[test]
fn ingress_rejects_every_unprovable_draft_instead_of_decoding_it_as_empty() {
    fn rejected(draft: PartitionDraft, admission: &SchemaAdmission) -> IngressRejection {
        draft
            .seal(admission, &account())
            .expect_err("draft must be rejected")
    }
    let a = unit("a");
    let base = || draft(&a, "v1", &["x"]);
    let in_family = |family: PartitionFamily| {
        let mut draft = base();
        draft.key.partition = family;
        draft.key.schema.table = family.table();
        draft.records[0].keys.clear();
        draft
    };

    let mut unknown_schema = base();
    unknown_schema.key.schema.version = 99;
    assert!(matches!(
        rejected(unknown_schema, &admission()),
        IngressRejection::UnsupportedSchema(_)
    ));

    let mut foreign_table = base();
    foreign_table.key.schema.table = TableKind::Links;
    assert!(matches!(
        rejected(foreign_table, &admission()),
        IngressRejection::UnsupportedSchema(_)
    ));

    let mut no_catalog = base();
    no_catalog.basis.catalog = None;
    assert_eq!(
        rejected(no_catalog, &admission()),
        IngressRejection::CatalogMismatch
    );

    let mut foreign_key = base();
    foreign_key.records[0].keys = vec![PostingKey::exact(PartitionFamily::CssVariables, "x")];
    assert!(matches!(
        rejected(foreign_key, &admission()),
        IngressRejection::KeyOutsidePartition { .. }
    ));

    let mut no_evidence = base();
    no_evidence.read_set = ReadSet::default();
    assert_eq!(
        rejected(no_evidence, &admission()),
        IngressRejection::MissingReadEvidence
    );

    let mut wrong_table = base();
    wrong_table.records[0].row = IndexRow::Name(NameRow {
        spelling: Arc::from("x"),
        role: NameRole::Declaration,
        declaration_kind: None,
    });
    assert!(matches!(
        rejected(wrong_table, &admission()),
        IngressRejection::RowOutsideTable { .. }
    ));

    let mut no_origin = base();
    no_origin.records[0].origins.clear();
    assert!(matches!(
        rejected(no_origin, &admission()),
        IngressRejection::MissingOrigin { .. }
    ));

    let mut duplicate = draft(&a, "v1", &["x", "y"]);
    duplicate.records[1].slot = duplicate.records[0].slot;
    assert!(matches!(
        rejected(duplicate, &admission()),
        IngressRejection::DuplicateRecordSlot { .. }
    ));

    let mut other_revision = base();
    other_revision.records[0].origins = vec![RecordOrigin::Authored(SourceAnchor {
        unit: a.clone(),
        revision: basis(&a, "v0").revision,
        content: basis(&a, "v0").content,
        range: SourceByteRange::new(SourceByteOffset::new(0), SourceByteOffset::new(1)),
    })];
    assert!(matches!(
        rejected(other_revision, &admission()),
        IngressRejection::AnchorOutsideBasis { .. }
    ));

    let unresolved_link = LinkRow {
        relation: LinkRelation::Import,
        role: LinkRole::Unresolved,
        source: Endpoint::Unit(a.clone()),
        target: Endpoint::Unresolved {
            spelling: Arc::from("./x"),
            cause: FactCause::SpecifierUnresolved,
        },
        resolution: None,
    };
    let mut exact_unresolved = in_family(PartitionFamily::Imports);
    exact_unresolved.records[0].row = IndexRow::Link(Box::new(unresolved_link));
    assert!(matches!(
        rejected(exact_unresolved, &admission()),
        IngressRejection::UnresolvedMarkedExact { .. }
    ));

    let extension = ExtensionId::of_extension(&Text("extension"));
    let extension_row = |kind_version| {
        IndexRow::Extension(ExtensionRow {
            extension: extension.clone(),
            extension_version: Arc::from("1.0.0"),
            permissions: Vec::new(),
            kind: Arc::from("kind"),
            kind_version,
            fields: Vec::new(),
        })
    };
    let extension_draft = |kind_version| {
        let mut draft = in_family(PartitionFamily::Extensions);
        draft.records[0].row = extension_row(kind_version);
        draft
    };
    assert_eq!(
        rejected(extension_draft(1), &admission()),
        IngressRejection::UnattributedExtension
    );
    let mut attributed = admission();
    attributed.extensions.insert(extension.clone());
    attributed.extension_kinds.insert((Arc::from("kind"), 1));
    assert!(matches!(
        rejected(extension_draft(2), &attributed),
        IngressRejection::UnsupportedExtensionKind { version: 2, .. }
    ));
    extension_draft(1)
        .seal(&attributed, &account())
        .expect("a registered, attributed kind is admitted");

    let mut undelivered = in_family(PartitionFamily::Registrations);
    undelivered.records[0].row = IndexRow::Registration(RegistrationRow {
        domain: Arc::from("store"),
        key: RegistrationKey::String(Arc::from("cart")),
        is_usage: false,
        operation: Arc::from("store.resolve"),
    });
    assert!(matches!(
        rejected(undelivered, &admission()),
        IngressRejection::UndeliveredOperation(_)
    ));
}

/// A store holding one slot with `count` records under one key.
fn crowded(count: u32) -> WorkspaceIndex {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let owners = Owners::with_source(&a, "v1");
    let mut rows = draft(&a, "v1", &[]);
    rows.records = (0..count)
        .map(|ordinal| class_record(ordinal, "same"))
        .collect();
    let published = index.publish(
        delta(vec![upsert(sealed(rows, &account), ExpectedPrior::Absent)]),
        &owners,
        &never(),
    );
    assert!(matches!(published, PublishOutcome::Published));
    index
}

#[test]
fn exhausting_a_budget_is_partial_and_its_continuation_reaches_every_record_once() {
    let index = crowded(5);
    let snapshot = index.snapshot();
    let mut query = lookup("same", 2, 100);
    let mut seen = BTreeSet::new();
    let mut pages = 0;
    loop {
        let page = query_candidates(&snapshot, &query, None, &never());
        assert!(
            page.work.posting_visits <= 2,
            "visits stay under the budget"
        );
        pages += 1;
        for candidate in &page.candidates {
            assert!(seen.insert(candidate.record_index), "no record twice");
        }
        match page.status {
            EnumerationStatus::Partial {
                cause: PartialCause::BudgetExhausted,
                continuation: Some(next),
                ..
            } => query.continuation = Some(next),
            EnumerationStatus::Partial {
                cause: PartialCause::OpenDiscovery,
                continuation: None,
                ..
            } => break,
            other => panic!("unexpected status {other:?}"),
        }
    }
    assert_eq!(seen.len(), 5);
    assert_eq!(pages, 3);
}

#[test]
fn a_continuation_is_refused_on_any_other_snapshot() {
    let index = crowded(3);
    let snapshot = index.snapshot();
    let page = query_candidates(&snapshot, &lookup("same", 1, 100), None, &never());
    let EnumerationStatus::Partial {
        continuation: Some(next),
        ..
    } = page.status
    else {
        panic!("expected a continuation");
    };
    // A second store at the same generation: its numbering is its own.
    let other_store = crowded(3);
    let other = other_store.snapshot();
    assert_eq!(other.generation(), snapshot.generation());
    let mut query = lookup("same", 10, 10);
    query.continuation = Some(next);
    let resumed = query_candidates(&other, &query, None, &never());
    assert!(resumed.candidates.is_empty());
    assert!(matches!(
        resumed.status,
        EnumerationStatus::Partial {
            cause: PartialCause::Stale,
            continuation: None,
            ..
        }
    ));
}

#[test]
fn exhausting_keys_without_a_certified_population_is_never_complete() {
    let index = crowded(1);
    let snapshot = index.snapshot();
    // Even an empty answer proves nothing without a coverage record.
    let mut query = lookup("nothing-here", 10, 10);
    query.scope = Some(declared(scope_id("declared")));
    let page = query_candidates(&snapshot, &query, None, &never());
    assert!(page.candidates.is_empty());
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::OpenDiscovery,
            ..
        }
    ));
}

#[test]
fn a_broad_walk_spends_its_budget_and_only_proves_exhaustion_by_running_out_of_keys() {
    let index = crowded(4);
    let snapshot = index.snapshot();
    let broad = |postings| IndexQuery {
        lookup: Lookup::Prefix {
            partition: PartitionFamily::StyleSelectors,
            family: KeyFamily::Exact,
            version: 0,
            prefix: Arc::from(""),
        },
        budgets: QueryBudgets {
            postings,
            records: 100,
        },
        scope: None,
        continuation: None,
    };
    let limited = query_candidates(&snapshot, &broad(2), None, &never());
    assert_eq!(limited.work.posting_visits, 2);
    assert!(matches!(
        limited.status,
        EnumerationStatus::Partial {
            cause: PartialCause::BudgetExhausted,
            ..
        }
    ));
    let enough = query_candidates(&snapshot, &broad(4), None, &never());
    assert_eq!(enough.candidates.len(), 4);
    assert!(matches!(
        enough.status,
        EnumerationStatus::Partial {
            cause: PartialCause::OpenDiscovery,
            ..
        }
    ));
}

#[test]
fn query_visits_stay_at_the_budget_as_matching_postings_grow() {
    for count in [10, 100, 1000] {
        let index = crowded(count);
        let page = query_candidates(&index.snapshot(), &lookup("same", 7, 100), None, &never());
        assert_eq!(page.work.posting_visits, 7, "{count} matching postings");
        assert_eq!(page.candidates.len(), 7);
    }
}

#[test]
fn cancelling_a_query_returns_no_page() {
    let index = crowded(5);
    let calls = Cell::new(0);
    let cancel_third = || {
        calls.set(calls.get() + 1);
        calls.get() == 3
    };
    let page = query_candidates(
        &index.snapshot(),
        &lookup("same", 10, 10),
        None,
        &cancel_third,
    );
    assert!(page.candidates.is_empty());
    assert_eq!(page.status, EnumerationStatus::Cancelled);
}

#[test]
fn a_stale_row_is_not_returned_and_leaves_the_answer_partial() {
    let index = crowded(2);
    let moved_on = Owners::with_source(&unit("a"), "v2");
    let page = query_candidates(
        &index.snapshot(),
        &lookup("same", 10, 10),
        Some(&moved_on),
        &never(),
    );
    assert!(page.candidates.is_empty());
    assert_eq!(page.work.posting_visits, 2, "stale rows still cost visits");
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::Stale,
            ..
        }
    ));
}

#[test]
fn invalidation_retires_dependents_and_their_population_and_keeps_unrelated_slots() {
    let account = account();
    let index = store(&account);
    let (a, b) = (unit("a"), unit("b"));
    let mut owners = Owners::with_source(&a, "v1");
    owners.set_source(&b, "v1");
    let scope = scope_id("population");
    let witness_key = ObservationKey {
        owner: ObservationOwner::Population,
        subject: scope.digest(),
    };
    owners.current.insert(witness_key, digest("population-1"));

    let (first, second) = (
        sealed(draft(&a, "v1", &["shared", "only-a"]), &account),
        sealed(draft(&b, "v1", &["shared"]), &account),
    );
    let mut change = delta(vec![]);
    change.coverage.push(CoverageRecord {
        scope: scope.clone(),
        partitions: selectors(),
        witness: ReadWitness {
            key: witness_key,
            fingerprint: digest("population-1"),
        },
        contributions: [first.id(), second.id()].into_iter().collect(),
    });
    change.changes = vec![
        upsert(first, ExpectedPrior::Absent),
        upsert(second, ExpectedPrior::Absent),
    ];
    let published = index.publish(change, &owners, &never());
    assert!(matches!(published, PublishOutcome::Published));

    let mut certified = lookup("shared", 10, 10);
    certified.scope = Some(declared(scope.clone()));
    let before = query_candidates(&index.snapshot(), &certified, Some(&owners), &never());
    assert_eq!(before.status, EnumerationStatus::Complete);
    assert_eq!(before.candidates.len(), 2);

    let work = index.invalidate(&[source_key(&a)]);
    assert_eq!(work.contributions_retired, 1, "only the dependent slot");
    assert_eq!(work.coverage_retired, 1);

    let current = index.snapshot();
    assert_eq!(found(&current, "only-a"), 0, "ineligible immediately");
    assert_eq!(
        found(&current, "shared"),
        1,
        "the unrelated partition stays"
    );
    let after = query_candidates(&current, &certified, Some(&owners), &never());
    assert!(
        matches!(after.status, EnumerationStatus::Partial { .. }),
        "a retired slot's population cannot stay certified"
    );
}

#[test]
fn a_changed_population_witness_decertifies_a_complete_empty_answer() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let scope = scope_id("population");
    let witness_key = ObservationKey {
        owner: ObservationOwner::Population,
        subject: scope.digest(),
    };
    let mut owners = Owners::with_source(&a, "v1");
    owners.current.insert(witness_key, digest("one-file"));
    let partition = sealed(draft(&a, "v1", &["x"]), &account);
    let mut change = delta(vec![]);
    change.coverage.push(CoverageRecord {
        scope: scope.clone(),
        partitions: selectors(),
        witness: ReadWitness {
            key: witness_key,
            fingerprint: digest("one-file"),
        },
        contributions: [partition.id()].into_iter().collect(),
    });
    change.changes = vec![upsert(partition, ExpectedPrior::Absent)];
    let published = index.publish(change, &owners, &never());
    assert!(matches!(published, PublishOutcome::Published));

    let mut empty = lookup("absent-name", 10, 10);
    empty.scope = Some(declared(scope));
    let certified = query_candidates(&index.snapshot(), &empty, Some(&owners), &never());
    assert_eq!(certified.status, EnumerationStatus::Complete);

    // A file was added: no existing source changed, but the population did.
    owners.current.insert(witness_key, digest("two-files"));
    let stale = query_candidates(&index.snapshot(), &empty, Some(&owners), &never());
    assert!(matches!(
        stale.status,
        EnumerationStatus::Partial {
            cause: PartialCause::Stale,
            ..
        }
    ));
}

#[test]
fn retention_pressure_keeps_nothing_and_commits_no_sibling() {
    let account = limited_account(64);
    let index = store(&account);
    let a = unit("a");
    let owners = Owners::with_source(&a, "v1");
    let outcome = index.publish(
        delta(vec![upsert(
            sealed(draft(&a, "v1", &["x"]), &account),
            ExpectedPrior::Absent,
        )]),
        &owners,
        &never(),
    );
    assert!(matches!(
        outcome,
        PublishOutcome::Refused(PublishRefusal::RetentionPressure)
    ));
    assert_eq!(index.snapshot().contribution_count(), 0);
    let held = account.snapshot();
    assert_eq!(held.retained_bytes, 0, "a refused value stays uncached");
}

#[test]
fn repeated_edits_plateau_retained_rows_postings_edges_and_bytes_once_readers_release() {
    let account = account();
    let index = store(&account);
    let units = [unit("a"), unit("b"), unit("c")];
    let mut owners = Owners::default();
    let mut expected: Vec<ExpectedPrior> = vec![ExpectedPrior::Absent; units.len()];
    let mut plateau = None;
    for cycle in 0..40 {
        let content = format!("cycle-{cycle}");
        let reader = index.snapshot();
        for (position, unit) in units.iter().enumerate() {
            owners.set_source(unit, &content);
            let classes = ["alpha", "beta"];
            let outcome = index.publish(
                delta(vec![upsert(
                    sealed(draft(unit, &content, &classes), &account),
                    expected[position].clone(),
                )]),
                &owners,
                &never(),
            );
            assert!(matches!(outcome, PublishOutcome::Published));
            expected[position] = ExpectedPrior::Basis(Box::new(basis(unit, &content)));
        }
        drop(reader);
        let current = index.snapshot();
        let measured = (
            current.contribution_count(),
            current.posting_count(),
            current.read_edge_count(),
            account.snapshot().retained_bytes,
        );
        drop(current);
        match plateau {
            None => plateau = Some(measured),
            Some(first) => assert_eq!(measured, first, "cycle {cycle}"),
        }
    }
    drop(index);
    assert_eq!(
        account.snapshot().retained_bytes,
        0,
        "every charge released"
    );
}

fn pm_project(name: &'static str) -> PmProjectId {
    PmProjectId::Configured(ConfiguredProjectId::from_canonical(&Text(name)))
}

fn fact(name: &'static str) -> ObservationRef {
    ObservationRef {
        query: QueryIdentity::compose(
            "verter.test.workspace_index.fact.v1",
            digest(name),
            &[],
            &ResultContractId::from_canonical(&Text("rows")),
        ),
        basis: InputBasisId::from_canonical(&Text(name)),
        role_slot: None,
    }
}

fn observation(
    unit: &SourceUnitId,
    status: MembershipStatus,
    members: &[&'static str],
    preferred: PreferredOwner,
) -> MembershipObservation {
    let rows = draft(unit, "v1", &[]);
    MembershipObservation {
        unit: unit.clone(),
        status,
        members: members
            .iter()
            .map(|name| ProjectMember {
                project: pm_project(name),
                environment: fact(name),
            })
            .collect(),
        preferred,
        observation: fact("membership"),
        basis: rows.basis,
        read_set: rows.read_set,
        producer: rows.key.producer,
        claim: claim(),
        provenance: rows.provenance,
    }
}

fn publish_membership(index: &WorkspaceIndex, observation: MembershipObservation, owners: &Owners) {
    let partition = publishable_membership(observation, &admission(), index.account().get())
        .expect("complete observation");
    let outcome = index.publish(
        delta(vec![upsert(partition, ExpectedPrior::Absent)]),
        owners,
        &never(),
    );
    assert!(matches!(outcome, PublishOutcome::Published));
}

#[test]
fn overlapping_membership_keeps_the_whole_project_set_and_a_separate_preference() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    publish_membership(
        &index,
        observation(
            &a,
            MembershipStatus::Complete,
            &["p2", "p1"],
            PreferredOwner::Selected(pm_project("p1")),
        ),
        &Owners::with_source(&a, "v1"),
    );

    let snapshot = index.snapshot();
    let page = unit_members(&snapshot, &a, 10);
    assert_eq!(page.status, EnumerationStatus::Complete);
    let mut expected = vec![pm_project("p1"), pm_project("p2")];
    expected.sort();
    assert_eq!(
        page.members
            .iter()
            .map(|m| m.project.clone())
            .collect::<Vec<_>>(),
        expected,
        "both projects in canonical order, not the preferred one alone"
    );
    assert_eq!(page.preferred, PreferredOwner::Selected(pm_project("p1")));

    let truncated = unit_members(&snapshot, &a, 1);
    assert_eq!(truncated.members.len(), 1);
    assert!(
        matches!(truncated.status, EnumerationStatus::Partial { .. }),
        "an oversized set is never reported complete"
    );
}

#[test]
fn reverse_enumeration_needs_the_population_witness_not_just_forward_sets() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    publish_membership(
        &index,
        observation(
            &a,
            MembershipStatus::Complete,
            &["p1"],
            PreferredOwner::None,
        ),
        &Owners::with_source(&a, "v1"),
    );
    let page = project_units(&index.snapshot(), &pm_project("p1"), None, 10);
    assert_eq!(page.units, vec![a]);
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::OpenDiscovery,
            ..
        }
    ));
}

#[test]
fn a_proved_empty_set_differs_from_an_unobserved_unit_and_partial_publishes_nothing() {
    let account = account();
    let index = store(&account);
    let (a, unseen) = (unit("a"), unit("unseen"));
    publish_membership(
        &index,
        observation(&a, MembershipStatus::Complete, &[], PreferredOwner::None),
        &Owners::with_source(&a, "v1"),
    );
    let snapshot = index.snapshot();
    let proved = unit_members(&snapshot, &a, 10);
    assert_eq!(proved.status, EnumerationStatus::Complete);
    assert!(proved.members.is_empty());
    assert!(matches!(
        unit_members(&snapshot, &unseen, 10).status,
        EnumerationStatus::Partial { .. }
    ));

    for status in [
        MembershipStatus::Partial,
        MembershipStatus::NeedInputs,
        MembershipStatus::Unsupported,
        MembershipStatus::Cancelled,
        MembershipStatus::Stale,
    ] {
        let declined = publishable_membership(
            observation(&unseen, status, &["p1"], PreferredOwner::Ambiguous),
            &admission(),
            &account,
        )
        .expect_err("only a complete observation is publishable");
        assert!(matches!(
            declined,
            MembershipDecline::NotComplete { status: seen, ref positive_members }
                if seen == status && positive_members.len() == 1
        ));
    }
}

#[test]
fn the_project_models_own_observation_is_sufficient_read_evidence_for_membership() {
    let account = account();
    let a = unit("a");
    let mut observed = observation(
        &a,
        MembershipStatus::Complete,
        &["p1"],
        PreferredOwner::None,
    );
    observed.read_set = ReadSet::of([ReadWitness {
        key: ObservationKey {
            owner: ObservationOwner::ProjectModel,
            subject: a.digest(),
        },
        fingerprint: digest("membership-v1"),
    }]);
    assert!(publishable_membership(observed.clone(), &admission(), &account).is_ok());

    observed.read_set = ReadSet::default();
    assert!(matches!(
        publishable_membership(observed, &admission(), &account),
        Err(MembershipDecline::Rejected(
            IngressRejection::MissingReadEvidence
        ))
    ));
}

fn continuation_of(page: CandidatePage) -> IndexContinuation {
    match page.status {
        EnumerationStatus::Partial {
            continuation: Some(next),
            ..
        } => next,
        other => panic!("expected a continuation, got {other:?}"),
    }
}

fn refuses_as_stale(snapshot: &IndexSnapshot, query: &IndexQuery) {
    let resumed = query_candidates(snapshot, query, None, &never());
    assert!(resumed.candidates.is_empty());
    assert!(matches!(
        resumed.status,
        EnumerationStatus::Partial {
            cause: PartialCause::Stale,
            continuation: None,
            ..
        }
    ));
}

#[test]
fn a_continuation_is_bound_to_its_lookup_its_population_and_its_exact_snapshot() {
    let index = crowded(3);
    let snapshot = index.snapshot();
    let next = continuation_of(query_candidates(
        &snapshot,
        &lookup("same", 1, 100),
        None,
        &never(),
    ));

    let mut other_key = lookup("other", 10, 10);
    other_key.continuation = Some(next.clone());
    refuses_as_stale(&snapshot, &other_key);

    let other_lookup = IndexQuery {
        lookup: Lookup::Prefix {
            partition: PartitionFamily::StyleSelectors,
            family: KeyFamily::Exact,
            version: 0,
            prefix: Arc::from("s"),
        },
        budgets: QueryBudgets {
            postings: 10,
            records: 10,
        },
        scope: None,
        continuation: Some(next.clone()),
    };
    refuses_as_stale(&snapshot, &other_lookup);

    let mut other_population = lookup("same", 10, 10);
    other_population.scope = Some(declared(scope_id("another")));
    other_population.continuation = Some(next.clone());
    refuses_as_stale(&snapshot, &other_population);

    // A private snapshot shares its generation with the one it was derived
    // from. Neither accepts the other's continuation.
    let private = snapshot.with_return_only([], &[]);
    assert_eq!(private.generation(), snapshot.generation());
    let mut on_private = lookup("same", 10, 10);
    on_private.continuation = Some(next.clone());
    refuses_as_stale(&private, &on_private);

    let from_private = continuation_of(query_candidates(
        &private,
        &lookup("same", 1, 100),
        None,
        &never(),
    ));
    let mut on_published = lookup("same", 10, 10);
    on_published.continuation = Some(from_private);
    refuses_as_stale(&snapshot, &on_published);

    // Its own query still resumes.
    let mut same = lookup("same", 10, 10);
    same.continuation = Some(next);
    let resumed = query_candidates(&snapshot, &same, None, &never());
    assert_eq!(resumed.candidates.len(), 2, "the rows after the first page");
}

#[test]
fn pagination_over_a_repeated_key_ends() {
    let index = crowded(2);
    let snapshot = index.snapshot();
    let key = PostingKey::exact(PartitionFamily::StyleSelectors, "same");
    let mut query = IndexQuery {
        lookup: Lookup::Keys(vec![key.clone(), key]),
        budgets: QueryBudgets {
            postings: 2,
            records: 100,
        },
        scope: None,
        continuation: None,
    };
    let mut seen = 0;
    for _ in 0..4 {
        let page = query_candidates(&snapshot, &query, None, &never());
        seen += page.candidates.len();
        match page.status {
            EnumerationStatus::Partial {
                continuation: Some(next),
                ..
            } => query.continuation = Some(next),
            _ => {
                assert_eq!(seen, 4, "each requested key contributes its postings");
                return;
            }
        }
    }
    panic!("pagination did not end");
}

#[test]
fn a_record_certifies_only_its_own_partitions_and_another_producers_rows_stay_out() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let scope = scope_id("population");
    let witness_key = ObservationKey {
        owner: ObservationOwner::Population,
        subject: scope.digest(),
    };
    let mut owners = Owners::with_source(&a, "v1");
    owners.current.insert(witness_key, digest("population"));

    let mut foreign = draft(&a, "v1", &["same"]);
    foreign.key.producer = ProducerDescriptorId::of_descriptor(&Text("another producer"));
    let ours = sealed(draft(&a, "v1", &["same"]), &account);
    let mut change = delta(vec![]);
    change.coverage.push(CoverageRecord {
        scope: scope.clone(),
        partitions: selectors(),
        witness: ReadWitness {
            key: witness_key,
            fingerprint: digest("population"),
        },
        contributions: [ours.id()].into_iter().collect(),
    });
    change.changes = vec![
        upsert(ours, ExpectedPrior::Absent),
        upsert(sealed(foreign, &account), ExpectedPrior::Absent),
    ];
    assert!(matches!(
        index.publish(change, &owners, &never()),
        PublishOutcome::Published
    ));
    let snapshot = index.snapshot();

    let mut certified = lookup("same", 10, 10);
    certified.scope = Some(declared(scope.clone()));
    let page = query_candidates(&snapshot, &certified, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(page.candidates.len(), 1, "only this population's producer");
    assert_eq!(
        page.work.posting_visits, 2,
        "filtered rows still cost visits"
    );

    // The scope declares a family its record does not certify.
    let wider = DeclaredScope::new(
        scope.clone(),
        selectors()
            .into_iter()
            .chain([PartitionId::new(producer(), PartitionFamily::CssVariables)])
            .collect(),
    );
    let uncertified = IndexQuery {
        lookup: Lookup::Keys(vec![PostingKey::exact(
            PartitionFamily::CssVariables,
            "same",
        )]),
        budgets: QueryBudgets {
            postings: 10,
            records: 10,
        },
        scope: Some(wider),
        continuation: None,
    };
    assert!(matches!(
        query_candidates(&snapshot, &uncertified, None, &never()).status,
        EnumerationStatus::Partial {
            cause: PartialCause::OpenDiscovery,
            ..
        }
    ));

    // A key of a family the scope does not read returns nothing.
    let mut outside = uncertified;
    outside.scope = Some(declared(scope));
    let page = query_candidates(&snapshot, &outside, None, &never());
    assert!(page.candidates.is_empty());
    assert_eq!(page.work.key_lookups, 0, "an outside key is not walked");
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::OutsideDemand,
            ..
        }
    ));
}

#[test]
fn a_decertified_population_leaves_with_the_delta_even_when_no_slot_changed() {
    let account = account();
    let index = store(&account);
    let a = unit("a");
    let scope = scope_id("population");
    let witness_key = ObservationKey {
        owner: ObservationOwner::Population,
        subject: scope.digest(),
    };
    let mut owners = Owners::with_source(&a, "v1");
    owners.current.insert(witness_key, digest("population"));
    let mut change = delta(vec![]);
    change.coverage.push(CoverageRecord {
        scope: scope.clone(),
        partitions: selectors(),
        witness: ReadWitness {
            key: witness_key,
            fingerprint: digest("population"),
        },
        contributions: BTreeSet::new(),
    });
    assert!(matches!(
        index.publish(change, &owners, &never()),
        PublishOutcome::Published
    ));
    assert!(index.snapshot().coverage(&scope).is_some());

    let mut withdraw = delta(vec![]);
    withdraw.decertified.push(scope.clone());
    assert!(matches!(
        index.publish(withdraw, &owners, &never()),
        PublishOutcome::Published
    ));
    assert!(index.snapshot().coverage(&scope).is_none());
}

#[test]
fn a_carrier_claim_reads_typed_language_fields_and_never_an_owner_path() {
    let template = |owner: &str| FileLanguage::FrameworkTemplate {
        adapter_id: FrameworkAdapterId::new("vue"),
        owner_hint: Some(Arc::from(owner)),
    };
    assert_eq!(
        carrier_claim(&template("/a/One.vue")),
        carrier_claim(&template("/b/Two.vue")),
        "a template's owning file is not part of its carrier"
    );
    let distinct = [
        FileLanguage::vue(),
        FileLanguage::svelte(),
        FileLanguage::script_ts(),
        FileLanguage::script(verter_language::ScriptSourceType::js()),
        FileLanguage::script(verter_language::ScriptSourceType::cjs()),
        template("/a/One.vue"),
    ];
    let claims: BTreeSet<ProfileClaim> = distinct.iter().map(carrier_claim).collect();
    assert_eq!(claims.len(), distinct.len());
}

fn upsert_in(host: &VerterHost, id: &str, source: &str) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
}

const WIDE: QueryBudgets = QueryBudgets {
    postings: 1000,
    records: 1000,
};

const CSS_AND_SCRIPT: &str = "<script setup>\nconst el = document.body;\nconst alpha = 1;\nel.style.setProperty('--brand', 'red');\n</script>\n<template><div>{{ alpha }}</div></template>\n<style>\n:root { --brand: blue; }\n</style>";

/// A file whose analysis commit has not landed: its source is current, its style
/// analysis is not available.
fn host_with_unavailable_style() -> VerterHost {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_in(&host, "/src/A.vue", CSS_AND_SCRIPT);
    upsert_in(&host, "/src/B.vue", CSS_AND_SCRIPT);
    host.scheduler.test_clear_analysis("/src/A.vue");
    host
}

#[test]
fn an_unavailable_surface_is_named_and_leaves_its_other_rows_and_other_partitions_answering() {
    let host = host_with_unavailable_style();
    let answer = host.css_var_flow("--brand", None);

    let EnumerationStatus::Partial { cause, covered, .. } = &answer.page.status else {
        panic!(
            "a missing surface is never complete: {:?}",
            answer.page.status
        );
    };
    assert_eq!(*cause, PartialCause::UnavailableInput);
    assert_eq!(
        covered.unavailable_units,
        vec![Arc::<str>::from("/src/A.vue")]
    );
    let paths = |list: &[Arc<std::path::Path>]| -> Vec<String> {
        let mut paths: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        paths.sort();
        paths
    };
    assert_eq!(
        paths(&answer.flow.script_manipulations),
        ["/src/A.vue", "/src/B.vue"],
        "the surfaces that could be read still answer"
    );
    assert_eq!(paths(&answer.flow.style_definitions), ["/src/B.vue"]);

    // Nothing incomplete was published.
    let store = host.project_type_store().workspace_index().snapshot();
    assert!(!store.is_return_only());
    let held: Vec<Arc<str>> = store
        .contributions_in_partition(&IndexDemandPartition::CssVariables.id())
        .map(|slot| Arc::clone(&slot.basis.locator))
        .collect();
    assert_eq!(held, vec![Arc::<str>::from("/src/B.vue")]);
    assert!(store
        .coverage(&IndexDemand::of([IndexDemandPartition::CssVariables]).scope())
        .is_none());
}

#[test]
fn a_unit_blocking_one_partition_leaves_the_rest_of_the_demand_certified() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_in(
        &host,
        "/src/A.vue",
        "<template><Foo /></template>\n<style>\n:root { --brand: blue; }\n</style>",
    );
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: "/src/theme.ts".to_string(),
            source: Arc::from("document.body.style.setProperty('--brand', 'red');\n"),
            file_language: FileLanguage::script_ts(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    // A script is no component carrier, so it holds no slot of that partition.
    host.scheduler.test_clear_analysis("/src/theme.ts");

    let both = IndexDemand::of([
        IndexDemandPartition::CssVariables,
        IndexDemandPartition::Components,
    ]);
    let uses = Lookup::Keys(vec![PostingKey::exact(PartitionFamily::Components, "Foo")]);
    let (page, _) = host.workspace_index_candidates(&both, uses, WIDE, None, &never());
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert_eq!(page.candidates.len(), 1);

    let css = Lookup::Keys(vec![PostingKey::exact(
        PartitionFamily::CssVariables,
        "--brand",
    )]);
    let (page, _) = host.workspace_index_candidates(&both, css, WIDE, None, &never());
    let EnumerationStatus::Partial { cause, covered, .. } = &page.status else {
        panic!("the blocked partition is not complete: {:?}", page.status);
    };
    assert_eq!(*cause, PartialCause::UnavailableInput);
    assert_eq!(
        covered.unavailable_units,
        vec![Arc::<str>::from("/src/theme.ts")]
    );
    assert!(!page.candidates.is_empty(), "positive rows still answer");

    let store = host.project_type_store().workspace_index().snapshot();
    let record = store
        .coverage(&both.scope())
        .expect("a partial certificate");
    assert_eq!(
        record.partitions,
        [IndexDemandPartition::Components.id()]
            .into_iter()
            .collect()
    );
}

#[test]
fn an_old_slot_does_not_outlive_a_replacement_that_could_not_be_built() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_in(&host, "/src/A.vue", CSS_AND_SCRIPT);
    let demand = IndexDemand::of([IndexDemandPartition::CssVariables]);
    let flow = host.css_var_flow("--brand", None);
    assert_eq!(flow.page.status, EnumerationStatus::Complete);

    upsert_in(
        &host,
        "/src/A.vue",
        &format!("<!-- edit -->{CSS_AND_SCRIPT}"),
    );
    host.scheduler.test_clear_analysis("/src/A.vue");
    let (snapshot, status, work) = host.workspace_index_reconcile(&demand, &never());
    assert!(matches!(status, ReconcileStatus::Uncertified(_)));
    assert_eq!(work.retired, 1);
    assert!(snapshot.is_return_only());

    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(
        store.contribution_count(),
        0,
        "the old value left the store"
    );
    assert!(store.coverage(&demand.scope()).is_none());
}

#[test]
fn a_new_file_that_cannot_be_read_ends_the_old_certificate() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_in(&host, "/src/A.vue", CSS_AND_SCRIPT);
    let demand = IndexDemand::of([IndexDemandPartition::CssVariables]);
    assert_eq!(
        host.css_var_flow("--brand", None).page.status,
        EnumerationStatus::Complete
    );

    upsert_in(&host, "/src/New.vue", CSS_AND_SCRIPT);
    host.scheduler.test_clear_analysis("/src/New.vue");
    let (page, _) = host.workspace_index_candidates(
        &demand,
        Lookup::Keys(vec![PostingKey::exact(
            PartitionFamily::CssVariables,
            "--absent",
        )]),
        WIDE,
        None,
        &never(),
    );
    assert!(matches!(
        page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::UnavailableInput,
            ..
        }
    ));
    let store = host.project_type_store().workspace_index().snapshot();
    assert!(store.coverage(&demand.scope()).is_none());
}
