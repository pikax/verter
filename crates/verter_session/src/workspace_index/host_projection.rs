//! Projects the host's per-file facts into the index on demand.
//!
//! A request demands fact surfaces ([`IndexSurface`]). For every file a
//! demanded surface applies to, the producers of that surface run once through
//! the shared cached accessors every other consumer uses, and each returns its
//! facts together with their coverage ([`FactCoverage`]). The projector
//! consumes exactly the value it certifies: a surface whose facts were produced
//! for the current source revision is certified, one that is unavailable or
//! unsupported blocks its unit and is answered privately, and nothing here
//! parses a file, branches on a framework or reads the analysis scope as a
//! proof of what was produced.
//!
//! The population is maintained from committed mutations. The scheduler's
//! source directory journals every source transition and the host journals
//! the eviction flag it owns, so a request after a change reads the changed
//! files only ([`IndexReconcileWork::population_checks`] counts them) and a
//! warm request reads none. The directory is enumerated once, when the index
//! has no population or a journal no longer reaches its cursor.
//!
//! Each surface is certified on its own. One the host cannot produce at all,
//! or one with a unit whose facts could not be read, answers `Partial` naming
//! why; the others stay certified. This producer has no catalog, selection or
//! demand-plan owner to read, so its bases record those as absent.

use std::cell::OnceCell;
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
use verter_scheduler::node::SourceSnapshot;
use verter_scheduler::source_root::SourceChanges;
use verter_semantic::analysis::AnalysisScope;
use verter_session_query::analysis::file_analysis::FileAnalysisSnapshot;
use verter_session_query::analysis::style::{
    BlockContentAvailability, StyleAnalysisLang, StyleBlockAnalysis,
};
use verter_session_query::analysis::template::{MarkupStyleVar, TemplateAnalysisSnapshot};
use verter_session_query::retention::heap_size::OwnedValue;
use verter_span::{SourceByteOffset, SourceByteRange, Span};

use super::delta::{
    ContributionDelta, ExpectedPrior, ObservationOracle, PartialCause, PartialEnumeration,
    PartitionDraft, PublishOutcome, PublishRefusal, RetirementEvidence, SchemaAdmission,
    SlotChange,
};
use super::facts::{FactCoverage, FactReceipt, IndexDemand, IndexDemandPartition, IndexSurface};
use super::identity::{
    ContributionId, ContributionKey, DemandEpoch, ObservationKey, ObservationOwner,
    OwnerSnapshotId, PartitionFamily, ProducerDescriptorId, ProfileClaim, ReadSet, ReadWitness,
    ScopeId, SelectionEpoch, SourceBasis, TableSchema,
};
use super::population::{HostPopulation, PendingSurfaces, PopulationCursor, PopulationEntry};
use super::query::{
    query_candidates, query_units, CandidatePage, EnumerationStatus, IndexContinuation, IndexQuery,
    Lookup, QueryBudgets, QueryWork, UnitPage,
};
use super::records::{
    ComponentRow, Endpoint, FactCause, FactQuality, IndexRecord, IndexRow, KeyFamily, LinkRelation,
    LinkRole, LinkRow, NameRole, NameRow, OriginUnavailable, PostingKey, Provenance, RecordOrigin,
    RecordSlot, RegistrationKey, RegistrationRow, SlotKind, SourceAnchor, StyleRole, StyleScope,
    StyleSurface, StyleSymbolKind, StylesheetRow, TypedSubject,
};
use super::snapshot::{Contribution, CoverageRecord, IndexSnapshot};
use crate::host_executor::HostSourceData;
use crate::VerterHost;

/// A publication attempt that lost to a concurrent change replans on a fresh
/// view at most this many times before answering return-only.
const MAX_PUBLISH_ATTEMPTS: u32 = 3;

const SCHEMA_VERSION: u32 = 1;

impl IndexSurface {
    /// A component surface exists only for a component carrier, and markup
    /// only in a carrier. Other files have no such slot: they are outside the
    /// surface, not empty in it.
    fn applies_to(self, language: &FileLanguage) -> bool {
        match self {
            Self::ComponentInventory | Self::ComponentUses | Self::MarkupClassReferences => {
                language.is_framework_carrier()
            }
            Self::StyleDefinitions
            | Self::CssVariables
            | Self::Bindings
            | Self::Imports
            | Self::Registrations => true,
        }
    }

    /// Whether this host has a producer for the surface at all. Style facts
    /// come from the eager style analysis the host's scope configures; every
    /// other surface is produced on demand.
    fn host_supported(self, scope: AnalysisScope) -> bool {
        match self {
            Self::StyleDefinitions | Self::CssVariables => scope.needs_style_analysis(),
            Self::ComponentInventory
            | Self::ComponentUses
            | Self::MarkupClassReferences
            | Self::Bindings
            | Self::Imports
            | Self::Registrations => true,
        }
    }
}

/// Counted work of one reconcile. Deterministic, so a test can assert it
/// exactly.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct IndexReconcileWork {
    /// Tracked files whose source identity was read to maintain the
    /// population: the changed files after a change, every file on the one
    /// whole read, none on a warm request.
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

/// Why one demanded surface is not certified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionShortfall {
    pub surface: IndexSurface,
    pub cause: PartialCause,
    /// Locators of the units this surface could not read.
    pub unavailable_units: Vec<Arc<str>>,
}

impl PartitionShortfall {
    /// The partition the uncertified surface posts under.
    #[must_use]
    pub fn partition(&self) -> IndexDemandPartition {
        self.surface.partition()
    }
}

/// How far the returned snapshot answers a demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReconcileStatus {
    /// Coverage for every demanded surface is published and current.
    Current,
    /// These surfaces are not certified. Any other demanded surface is, and
    /// rows only private to this request warm nothing.
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

pub(super) fn producer() -> &'static ProducerDescriptorId {
    static PRODUCER: OnceLock<ProducerDescriptorId> = OnceLock::new();
    PRODUCER.get_or_init(|| ProducerDescriptorId::of_descriptor(&Named("host-file-facts")))
}

fn result_contract() -> &'static ResultContractId {
    static CONTRACT: OnceLock<ResultContractId> = OnceLock::new();
    CONTRACT.get_or_init(|| ResultContractId::from_canonical(&Named("host-file-facts-rows")))
}

/// The registration operations this producer delivers: each names the owner
/// fact a registration row copies.
const REGISTRATION_OPERATIONS: [&str; 6] = ["provide", "inject", "define", "use", "emit", "listen"];

/// Registration domains the host posts. Each names the owner analysis fact a
/// row copies; the owning operation still validates every candidate.
pub mod registration_domain {
    /// `provide`/`inject` calls with a literal key.
    pub const PROVIDE_INJECT: &str = "provide-inject";
    /// Store definitions and store uses, keyed by the exported or called name.
    pub const STORE: &str = "store";
    /// Declared emits and template listeners, keyed by event name.
    pub const EVENT: &str = "event";
}

/// The posting key of one registration: its domain, a colon, then its key,
/// so one domain is one prefix walk.
#[must_use]
pub fn registration_posting_key(domain: &str, key: &str) -> PostingKey {
    PostingKey::exact(PartitionFamily::Registrations, &format!("{domain}:{key}"))
}

/// A walk of every registration of one domain.
#[must_use]
pub fn registration_domain_lookup(domain: &str) -> Lookup {
    Lookup::Prefix {
        partition: PartitionFamily::Registrations,
        family: KeyFamily::Exact,
        version: 0,
        prefix: Arc::from(format!("{domain}:")),
    }
}

/// This producer's own table layouts. No catalog owner covers them, which the
/// admission and every basis under it record as absent.
fn admission() -> &'static SchemaAdmission {
    static ADMISSION: OnceLock<SchemaAdmission> = OnceLock::new();
    ADMISSION.get_or_init(|| SchemaAdmission {
        catalog: None,
        table_schemas: IndexSurface::ALL
            .into_iter()
            .map(|surface| TableSchema {
                table: surface.family().table(),
                version: SCHEMA_VERSION,
            })
            .collect(),
        delivered_operations: REGISTRATION_OPERATIONS
            .iter()
            .map(|operation| Arc::from(*operation))
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

/// The population witness: where the committed-mutation feeds stood when the
/// population was read. Any source transition, membership change or project
/// generation moves it, so a walk whose population moved under it cannot be
/// certified, and reading it costs no file.
fn population_fingerprint(cursor: PopulationCursor) -> CanonicalDigest {
    let mut encoder = CanonicalEncoder::new("verter.session.workspace_index.population.v2");
    encoder.field_u64(1, cursor.source_epoch);
    encoder.field_u64(2, cursor.membership_seq);
    encoder.field_u64(3, cursor.project_generation);
    encoder.digest()
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

/// What the scheduler holds for one tracked canonical.
enum Tracked {
    /// A readable source the host parsed.
    Readable(Box<PopulationEntry>),
    /// A source without host data, with its content hash.
    Unreadable([u8; 16]),
    /// No source: never loaded, failed to load, removed or evicted.
    Gone,
}

/// A unit whose slots of every surface leave: its file is gone, unreadable or
/// now claims another carrier. The population owes it until a publication
/// carrying the retirement commits.
struct RetiredUnit {
    canonical: Arc<str>,
    unit: SourceUnitId,
    claim: ProfileClaim,
}

/// What one walk visits, as the population planned it under its lock.
struct WalkPlan {
    cursor: PopulationCursor,
    /// Readable files to visit, in canonical order.
    visit: Vec<(Arc<str>, PopulationEntry)>,
    retire: Vec<RetiredUnit>,
    /// Tracked files whose source the host cannot read.
    unreadable: Vec<Arc<str>>,
    /// Whether this walk visits every file of the population for its scope.
    full_visit: bool,
}

/// The facts of one file as one walk reads them. Each producer runs at most
/// once per file however many surfaces consume it.
struct FileFacts<'a> {
    host: &'a VerterHost,
    canonical: &'a Arc<str>,
    /// The file as read: the population's entry, or a fresh one when the
    /// file moved past it.
    entry: PopulationEntry,
    snapshot: Arc<SourceSnapshot>,
    content: ContentId,
    analysis: OnceCell<FactReceipt<FileAnalysisSnapshot>>,
    template: OnceCell<FactReceipt<OwnedValue<TemplateAnalysisSnapshot>>>,
    styles: OnceCell<FactReceipt<Vec<StyleBlockAnalysis>>>,
}

impl<'a> FileFacts<'a> {
    /// Read the file's current source. `None` when the host has no readable
    /// source for it any more; otherwise the facts and whether the file moved
    /// past the population's view, in which case `entry` describes the file
    /// as read and a publishing walk replans on the feeds' next cursor.
    fn read(
        host: &'a VerterHost,
        canonical: &'a Arc<str>,
        entry: &PopulationEntry,
        project_generation: u64,
    ) -> Option<(Self, bool)> {
        let Tracked::Readable(read) = host.read_tracked(canonical, project_generation) else {
            return None;
        };
        let snapshot = host.scheduler.try_get_source(canonical)?;
        if snapshot.whole_hash != read.whole_hash || snapshot.generation != read.generation {
            return None;
        }
        let moved = read.whole_hash != entry.whole_hash || read.generation != entry.generation;
        let facts = Self {
            host,
            canonical,
            entry: *read,
            content: ContentId::from_content_bytes(snapshot.source.as_bytes()),
            snapshot,
            analysis: OnceCell::new(),
            template: OnceCell::new(),
            styles: OnceCell::new(),
        };
        Some((facts, moved))
    }

    /// Script-side custom property manipulations. A parse fact of the source
    /// itself, there whenever the source is.
    fn script_manipulations(&self) -> &[verter_session_query::analysis::types::CssVarManipulation] {
        self.snapshot
            .downcast_data::<HostSourceData>()
            .map_or(&[], |data| {
                data.parse.script_analysis.css_var_manipulations.as_slice()
            })
    }

    fn source(&self) -> &Arc<str> {
        &self.snapshot.source
    }

    /// The file's analysis snapshot: script facts under every scope, the
    /// template when the host already holds it.
    fn analysis(&self) -> &FactReceipt<FileAnalysisSnapshot> {
        self.analysis.get_or_init(|| {
            self.host.get_analysis(self.canonical).map_or_else(
                || {
                    FactReceipt::without_value(FactCoverage::unavailable(
                        "the file's analysis snapshot is not available",
                    ))
                },
                FactReceipt::complete,
            )
        })
    }

    /// The file's template facts. A template the IDE already produced
    /// satisfies the demand; a cold carrier produces one here, through the
    /// same retained parse artifact and shared cache every reader uses.
    fn template(&self) -> &FactReceipt<OwnedValue<TemplateAnalysisSnapshot>> {
        self.template.get_or_init(|| {
            let held = || {
                self.analysis()
                    .value
                    .as_ref()
                    .and_then(|analysis| analysis.template.clone())
            };
            self.host
                .template_receipt(self.canonical, &self.entry.language, held)
        })
    }

    /// The file's style blocks. Every block answers; a block without native
    /// style facts lowers the coverage, never silently.
    fn styles(&self) -> &FactReceipt<Vec<StyleBlockAnalysis>> {
        self.styles.get_or_init(|| {
            let Some(blocks) = self.host.effective_style_analyses(self.canonical, None) else {
                return FactReceipt::without_value(FactCoverage::unavailable(
                    "style analysis is not available",
                ));
            };
            let mut coverage = if blocks.is_empty() {
                FactCoverage::Absent
            } else {
                FactCoverage::Complete
            };
            for block in &blocks {
                if block.css.is_some() {
                    continue;
                }
                let block_coverage = if !matches!(
                    block.content_availability,
                    BlockContentAvailability::NativeAvailable
                ) {
                    FactCoverage::unavailable(
                        "a style block's content is external or awaits processing",
                    )
                } else if block.lang != StyleAnalysisLang::Css {
                    FactCoverage::unsupported("a style block's language needs preprocessing")
                } else {
                    FactCoverage::unavailable("a style block produced no native style IR")
                };
                coverage = coverage.join(block_coverage);
            }
            FactReceipt {
                value: Some(blocks),
                coverage,
            }
        })
    }

    /// Inline-style custom properties of carrier markup without a template
    /// element tree. A parse fact: it is there whenever the source is.
    fn markup_style_vars(&self) -> &[MarkupStyleVar] {
        self.snapshot
            .downcast_data::<HostSourceData>()
            .map_or(&[], |data| data.parse.markup_style_vars.as_slice())
    }
}

/// Rows and origins under one file's basis.
struct RowBuilder<'a> {
    file: &'a FileFacts<'a>,
    records: Vec<IndexRecord>,
}

impl RowBuilder<'_> {
    fn unit(&self) -> &SourceUnitId {
        &self.file.entry.unit
    }

    fn origin(&self, span: Span) -> RecordOrigin {
        let range = SourceByteRange::checked_new(
            SourceByteOffset::new(span.start),
            SourceByteOffset::new(span.end),
        )
        .and_then(|range| range.validate(self.file.source()));
        match range {
            Ok(range) => RecordOrigin::Authored(SourceAnchor {
                unit: self.file.entry.unit.clone(),
                revision: self.file.entry.revision.clone(),
                content: self.file.content.clone(),
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

    /// A class token of carrier markup. It is reached only by its reverse
    /// key, so a stylesheet lookup never meets it. A token of a bound
    /// attribute is an approximate reading of an expression.
    fn push_markup_class(&mut self, name: &str, origin: RecordOrigin, bound: bool) {
        let partition = PartitionFamily::StyleSelectors;
        let name: Arc<str> = Arc::from(name);
        let quality = if bound {
            FactQuality::Approximate
        } else {
            FactQuality::Exact
        };
        self.push(
            SlotKind::Occurrence,
            TypedSubject::StyleSymbol {
                kind: StyleSymbolKind::Class,
                name: Arc::clone(&name),
            },
            vec![PostingKey::reverse(partition, &name)],
            IndexRow::Stylesheet(StylesheetRow {
                kind: StyleSymbolKind::Class,
                name,
                role: StyleRole::Reference,
                surface: StyleSurface::Template,
                scope: StyleScope::NotApplicable,
            }),
            origin,
            quality,
        );
        if bound {
            if let Some(record) = self.records.last_mut() {
                record.causes.push(FactCause::DynamicValue);
            }
        }
    }

    /// One owner-qualified registration under its domain.
    fn push_registration(
        &mut self,
        domain: &'static str,
        key: RegistrationKey,
        is_usage: bool,
        operation: &'static str,
        origin: RecordOrigin,
    ) {
        let text = match &key {
            RegistrationKey::String(text) | RegistrationKey::Symbol(text) => Arc::clone(text),
        };
        let qualified: Arc<str> = Arc::from(format!("{domain}:{text}"));
        let subject_key = match &key {
            RegistrationKey::String(_) => RegistrationKey::String(qualified),
            RegistrationKey::Symbol(_) => RegistrationKey::Symbol(qualified),
        };
        let kind = if is_usage {
            SlotKind::Occurrence
        } else {
            SlotKind::Declaration
        };
        self.push(
            kind,
            TypedSubject::Registration {
                owner: producer().clone(),
                key: subject_key,
            },
            vec![registration_posting_key(domain, &text)],
            IndexRow::Registration(RegistrationRow {
                domain: Arc::from(domain),
                key,
                is_usage,
                operation: Arc::from(operation),
            }),
            origin,
            FactQuality::Exact,
        );
    }
}

impl VerterHost {
    /// The template facts of `canonical` with their coverage: a template the
    /// host already holds (an editor compiled the file) satisfies the demand
    /// at once; a cold carrier produces one through the same retained parse
    /// artifact and shared cache every reader uses; a file that is not a
    /// carrier has none; a carrier whose template cannot be produced, such as
    /// one with an external template source, answers `Unavailable`, and one
    /// whose carrier registers no template producer `Unsupported`. Consumers
    /// that need a carrier's full component uses (props, import source, slots)
    /// under a template-free host scope read this instead of
    /// [`Self::get_analysis`]'s template.
    pub fn template_facts(
        &self,
        canonical: &str,
    ) -> FactReceipt<OwnedValue<TemplateAnalysisSnapshot>> {
        let canonical = self.resolve_alias_or_canonical(canonical);
        let language = match self.read_tracked(&canonical, 0) {
            Tracked::Readable(entry) => entry.language,
            Tracked::Unreadable(_) => {
                return FactReceipt::without_value(FactCoverage::unavailable(
                    "the source has no host data",
                ));
            }
            Tracked::Gone => {
                return FactReceipt::without_value(FactCoverage::unavailable(
                    "the file is not tracked",
                ));
            }
        };
        let held = || {
            self.get_analysis(&canonical)
                .and_then(|analysis| analysis.template)
        };
        self.template_receipt(&canonical, &language, held)
    }

    /// One template receipt. `held` yields a template the host already has.
    fn template_receipt(
        &self,
        canonical: &str,
        language: &FileLanguage,
        held: impl FnOnce() -> Option<OwnedValue<TemplateAnalysisSnapshot>>,
    ) -> FactReceipt<OwnedValue<TemplateAnalysisSnapshot>> {
        if !language.is_framework_carrier() {
            return FactReceipt::absent();
        }
        if !crate::parse::file_language_has_template_data_compiler(language) {
            return FactReceipt::without_value(FactCoverage::unsupported(
                "the carrier registers no template-data producer",
            ));
        }
        if let Some(template) = held() {
            return FactReceipt::complete(template);
        }
        self.raw_template_analysis_for_file(canonical).map_or_else(
            || {
                FactReceipt::without_value(FactCoverage::unavailable(
                    "template facts were not produced: an external template source, or a failed admission",
                ))
            },
            FactReceipt::complete,
        )
    }

    /// Project one surface of one file, consuming the exact facts whose
    /// coverage it returns.
    fn project_surface(&self, surface: IndexSurface, rows: &mut RowBuilder<'_>) -> FactCoverage {
        match surface {
            IndexSurface::ComponentInventory => Self::project_component_inventory(rows),
            IndexSurface::ComponentUses => Self::project_component_uses(rows),
            IndexSurface::StyleDefinitions => Self::project_style_definitions(rows),
            IndexSurface::MarkupClassReferences => Self::project_markup_class_references(rows),
            IndexSurface::CssVariables => Self::project_css_variables(rows),
            IndexSurface::Bindings => Self::project_bindings(rows),
            IndexSurface::Imports => Self::project_imports(rows),
            IndexSurface::Registrations => Self::project_registrations(rows),
        }
    }

    /// The admitted carrier identity is the inventory: it needs no template.
    /// The view of a component belongs to its TypeInfo owner, which this host
    /// does not run, and no name identifies a subject. This row says so where
    /// the unit is looked up, and no name finds it.
    fn project_component_inventory(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let unit = rows.unit().clone();
        rows.push(
            SlotKind::Declaration,
            TypedSubject::Component {
                unit: unit.clone(),
                export: None,
            },
            vec![PostingKey::forward(
                PartitionFamily::Components,
                &unit.digest().to_hex(),
            )],
            IndexRow::Component(ComponentRow::ViewUnavailable {
                cause: FactCause::OwnerUnavailable,
            }),
            RecordOrigin::Unavailable(OriginUnavailable::OwnerRecordsNoSpan),
            FactQuality::Unresolved,
        );
        FactCoverage::Complete
    }

    fn project_component_uses(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let template = rows.file.template();
        let unit = rows.unit().clone();
        let partition = PartitionFamily::Components;
        for usage in template
            .value
            .iter()
            .flat_map(|template| &template.components)
        {
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
        template.coverage.clone()
    }

    fn project_style_definitions(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let styles = rows.file.styles();
        for (block, style) in styles.value.iter().flatten().enumerate() {
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
        styles.coverage.clone()
    }

    /// Class tokens of carrier markup: the `class` attributes of a template
    /// element tree, and the markup class tokens of a carrier without one.
    fn project_markup_class_references(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let template = rows.file.template();
        for element in template
            .value
            .iter()
            .flat_map(|template| &template.elements)
        {
            let mut bound_origin = None;
            for attribute in element
                .attributes
                .iter()
                .filter(|attribute| attribute.name == "class")
            {
                let origin = rows.origin(attribute.value_span.unwrap_or(attribute.span));
                if attribute.is_dynamic {
                    bound_origin.get_or_insert(origin);
                    continue;
                }
                for class in attribute.value.as_deref().unwrap_or("").split_whitespace() {
                    rows.push_markup_class(class, origin.clone(), false);
                }
            }
            // The statically named classes of the element's bound class
            // expressions, as the shared converter read them; a computed
            // name is no token.
            if element.dynamic_classes.is_empty() {
                continue;
            }
            let origin = bound_origin.unwrap_or_else(|| rows.origin(element.span));
            for class in &element.dynamic_classes {
                rows.push_markup_class(class, origin.clone(), true);
            }
        }
        let analysis = rows.file.analysis();
        for token in analysis
            .value
            .iter()
            .flat_map(|analysis| analysis.markup_class_tokens.iter())
        {
            let origin = rows.origin(token.span);
            rows.push_markup_class(&token.name, origin, false);
        }
        template.coverage.clone().join(analysis.coverage.clone())
    }

    /// Each leg is read on its own, so one that is not available does not hide
    /// what the others report; the coverage is the weakest leg's.
    fn project_css_variables(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let styles = rows.file.styles();
        for (block, style) in styles.value.iter().flatten().enumerate() {
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
        let template = rows.file.template();
        for name in template
            .value
            .iter()
            .flat_map(|template| &template.css_var_names)
        {
            rows.push_style(
                StyleSymbolKind::CustomProperty,
                name,
                StyleRole::Definition,
                StyleSurface::Template,
                StyleScope::NotApplicable,
                RecordOrigin::Unavailable(OriginUnavailable::OwnerRecordsNoSpan),
            );
        }
        for var in rows.file.markup_style_vars() {
            let origin = rows.origin(var.span);
            rows.push_style(
                StyleSymbolKind::CustomProperty,
                &var.name,
                StyleRole::Definition,
                StyleSurface::Template,
                StyleScope::NotApplicable,
                origin,
            );
        }
        for manipulation in rows.file.script_manipulations() {
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
        styles.coverage.clone().join(template.coverage.clone())
    }

    fn project_bindings(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let analysis = rows.file.analysis();
        let unit = rows.unit().clone();
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
        if let Some(analysis) = analysis.value.as_ref() {
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
        }
        analysis.coverage.clone()
    }

    fn project_imports(rows: &mut RowBuilder<'_>) -> FactCoverage {
        let analysis = rows.file.analysis();
        let unit = rows.unit().clone();
        let partition = PartitionFamily::Imports;
        for import in analysis.value.iter().flat_map(|analysis| &analysis.imports) {
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
        analysis.coverage.clone()
    }

    /// Provide/inject keys and store facts from the script analysis, and
    /// declared emits and listeners from the template facts. Each row is the
    /// owner's fact as recorded; validating it stays with the owner. The
    /// coverage is the weaker of the two facts it read.
    fn project_registrations(rows: &mut RowBuilder<'_>) -> FactCoverage {
        use verter_session_query::analysis::types::VueApiClassification;

        let analysis = rows.file.analysis();
        if let Some(analysis) = analysis.value.as_ref() {
            for call in analysis.vue_api_calls.iter() {
                let (operation, is_usage) = match call.api {
                    VueApiClassification::Provide => ("provide", false),
                    VueApiClassification::Inject => ("inject", true),
                    _ => continue,
                };
                let Some(key) = call.arg_value.as_deref() else {
                    continue;
                };
                let origin = rows.origin(call.span);
                rows.push_registration(
                    registration_domain::PROVIDE_INJECT,
                    RegistrationKey::String(Arc::from(key)),
                    is_usage,
                    operation,
                    origin,
                );
            }
            for definition in analysis.store_definitions.iter() {
                let origin = rows.origin(definition.span);
                rows.push_registration(
                    registration_domain::STORE,
                    RegistrationKey::Symbol(Arc::from(definition.export_name.as_str())),
                    false,
                    "define",
                    origin,
                );
            }
            for usage in analysis.store_usages.iter() {
                let origin = rows.origin(usage.span);
                rows.push_registration(
                    registration_domain::STORE,
                    RegistrationKey::Symbol(Arc::from(usage.callee.as_str())),
                    true,
                    "use",
                    origin,
                );
            }
        }
        let template = rows.file.template();
        if let Some(template) = template.value.as_ref() {
            for emit in &template.emit_definitions {
                let origin = rows.origin(emit.span);
                rows.push_registration(
                    registration_domain::EVENT,
                    RegistrationKey::String(Arc::from(emit.event_name.as_str())),
                    false,
                    "emit",
                    origin,
                );
            }
            for handler in &template.event_handlers {
                let origin = rows.origin(handler.span);
                rows.push_registration(
                    registration_domain::EVENT,
                    RegistrationKey::String(Arc::from(handler.event_name.as_str())),
                    true,
                    "listen",
                    origin,
                );
            }
        }
        analysis.coverage.clone().join(template.coverage.clone())
    }

    /// What the scheduler holds for `canonical` now, as a population entry.
    fn read_tracked(&self, canonical: &str, project_generation: u64) -> Tracked {
        if self.is_canonical_evicted(canonical) {
            return Tracked::Gone;
        }
        let Some(snapshot) = self.scheduler.try_get_source(canonical) else {
            return Tracked::Gone;
        };
        let Some(data) = snapshot.downcast_data::<HostSourceData>() else {
            return Tracked::Unreadable(snapshot.whole_hash);
        };
        let language = data.file_language.clone();
        Tracked::Readable(Box::new(PopulationEntry {
            unit: host_unit(canonical),
            claim: carrier_claim(&language),
            language,
            whole_hash: snapshot.whole_hash,
            generation: snapshot.generation,
            revision: SourceRevision::from_canonical(&RevisionDescriptor {
                whole_hash: snapshot.whole_hash,
                generation: project_generation,
            }),
            pending: PendingSurfaces::all(),
        }))
    }

    /// The source owner's current fingerprint for `canonical`, as the
    /// publication fence will read it.
    fn live_source_fingerprint(&self, canonical: &str) -> CanonicalDigest {
        if self.is_canonical_evicted(canonical) {
            return absent_source_fingerprint();
        }
        self.scheduler
            .try_get_source(canonical)
            .map_or_else(absent_source_fingerprint, |source| {
                CanonicalDigest::of_bytes(&source.whole_hash)
            })
    }

    /// Where the committed-mutation feeds stand now.
    fn population_cursor(&self) -> PopulationCursor {
        let store = self.project_type_store();
        PopulationCursor {
            source_epoch: self.scheduler.source_directory().current_epoch(),
            membership_seq: store.workspace_index().membership().current(),
            project_generation: store.project_generation(),
        }
    }

    /// Re-read one changed canonical into the population. A unit whose slots
    /// must leave, because its file is gone, unreadable or claims another
    /// carrier, becomes a retirement the population owes until a publication
    /// carrying it commits.
    fn refresh_population_entry(
        &self,
        population: &mut HostPopulation,
        canonical: Arc<str>,
        project_generation: u64,
    ) {
        let previous = population.files.remove(&canonical);
        population.unreadable.remove(&canonical);
        let owe = |population: &mut HostPopulation, previous: PopulationEntry| {
            population
                .retirements
                .insert((Arc::clone(&canonical), previous.claim), previous.unit);
        };
        match self.read_tracked(&canonical, project_generation) {
            Tracked::Readable(entry) => {
                // The file is back under this claim: its slots are re-read
                // against their revision, not retired.
                population
                    .retirements
                    .remove(&(Arc::clone(&canonical), entry.claim.clone()));
                if let Some(previous) = previous {
                    if previous.claim != entry.claim {
                        owe(population, previous);
                    }
                }
                population.files.insert(canonical, *entry);
            }
            Tracked::Unreadable(whole_hash) => {
                if let Some(previous) = previous {
                    owe(population, previous);
                }
                population.unreadable.insert(canonical, whole_hash);
            }
            Tracked::Gone => {
                if let Some(previous) = previous {
                    owe(population, previous);
                }
            }
        }
    }

    /// Bring the population to the feeds' current cursor and plan what this
    /// walk visits. Runs under the population lock and reads no fact.
    fn plan_walk(
        &self,
        population: &mut HostPopulation,
        scope: &ScopeId,
        surfaces: &BTreeSet<IndexSurface>,
        work: &mut IndexReconcileWork,
    ) -> WalkPlan {
        let store = self.project_type_store();
        let project_generation = store.project_generation();
        let directory = self.scheduler.source_directory();
        let membership = store.workspace_index().membership();

        let incremental = population
            .cursor
            .filter(|cursor| cursor.project_generation == project_generation)
            .and_then(|previous| {
                let (source_epoch, sources) = directory.changes_since(previous.source_epoch);
                let (membership_seq, flags) = membership.changes_since(previous.membership_seq);
                let SourceChanges::Changed(sources) = sources else {
                    return None;
                };
                let flags = flags?;
                let cursor = PopulationCursor {
                    source_epoch,
                    membership_seq,
                    project_generation,
                };
                Some((cursor, sources, flags))
            });
        let cursor = match incremental {
            Some((cursor, sources, flags)) => {
                let changed: BTreeSet<Arc<str>> = sources.into_iter().chain(flags).collect();
                for canonical in changed {
                    work.population_checks += 1;
                    self.refresh_population_entry(population, canonical, project_generation);
                }
                cursor
            }
            None => {
                // The one whole read. The cursor is taken first, so a
                // transition landing during the enumeration is read again by
                // the next request, never missed.
                let cursor = PopulationCursor {
                    source_epoch: directory.current_epoch(),
                    membership_seq: membership.current(),
                    project_generation,
                };
                let mut ids = tracked_ids(self);
                ids.sort();
                work.population_checks += count_u32(ids.len());
                population.files.clear();
                population.unreadable.clear();
                population.settled.clear();
                for id in ids {
                    let canonical: Arc<str> = Arc::from(id.as_str());
                    match self.read_tracked(&id, project_generation) {
                        Tracked::Readable(entry) => {
                            population.files.insert(canonical, *entry);
                        }
                        Tracked::Unreadable(whole_hash) => {
                            population.unreadable.insert(canonical, whole_hash);
                        }
                        Tracked::Gone => {}
                    }
                }
                // Slots of units the population no longer holds are owed as
                // retirements now, under this lock, so a walk that is
                // cancelled or refused before paying them leaves the debt to
                // the next one. Reading them here is the one O(slots) scan,
                // and it runs with the one whole read.
                let held = store.workspace_index().snapshot();
                for surface in IndexSurface::ALL {
                    for slot in held.contributions_in_partition(&surface.id()) {
                        let current = population
                            .files
                            .get(&slot.basis.locator)
                            .is_some_and(|entry| entry.claim == slot.key.claim);
                        if !current {
                            population.retirements.insert(
                                (Arc::clone(&slot.basis.locator), slot.key.claim.clone()),
                                slot.basis.unit.clone(),
                            );
                        }
                    }
                }
                cursor
            }
        };
        population.cursor = Some(cursor);

        let full_visit = !population.settled.contains(scope);
        let visit = population
            .files
            .iter()
            .filter(|(_, entry)| {
                full_visit
                    || surfaces
                        .iter()
                        .any(|surface| entry.pending.contains(*surface))
            })
            .map(|(canonical, entry)| (Arc::clone(canonical), entry.clone()))
            .collect();
        let retire = population
            .retirements
            .iter()
            .map(|((canonical, claim), unit)| RetiredUnit {
                canonical: Arc::clone(canonical),
                unit: unit.clone(),
                claim: claim.clone(),
            })
            .collect();
        WalkPlan {
            cursor,
            visit,
            retire,
            unreadable: population.unreadable.keys().cloned().collect(),
            full_visit,
        }
    }

    fn slot_key(
        unit: &SourceUnitId,
        claim: &ProfileClaim,
        surface: IndexSurface,
    ) -> ContributionKey {
        ContributionKey {
            producer: producer().clone(),
            schema: TableSchema {
                table: surface.family().table(),
                version: SCHEMA_VERSION,
            },
            unit: unit.clone(),
            claim: claim.clone(),
            project: None,
            partition: surface.family(),
            facet: surface.facet(),
        }
    }

    /// Project one slot. An uncertified projection still carries the rows of
    /// the facts it could read.
    fn draft_slot(
        &self,
        file: &FileFacts<'_>,
        surface: IndexSurface,
        project_generation: u64,
    ) -> (PartitionDraft, FactCoverage) {
        let mut rows = RowBuilder {
            file,
            records: Vec::new(),
        };
        let coverage = self.project_surface(surface, &mut rows);
        let entry = &file.entry;
        let draft = PartitionDraft {
            key: Self::slot_key(&entry.unit, &entry.claim, surface),
            basis: SourceBasis {
                unit: entry.unit.clone(),
                locator: Arc::clone(file.canonical),
                revision: entry.revision.clone(),
                content: file.content.clone(),
                catalog: None,
                selection_epoch: None,
                demand_epoch: None,
                owner_snapshot: OwnerSnapshotId::of_owner_snapshot(&RevisionDescriptor {
                    whole_hash: entry.whole_hash,
                    generation: entry.generation,
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
                    key: source_key(&entry.unit),
                    fingerprint: CanonicalDigest::of_bytes(&entry.whole_hash),
                },
                ReadWitness {
                    key: activation_key(),
                    fingerprint: generation_fingerprint(project_generation),
                },
            ]),
            records: rows.records,
        };
        (draft, coverage)
    }

    /// The demanded surfaces this host has a producer for. Every other one
    /// answers unsupported without visiting a file.
    fn supported_demand(&self, demand: &IndexDemand) -> IndexDemand {
        let scope = self.config.effective_scope();
        IndexDemand::of(
            demand
                .surfaces()
                .iter()
                .copied()
                .filter(|surface| surface.host_supported(scope)),
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
        let supported = self.supported_demand(demand);
        let mut work = IndexReconcileWork::default();
        let mut shortfalls: Vec<PartitionShortfall> = demand
            .surfaces()
            .difference(supported.surfaces())
            .map(|&surface| PartitionShortfall {
                surface,
                cause: PartialCause::UnsupportedCoverage,
                unavailable_units: Vec::new(),
            })
            .collect();
        let finished = |snapshot, mut shortfalls: Vec<PartitionShortfall>, work| {
            shortfalls.sort_by_key(|shortfall| shortfall.surface);
            let status = if shortfalls.is_empty() {
                ReconcileStatus::Current
            } else {
                ReconcileStatus::Uncertified(shortfalls)
            };
            (snapshot, status, work)
        };
        if supported.surfaces().is_empty() {
            return finished(index.snapshot(), shortfalls, work);
        }

        // Warm: every supported surface is certified at the feeds' current
        // cursor, so no mutation has landed since. No file is touched.
        let scope = supported.scope();
        let snapshot = index.snapshot();
        let certified = snapshot.coverage(&scope).is_some_and(|record| {
            record.partitions == supported.partition_ids()
                && record.witness.fingerprint == population_fingerprint(self.population_cursor())
        });
        if certified {
            return finished(snapshot, shortfalls, work);
        }

        let mut refusal_cause = PartialCause::Stale;
        for _ in 0..MAX_PUBLISH_ATTEMPTS {
            match self.reconcile_once(supported.surfaces(), &scope, is_cancelled, &mut work, true) {
                Attempt::Published(snapshot) => {
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
        match self.reconcile_once(supported.surfaces(), &scope, is_cancelled, &mut work, false) {
            Attempt::Uncertified(snapshot, blocked) => {
                let has_own_cause = |surface| blocked.iter().any(|b| b.surface == surface);
                let walk_wide: Vec<PartitionShortfall> = supported
                    .surfaces()
                    .iter()
                    .filter(|&&surface| !has_own_cause(surface))
                    .map(|&surface| PartitionShortfall {
                        surface,
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
        surfaces: &BTreeSet<IndexSurface>,
        scope: &ScopeId,
        is_cancelled: &dyn Fn() -> bool,
        work: &mut IndexReconcileWork,
        publish: bool,
    ) -> Attempt {
        let store = self.project_type_store();
        let index = store.workspace_index();
        let account = index.account().get();
        let project_generation = store.project_generation();
        let plan = {
            let mut population = index.population().lock();
            self.plan_walk(&mut population, scope, surfaces, work)
        };
        // Captured after planning, so every slot a retirement the plan owes
        // could hold is in view; a slot published later needs witnesses the
        // owed unit's source no longer gives.
        let captured = index.snapshot();

        let mut canonical_by_unit: BTreeMap<CanonicalDigest, Arc<str>> = BTreeMap::new();
        let mut changes = Vec::new();
        let mut private_rows = Vec::new();
        let mut gone: BTreeSet<ContributionId> = BTreeSet::new();
        // Units a surface could not read, with the coverage each answered.
        let mut blocked: BTreeMap<IndexSurface, BTreeMap<Arc<str>, FactCoverage>> = BTreeMap::new();
        for canonical in &plan.unreadable {
            for &surface in surfaces {
                blocked.entry(surface).or_default().insert(
                    Arc::clone(canonical),
                    FactCoverage::unavailable("the source has no host data"),
                );
            }
        }
        let mut unsupported = BTreeSet::new();
        // What each visited file was published for, to clear its debt.
        let mut visited: Vec<(Arc<str>, SourceRevision, PendingSurfaces)> = Vec::new();

        for (canonical, entry) in &plan.visit {
            canonical_by_unit.insert(entry.unit.digest(), Arc::clone(canonical));
            let mut facts: Option<FileFacts<'_>> = None;
            let mut cleared = PendingSurfaces::none();
            for &surface in surfaces {
                if !surface.applies_to(&entry.language) {
                    cleared.insert(surface);
                    continue;
                }
                if is_cancelled() {
                    return Attempt::Cancelled;
                }
                let key = Self::slot_key(&entry.unit, &entry.claim, surface);
                let id = key.id();
                let held = captured.contribution(&id);
                if held.is_some_and(|held| held.basis.revision == entry.revision) {
                    work.unchanged += 1;
                    cleared.insert(surface);
                    continue;
                }
                if facts.is_none() {
                    // A file that moved past the population's view cannot
                    // be published from it: a publishing walk replans on the
                    // feeds' next cursor. A private walk answers from the file
                    // as it is now.
                    match FileFacts::read(self, canonical, entry, project_generation) {
                        Some((read, moved)) if !(publish && moved) => facts = Some(read),
                        Some(_) => return Attempt::Refused(PublishRefusal::Stale),
                        None if publish => return Attempt::Refused(PublishRefusal::Stale),
                        None => break,
                    }
                }
                let Some(file) = facts.as_ref() else {
                    break;
                };
                work.projections += 1;
                let (draft, coverage) = self.draft_slot(file, surface, project_generation);
                if !coverage.certifies() {
                    // An old value does not stay current because its
                    // replacement could not be built.
                    blocked
                        .entry(surface)
                        .or_default()
                        .insert(Arc::clone(canonical), coverage);
                    if let Some(held) = held {
                        gone.insert(id);
                        changes.push(retire(held, CanonicalDigest::of_bytes(&entry.whole_hash)));
                        work.retired += 1;
                    }
                    private_rows.push(PartialEnumeration::new(draft, account));
                    continue;
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
                        cleared.insert(surface);
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
                        unsupported.insert(surface);
                    }
                }
            }
            visited.push((Arc::clone(canonical), entry.revision.clone(), cleared));
        }

        // Slots of units that left the population, of every surface. The
        // retirement witnesses the source owner's answer as the fence will
        // read it.
        for retired in &plan.retire {
            canonical_by_unit.insert(retired.unit.digest(), Arc::clone(&retired.canonical));
            let fingerprint = self.live_source_fingerprint(&retired.canonical);
            for surface in IndexSurface::ALL {
                let id = Self::slot_key(&retired.unit, &retired.claim, surface).id();
                if let Some(held) = captured.contribution(&id) {
                    if gone.insert(id) {
                        changes.push(retire(held, fingerprint));
                        work.retired += 1;
                    }
                }
            }
        }
        let shortfalls: Vec<PartitionShortfall> = blocked
            .iter()
            .map(|(&surface, units)| PartitionShortfall {
                surface,
                cause: if units
                    .values()
                    .all(|coverage| matches!(coverage, FactCoverage::Unsupported { .. }))
                {
                    PartialCause::UnsupportedCoverage
                } else {
                    PartialCause::UnavailableInput
                },
                unavailable_units: units.keys().cloned().collect(),
            })
            .chain(unsupported.iter().map(|&surface| PartitionShortfall {
                surface,
                cause: PartialCause::UnsupportedCoverage,
                unavailable_units: Vec::new(),
            }))
            .collect();

        let gone: Vec<ContributionId> = gone.into_iter().collect();
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

        // Each surface is certified on its own: a unit that blocks one says
        // nothing about the others.
        let certified: BTreeSet<IndexSurface> = surfaces
            .iter()
            .copied()
            .filter(|surface| !blocked.contains_key(surface) && !unsupported.contains(surface))
            .collect();
        let coverage = if certified.is_empty() {
            Vec::new()
        } else {
            vec![CoverageRecord {
                scope: scope.clone(),
                partitions: certified.iter().map(|surface| surface.id()).collect(),
                witness: ReadWitness {
                    key: population_key(scope),
                    fingerprint: population_fingerprint(plan.cursor),
                },
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
                {
                    // The walk's debt is paid for what it published; a file
                    // that moved since keeps its debt.
                    let mut population = index.population().lock();
                    for (canonical, revision, cleared) in visited {
                        if let Some(entry) = population.files.get_mut(&canonical) {
                            if entry.revision == revision {
                                entry.pending.remove_all(cleared);
                            }
                        }
                    }
                    if plan.full_visit {
                        population.settled.insert(scope.clone());
                    }
                    // A retirement is paid once the store holds none of the
                    // unit's slots. One a concurrent writer committed between
                    // this walk's capture and its publication stays owed.
                    let committed = index.snapshot();
                    for retired in &plan.retire {
                        let still_held = IndexSurface::ALL.into_iter().any(|surface| {
                            let id = Self::slot_key(&retired.unit, &retired.claim, surface).id();
                            committed.contribution(&id).is_some()
                        });
                        if !still_held {
                            population
                                .retirements
                                .remove(&(Arc::clone(&retired.canonical), retired.claim.clone()));
                        }
                    }
                }
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

    /// Enumerate candidates for `demand` under finite budgets.
    ///
    /// The answer is `Complete` only when the walk exhausted its keys and every
    /// surface the lookup reads is certified. A key outside the demand returns
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
            .filter(|shortfall| lookup.reads(shortfall.surface.family()))
            .collect();
        let key_count = match &lookup {
            Lookup::Keys(keys) => count_u32(keys.len()),
            Lookup::Prefix { .. } => 1,
        };
        let query = IndexQuery {
            lookup,
            budgets,
            scope: Some(self.supported_demand(demand).declared_scope()),
            continuation,
        };
        let mut page = query_candidates(&snapshot, &query, None, is_cancelled);
        // A surface the host cannot produce leaves the declared scope, so the
        // walk can prove its keys exhausted over the produced surfaces of the
        // family; the family is still not certified for this demand.
        if page.status == EnumerationStatus::Complete {
            if let Some(shortfall) = relevant.first() {
                page.status = EnumerationStatus::Partial {
                    cause: shortfall.cause.clone(),
                    covered: super::query::CoveredScope {
                        keys_exhausted: key_count,
                        postings_visited: page.work.posting_visits,
                        unavailable_units: Vec::new(),
                    },
                    continuation: None,
                };
            }
        }
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

    /// Enumerate the units of one demanded surface under finite budgets:
    /// every tracked file the surface applies to, as the slot its facts were
    /// projected into. `IndexSurface::Imports` or `Bindings` reaches every
    /// file; `ComponentInventory` reaches the component carriers without
    /// producing a template.
    ///
    /// The answer is `Complete` only when the walk exhausted the surface and
    /// its coverage is certified. A surface the demand does not include, one
    /// the host cannot produce, a unit whose facts could not be read, or an
    /// exhausted budget answers `Partial` naming why, and nothing it returns
    /// is kept.
    pub fn workspace_index_units(
        &self,
        demand: &IndexDemand,
        surface: IndexSurface,
        budgets: QueryBudgets,
        is_cancelled: &dyn Fn() -> bool,
    ) -> (UnitPage, IndexReconcileWork) {
        let (snapshot, status, work) = self.workspace_index_reconcile(demand, is_cancelled);
        if status == ReconcileStatus::Cancelled {
            let page = UnitPage {
                units: Vec::new(),
                status: EnumerationStatus::Cancelled,
                work: QueryWork::default(),
                generation: snapshot.generation(),
            };
            return (page, work);
        }
        let shortfall = match &status {
            ReconcileStatus::Uncertified(shortfalls) => shortfalls
                .iter()
                .find(|shortfall| shortfall.surface == surface),
            ReconcileStatus::Current | ReconcileStatus::Cancelled => None,
        };
        let scope = self.supported_demand(demand).declared_scope();
        let mut page = query_units(&snapshot, &scope, &surface.id(), budgets, is_cancelled);
        if let (EnumerationStatus::Partial { cause, covered, .. }, Some(shortfall)) =
            (&mut page.status, shortfall)
        {
            if *cause != PartialCause::BudgetExhausted {
                shortfall.cause.clone_into(cause);
            }
            covered
                .unavailable_units
                .clone_from(&shortfall.unavailable_units);
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
        let demand = IndexDemand::of([IndexSurface::CssVariables]);
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

/// The owners' live answers for one walk's fence.
struct HostObservations<'host> {
    host: &'host VerterHost,
    canonical_by_unit: BTreeMap<CanonicalDigest, Arc<str>>,
    scope: ScopeId,
}

impl ObservationOracle for HostObservations<'_> {
    fn current(&self, key: &ObservationKey) -> Option<CanonicalDigest> {
        match key.owner {
            ObservationOwner::Source => {
                let canonical = self.canonical_by_unit.get(&key.subject)?;
                Some(self.host.live_source_fingerprint(canonical))
            }
            ObservationOwner::Activation => Some(generation_fingerprint(
                self.host.project_type_store().project_generation(),
            )),
            ObservationOwner::Population if key.subject == self.scope.digest() => {
                Some(population_fingerprint(self.host.population_cursor()))
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

enum Attempt {
    /// Coverage for every surface of the demand is current.
    Published(Arc<IndexSnapshot>),
    /// The snapshot that answers, and the surfaces it could not certify.
    Uncertified(Arc<IndexSnapshot>, Vec<PartitionShortfall>),
    Refused(PublishRefusal),
    Cancelled,
}

fn retire(held: &Contribution, fingerprint: CanonicalDigest) -> SlotChange {
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
