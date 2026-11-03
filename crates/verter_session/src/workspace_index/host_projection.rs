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
//!
//! Each partition is certified on its own. One the host's analysis scope does
//! not compute, or one with a unit whose facts cannot be read, answers `Partial`
//! naming why; the others stay certified.
//!
//! This producer has no catalog, selection or demand-plan owner to read, so its
//! bases record those as absent. Its table layouts are its own declaration, and
//! a project-generation change reaches the fence through each slot's activation
//! witness and the population witness.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, OnceLock};

use rustc_hash::FxHashMap;
use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalDigest, CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::{
    ContentId, ResultContractId, SourceId, SourceRevision, SourceUnitId,
};
use verter_identity::successor::{CarrierProfileId, ProfileSchemaEpoch};
use verter_language::{FileLanguage, JsModuleKind, ScriptFlavor, ScriptSourceType};
use verter_semantic::analysis::AnalysisScope;
use verter_span::{SourceByteOffset, SourceByteRange, Span};

use super::delta::{
    ContributionDelta, ExpectedPrior, ObservationOracle, PartialCause, PartialEnumeration,
    PartitionDraft, PublishOutcome, PublishRefusal, RetirementEvidence, SchemaAdmission,
    SlotChange,
};
use super::identity::{
    ContributionId, ContributionKey, DemandEpoch, ObservationKey, ObservationOwner,
    OwnerSnapshotId, PartitionFamily, PartitionId, ProducerDescriptorId, ProfileClaim, ReadSet,
    ReadWitness, ScopeId, SelectionEpoch, SourceBasis, TableSchema,
};
use super::query::{
    query_candidates, CandidatePage, DeclaredScope, EnumerationStatus, IndexContinuation,
    IndexQuery, Lookup, QueryBudgets,
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
    /// The fact family this demand reads.
    #[must_use]
    pub fn family(self) -> PartitionFamily {
        match self {
            Self::CssVariables => PartitionFamily::CssVariables,
            Self::StyleSelectors => PartitionFamily::StyleSelectors,
            Self::Bindings => PartitionFamily::Bindings,
            Self::Components => PartitionFamily::Components,
            Self::Imports => PartitionFamily::Imports,
        }
    }

    /// The host's partition of this family.
    #[must_use]
    pub fn id(self) -> PartitionId {
        PartitionId::new(producer().clone(), self.family())
    }

    /// Whether `scope` computes every owner fact this partition's rows read.
    /// Empty owner facts under any other scope say nothing about the source.
    fn computed_under(self, scope: AnalysisScope) -> bool {
        match self {
            Self::CssVariables => {
                scope.needs_style_analysis()
                    && scope.needs_template_analysis()
                    && scope.needs_script_analysis()
            }
            Self::StyleSelectors => scope.needs_style_analysis(),
            Self::Bindings => scope.contains(AnalysisScope::BINDINGS | AnalysisScope::IMPORTS),
            Self::Components => scope.contains(AnalysisScope::TPL_COMPONENTS),
            Self::Imports => scope.contains(AnalysisScope::IMPORTS),
        }
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
        ScopeId::of_population(self)
    }

    /// The declared population a query of this demand reads. A key of any
    /// other partition is outside it.
    #[must_use]
    pub fn declared_scope(&self) -> DeclaredScope {
        DeclaredScope::new(self.scope(), self.partition_ids())
    }

    fn partition_ids(&self) -> BTreeSet<PartitionId> {
        self.partitions
            .iter()
            .map(|partition| partition.id())
            .collect()
    }
}

impl CanonicalEncode for IndexDemand {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_scope.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_u32(1, SCHEMA_VERSION);
        for partition in &self.partitions {
            encoder.field_enum_discriminant(2, partition.family().encoding());
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

/// Why one demanded partition is not certified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionShortfall {
    pub partition: IndexDemandPartition,
    pub cause: PartialCause,
    /// Locators of the units this partition could not read.
    pub unavailable_units: Vec<Arc<str>>,
}

/// How far the returned snapshot answers a demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileStatus {
    /// Coverage for every demanded partition is published and current.
    Current,
    /// These partitions are not certified. Any other demanded partition is,
    /// and rows only private to this request warm nothing.
    Uncertified(Vec<PartitionShortfall>),
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
    PRODUCER.get_or_init(|| ProducerDescriptorId::of_descriptor(&Named("host-file-facts")))
}

fn result_contract() -> &'static ResultContractId {
    static CONTRACT: OnceLock<ResultContractId> = OnceLock::new();
    CONTRACT.get_or_init(|| ResultContractId::from_canonical(&Named("host-file-facts-rows")))
}

/// This producer's own table layouts. No catalog owner covers them, which the
/// admission and every basis under it record as absent.
fn admission() -> &'static SchemaAdmission {
    static ADMISSION: OnceLock<SchemaAdmission> = OnceLock::new();
    ADMISSION.get_or_init(|| SchemaAdmission {
        catalog: None,
        table_schemas: [
            IndexDemandPartition::CssVariables,
            IndexDemandPartition::StyleSelectors,
            IndexDemandPartition::Bindings,
            IndexDemandPartition::Components,
            IndexDemandPartition::Imports,
        ]
        .into_iter()
        .map(|partition| TableSchema {
            table: partition.family().table(),
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
    generation: u64,
}

impl CanonicalEncode for RevisionDescriptor {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.host_revision.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_bytes(1, &self.whole_hash);
        encoder.field_u64(2, self.generation);
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

/// The typed fields of a language row that change how its bytes parse. An
/// owner path never enters a carrier's identity.
struct CarrierFamily<'a>(&'a FileLanguage);

const fn js_module_encoding(kind: JsModuleKind) -> u32 {
    match kind {
        JsModuleKind::Unambiguous => 1,
        JsModuleKind::Module => 2,
        JsModuleKind::CommonJs => 3,
        JsModuleKind::Script => 4,
    }
}

impl CanonicalEncode for CarrierFamily<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.workspace_index.carrier_family.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        match self.0 {
            FileLanguage::Script {
                source_type,
                flavor,
            } => {
                encoder.field_enum_discriminant(1, 1);
                let (dialect, module) = match source_type {
                    ScriptSourceType::Ts => (1, 0),
                    ScriptSourceType::Tsx => (2, 0),
                    ScriptSourceType::Js(kind) => (3, js_module_encoding(*kind)),
                    ScriptSourceType::Jsx(kind) => (4, js_module_encoding(*kind)),
                    ScriptSourceType::Dts => (5, 0),
                };
                encoder.field_enum_discriminant(2, dialect);
                encoder.field_u32(3, module);
                if let ScriptFlavor::AdapterModule { language_id, .. } = flavor {
                    encoder.field_str(4, language_id.as_str());
                }
            }
            FileLanguage::Framework { language_id, .. } => {
                encoder.field_enum_discriminant(1, 2);
                encoder.field_str(4, language_id.as_str());
            }
            FileLanguage::FrameworkTemplate { adapter_id, .. } => {
                encoder.field_enum_discriminant(1, 3);
                encoder.field_str(5, adapter_id.as_str());
            }
        }
    }
}

/// Carrier profile of a file. Facts here come from the carrier and its shared
/// analysis, so this claims the carrier and never fabricates a framework.
pub(super) fn carrier_claim(language: &FileLanguage) -> ProfileClaim {
    let family = Canonical::from_encodable(&CarrierFamily(language))
        .digest()
        .to_hex();
    let schema = ProfileSchemaEpoch::for_schema_family(
        "verter.session.workspace_index.carrier_family",
        SCHEMA_VERSION,
    );
    ProfileClaim::Carrier(CarrierProfileId::from_carrier(&family, &schema))
}

/// A file with a source the host cannot read: its locator and content hash.
type UnreadableFile = (Arc<str>, [u8; 16]);

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
        let mut files: Vec<(String, [u8; 16])> = tracked_ids(self.host)
            .into_iter()
            .filter_map(|canonical| {
                let whole_hash = self.host.scheduler.try_get_source(&canonical)?.whole_hash;
                Some((canonical, whole_hash))
            })
            .collect();
        files.sort();
        population_fingerprint(
            self.host.project_type_store().project_generation(),
            files
                .iter()
                .map(|(canonical, hash)| (canonical.as_str(), hash)),
        )
    }
}

/// The population witness: the project generation, then every file with a
/// source and its content, in canonical order. Adding, removing or editing any
/// file changes it. A scheduler node with no source, such as a failed load, is
/// not a file of the workspace.
fn population_fingerprint<'a>(
    project_generation: u64,
    files: impl Iterator<Item = (&'a str, &'a [u8; 16])>,
) -> CanonicalDigest {
    let mut encoder = CanonicalEncoder::new("verter.session.workspace_index.population.v1");
    encoder.field_u64(3, project_generation);
    for (canonical, whole_hash) in files {
        encoder.field_str(1, canonical);
        encoder.field_bytes(2, whole_hash);
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

    fn current_epochs(&self) -> (Option<SelectionEpoch>, Option<DemandEpoch>) {
        (None, None)
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
        let partition = match kind {
            StyleSymbolKind::CustomProperty => PartitionFamily::CssVariables,
            StyleSymbolKind::Class | StyleSymbolKind::Id => PartitionFamily::StyleSelectors,
        };
        let name: Arc<str> = Arc::from(name);
        self.push(
            SlotKind::Occurrence,
            TypedSubject::StyleSymbol {
                kind,
                name: Arc::clone(&name),
            },
            vec![
                PostingKey::exact(partition, &name),
                PostingKey::lexical(partition, &name),
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

/// Whether a projection read every owner surface its partition needs. Rows of
/// the surfaces it could read still answer the request, privately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Projection {
    Complete,
    /// An owner surface this file should have was not available.
    MissingFacts,
}

impl VerterHost {
    /// Read `partition`'s owner facts for one file.
    ///
    /// Reads are the shared cached accessors, so a demanded carrier is parsed
    /// and analyzed once however many partitions read it.
    fn project_partition(
        &self,
        partition: IndexDemandPartition,
        rows: &mut RowBuilder<'_>,
    ) -> Projection {
        match partition {
            IndexDemandPartition::CssVariables => self.project_css_variables(rows),
            IndexDemandPartition::StyleSelectors => self.project_style_selectors(rows),
            IndexDemandPartition::Bindings => self.project_bindings(rows),
            IndexDemandPartition::Components => self.project_components(rows),
            IndexDemandPartition::Imports => self.project_imports(rows),
        }
    }

    /// A file's template facts, and whether any it should have are missing.
    fn template_facts(
        &self,
        canonical: &str,
    ) -> (
        Option<
            verter_session_query::retention::heap_size::OwnedValue<
                verter_session_query::analysis::template::TemplateAnalysisSnapshot,
            >,
        >,
        Projection,
    ) {
        let template = self.raw_template_analysis_for_file(canonical);
        let expected = self
            .effective_file_state(canonical, None)
            .is_none_or(|state| state.meta.has_template);
        let projection = if template.is_none() && expected {
            Projection::MissingFacts
        } else {
            Projection::Complete
        };
        (template, projection)
    }

    /// Each surface is read on its own, so one that is not available does not
    /// hide what the others report.
    fn project_css_variables(&self, rows: &mut RowBuilder<'_>) -> Projection {
        let canonical = rows.file.canonical.clone();
        let mut projection = Projection::Complete;
        let styles = self.effective_style_analyses(&canonical, None);
        if styles.is_none() {
            projection = Projection::MissingFacts;
        }
        for (block, style) in styles.iter().flatten().enumerate() {
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
        let (template, template_projection) = self.template_facts(&canonical);
        if template_projection == Projection::MissingFacts {
            projection = Projection::MissingFacts;
        }
        if let Some(template) = template {
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
        let Some(state) = self.effective_file_state(&canonical, None) else {
            return Projection::MissingFacts;
        };
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
        projection
    }

    fn project_style_selectors(&self, rows: &mut RowBuilder<'_>) -> Projection {
        let Some(styles) = self.effective_style_analyses(&rows.file.canonical, None) else {
            return Projection::MissingFacts;
        };
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
        Projection::Complete
    }

    fn project_bindings(&self, rows: &mut RowBuilder<'_>) -> Projection {
        let Some(analysis) = self.get_analysis(&rows.file.canonical) else {
            return Projection::MissingFacts;
        };
        let unit = rows.file.unit.clone();
        let partition = PartitionFamily::Bindings;
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
                    PostingKey::exact(partition, name),
                    PostingKey::lexical(partition, name),
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
            push_name(
                rows,
                &binding.name,
                NameRole::Declaration,
                Some(binding.kind),
                binding.span,
            );
        }
        for import in &analysis.imports {
            for binding in &import.bindings {
                push_name(rows, &binding.name, NameRole::Import, None, binding.span);
            }
        }
        Projection::Complete
    }

    fn project_components(&self, rows: &mut RowBuilder<'_>) -> Projection {
        let canonical = rows.file.canonical.clone();
        let Some(analysis) = self.get_analysis(&canonical) else {
            return Projection::MissingFacts;
        };
        let unit = rows.file.unit.clone();
        let partition = PartitionFamily::Components;

        // The view of a component belongs to its TypeInfo owner, which this
        // host does not run, and no name identifies a subject. This row says
        // so where the unit is looked up, and no name finds it.
        rows.push(
            SlotKind::Declaration,
            TypedSubject::Component {
                unit: unit.clone(),
                export: None,
            },
            vec![PostingKey::forward(partition, &unit.digest().to_hex())],
            IndexRow::Component(ComponentRow::ViewUnavailable {
                cause: FactCause::OwnerUnavailable,
            }),
            RecordOrigin::Unavailable(OriginUnavailable::OwnerRecordsNoSpan),
            FactQuality::Unresolved,
        );

        let Some(template) = analysis.template.as_ref() else {
            return self.template_facts(&canonical).1;
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
                    PostingKey::exact(partition, &usage.name),
                    PostingKey::lexical(partition, &usage.name),
                ],
                IndexRow::Component(ComponentRow::Usage { tag, target }),
                origin,
                FactQuality::Unresolved,
            );
        }
        Projection::Complete
    }

    fn project_imports(&self, rows: &mut RowBuilder<'_>) -> Projection {
        let Some(analysis) = self.get_analysis(&rows.file.canonical) else {
            return Projection::MissingFacts;
        };
        let unit = rows.file.unit.clone();
        let partition = PartitionFamily::Imports;
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
                    PostingKey::forward(partition, &unit.digest().to_hex()),
                    PostingKey::reverse(partition, &specifier),
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
        Projection::Complete
    }

    /// One read of every tracked file's identity and content hash. A file with
    /// a source the host cannot read comes back separately, with its hash.
    fn capture_tracked(&self, project_generation: u64) -> (Vec<TrackedFile>, Vec<UnreadableFile>) {
        use crate::host_executor::HostSourceData;

        let mut ids = tracked_ids(self);
        ids.sort();
        let mut files = Vec::with_capacity(ids.len());
        let mut unavailable = Vec::new();
        for id in ids {
            let canonical: Arc<str> = Arc::from(id.as_str());
            let Some(snapshot) = self.scheduler.try_get_source(&id) else {
                continue;
            };
            let Some(data) = snapshot.downcast_data::<HostSourceData>() else {
                unavailable.push((canonical, snapshot.whole_hash));
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
                    generation: project_generation,
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
                table: partition.family().table(),
                version: SCHEMA_VERSION,
            },
            unit: file.unit.clone(),
            claim: file.claim.clone(),
            project: None,
            partition: partition.family(),
        }
    }

    /// Project one slot. An incomplete projection still carries the rows of the
    /// surfaces it could read.
    fn draft_slot(
        &self,
        file: &TrackedFile,
        partition: IndexDemandPartition,
        project_generation: u64,
    ) -> (PartitionDraft, Projection) {
        let mut rows = RowBuilder {
            file,
            content: ContentId::from_content_bytes(file.source.as_bytes()),
            records: Vec::new(),
        };
        let projection = self.project_partition(partition, &mut rows);
        let RowBuilder {
            content, records, ..
        } = rows;
        let draft = PartitionDraft {
            key: Self::slot_key(file, partition),
            basis: SourceBasis {
                unit: file.unit.clone(),
                locator: Arc::clone(&file.canonical),
                revision: file.revision.clone(),
                content,
                catalog: None,
                selection_epoch: None,
                demand_epoch: None,
                owner_snapshot: OwnerSnapshotId::of_owner_snapshot(&RevisionDescriptor {
                    whole_hash: file.whole_hash,
                    generation: file.generation,
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
        };
        (draft, projection)
    }

    /// The demanded partitions this host's analysis scope computes. Every other
    /// one answers unsupported without visiting a file.
    fn computed_demand(&self, demand: &IndexDemand) -> IndexDemand {
        let scope = self.config.effective_scope();
        IndexDemand::of(
            demand
                .partitions
                .iter()
                .copied()
                .filter(|partition| partition.computed_under(scope)),
        )
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
        let computed = self.computed_demand(demand);
        let mut work = IndexReconcileWork::default();
        let mut shortfalls: Vec<PartitionShortfall> = demand
            .partitions
            .difference(&computed.partitions)
            .map(|&partition| PartitionShortfall {
                partition,
                cause: PartialCause::UnsupportedCoverage,
                unavailable_units: Vec::new(),
            })
            .collect();
        let finished = |snapshot, mut shortfalls: Vec<PartitionShortfall>, work| {
            shortfalls.sort_by_key(|shortfall| shortfall.partition);
            let status = if shortfalls.is_empty() {
                ReconcileStatus::Current
            } else {
                ReconcileStatus::Uncertified(shortfalls)
            };
            (snapshot, status, work)
        };
        if computed.partitions.is_empty() {
            return finished(index.snapshot(), shortfalls, work);
        }

        // The epochs are bumped after a mutation commits, so reading them
        // first can only make a later request re-check, never miss a change.
        let scope = computed.scope();
        let stamp = (
            self.current_store_view_epoch(),
            self.current_load_generation(),
        );
        if index.population_stamp(&scope) == Some(stamp) {
            let snapshot = index.snapshot();
            let certified = snapshot
                .coverage(&scope)
                .is_some_and(|record| record.partitions == computed.partition_ids());
            if certified {
                return finished(snapshot, shortfalls, work);
            }
        }

        let mut refusal_cause = PartialCause::Stale;
        for _ in 0..MAX_PUBLISH_ATTEMPTS {
            match self.reconcile_once(&computed, &scope, is_cancelled, &mut work, true) {
                Attempt::Published(snapshot) => {
                    index.record_population_stamp(scope, stamp);
                    return finished(snapshot, shortfalls, work);
                }
                Attempt::Uncertified(snapshot, blocked) => {
                    shortfalls.extend(blocked);
                    return finished(snapshot, shortfalls, work);
                }
                Attempt::Cancelled | Attempt::Refused(PublishRefusal::Cancelled) => {
                    return (index.snapshot(), ReconcileStatus::Cancelled, work);
                }
                Attempt::Refused(PublishRefusal::RetentionPressure) => {
                    refusal_cause = PartialCause::RetentionPressure;
                    break;
                }
                Attempt::Refused(_) => {}
            }
        }
        // Lost every attempt, or the account refused retention: answer from
        // private rows, certifying nothing.
        match self.reconcile_once(&computed, &scope, is_cancelled, &mut work, false) {
            Attempt::Uncertified(snapshot, blocked) => {
                let has_own_cause = |partition| blocked.iter().any(|b| b.partition == partition);
                let walk_wide: Vec<PartitionShortfall> = computed
                    .partitions
                    .iter()
                    .filter(|&&partition| !has_own_cause(partition))
                    .map(|&partition| PartitionShortfall {
                        partition,
                        cause: refusal_cause.clone(),
                        unavailable_units: Vec::new(),
                    })
                    .collect();
                shortfalls.extend(walk_wide);
                shortfalls.extend(blocked);
                finished(snapshot, shortfalls, work)
            }
            Attempt::Published(_) | Attempt::Cancelled | Attempt::Refused(_) => {
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
        let captured_population = captured_population(project_generation, &files, &unavailable);

        let mut canonical_by_unit: BTreeMap<CanonicalDigest, Arc<str>> = files
            .iter()
            .map(|file| (file.unit.digest(), Arc::clone(&file.canonical)))
            .collect();

        let mut changes = Vec::new();
        let mut private_rows = Vec::new();
        let mut current_slots: BTreeMap<IndexDemandPartition, BTreeSet<ContributionId>> =
            BTreeMap::new();
        let mut gone = Vec::new();
        // A unit the host cannot read may hold rows of any partition.
        let mut blocked: BTreeMap<IndexDemandPartition, BTreeSet<Arc<str>>> = BTreeMap::new();
        for (canonical, _) in &unavailable {
            for &partition in &demand.partitions {
                blocked
                    .entry(partition)
                    .or_default()
                    .insert(Arc::clone(canonical));
            }
        }
        let mut unsupported = BTreeSet::new();

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
                    current_slots.entry(partition).or_default().insert(id);
                    continue;
                }
                work.projections += 1;
                let (draft, projection) = self.draft_slot(file, partition, project_generation);
                if projection == Projection::MissingFacts {
                    // An old value does not stay current because its
                    // replacement could not be built.
                    blocked
                        .entry(partition)
                        .or_default()
                        .insert(Arc::clone(&file.canonical));
                    if let Some(held) = held {
                        gone.push(id);
                        changes.push(retire(held, CanonicalDigest::of_bytes(&file.whole_hash)));
                        work.retired += 1;
                    }
                    private_rows.push(PartialEnumeration::new(draft, account));
                    continue;
                }
                if publish && !self.source_unchanged(file) {
                    return Attempt::Refused(PublishRefusal::Stale);
                }
                if !publish {
                    private_rows.push(PartialEnumeration::new(draft, account));
                    continue;
                }
                let expected = held.map_or(ExpectedPrior::Absent, |held| {
                    ExpectedPrior::Basis(Box::new(held.basis.clone()))
                });
                match draft.seal(admission(), account) {
                    Ok(sealed) => {
                        current_slots.entry(partition).or_default().insert(id);
                        changes.push(SlotChange::Upsert {
                            partition: Box::new(sealed),
                            expected,
                        });
                    }
                    Err(rejection) => {
                        verter_debug_assert::verter_debug_assert!(
                            false,
                            "a host partition failed its own registered schema: {rejection:?}"
                        );
                        unsupported.insert(partition);
                    }
                }
            }
        }

        // Slots of this demand that no tracked file claims any more.
        let blocked_slots: BTreeSet<ContributionId> = gone.iter().cloned().collect();
        for &partition in &demand.partitions {
            let slots = current_slots.get(&partition);
            for held in captured.contributions_in_partition(&partition.id()) {
                if slots.is_some_and(|slots| slots.contains(&held.id))
                    || blocked_slots.contains(&held.id)
                {
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

        let shortfalls: Vec<PartitionShortfall> = blocked
            .iter()
            .map(|(&partition, units)| PartitionShortfall {
                partition,
                cause: PartialCause::UnavailableInput,
                unavailable_units: units.iter().cloned().collect(),
            })
            .chain(unsupported.iter().map(|&partition| PartitionShortfall {
                partition,
                cause: PartialCause::UnsupportedCoverage,
                unavailable_units: Vec::new(),
            }))
            .collect();

        let private_contributions = private_rows
            .into_iter()
            .map(PartialEnumeration::into_return_only);

        if !publish {
            let private = captured
                .with_return_only(private_contributions, &gone)
                .without_coverage();
            return Attempt::Uncertified(Arc::new(private), shortfalls);
        }

        let observations = HostObservations {
            host: self,
            canonical_by_unit,
            scope: scope.clone(),
        };

        // Each partition is certified on its own: a unit that blocks one says
        // nothing about the others.
        let certified: BTreeSet<IndexDemandPartition> = demand
            .partitions
            .iter()
            .copied()
            .filter(|partition| {
                !blocked.contains_key(partition) && !unsupported.contains(partition)
            })
            .collect();
        let coverage = if certified.is_empty() {
            Vec::new()
        } else {
            vec![CoverageRecord {
                scope: scope.clone(),
                partitions: certified.iter().map(|p| p.id()).collect(),
                witness: ReadWitness {
                    key: population_key(scope),
                    fingerprint: captured_population,
                },
                contributions: certified
                    .iter()
                    .filter_map(|partition| current_slots.get(partition))
                    .flatten()
                    .cloned()
                    .collect(),
            }]
        };
        let decertified = if certified.is_empty() {
            vec![scope.clone()]
        } else {
            Vec::new()
        };

        let delta = ContributionDelta {
            changes,
            coverage,
            decertified,
            selection_epoch: None,
            demand_epoch: None,
        };
        work.publish_attempts += 1;
        match index.publish(delta, &observations, is_cancelled) {
            PublishOutcome::Published | PublishOutcome::AlreadyCurrent => {
                let snapshot = index.snapshot();
                if shortfalls.is_empty() {
                    Attempt::Published(snapshot)
                } else {
                    let private = snapshot.with_return_only(private_contributions, &[]);
                    Attempt::Uncertified(Arc::new(private), shortfalls)
                }
            }
            PublishOutcome::Refused(refusal) => Attempt::Refused(refusal),
        }
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
    /// The answer is `Complete` only when the walk exhausted its keys and every
    /// partition the lookup reads is certified. A key outside the demand returns
    /// nothing and answers `Partial`. `continuation` resumes an earlier page of
    /// the same lookup and demand; once the index has moved on it answers
    /// `Partial(Stale)`, and a request-local snapshot issues none, because no
    /// later request can hold it.
    ///
    /// Candidates are candidates: the owning operation still validates each one
    /// before a semantic answer or edit.
    pub fn workspace_index_candidates(
        &self,
        demand: &IndexDemand,
        lookup: Lookup,
        budgets: QueryBudgets,
        continuation: Option<IndexContinuation>,
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
        let shortfalls: &[PartitionShortfall] = match &status {
            ReconcileStatus::Uncertified(shortfalls) => shortfalls,
            ReconcileStatus::Current | ReconcileStatus::Cancelled => &[],
        };
        let relevant: Vec<&PartitionShortfall> = shortfalls
            .iter()
            .filter(|shortfall| lookup.reads(shortfall.partition.family()))
            .collect();
        let query = IndexQuery {
            lookup,
            budgets,
            scope: Some(self.computed_demand(demand).declared_scope()),
            continuation,
        };
        let mut page = query_candidates(&snapshot, &query, None, is_cancelled);
        if let EnumerationStatus::Partial {
            cause,
            covered,
            continuation,
        } = &mut page.status
        {
            let resumable = continuation.is_some() && !snapshot.is_return_only();
            if !resumable {
                *continuation = None;
            }
            if let (false, Some(shortfall)) = (resumable, relevant.first()) {
                shortfall.cause.clone_into(cause);
            }
            let units: BTreeSet<&Arc<str>> = relevant
                .iter()
                .flat_map(|shortfall| &shortfall.unavailable_units)
                .collect();
            covered.unavailable_units = units.into_iter().cloned().collect();
        }
        (page, work)
    }

    /// Candidates of one custom property under finite budgets.
    pub fn css_var_candidates(
        &self,
        var_name: &str,
        budgets: QueryBudgets,
        continuation: Option<IndexContinuation>,
        is_cancelled: &dyn Fn() -> bool,
    ) -> CssVarCandidates {
        let demand = IndexDemand::of([IndexDemandPartition::CssVariables]);
        let key = PostingKey::exact(PartitionFamily::CssVariables, var_name);
        let (page, work) = self.workspace_index_candidates(
            &demand,
            Lookup::Keys(vec![key]),
            budgets,
            continuation,
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
fn captured_population(
    project_generation: u64,
    files: &[TrackedFile],
    unavailable: &[UnreadableFile],
) -> CanonicalDigest {
    let mut entries: Vec<(&str, &[u8; 16])> = files
        .iter()
        .map(|file| (file.canonical.as_ref(), &file.whole_hash))
        .chain(
            unavailable
                .iter()
                .map(|(canonical, hash)| (canonical.as_ref(), hash)),
        )
        .collect();
    entries.sort_by_key(|(canonical, _)| *canonical);
    population_fingerprint(project_generation, entries.into_iter())
}

enum Attempt {
    /// Coverage for every partition of the demand is current.
    Published(Arc<IndexSnapshot>),
    /// The snapshot that answers, and the partitions it could not certify.
    Uncertified(Arc<IndexSnapshot>, Vec<PartitionShortfall>),
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
