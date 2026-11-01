//! The workspace candidate index: atomic publication, bounded enumeration,
//! incremental equivalence and set-valued membership, through its public store
//! and through the real host route.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, InputBasisId, QueryIdentity, ResultContractId, SourceId, SourceRevision,
    SourceUnitId,
};
use verter_identity::successor::{CarrierProfileId, ConfiguredProjectId, ProfileSchemaEpoch};
use verter_session::project_type_store::workspace_index::*;
use verter_session::*;
use verter_session_query::retention::{RetentionLimits, SemanticRetentionAccount, StoreAccount};
use verter_span::{SourceByteOffset, SourceByteRange};

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

fn admission() -> SchemaAdmission {
    SchemaAdmission {
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

    fn current_epochs(&self) -> (SelectionEpoch, DemandEpoch) {
        (SelectionEpoch(1), DemandEpoch(1))
    }
}

fn key(unit: &SourceUnitId, partition: &str) -> ContributionKey {
    ContributionKey {
        producer: ProducerDescriptorId::from_canonical(&Text("producer")),
        schema: stylesheet_schema(),
        unit: unit.clone(),
        claim: claim(),
        project: None,
        partition: PartitionId(Arc::from(partition)),
    }
}

fn basis(unit: &SourceUnitId, content: &str) -> SourceBasis {
    SourceBasis {
        unit: unit.clone(),
        locator: Arc::from("locator"),
        revision: SourceRevision::from_canonical(&ContentText(content.to_string())),
        content: ContentId::from_content_bytes(content.as_bytes()),
        catalog: CatalogRevisionId::from_canonical(&Text("catalog")),
        selection_epoch: SelectionEpoch(1),
        demand_epoch: DemandEpoch(1),
        owner_snapshot: OwnerSnapshotId::from_canonical(&Text("owner")),
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
        keys: vec![PostingKey::exact(TableKind::StylesheetSymbols, name)],
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
        key: key(unit, "classes"),
        basis: basis(unit, content),
        provenance: Provenance {
            producer: ProducerDescriptorId::from_canonical(&Text("producer")),
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
        selection_epoch: SelectionEpoch(1),
        demand_epoch: DemandEpoch(1),
    }
}

fn store(account: &Arc<SemanticRetentionAccount>) -> WorkspaceIndex {
    WorkspaceIndex::new(StoreAccount::new(Arc::clone(account)))
}

fn upsert(partition: CompletePartition, expected: ExpectedPrior) -> SlotChange {
    SlotChange::Upsert {
        partition,
        expected,
    }
}

fn never() -> impl Fn() -> bool {
    || false
}

fn lookup(name: &str, postings: u32, records: u32) -> IndexQuery {
    IndexQuery {
        lookup: Lookup::Keys(vec![PostingKey::exact(TableKind::StylesheetSymbols, name)]),
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
    assert!(matches!(published, PublishOutcome::Published { .. }));
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
    assert!(matches!(published, PublishOutcome::Published { .. }));

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
        PublishOutcome::Published { .. }
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
        PublishOutcome::AlreadyCurrent { generation: seen } if seen == generation
    ));
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
    superseded.selection_epoch = SelectionEpoch(0);
    assert!(matches!(
        index.publish(superseded, &owners, &never()),
        PublishOutcome::Refused(PublishRefusal::Stale)
    ));
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
    assert!(matches!(published, PublishOutcome::Published { .. }));

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
    assert!(matches!(published, PublishOutcome::Published { .. }));
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
    assert!(matches!(published, PublishOutcome::Published { .. }));

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

    let mut unknown_schema = base();
    unknown_schema.key.schema.version = 99;
    assert!(matches!(
        rejected(unknown_schema, &admission()),
        IngressRejection::UnsupportedSchema(_)
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
    let mut exact_unresolved = base();
    exact_unresolved.key.schema.table = TableKind::Links;
    exact_unresolved.records[0].row = IndexRow::Link(unresolved_link);
    assert!(matches!(
        rejected(exact_unresolved, &admission()),
        IngressRejection::UnresolvedMarkedExact { .. }
    ));

    let extension = ExtensionId::from_canonical(&Text("extension"));
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
        let mut draft = base();
        draft.key.schema.table = TableKind::ExtensionFacts;
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

    let mut undelivered = base();
    undelivered.key.schema.table = TableKind::Registrations;
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
    assert!(matches!(published, PublishOutcome::Published { .. }));
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
    let other = IndexSnapshot::default();
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
    query.scope = Some(ScopeId::from_canonical(&Text("declared")));
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
            table: TableKind::StylesheetSymbols,
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
    let scope = ScopeId::from_canonical(&Text("population"));
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
        partitions: BTreeSet::new(),
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
    assert!(matches!(published, PublishOutcome::Published { .. }));

    let mut certified = lookup("shared", 10, 10);
    certified.scope = Some(scope.clone());
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
    let scope = ScopeId::from_canonical(&Text("population"));
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
        partitions: BTreeSet::new(),
        witness: ReadWitness {
            key: witness_key,
            fingerprint: digest("one-file"),
        },
        contributions: [partition.id()].into_iter().collect(),
    });
    change.changes = vec![upsert(partition, ExpectedPrior::Absent)];
    let published = index.publish(change, &owners, &never());
    assert!(matches!(published, PublishOutcome::Published { .. }));

    let mut empty = lookup("absent-name", 10, 10);
    empty.scope = Some(scope);
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
            assert!(matches!(outcome, PublishOutcome::Published { .. }));
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
    let mut rows = draft(unit, "v1", &[]);
    rows.key.schema.table = TableKind::ProjectMemberships;
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
    assert!(matches!(outcome, PublishOutcome::Published { .. }));
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

fn upsert_file(host: &VerterHost, id: &str, source: &str, language: FileLanguage) {
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: None,
            input_id: id.to_string(),
            source: Arc::from(source),
            file_language: language,
            aliases: Vec::new(),
        })
        .expect("upsert");
}

const WIDE: QueryBudgets = QueryBudgets {
    postings: 1000,
    records: 1000,
};

fn vue_source(variable: &str) -> String {
    format!(
        "<script setup>\nconst el = document.body;\nel.style.setProperty('{variable}', 'red');\n</script>\n<template><div :style=\"{{ '{variable}': 'blue' }}\">A</div></template>\n<style>\n:root {{ {variable}: blue; }}\n.a {{ color: var({variable}); }}\n</style>"
    )
}

const SVELTE_SOURCE: &str =
    "<script>\nlet a = 1;\n</script>\n<div>{a}</div>\n<style>\n:global(html) { --brand: blue; }\n.b { color: var(--brand); }\n</style>";

/// Everything an answer claims about a candidate, in answer order.
fn fingerprint(page: &CandidatePage) -> Vec<String> {
    page.candidates
        .iter()
        .map(|candidate| {
            let record = candidate.record().expect("candidate record");
            format!(
                "{}|{:?}|{:?}|{:?}|{:?}|{:?}",
                candidate.contribution.basis.locator,
                record.slot,
                record.subject,
                record.row,
                record.origins,
                record.quality
            )
        })
        .collect()
}

fn css_keys(name: &str) -> Lookup {
    Lookup::Keys(vec![PostingKey::exact(TableKind::StylesheetSymbols, name)])
}

fn css_demand() -> IndexDemand {
    IndexDemand::of([IndexDemandPartition::CssVariables])
}

#[test]
fn css_var_flow_reports_each_surface_across_vue_and_svelte() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.svelte",
        SVELTE_SOURCE,
        FileLanguage::svelte(),
    );

    let flow = host.css_var_flow("--brand", None);
    let paths = |list: &[Arc<std::path::Path>]| {
        let mut paths: Vec<String> = list.iter().map(|p| p.display().to_string()).collect();
        paths.sort();
        paths
    };
    assert_eq!(
        paths(&flow.style_definitions),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(
        paths(&flow.style_var_usages),
        ["/src/A.vue", "/src/B.svelte"]
    );
    assert_eq!(paths(&flow.template_definitions), ["/src/A.vue"]);
    assert_eq!(paths(&flow.script_manipulations), ["/src/A.vue"]);
}

#[test]
fn incremental_answers_equal_a_fresh_host_across_edit_revert_add_and_delete() {
    let incremental = VerterHost::new_standalone(HostConfig::default());
    let mut files: BTreeMap<&str, String> = BTreeMap::new();
    let vue = |host: &VerterHost, id: &str, source: &str| {
        upsert_file(host, id, source, FileLanguage::vue());
    };

    let assert_equal = |incremental: &VerterHost, files: &BTreeMap<&str, String>, step: &str| {
        let fresh = VerterHost::new_standalone(HostConfig::default());
        // Reverse load order: answers must not depend on it.
        for (id, source) in files.iter().rev() {
            vue(&fresh, id, source);
        }
        for name in ["--brand", "--other"] {
            let demand = css_demand();
            let (live, _) =
                incremental.workspace_index_candidates(&demand, css_keys(name), WIDE, &never());
            let (clean, _) =
                fresh.workspace_index_candidates(&demand, css_keys(name), WIDE, &never());
            assert_eq!(fingerprint(&live), fingerprint(&clean), "{step}: {name}");
            assert_eq!(live.status, clean.status, "{step}: {name}");
            assert_eq!(live.status, EnumerationStatus::Complete, "{step}: {name}");
        }
    };

    files.insert("/src/A.vue", vue_source("--brand"));
    files.insert("/src/B.vue", vue_source("--other"));
    for (id, source) in &files {
        vue(&incremental, id, source);
    }
    assert_equal(&incremental, &files, "initial");

    files.insert("/src/A.vue", vue_source("--other"));
    vue(&incremental, "/src/A.vue", &vue_source("--other"));
    assert_equal(&incremental, &files, "edit");

    files.insert("/src/A.vue", vue_source("--brand"));
    vue(&incremental, "/src/A.vue", &vue_source("--brand"));
    assert_equal(&incremental, &files, "revert");

    files.insert("/src/C.vue", vue_source("--brand"));
    vue(&incremental, "/src/C.vue", &vue_source("--brand"));
    assert_equal(&incremental, &files, "add");

    files.remove("/src/C.vue");
    incremental.remove("/src/C.vue");
    assert_equal(&incremental, &files, "delete");
}

#[test]
fn a_warm_request_visits_no_file_and_an_edit_projects_only_that_file() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue", "/src/C.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let (_, cold) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(cold.projections, 3);

    let (warm_page, warm) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(
        warm,
        IndexReconcileWork::default(),
        "warm does no file work"
    );
    assert_eq!(warm_page.status, EnumerationStatus::Complete);

    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let (_, edited) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(edited.projections, 1, "only the edited file");
    assert_eq!(edited.unchanged, 2);
    assert_eq!(edited.population_checks, 3, "the witness read is counted");
}

#[test]
fn growing_unrelated_files_adds_no_candidate_visits() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (small, _) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    for file in 0..8 {
        let id = format!("/src/Unrelated{file}.vue");
        upsert_file(&host, &id, &vue_source("--other"), FileLanguage::vue());
    }
    let (grown, reconcile) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(grown.work, small.work, "lookups and visits stay fixed");
    assert_eq!(reconcile.projections, 8, "each new file is projected once");
}

#[test]
fn an_undemanded_partition_is_never_projected() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let (snapshot, status, work) = host.workspace_index_reconcile(&css_demand(), &never());
    assert_eq!(status, ReconcileStatus::Current);
    assert_eq!(work.projections, 1);
    assert_eq!(snapshot.contribution_count(), 1);
    for partition in ["bindings", "components", "imports", "style-selectors"] {
        let id = PartitionId(Arc::from(partition));
        assert_eq!(
            snapshot.contributions_in_partition(&id).count(),
            0,
            "{partition}"
        );
    }
}

#[test]
fn a_cancelled_reconcile_warms_nothing_and_a_retry_equals_fresh() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let demand = css_demand();
    let calls = Cell::new(0);
    let cancel_mid_walk = || {
        calls.set(calls.get() + 1);
        calls.get() == 2
    };
    let (page, _) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &cancel_mid_walk);
    assert_eq!(page.status, EnumerationStatus::Cancelled);
    assert!(page.candidates.is_empty());
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(
        store.contribution_count(),
        0,
        "no staged prefix was published"
    );
    assert!(store.coverage(&demand.scope()).is_none());

    let (retry, retried) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(retry.status, EnumerationStatus::Complete);
    assert_eq!(retried.projections, 2);

    let fresh = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/B.vue", "/src/A.vue"] {
        upsert_file(&fresh, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let (clean, _) = fresh.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(fingerprint(&retry), fingerprint(&clean));
}

#[test]
fn a_host_budget_shortfall_is_partial_and_certifies_nothing_about_the_rest() {
    let host = VerterHost::new_standalone(HostConfig::default());
    for id in ["/src/A.vue", "/src/B.vue"] {
        upsert_file(&host, id, &vue_source("--brand"), FileLanguage::vue());
    }
    let tight = QueryBudgets {
        postings: 1,
        records: 1,
    };
    let answer = host.css_var_candidates("--brand", tight, &never());
    assert_eq!(answer.page.candidates.len(), 1);
    assert!(matches!(
        answer.page.status,
        EnumerationStatus::Partial {
            cause: PartialCause::BudgetExhausted,
            continuation: Some(_),
            ..
        }
    ));
}

#[test]
fn same_basenames_stay_distinct_candidates_and_load_order_changes_nothing() {
    let sources = [
        (
            "/a/Card.vue",
            "<template><Badge /><Badge /></template>",
            FileLanguage::vue(),
        ),
        ("/b/Card.svelte", "<div>card</div>", FileLanguage::svelte()),
        (
            "/c/Badge.vue",
            "<template><div /></template>",
            FileLanguage::vue(),
        ),
    ];
    let demand = IndexDemand::of([IndexDemandPartition::Components]);
    let answer = |order: &[usize], name: &str| {
        let host = VerterHost::new_standalone(HostConfig::default());
        for &position in order {
            let (id, source, language) = &sources[position];
            upsert_file(&host, id, source, language.clone());
        }
        let keys = Lookup::Keys(vec![PostingKey::exact(TableKind::Components, name)]);
        host.workspace_index_candidates(&demand, keys, WIDE, &never())
            .0
    };

    let cards = answer(&[0, 1, 2], "Card");
    assert_eq!(cards.status, EnumerationStatus::Complete);
    let locators: BTreeSet<&str> = cards
        .candidates
        .iter()
        .map(|candidate| candidate.contribution.basis.locator.as_ref())
        .collect();
    assert_eq!(
        locators,
        ["/a/Card.vue", "/b/Card.svelte"].into_iter().collect(),
        "one basename across carriers stays two component subjects"
    );
    assert!(cards.candidates.iter().all(|candidate| matches!(
        candidate.record().expect("record").row,
        IndexRow::Component(ComponentRow::View { .. })
    )));
    assert_eq!(
        fingerprint(&cards),
        fingerprint(&answer(&[2, 1, 0], "Card"))
    );

    // Two uses of one tag are two occurrences, each still an unresolved
    // candidate plus the view of the file with that basename. Choosing the
    // target stays with the owner.
    let badges = answer(&[1, 2, 0], "Badge");
    let usages = badges
        .candidates
        .iter()
        .filter(|candidate| {
            let record = candidate.record().expect("record");
            matches!(
                &record.row,
                IndexRow::Component(ComponentRow::Usage {
                    target: Endpoint::Unresolved { .. },
                    ..
                })
            ) && record.quality == FactQuality::Unresolved
        })
        .count();
    assert_eq!(usages, 2);
    assert_eq!(badges.candidates.len(), 3);
}

#[test]
fn name_search_returns_similarly_named_candidates_without_choosing_a_target() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = |name: &str| format!("<script setup>\nconst {name} = 1;\n</script>");
    upsert_file(&host, "/src/A.vue", &source("Total"), FileLanguage::vue());
    upsert_file(
        &host,
        "/src/B.svelte",
        &source("total"),
        FileLanguage::svelte(),
    );
    upsert_file(
        &host,
        "/src/C.vue",
        &source("subtotal"),
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::Bindings]);
    let search = Lookup::Prefix {
        table: TableKind::Names,
        family: KeyFamily::Lexical,
        version: LEXICAL_PROJECTION_VERSION,
        prefix: Arc::from("total"),
    };
    let (page, _) = host.workspace_index_candidates(&demand, search, WIDE, &never());
    let mut spellings: Vec<String> = page
        .candidates
        .iter()
        .filter_map(|candidate| match &candidate.record()?.row {
            IndexRow::Name(row) => Some(row.spelling.to_string()),
            _ => None,
        })
        .collect();
    spellings.sort();
    assert_eq!(
        spellings,
        ["Total", "total"],
        "both stay candidates; prefix excludes subtotal"
    );
    assert_eq!(page.status, EnumerationStatus::Complete);
}

#[test]
fn imports_stay_explicitly_unresolved_with_their_cause() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<script setup>\nimport x from './missing';\nconsole.log(x);\n</script>",
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::Imports]);
    let keys = Lookup::Keys(vec![PostingKey::reverse(TableKind::Links, "./missing")]);
    let (page, _) = host.workspace_index_candidates(&demand, keys, WIDE, &never());
    let [candidate] = page.candidates.as_slice() else {
        panic!("one import edge, got {}", page.candidates.len());
    };
    let record = candidate.record().expect("record");
    assert_eq!(record.quality, FactQuality::Unresolved);
    let IndexRow::Link(link) = &record.row else {
        panic!("a link row");
    };
    assert_eq!(link.role, LinkRole::Unresolved);
    assert!(link.resolution.is_none());
    assert!(matches!(
        link.target,
        Endpoint::Unresolved {
            cause: FactCause::SpecifierUnresolved,
            ..
        }
    ));
}

#[test]
fn stylesheet_scopes_stay_producer_facts_across_vue_and_svelte() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<template><div /></template>\n<style scoped>\n.same { color: red; }\n</style>\n<style module>\n.same { color: red; }\n</style>\n<style>\n.same { color: red; }\n</style>",
        FileLanguage::vue(),
    );
    let demand = IndexDemand::of([IndexDemandPartition::StyleSelectors]);
    let (page, _) = host.workspace_index_candidates(&demand, css_keys("same"), WIDE, &never());
    let mut by_block: Vec<(u32, StyleScope)> = page
        .candidates
        .iter()
        .filter_map(|candidate| match &candidate.record()?.row {
            IndexRow::Stylesheet(StylesheetRow {
                surface: StyleSurface::Style { block },
                scope,
                ..
            }) => Some((*block, *scope)),
            _ => None,
        })
        .collect();
    by_block.sort_by_key(|(block, _)| *block);
    assert_eq!(
        by_block,
        [
            (0, StyleScope::Scoped),
            (1, StyleScope::Module),
            (2, StyleScope::Global)
        ]
    );
}

#[test]
fn origins_are_exact_authored_bytes_past_non_ascii_text_and_explicit_when_absent() {
    let host = VerterHost::new_standalone(HostConfig::default());
    let source = "<template><div :style=\"{ '--brand': 'x' }\" /></template>\n<style>\n/* héllo 日本語 */\n:root { --brand: blue; }\n</style>";
    upsert_file(&host, "/src/A.vue", source, FileLanguage::vue());
    let (page, _) =
        host.workspace_index_candidates(&css_demand(), css_keys("--brand"), WIDE, &never());
    let mut style_origin = None;
    let mut template_origin = None;
    for candidate in &page.candidates {
        let record = candidate.record().expect("record");
        let IndexRow::Stylesheet(row) = &record.row else {
            continue;
        };
        match row.surface {
            StyleSurface::Style { .. } => style_origin = Some(record.origins.clone()),
            StyleSurface::Template => template_origin = Some(record.origins.clone()),
            StyleSurface::Script => {}
        }
    }
    let style_origin = style_origin.expect("style definition");
    let [RecordOrigin::Authored(anchor)] = style_origin.as_slice() else {
        panic!("expected one authored origin, got {style_origin:?}");
    };
    let start = usize::try_from(anchor.range.start.get()).unwrap();
    let end = usize::try_from(anchor.range.end.get()).unwrap();
    assert_eq!(&source[start..end], "--brand");
    assert_eq!(
        anchor.content,
        ContentId::from_content_bytes(source.as_bytes())
    );
    assert_eq!(
        template_origin.expect("template binding"),
        vec![RecordOrigin::Unavailable(
            OriginUnavailable::OwnerRecordsNoSpan
        )],
        "no fabricated zero range"
    );
}

#[test]
fn typescript_files_join_the_certified_population() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/theme.ts",
        "document.body.style.setProperty('--brand', 'red');\n",
        FileLanguage::script_ts(),
    );
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let answer = host.css_var_candidates("--brand", WIDE, &never());
    assert_eq!(answer.page.status, EnumerationStatus::Complete);
    assert_eq!(answer.flow.script_manipulations.len(), 1);
    let again = host.css_var_candidates("--brand", WIDE, &never());
    assert_eq!(again.work, IndexReconcileWork::default());
}

#[test]
fn deleting_a_file_retires_its_slots_and_changing_its_language_retires_the_old_claim() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (first, _) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    let per_file = first.candidates.len() / 2;

    host.remove("/src/B.vue");
    let (after, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(work.retired, 1);
    assert_eq!(after.status, EnumerationStatus::Complete);
    assert_eq!(after.candidates.len(), per_file);
    assert!(after
        .candidates
        .iter()
        .all(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue"));

    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::svelte(),
    );
    let (moved, moved_work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(moved_work.retired, 1, "the old claim's slot leaves");
    assert_eq!(moved.status, EnumerationStatus::Complete);
    let snapshot = host.project_type_store().workspace_index().snapshot();
    assert_eq!(
        snapshot.contribution_count(),
        1,
        "no parallel claim stays current"
    );
}

#[test]
fn a_file_edited_during_the_walk_cannot_be_certified_and_the_retry_sees_the_edit() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    upsert_file(
        &host,
        "/src/B.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (first, _) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert_eq!(first.status, EnumerationStatus::Complete);

    // Force a second walk, then edit a file the walk has already read as
    // unchanged. The edit lands after capture and before publication.
    upsert_file(
        &host,
        "/src/C.vue",
        &vue_source("--other"),
        FileLanguage::vue(),
    );
    let edited = Cell::new(false);
    let edit_mid_walk = || {
        if !edited.replace(true) {
            upsert_file(
                &host,
                "/src/B.vue",
                &vue_source("--other"),
                FileLanguage::vue(),
            );
        }
        false
    };
    let (page, work) =
        host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &edit_mid_walk);
    assert_eq!(
        work.publish_attempts, 2,
        "the fence refused the first attempt"
    );
    assert_eq!(page.status, EnumerationStatus::Complete);
    assert!(
        page.candidates
            .iter()
            .all(|candidate| candidate.contribution.basis.locator.as_ref() == "/src/A.vue"),
        "the edited file no longer defines the variable"
    );
}

#[test]
fn a_multi_partition_demand_certifies_its_own_population_and_a_narrower_demand_stays_separate() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let both = IndexDemand::of([
        IndexDemandPartition::CssVariables,
        IndexDemandPartition::StyleSelectors,
    ]);
    assert_ne!(both.scope(), css_demand().scope());

    let (variables, _) =
        host.workspace_index_candidates(&both, css_keys("--brand"), WIDE, &never());
    let (classes, work) = host.workspace_index_candidates(&both, css_keys("a"), WIDE, &never());
    assert_eq!(variables.status, EnumerationStatus::Complete);
    assert_eq!(classes.status, EnumerationStatus::Complete);
    assert_eq!(classes.candidates.len(), 1);
    assert_eq!(
        work,
        IndexReconcileWork::default(),
        "the second read is warm"
    );

    // The narrower demand reuses the slot but proves its own population once.
    let (narrow, narrow_work) =
        host.workspace_index_candidates(&css_demand(), css_keys("--brand"), WIDE, &never());
    assert_eq!(narrow_work.projections, 0);
    assert_eq!(narrow_work.unchanged, 1);
    assert_eq!(fingerprint(&narrow), fingerprint(&variables));
}

#[test]
fn closing_the_host_releases_the_index() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        &vue_source("--brand"),
        FileLanguage::vue(),
    );
    let demand = css_demand();
    let (open, _) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert!(!open.candidates.is_empty());
    drop(open);

    host.close();
    let store = host.project_type_store().workspace_index().snapshot();
    assert_eq!(store.contribution_count(), 0);
    assert_eq!(store.posting_count(), 0);
    let (closed, _) = host.workspace_index_candidates(&demand, css_keys("--brand"), WIDE, &never());
    assert!(closed.candidates.is_empty());
    assert_eq!(closed.status, EnumerationStatus::Complete);
}

#[test]
fn style_definitions_count_each_block_once_however_many_occurrences_it_holds() {
    let host = VerterHost::new_standalone(HostConfig::default());
    upsert_file(
        &host,
        "/src/A.vue",
        "<template><div /></template>
<style>
:root { --x: 1; }
.a { --x: 2; color: var(--x); background: var(--x); }
</style>
<style>
.b { --x: 3; }
</style>",
        FileLanguage::vue(),
    );
    let flow = host.css_var_flow("--x", None);
    assert_eq!(
        flow.style_definitions.len(),
        2,
        "one path per defining block"
    );
    assert_eq!(flow.style_var_usages.len(), 1, "one path per using block");
}
