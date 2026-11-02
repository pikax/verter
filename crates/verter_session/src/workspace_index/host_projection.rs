//! Projects the host's cached per-file facts into the index on demand.
//!
//! Only demanded partitions are projected, and only for files whose source or
//! project generation changed since the slot was written. A warm request
//! touches no file. A request after any workspace change compares each tracked
//! file's content hash once, which [`IndexReconcileWork::population_checks`]
//! counts, because the host's population witness is read-side authoritative
//! rather than fed by mutation hooks. All facts come through the same shared
//! cached accessors every other consumer uses: nothing here parses a file or
//! branches on a framework.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, QueryIdentity, ResultContractId, SourceId, SourceRevision, SourceUnitId,
};
use verter_identity::successor::{CarrierProfileId, ProfileSchemaEpoch};
use verter_language::FileLanguage;
use verter_span::{SourceByteOffset, SourceByteRange, Span};

use super::delta::{
    ContributionDelta, ExpectedPrior, ObservationOracle, PartialCause, PartialEnumeration,
    PartitionDraft, PublishOutcome, PublishRefusal, RetirementEvidence, SchemaAdmission,
    SlotChange,
};
use super::identity::{
    CatalogRevisionId, ContributionId, ContributionKey, DemandEpoch, ObservationKey,
    ObservationOwner, ObservedFact, OwnerSnapshotId, PartitionId, ProducerDescriptorId,
    ProfileClaim, ReadSet, ReadWitness, ScopeId, SelectionEpoch, SourceBasis, TableKind,
    TableSchema,
};
use super::query::{
    query_candidates, CandidatePage, EnumerationStatus, IndexQuery, Lookup, QueryBudgets,
};
use super::records::{
    ComponentRow, Endpoint, FactCause, FactQuality, IndexRecord, IndexRow, LinkRelation, LinkRole,
    LinkRow, NameRole, NameRow, OriginUnavailable, PostingKey, Provenance, RecordOrigin,
    RecordSlot, SlotKind, SourceAnchor, StyleRole, StyleScope, StyleSurface, StyleSymbolKind,
    StylesheetRow, TypedSubject,
};
use super::snapshot::{CoverageRecord, IndexSnapshot};
use crate::VerterHost;

/// A publication attempt that lost to a concurrent change replans on a fresh
/// view at most this many times before answering return-only.
const MAX_PUBLISH_ATTEMPTS: u32 = 3;

const SCHEMA_VERSION: u32 = 1;

/// A fact family the host can project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndexDemandPartition {
    /// CSS custom property definitions, `var()` references, template style
    /// bindings and script manipulations.
    CssVariables,
    /// Stylesheet classes and IDs with their scope.
    StyleSelectors,
    /// Script declarations and imported local names.
    Bindings,
    /// Component views and template component uses.
    Components,
    /// Authored import relations.
    Imports,
}

impl IndexDemandPartition {
    fn table(self) -> TableKind {
        match self {
            Self::CssVariables | Self::StyleSelectors => TableKind::StylesheetSymbols,
            Self::Bindings => TableKind::Names,
            Self::Components => TableKind::Components,
            Self::Imports => TableKind::Links,
        }
    }

    fn id(self) -> PartitionId {
        PartitionId(Arc::from(match self {
            Self::CssVariables => "css-variables",
            Self::StyleSelectors => "style-selectors",
            Self::Bindings => "bindings",
            Self::Components => "components",
            Self::Imports => "imports",
        }))
    }

    /// A component view exists only for a component carrier. Other files have
    /// no such slot: they are outside this partition, not empty in it.
    fn applies_to(self, language: &FileLanguage) -> bool {
        self != Self::Components || language.is_framework_carrier()
    }
}

/// One explicit request for index facts. Undemanded partitions run nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexDemand {
    pub partitions: BTreeSet<IndexDemandPartition>,
}

impl IndexDemand {
    #[must_use]
    pub fn of(partitions: impl IntoIterator<Item = IndexDemandPartition>) -> Self {
        Self {
            partitions: partitions.into_iter().collect(),
        }
    }

    /// The population a complete walk of this demand certifies.
    #[must_use]
    pub fn scope(&self) -> ScopeId {
        ScopeId::from_canonical(self)
    }
}

impl CanonicalEncode for IndexDemand {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_scope.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u32(1, SCHEMA_VERSION);
        for partition in &self.partitions {
            encoder.field_str(2, &partition.id().0);
        }
    }
}

/// Counted work of one reconcile. Deterministic, so a test can assert it
/// exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexReconcileWork {
    /// Tracked-file content comparisons. Zero on a warm request.
    pub population_checks: u32,
    /// Slots whose facts were read and projected.
    pub projections: u32,
    /// Slots that needed no change.
    pub unchanged: u32,
    /// Slots retired by a deletion or claim change.
    pub retired: u32,
    /// Publications attempted.
    pub publish_attempts: u32,
}

/// How far the current snapshot answers a demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileStatus {
    /// Coverage for the demanded population is published and current.
    Current,
    /// The request is answered from private rows. They warm nothing.
    ReturnOnly(PartialCause),
    /// Nothing is answered or kept.
    Cancelled,
}

/// The candidates of one custom property, with the facts of their walk.
#[derive(Debug, Clone)]
pub struct CssVarCandidates {
    pub flow: verter_semantic::analysis::CssVarFlow,
    pub page: CandidatePage,
    pub work: IndexReconcileWork,
}

fn producer() -> &'static ProducerDescriptorId {
    static PRODUCER: OnceLock<ProducerDescriptorId> = OnceLock::new();
    PRODUCER.get_or_init(|| ProducerDescriptorId::from_canonical(&Named("host-file-facts")))
}

fn result_contract() -> &'static ResultContractId {
    static CONTRACT: OnceLock<ResultContractId> = OnceLock::new();
    CONTRACT.get_or_init(|| ResultContractId::from_canonical(&Named("host-file-facts-rows")))
}

fn catalog() -> &'static CatalogRevisionId {
    static CATALOG: OnceLock<CatalogRevisionId> = OnceLock::new();
    CATALOG.get_or_init(|| CatalogRevisionId::from_canonical(&Named("host-file-facts-catalog")))
}

/// Registered rows of the tables the host writes.
fn admission() -> &'static SchemaAdmission {
    static ADMISSION: OnceLock<SchemaAdmission> = OnceLock::new();
    ADMISSION.get_or_init(|| SchemaAdmission {
        table_schemas: [
            TableKind::StylesheetSymbols,
            TableKind::Names,
            TableKind::Components,
            TableKind::Links,
        ]
        .into_iter()
        .map(|table| TableSchema {
            table,
            version: SCHEMA_VERSION,
        })
        .collect(),
        ..SchemaAdmission::default()
    })
}

struct Named(&'static str);

impl CanonicalEncode for Named {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_descriptor.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

struct SourceDescriptor<'a>(&'a str);

impl CanonicalEncode for SourceDescriptor<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_source.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, self.0);
    }
}

struct RevisionDescriptor {
    whole_hash: [u8; 16],
    project_generation: u64,
}

impl CanonicalEncode for RevisionDescriptor {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_revision.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, &self.whole_hash);
        encoder.field_u64(2, self.project_generation);
    }
}

/// The unit of one host file. Lineage is the canonical id, so an edit keeps it.
fn host_unit(canonical: &str) -> SourceUnitId {
    SourceUnitId::from_lineage(
        &SourceId::from_canonical(&SourceDescriptor(canonical)),
        "file",
    )
}

fn source_key(unit: &SourceUnitId) -> ObservationKey {
    ObservationKey {
        owner: ObservationOwner::Source,
        subject: unit.digest(),
    }
}

fn absent_source_fingerprint() -> CanonicalDigest {
    CanonicalDigest::of_bytes(b"verter.session.workspace_index.source_absent.v1")
}

fn activation_key() -> ObservationKey {
    ObservationKey {
        owner: ObservationOwner::Activation,
        subject: CanonicalDigest::of_bytes(b"verter.session.workspace_index.project_generation.v1"),
    }
}

fn generation_fingerprint(project_generation: u64) -> CanonicalDigest {
    CanonicalDigest::of_bytes(&project_generation.to_le_bytes())
}

fn population_key(scope: &ScopeId) -> ObservationKey {
    ObservationKey {
        owner: ObservationOwner::Population,
        subject: scope.digest(),
    }
}

/// Carrier profile of a file. Facts here come from the carrier and its shared
/// analysis, so this claims the carrier and never fabricates a framework.
fn carrier_claim(language: &FileLanguage) -> ProfileClaim {
    // The whole language row: a script dialect, adapter module or template
    // language parses differently under the same bytes.
    let family = format!("{language:?}");
    let schema = ProfileSchemaEpoch::for_schema_family("host-carrier", SCHEMA_VERSION);
    ProfileClaim::Carrier(CarrierProfileId::from_carrier(&family, &schema))
}

/// One tracked file as captured for a walk.
struct TrackedFile {
    canonical: Arc<str>,
    unit: SourceUnitId,
    language: FileLanguage,
    claim: ProfileClaim,
    whole_hash: [u8; 16],
    generation: u64,
    source: Arc<str>,
    revision: SourceRevision,
}

/// The owners' live answers for one walk's fence.
struct HostObservations<'host> {
    host: &'host VerterHost,
    canonical_by_unit: BTreeMap<CanonicalDigest, Arc<str>>,
    scope: ScopeId,
}

impl HostObservations<'_> {
    fn live_population(&self) -> CanonicalDigest {
        let mut files = tracked_ids(self.host);
        files.sort();
        population_fingerprint(files.iter().map(|canonical| {
            let source = self.host.scheduler.try_get_source(canonical);
            (canonical.as_str(), source.map(|source| source.whole_hash))
        }))
    }
}

/// The population witness: every tracked file and its content, in canonical
/// order. Adding, removing or editing any file changes it.
fn population_fingerprint<'a>(
    files: impl Iterator<Item = (&'a str, Option<[u8; 16]>)>,
) -> CanonicalDigest {
    let mut encoder = CanonicalEncoder::new("verter.session.workspace_index.population.v1");
    for (canonical, whole_hash) in files {
        encoder.field_str(1, canonical);
        if let Some(whole_hash) = whole_hash {
            encoder.field_bytes(2, &whole_hash);
        }
    }
    encoder.digest()
}

impl ObservationOracle for HostObservations<'_> {
    fn current(&self, key: &ObservationKey) -> Option<CanonicalDigest> {
        match key.owner {
            ObservationOwner::Source => {
                let canonical = self.canonical_by_unit.get(&key.subject)?;
                if self.host.is_canonical_evicted(canonical) {
                    return Some(absent_source_fingerprint());
                }
                Some(
                    self.host
                        .scheduler
                        .try_get_source(canonical)
                        .map_or_else(absent_source_fingerprint, |source| {
                            CanonicalDigest::of_bytes(&source.whole_hash)
                        }),
                )
            }
            ObservationOwner::Activation => Some(generation_fingerprint(
                self.host.project_type_store().project_generation(),
            )),
            ObservationOwner::Population if key.subject == self.scope.digest() => {
                Some(self.live_population())
            }
            _ => None,
        }
    }

    fn current_epochs(&self) -> (SelectionEpoch, DemandEpoch) {
        // A direct host request is one fixed demand with no plan epoch, so
        // selection supersession is the project generation alone.
        let live = self.host.project_type_store().project_generation();
        (SelectionEpoch(live), DemandEpoch(0))
    }
}

fn tracked_ids(host: &VerterHost) -> Vec<String> {
    host.scheduler
        .node_ids()
        .into_iter()
        .filter(|id| !host.is_canonical_evicted(id))
        .collect()
}

/// Rows and origins under one file's basis.
struct RowBuilder<'a> {
    file: &'a TrackedFile,
    content: ContentId,
    records: Vec<IndexRecord>,
}

impl RowBuilder<'_> {
    fn origin(&self, span: Span) -> RecordOrigin {
        let range = SourceByteRange::checked_new(
            SourceByteOffset::new(span.start),
            SourceByteOffset::new(span.end),
        )
        .and_then(|range| range.validate(&self.file.source));
        match range {
            Ok(range) => RecordOrigin::Authored(SourceAnchor {
                unit: self.file.unit.clone(),
                revision: self.file.revision.clone(),
                content: self.content.clone(),
                range,
            }),
            Err(_) => RecordOrigin::Unavailable(OriginUnavailable::InvalidOwnerSpan),
        }
    }

    fn push(
        &mut self,
        kind: SlotKind,
        subject: TypedSubject,
        keys: Vec<PostingKey>,
        row: IndexRow,
        origin: RecordOrigin,
        quality: FactQuality,
    ) {
        let ordinal = u32::try_from(self.records.len()).unwrap_or(u32::MAX);
        self.records.push(IndexRecord {
            slot: RecordSlot { kind, ordinal },
            subject,
            keys,
            row,
            origins: vec![origin],
            quality,
            causes: Vec::new(),
        });
    }

    fn push_style(
        &mut self,
        kind: StyleSymbolKind,
        name: &str,
        role: StyleRole,
        surface: StyleSurface,
        scope: StyleScope,
        origin: RecordOrigin,
    ) {
        let table = TableKind::StylesheetSymbols;
        let name: Arc<str> = Arc::from(name);
        self.push(
            SlotKind::Occurrence,
            TypedSubject::StyleSymbol {
                kind,
                name: Arc::clone(&name),
            },
            vec![
                PostingKey::exact(table, &name),
                PostingKey::lexical(table, &name),
            ],
            IndexRow::Stylesheet(StylesheetRow {
                kind,
                name,
                role,
                surface,
                scope,
            }),
            origin,
            FactQuality::Exact,
        );
    }
}

/// Facts the owner could not supply for a walk.
struct MissingFacts;

impl VerterHost {
    /// Read `partition`'s owner facts for one file.
    ///
    /// Reads are the shared cached accessors, so a demanded carrier is parsed
    /// and analyzed once however many partitions read it.
    fn project_partition(
        &self,
        partition: IndexDemandPartition,
        rows: &mut RowBuilder<'_>,
    ) -> Result<(), MissingFacts> {
        match partition {
            IndexDemandPartition::CssVariables => self.project_css_variables(rows),
            IndexDemandPartition::StyleSelectors => self.project_style_selectors(rows),
            IndexDemandPartition::Bindings => self.project_bindings(rows),
            IndexDemandPartition::Components => self.project_components(rows),
            IndexDemandPartition::Imports => self.project_imports(rows),
        }
    }

    fn project_css_variables(&self, rows: &mut RowBuilder<'_>) -> Result<(), MissingFacts> {
        let canonical = rows.file.canonical.clone();
        let styles = self
            .effective_style_analyses(&canonical, None)
            .ok_or(MissingFacts)?;
        for (block, style) in styles.iter().enumerate() {
            let Some(css) = style.css.as_ref() else {
                continue;
            };
            let surface = StyleSurface::Style {
                block: u32::try_from(block).unwrap_or(u32::MAX),
            };
            let scope = block_scope(style.scoped, style.is_module);
            for property in &css.custom_properties {
                let origin = rows.origin(property.name_span);
                rows.push_style(
                    StyleSymbolKind::CustomProperty,
                    &property.name,
                    StyleRole::Definition,
                    surface,
                    scope,
                    origin,
                );
            }
            for usage in &css.var_usages {
                let origin = rows.origin(usage.reference.name_span);
                rows.push_style(
                    StyleSymbolKind::CustomProperty,
                    &usage.reference.name,
                    StyleRole::Reference,
                    surface,
                    scope,
                    origin,
                );
            }
        }
        if let Some(template) = self.raw_template_analysis_for_file(&canonical) {
            for name in &template.css_var_names {
                rows.push_style(
                    StyleSymbolKind::CustomProperty,
                    name,
                    StyleRole::Definition,
                    StyleSurface::Template,
                    StyleScope::NotApplicable,
                    RecordOrigin::Unavailable(OriginUnavailable::OwnerRecordsNoSpan),
                );
            }
        }
        let state = self
            .effective_file_state(&canonical, None)
            .ok_or(MissingFacts)?;
        for manipulation in state.script_analysis.css_var_manipulations.iter() {
            let origin = rows.origin(manipulation.span);
            rows.push_style(
                StyleSymbolKind::CustomProperty,
                &manipulation.var_name,
                StyleRole::Manipulation,
                StyleSurface::Script,
                StyleScope::NotApplicable,
                origin,
            );
        }
        Ok(())
    }

    fn project_style_selectors(&self, rows: &mut RowBuilder<'_>) -> Result<(), MissingFacts> {
        let styles = self
            .effective_style_analyses(&rows.file.canonical, None)
            .ok_or(MissingFacts)?;
        for (block, style) in styles.iter().enumerate() {
            let Some(css) = style.css.as_ref() else {
                continue;
            };
            let surface = StyleSurface::Style {
                block: u32::try_from(block).unwrap_or(u32::MAX),
            };
            let scope = block_scope(style.scoped, style.is_module);
            for class in &css.classes {
                let origin = rows.origin(class.span);
                rows.push_style(
                    StyleSymbolKind::Class,
                    &class.name,
                    StyleRole::Definition,
                    surface,
                    scope,
                    origin,
                );
            }
            for id in &css.ids {
                let origin = rows.origin(id.span);
                rows.push_style(
                    StyleSymbolKind::Id,
                    &id.name,
                    StyleRole::Definition,
                    surface,
                    scope,
                    origin,
                );
            }
        }
        Ok(())
    }

    fn project_bindings(&self, rows: &mut RowBuilder<'_>) -> Result<(), MissingFacts> {
        let analysis = self
            .get_analysis(&rows.file.canonical)
            .ok_or(MissingFacts)?;
        let unit = rows.file.unit.clone();
        let table = TableKind::Names;
        let push_name = |rows: &mut RowBuilder<'_>, name: &str, role, kind, span| {
            let spelling: Arc<str> = Arc::from(name);
            let origin = rows.origin(span);
            rows.push(
                SlotKind::Declaration,
                TypedSubject::Name {
                    unit: unit.clone(),
                    spelling: Arc::clone(&spelling),
                },
                vec![
                    PostingKey::exact(table, name),
                    PostingKey::lexical(table, name),
                ],
                IndexRow::Name(NameRow {
                    spelling,
                    role,
                    declaration_kind: kind,
                }),
                origin,
                FactQuality::Exact,
            );
        };
        for binding in &analysis.bindings {
            let kind = Some(Arc::from(format!("{:?}", binding.kind)));
            push_name(
                rows,
                &binding.name,
                NameRole::Declaration,
                kind,
                binding.span,
            );
        }
        for import in &analysis.imports {
            for binding in &import.bindings {
                push_name(rows, &binding.name, NameRole::Import, None, binding.span);
            }
        }
        Ok(())
    }

    fn project_components(&self, rows: &mut RowBuilder<'_>) -> Result<(), MissingFacts> {
        let canonical = rows.file.canonical.clone();
        let analysis = self.get_analysis(&canonical).ok_or(MissingFacts)?;
        let unit = rows.file.unit.clone();
        let table = TableKind::Components;

        // The basename is a candidate key and a display label, never identity.
        let filename = canonical.rsplit('/').next().unwrap_or(&canonical);
        let stem = verter_session_query::resolution::strip_carrier_extension(filename);
        let observed = [claim_digest(&rows.file.claim)];
        rows.push(
            SlotKind::Declaration,
            TypedSubject::Component {
                unit: unit.clone(),
                export: Arc::from("default"),
            },
            vec![
                PostingKey::exact(table, stem),
                PostingKey::lexical(table, stem),
            ],
            IndexRow::Component(ComponentRow::View {
                query: QueryIdentity::<ObservedFact>::compose(
                    "verter.typeinfo.component_facets.v1",
                    unit.digest(),
                    &observed,
                    result_contract(),
                ),
                native_basis: None,
                composed_children: Vec::new(),
                display: Arc::from(stem),
            }),
            RecordOrigin::Unavailable(OriginUnavailable::OwnerRecordsNoSpan),
            FactQuality::Exact,
        );

        let Some(template) = analysis.template.as_ref() else {
            return Ok(());
        };
        for usage in &template.components {
            let tag: Arc<str> = Arc::from(usage.name.as_str());
            let target = if usage.is_dynamic {
                Endpoint::Dynamic {
                    cause: FactCause::DynamicValue,
                }
            } else {
                Endpoint::Unresolved {
                    spelling: Arc::from(usage.import_source.as_deref().unwrap_or(&usage.name)),
                    cause: FactCause::SpecifierUnresolved,
                }
            };
            let origin = rows.origin(usage.span);
            rows.push(
                SlotKind::Occurrence,
                TypedSubject::Link {
                    unit: unit.clone(),
                    relation: LinkRelation::ComponentUse,
                    target: Arc::clone(&tag),
                },
                vec![
                    PostingKey::exact(table, &usage.name),
                    PostingKey::lexical(table, &usage.name),
                ],
                IndexRow::Component(ComponentRow::Usage { tag, target }),
                origin,
                FactQuality::Unresolved,
            );
        }
        Ok(())
    }

    fn project_imports(&self, rows: &mut RowBuilder<'_>) -> Result<(), MissingFacts> {
        let analysis = self
            .get_analysis(&rows.file.canonical)
            .ok_or(MissingFacts)?;
        let unit = rows.file.unit.clone();
        let table = TableKind::Links;
        for import in &analysis.imports {
            let specifier: Arc<str> = Arc::from(import.source.as_str());
            let origin = rows.origin(import.span);
            rows.push(
                SlotKind::Relation,
                TypedSubject::Link {
                    unit: unit.clone(),
                    relation: LinkRelation::Import,
                    target: Arc::clone(&specifier),
                },
                vec![
                    PostingKey::forward(table, &unit.digest().to_hex()),
                    PostingKey::reverse(table, &specifier),
                ],
                // Resolving the specifier is the project model's proof to give;
                // the edge stays explicit about not having it.
                IndexRow::Link(Box::new(LinkRow {
                    relation: LinkRelation::Import,
                    role: LinkRole::Unresolved,
                    source: Endpoint::Unit(unit.clone()),
                    target: Endpoint::Unresolved {
                        spelling: specifier,
                        cause: FactCause::SpecifierUnresolved,
                    },
                    resolution: None,
                })),
                origin,
                FactQuality::Unresolved,
            );
        }
        Ok(())
    }

    /// One read of every tracked file's identity and content hash.
    fn capture_tracked(&self, project_generation: u64) -> (Vec<TrackedFile>, Vec<Arc<str>>) {
        use crate::host_executor::HostSourceData;

        let mut ids = tracked_ids(self);
        ids.sort();
        let mut files = Vec::with_capacity(ids.len());
        let mut unavailable = Vec::new();
        for id in ids {
            let canonical: Arc<str> = Arc::from(id.as_str());
            let Some(snapshot) = self.scheduler.try_get_source(&id) else {
                unavailable.push(canonical);
                continue;
            };
            let Some(data) = snapshot.downcast_data::<HostSourceData>() else {
                unavailable.push(canonical);
                continue;
            };
            let language = data.file_language.clone();
            files.push(TrackedFile {
                unit: host_unit(&id),
                claim: carrier_claim(&language),
                language,
                whole_hash: snapshot.whole_hash,
                generation: snapshot.generation,
                source: Arc::clone(&snapshot.source),
                revision: SourceRevision::from_canonical(&RevisionDescriptor {
                    whole_hash: snapshot.whole_hash,
                    project_generation,
                }),
                canonical,
            });
        }
        (files, unavailable)
    }

    fn slot_key(file: &TrackedFile, partition: IndexDemandPartition) -> ContributionKey {
        ContributionKey {
            producer: producer().clone(),
            schema: TableSchema {
                table: partition.table(),
                version: SCHEMA_VERSION,
            },
            unit: file.unit.clone(),
            claim: file.claim.clone(),
            project: None,
            partition: partition.id(),
        }
    }

    fn draft_slot(
        &self,
        file: &TrackedFile,
        partition: IndexDemandPartition,
        project_generation: u64,
    ) -> Result<PartitionDraft, MissingFacts> {
        let mut rows = RowBuilder {
            file,
            content: ContentId::from_content_bytes(file.source.as_bytes()),
            records: Vec::new(),
        };
        self.project_partition(partition, &mut rows)?;
        let RowBuilder {
            content, records, ..
        } = rows;
        Ok(PartitionDraft {
            key: Self::slot_key(file, partition),
            basis: SourceBasis {
                unit: file.unit.clone(),
                locator: Arc::clone(&file.canonical),
                revision: file.revision.clone(),
                content,
                catalog: catalog().clone(),
                selection_epoch: SelectionEpoch(project_generation),
                demand_epoch: DemandEpoch(0),
                owner_snapshot: OwnerSnapshotId::from_canonical(&RevisionDescriptor {
                    whole_hash: file.whole_hash,
                    project_generation: file.generation,
                }),
            },
            provenance: Provenance {
                producer: producer().clone(),
                result_contract: result_contract().clone(),
                release: None,
                consumed: Vec::new(),
            },
            read_set: ReadSet::of([
                ReadWitness {
                    key: source_key(&file.unit),
                    fingerprint: CanonicalDigest::of_bytes(&file.whole_hash),
                },
                ReadWitness {
                    key: activation_key(),
                    fingerprint: generation_fingerprint(project_generation),
                },
            ]),
            records,
        })
    }

    /// Bring the index up to date for `demand` and return the snapshot that
    /// answers it.
    ///
    /// On any refusal the store keeps what it had. A complete value the store
    /// could not keep answers this request privately; a partial walk's rows
    /// always do, and neither warms anything.
    pub fn workspace_index_reconcile(
        &self,
        demand: &IndexDemand,
        is_cancelled: &dyn Fn() -> bool,
    ) -> (Arc<IndexSnapshot>, ReconcileStatus, IndexReconcileWork) {
        let index = self.project_type_store().workspace_index();
        let scope = demand.scope();
        let mut work = IndexReconcileWork::default();

        // The epochs are bumped after a mutation commits, so reading them
        // first can only make a later request re-check, never miss a change.
        let stamp = (
            self.current_store_view_epoch(),
            self.current_load_generation(),
        );
        if index.population_stamp(&scope) == Some(stamp) {
            let snapshot = index.snapshot();
            if snapshot.coverage(&scope).is_some() {
                return (snapshot, ReconcileStatus::Current, work);
            }
        }

        let mut return_only_cause = None;
        for _ in 0..MAX_PUBLISH_ATTEMPTS {
            match self.reconcile_once(demand, &scope, is_cancelled, &mut work, true) {
                Attempt::Published(snapshot) => {
                    index.record_population_stamp(scope, stamp);
                    return (snapshot, ReconcileStatus::Current, work);
                }
                Attempt::ReturnOnly(snapshot, cause) => {
                    return (snapshot, ReconcileStatus::ReturnOnly(cause), work);
                }
                Attempt::Cancelled => {
                    return (index.snapshot(), ReconcileStatus::Cancelled, work);
                }
                Attempt::Refused(PublishRefusal::RetentionPressure) => {
                    return_only_cause = Some(PartialCause::RetentionPressure);
                    break;
                }
                Attempt::Refused(PublishRefusal::Cancelled) => {
                    return (index.snapshot(), ReconcileStatus::Cancelled, work);
                }
                Attempt::Refused(_) => {}
            }
        }
        // Lost every attempt, or the account refused retention: answer from
        // private rows.
        let cause = return_only_cause.unwrap_or(PartialCause::Stale);
        match self.reconcile_once(demand, &scope, is_cancelled, &mut work, false) {
            Attempt::ReturnOnly(snapshot, partial_cause) => {
                let cause = if partial_cause == PartialCause::Stale {
                    cause
                } else {
                    partial_cause
                };
                (snapshot, ReconcileStatus::ReturnOnly(cause), work)
            }
            Attempt::Published(snapshot) => (snapshot, ReconcileStatus::ReturnOnly(cause), work),
            Attempt::Cancelled | Attempt::Refused(_) => {
                (index.snapshot(), ReconcileStatus::Cancelled, work)
            }
        }
    }

    fn reconcile_once(
        &self,
        demand: &IndexDemand,
        scope: &ScopeId,
        is_cancelled: &dyn Fn() -> bool,
        work: &mut IndexReconcileWork,
        publish: bool,
    ) -> Attempt {
        let store = self.project_type_store();
        let index = store.workspace_index();
        let account = index.account().get();
        let project_generation = store.project_generation();
        let captured = index.snapshot();
        let (files, unavailable) = self.capture_tracked(project_generation);
        work.population_checks += count_u32(files.len() + unavailable.len());
        let captured_population = captured_population(&files, &unavailable);

        let mut canonical_by_unit: BTreeMap<CanonicalDigest, Arc<str>> = files
            .iter()
            .map(|file| (file.unit.digest(), Arc::clone(&file.canonical)))
            .collect();

        let mut changes = Vec::new();
        let mut private_rows = Vec::new();
        let mut current_slots = BTreeSet::new();
        let mut gone = Vec::new();
        let mut ineligible = Vec::new();
        let mut partial_cause = (!unavailable.is_empty()).then_some(PartialCause::UnavailableInput);

        for file in &files {
            for &partition in &demand.partitions {
                if !partition.applies_to(&file.language) {
                    continue;
                }
                if is_cancelled() {
                    return Attempt::Cancelled;
                }
                let key = Self::slot_key(file, partition);
                let id = key.id();
                let held = captured.contribution(&id);
                if held.is_some_and(|held| held.basis.revision == file.revision) {
                    work.unchanged += 1;
                    current_slots.insert(id);
                    continue;
                }
                work.projections += 1;
                let Ok(draft) = self.draft_slot(file, partition, project_generation) else {
                    // An old value does not stay current because its
                    // replacement could not be built.
                    partial_cause = Some(PartialCause::UnavailableInput);
                    if held.is_some() {
                        ineligible.push(source_key(&file.unit));
                        gone.push(id);
                    }
                    continue;
                };
                if !self.source_unchanged(file) {
                    if publish {
                        return Attempt::Refused(PublishRefusal::Stale);
                    }
                    partial_cause = Some(PartialCause::Stale);
                }
                if !publish {
                    private_rows.push(PartialEnumeration::new(draft, PartialCause::Stale, account));
                    continue;
                }
                let expected = held.map_or(ExpectedPrior::Absent, |held| {
                    ExpectedPrior::Basis(Box::new(held.basis.clone()))
                });
                match draft.seal(admission(), account) {
                    Ok(partition) => {
                        current_slots.insert(id);
                        changes.push(SlotChange::Upsert {
                            partition: Box::new(partition),
                            expected,
                        });
                    }
                    Err(rejection) => {
                        verter_debug_assert::verter_debug_assert!(
                            false,
                            "a host partition failed its own registered schema: {rejection:?}"
                        );
                        partial_cause = Some(PartialCause::UnsupportedCoverage);
                    }
                }
            }
        }

        // Slots of this demand that no tracked file claims any more.
        for &partition in &demand.partitions {
            for held in captured.contributions_in_partition(&partition.id()) {
                if held.key.producer != *producer() || current_slots.contains(&held.id) {
                    continue;
                }
                let Some(fingerprint_owner) = self.live_slot_owner(&files, &held.basis) else {
                    canonical_by_unit
                        .insert(held.basis.unit.digest(), Arc::clone(&held.basis.locator));
                    gone.push(held.id.clone());
                    changes.push(retire(&held, absent_source_fingerprint()));
                    work.retired += 1;
                    continue;
                };
                // The file is tracked but this slot's claim no longer applies.
                let key_matches_claim = held.key.claim == fingerprint_owner.claim
                    && partition.applies_to(&fingerprint_owner.language);
                if !key_matches_claim {
                    gone.push(held.id.clone());
                    changes.push(retire(
                        &held,
                        CanonicalDigest::of_bytes(&fingerprint_owner.whole_hash),
                    ));
                    work.retired += 1;
                }
            }
        }

        let observations = HostObservations {
            host: self,
            canonical_by_unit,
            scope: scope.clone(),
        };

        if !publish {
            let cause = partial_cause.unwrap_or(PartialCause::Stale);
            let rows = private_rows
                .into_iter()
                .map(super::snapshot::return_only_contribution);
            return Attempt::ReturnOnly(Arc::new(captured.with_return_only(rows, &gone)), cause);
        }

        if !ineligible.is_empty() {
            index.invalidate(&ineligible);
        }

        let coverage = if partial_cause.is_none() {
            vec![CoverageRecord {
                scope: scope.clone(),
                partitions: demand.partitions.iter().map(|p| p.id()).collect(),
                witness: ReadWitness {
                    key: population_key(scope),
                    fingerprint: captured_population,
                },
                contributions: current_slots,
            }]
        } else {
            Vec::new()
        };

        let delta = ContributionDelta {
            changes,
            coverage,
            selection_epoch: SelectionEpoch(project_generation),
            demand_epoch: DemandEpoch(0),
        };
        work.publish_attempts += 1;
        match index.publish(delta, &observations, is_cancelled) {
            PublishOutcome::Published { .. } | PublishOutcome::AlreadyCurrent { .. } => {
                let snapshot = index.snapshot();
                match partial_cause {
                    None => Attempt::Published(snapshot),
                    Some(cause) => {
                        // Complete slots were kept; the rest answers privately.
                        let rows = self.private_rows_for_incomplete(demand, &files, &snapshot);
                        let private = snapshot.with_return_only(rows, &[]);
                        Attempt::ReturnOnly(Arc::new(private), cause)
                    }
                }
            }
            PublishOutcome::Refused(refusal) => Attempt::Refused(refusal),
        }
    }

    /// Current positive rows of the slots a partial walk could not publish.
    fn private_rows_for_incomplete(
        &self,
        demand: &IndexDemand,
        files: &[TrackedFile],
        snapshot: &IndexSnapshot,
    ) -> Vec<super::snapshot::Contribution> {
        let account = self.project_type_store().workspace_index().account().get();
        let generation = self.project_type_store().project_generation();
        let mut rows = Vec::new();
        for file in files {
            for &partition in &demand.partitions {
                if !partition.applies_to(&file.language) {
                    continue;
                }
                let held = snapshot.contribution(&Self::slot_key(file, partition).id());
                if held.is_some_and(|held| held.basis.revision == file.revision) {
                    continue;
                }
                if let Ok(draft) = self.draft_slot(file, partition, generation) {
                    let partial =
                        PartialEnumeration::new(draft, PartialCause::UnavailableInput, account);
                    rows.push(super::snapshot::return_only_contribution(partial));
                }
            }
        }
        rows
    }

    fn source_unchanged(&self, file: &TrackedFile) -> bool {
        self.scheduler
            .try_get_source(&file.canonical)
            .is_some_and(|source| {
                source.whole_hash == file.whole_hash && source.generation == file.generation
            })
    }

    fn live_slot_owner<'files>(
        &self,
        files: &'files [TrackedFile],
        basis: &SourceBasis,
    ) -> Option<&'files TrackedFile> {
        files
            .binary_search_by(|file| file.canonical.as_ref().cmp(basis.locator.as_ref()))
            .ok()
            .and_then(|position| files.get(position))
    }

    /// Enumerate candidates for `demand` under finite budgets.
    ///
    /// The answer is `Complete` only when the walk exhausted its keys and the
    /// demand's population is certified. Candidates are candidates: the owning
    /// operation still validates each one before a semantic answer or edit.
    pub fn workspace_index_candidates(
        &self,
        demand: &IndexDemand,
        lookup: Lookup,
        budgets: QueryBudgets,
        is_cancelled: &dyn Fn() -> bool,
    ) -> (CandidatePage, IndexReconcileWork) {
        let (snapshot, status, work) = self.workspace_index_reconcile(demand, is_cancelled);
        if status == ReconcileStatus::Cancelled {
            let page = query_candidates(
                &snapshot,
                &IndexQuery {
                    lookup,
                    budgets,
                    scope: None,
                    continuation: None,
                },
                None,
                &|| true,
            );
            return (page, work);
        }
        let query = IndexQuery {
            lookup,
            budgets,
            scope: Some(demand.scope()),
            continuation: None,
        };
        let mut page = query_candidates(&snapshot, &query, None, is_cancelled);
        if let (
            ReconcileStatus::ReturnOnly(cause),
            EnumerationStatus::Partial { cause: slot, .. },
        ) = (&status, &mut page.status)
        {
            cause.clone_into(slot);
        }
        (page, work)
    }

    /// Candidates of one custom property under finite budgets.
    pub fn css_var_candidates(
        &self,
        var_name: &str,
        budgets: QueryBudgets,
        is_cancelled: &dyn Fn() -> bool,
    ) -> CssVarCandidates {
        let demand = IndexDemand::of([IndexDemandPartition::CssVariables]);
        let key = PostingKey::exact(TableKind::StylesheetSymbols, var_name);
        let (page, work) = self.workspace_index_candidates(
            &demand,
            Lookup::Keys(vec![key]),
            budgets,
            is_cancelled,
        );
        CssVarCandidates {
            flow: css_var_flow_from_candidates(var_name, &page),
            page,
            work,
        }
    }
}

/// The population as this walk saw it. The fence compares it with the live
/// population, so a file edited after capture cannot be certified.
fn captured_population(files: &[TrackedFile], unavailable: &[Arc<str>]) -> CanonicalDigest {
    let mut entries: Vec<(&str, Option<[u8; 16]>)> = files
        .iter()
        .map(|file| (file.canonical.as_ref(), Some(file.whole_hash)))
        .chain(
            unavailable
                .iter()
                .map(|canonical| (canonical.as_ref(), None)),
        )
        .collect();
    entries.sort_by_key(|(canonical, _)| *canonical);
    population_fingerprint(entries.into_iter())
}

enum Attempt {
    Published(Arc<IndexSnapshot>),
    ReturnOnly(Arc<IndexSnapshot>, PartialCause),
    Refused(PublishRefusal),
    Cancelled,
}

fn retire(held: &super::snapshot::Contribution, fingerprint: CanonicalDigest) -> SlotChange {
    SlotChange::Retire {
        id: held.id.clone(),
        expected: Box::new(held.basis.clone()),
        evidence: RetirementEvidence {
            witness: ReadWitness {
                key: source_key(&held.basis.unit),
                fingerprint,
            },
        },
    }
}

fn block_scope(scoped: bool, is_module: bool) -> StyleScope {
    if is_module {
        StyleScope::Module
    } else if scoped {
        StyleScope::Scoped
    } else {
        StyleScope::Global
    }
}

fn claim_digest(claim: &ProfileClaim) -> CanonicalDigest {
    match claim {
        ProfileClaim::Carrier(id) => id.digest(),
        ProfileClaim::Framework { profile, .. } => profile.digest(),
        ProfileClaim::Project(id) => id.digest(),
    }
}

fn count_u32(count: usize) -> u32 {
    u32::try_from(count).unwrap_or(u32::MAX)
}

/// Project candidates onto the public flow shape: one path per style block
/// that defines or uses the variable, and one per file for a template binding
/// or a script manipulation.
fn css_var_flow_from_candidates(
    var_name: &str,
    page: &CandidatePage,
) -> verter_semantic::analysis::CssVarFlow {
    let mut flow = verter_semantic::analysis::CssVarFlow {
        name: var_name.to_string(),
        ..Default::default()
    };
    let mut seen: FxHashMap<(ContributionId, StyleRole, StyleSurface), ()> = FxHashMap::default();
    for candidate in &page.candidates {
        let Some(IndexRow::Stylesheet(row)) = candidate.record().map(|record| &record.row) else {
            continue;
        };
        if row.kind != StyleSymbolKind::CustomProperty {
            continue;
        }
        let distinct = seen
            .insert(
                (candidate.contribution.id.clone(), row.role, row.surface),
                (),
            )
            .is_none();
        if !distinct {
            continue;
        }
        let path: Arc<std::path::Path> = Arc::from(std::path::Path::new(
            candidate.contribution.basis.locator.as_ref(),
        ));
        match (row.surface, row.role) {
            (StyleSurface::Style { .. }, StyleRole::Definition) => {
                flow.style_definitions.push(path);
            }
            (StyleSurface::Style { .. }, StyleRole::Reference) => flow.style_var_usages.push(path),
            (StyleSurface::Template, _) => flow.template_definitions.push(path),
            (StyleSurface::Script, _) => flow.script_manipulations.push(path),
            (StyleSurface::Style { .. }, StyleRole::Manipulation) => {}
        }
    }
    flow
}
