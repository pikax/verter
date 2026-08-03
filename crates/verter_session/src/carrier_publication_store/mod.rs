//! Carrier-only registered publication authority.

pub mod persistence;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierAcceptanceError, CarrierGrammarAuthority,
    CarrierGrammarFingerprint, GrammarAuthorityNamespaceId,
};
use verter_language::carrier_versions::{
    CarrierParserVersion, FrameworkParseArtifactSchemaVersion,
    FRAMEWORK_PARSE_ARTIFACT_SCHEMA_VERSION,
};
use verter_language::registered_source_authority::{
    FileIncarnation, RegisteredSourceAuthority, RegisteredSourceSnapshot,
    RegisteredSourceSnapshotId, SourceAuthorityNamespaceId, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, FrameworkParseArtifact, LanguageId};
use verter_scheduler::cancellation::CancellationToken;

use crate::carrier_artifact_cohort::current_persisted_carrier_artifact_cohort;
use crate::types::MetaProvenance;
use persistence::{CarrierPersistence, InMemoryCarrierPersistence};

pub(crate) type RegisteredEnvelopeIngest =
    Arc<parking_lot::Mutex<rustc_hash::FxHashMap<String, RegisteredFileStructure>>>;
pub(crate) type SourceAuthorityHandle = Arc<RegisteredSourceAuthority>;
pub(crate) type GrammarAuthorityHandle = Arc<CarrierGrammarAuthority>;
pub(crate) type PublicationStoreHandle = Arc<CarrierPublicationStore>;

pub const MAX_PUBLICATION_COORDINATION_RETRIES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AuditRequestId(u64);

impl AuditRequestId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PublicationSurface {
    ProjectionHost,
    SemanticHost,
    Compile,
    AuditCompile,
    Overlay,
    PreparedDeclaration,
    AnalysisIo,
    ColdWorkspace,
    Extension,
    Playground,
    Unplugin,
}

#[derive(Debug, Clone)]
pub struct PublicationRequestContext {
    pub audit_request_id: AuditRequestId,
    pub surface: PublicationSurface,
    pub cancellation: CancellationToken,
    pub expected_source: RegisteredSourceSnapshotId,
}

impl PublicationRequestContext {
    pub fn new(
        audit_request_id: AuditRequestId,
        surface: PublicationSurface,
        cancellation: CancellationToken,
        expected_source: RegisteredSourceSnapshotId,
    ) -> Self {
        Self {
            audit_request_id,
            surface,
            cancellation,
            expected_source,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisteredCarrierUnsupported {
    NotCarrier,
    NoRegisteredProducer,
    UnsupportedLanguageMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegistryMismatch {
    SourceLanguageAdapterMismatch,
    GrammarAdapterMismatch,
    ProducerVersionMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CarrierParseFailure {
    ParserRejected,
    InvalidProjectedArtifact,
    RecoveryInvariantViolation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistentAdoptionRejection {
    CohortMismatch,
    StableGrammarMismatch,
    SourceFactMismatch,
    ChecksumMismatch,
    SourceSpaceInvalid,
    ParserValidationFailed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentSourceEvidence {
    pub current_source: Option<RegisteredSourceSnapshotId>,
}

#[derive(Debug, Clone)]
pub enum PublicationOutcome {
    Published(Arc<FrameworkArtifactEnvelope>),
    Adopted(Arc<FrameworkArtifactEnvelope>),
    Unsupported(RegisteredCarrierUnsupported),
    RegistryMismatch(RegistryMismatch),
    Failed(CarrierParseFailure),
    Superseded(CurrentSourceEvidence),
    Closed,
    Cancelled,
    WinnerPanicked,
    RetryExhausted,
}

impl PublicationOutcome {
    pub fn into_envelope(self) -> Option<Arc<FrameworkArtifactEnvelope>> {
        match self {
            Self::Published(value) | Self::Adopted(value) => Some(value),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameworkArtifactId {
    authority: SourceAuthorityNamespaceId,
    source: RegisteredSourceSnapshotId,
    grammar_authority: GrammarAuthorityNamespaceId,
    grammar_fingerprint: CarrierGrammarFingerprint,
    adapter_id: FrameworkAdapterId,
    language_id: LanguageId,
    carrier_parser_version: CarrierParserVersion,
    artifact_schema_version: FrameworkParseArtifactSchemaVersion,
}

impl FrameworkArtifactId {
    fn derive(
        accepted: &AcceptedRegisteredCarrierSource,
        carrier_parser_version: CarrierParserVersion,
    ) -> Self {
        Self {
            authority: accepted.source().authority(),
            source: accepted.source().snapshot_id().clone(),
            grammar_authority: accepted.grammar().authority(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            adapter_id: accepted.grammar().adapter_id().clone(),
            language_id: accepted.grammar().language_id().clone(),
            carrier_parser_version,
            artifact_schema_version: FRAMEWORK_PARSE_ARTIFACT_SCHEMA_VERSION,
        }
    }
}

pub struct FrameworkArtifactEnvelope {
    id: FrameworkArtifactId,
    source: RegisteredSourceSnapshot,
    artifact: Arc<FrameworkParseArtifact>,
}

impl std::fmt::Debug for FrameworkArtifactEnvelope {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FrameworkArtifactEnvelope")
            .field("id", &self.id)
            .field("source", &self.source.snapshot_id())
            .field("artifact", &self.artifact)
            .finish()
    }
}

impl FrameworkArtifactEnvelope {
    pub fn id(&self) -> &FrameworkArtifactId {
        &self.id
    }
    pub fn source(&self) -> &RegisteredSourceSnapshot {
        &self.source
    }
    pub fn artifact(&self) -> &Arc<FrameworkParseArtifact> {
        &self.artifact
    }
    pub fn inventory(
        &self,
    ) -> &Arc<verter_language::parse_artifact::carrier_inventory::CarrierBlockInventory> {
        &self.artifact.common.inventory
    }
}

#[derive(Debug, Clone)]
pub struct RegisteredFileStructure {
    envelope: Arc<FrameworkArtifactEnvelope>,
}

impl RegisteredFileStructure {
    pub(crate) fn new(envelope: Arc<FrameworkArtifactEnvelope>) -> Self {
        Self { envelope }
    }
    pub fn envelope(&self) -> &Arc<FrameworkArtifactEnvelope> {
        &self.envelope
    }
    pub fn artifact(&self) -> &Arc<FrameworkParseArtifact> {
        self.envelope.artifact()
    }

    pub fn artifact_id(&self) -> &FrameworkArtifactId {
        self.envelope.id()
    }

    pub fn source(&self) -> &RegisteredSourceSnapshot {
        self.envelope.source()
    }

    pub fn inventory(
        &self,
    ) -> &Arc<verter_language::parse_artifact::carrier_inventory::CarrierBlockInventory> {
        self.envelope.inventory()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostInstanceId(u64);

impl HostInstanceId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }
    pub const fn get(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HostSourceRevisionToken {
    pub host_instance: HostInstanceId,
    pub file_incarnation: FileIncarnation,
    pub source_generation: SourceGeneration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationLaneRole {
    Leader,
    Waiter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublicationAuditKind {
    PublicationRequested,
    LiveHit,
    CoordinationLaneEntered(PublicationLaneRole),
    PersistentCandidateFound,
    PersistentAdoptionAccepted,
    PersistentAdoptionRejected(PersistentAdoptionRejection),
    PersistentCandidateDiscarded,
    ParserStarted,
    ParserFinished,
    PublishFencePassed,
    PublishFenceRejected,
    Published,
    Adopted,
    WaiterDetachedCancelled,
    TerminalFailure,
    LiveRecordRetired,
}

#[derive(Debug, Clone)]
pub struct PublicationAuditEvent {
    pub request: AuditRequestId,
    pub artifact_id: FrameworkArtifactId,
    pub surface: PublicationSurface,
    pub kind: PublicationAuditKind,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PublicationAuditSnapshot {
    pub parser_started: u64,
    pub leaders: u64,
    pub waiters: u64,
    pub live_hits: u64,
    pub adopted: u64,
    pub rejected_candidates: u64,
}

#[derive(Default)]
struct PublicationAuditLog {
    events: Mutex<Vec<PublicationAuditEvent>>,
}

impl PublicationAuditLog {
    fn push(
        &self,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
        kind: PublicationAuditKind,
    ) {
        if let Ok(mut events) = self.events.lock() {
            events.push(PublicationAuditEvent {
                request: request.audit_request_id,
                artifact_id: artifact_id.clone(),
                surface: request.surface,
                kind,
            });
        }
    }

    fn snapshot(&self) -> PublicationAuditSnapshot {
        let mut snapshot = PublicationAuditSnapshot::default();
        let Ok(events) = self.events.lock() else {
            return snapshot;
        };
        for event in events.iter() {
            match event.kind {
                PublicationAuditKind::ParserStarted => snapshot.parser_started += 1,
                PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Leader) => {
                    snapshot.leaders += 1;
                }
                PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Waiter) => {
                    snapshot.waiters += 1;
                }
                PublicationAuditKind::LiveHit => snapshot.live_hits += 1,
                PublicationAuditKind::Adopted => snapshot.adopted += 1,
                PublicationAuditKind::PersistentAdoptionRejected(_) => {
                    snapshot.rejected_candidates += 1;
                }
                _ => {}
            }
        }
        snapshot
    }
}

enum LaneState {
    Vacant,
    Producing,
    Terminal(TerminalOutcome),
}

#[derive(Clone)]
enum TerminalOutcome {
    Published(std::sync::Weak<FrameworkArtifactEnvelope>),
    Adopted(std::sync::Weak<FrameworkArtifactEnvelope>),
    Other(PublicationOutcome),
}

impl TerminalOutcome {
    fn from_outcome(outcome: &PublicationOutcome) -> Self {
        match outcome {
            PublicationOutcome::Published(envelope) => Self::Published(Arc::downgrade(envelope)),
            PublicationOutcome::Adopted(envelope) => Self::Adopted(Arc::downgrade(envelope)),
            other => Self::Other(other.clone()),
        }
    }

    fn outcome(&self) -> Option<PublicationOutcome> {
        match self {
            Self::Published(envelope) => envelope.upgrade().map(PublicationOutcome::Published),
            Self::Adopted(envelope) => envelope.upgrade().map(PublicationOutcome::Adopted),
            Self::Other(outcome) => Some(outcome.clone()),
        }
    }

    fn artifact_expired(&self) -> bool {
        matches!(self, Self::Published(value) | Self::Adopted(value) if value.strong_count() == 0)
    }
}

struct PublicationLane {
    state: Mutex<LaneState>,
    wake: Condvar,
}

impl PublicationLane {
    fn vacant() -> Self {
        Self {
            state: Mutex::new(LaneState::Vacant),
            wake: Condvar::new(),
        }
    }
}

pub struct CarrierPublicationStore {
    source_authority: Arc<RegisteredSourceAuthority>,
    grammar_authority: Arc<CarrierGrammarAuthority>,
    lanes: Mutex<HashMap<FrameworkArtifactId, Arc<PublicationLane>>>,
    persistence: Arc<dyn CarrierPersistence>,
    provenance: Arc<MetaProvenance>,
    audit: PublicationAuditLog,
}

impl std::fmt::Debug for CarrierPublicationStore {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CarrierPublicationStore")
            .finish_non_exhaustive()
    }
}

impl CarrierPublicationStore {
    pub fn new(
        source_authority: Arc<RegisteredSourceAuthority>,
        grammar_authority: Arc<CarrierGrammarAuthority>,
    ) -> Self {
        Self::with_dependencies(
            source_authority,
            grammar_authority,
            Arc::new(InMemoryCarrierPersistence::default()),
            Arc::new(MetaProvenance::default()),
        )
    }

    pub(crate) fn with_provenance(
        source_authority: Arc<RegisteredSourceAuthority>,
        grammar_authority: Arc<CarrierGrammarAuthority>,
        provenance: Arc<MetaProvenance>,
    ) -> Self {
        Self::with_dependencies(
            source_authority,
            grammar_authority,
            Arc::new(InMemoryCarrierPersistence::default()),
            provenance,
        )
    }

    pub(crate) fn with_dependencies(
        source_authority: Arc<RegisteredSourceAuthority>,
        grammar_authority: Arc<CarrierGrammarAuthority>,
        persistence: Arc<dyn CarrierPersistence>,
        provenance: Arc<MetaProvenance>,
    ) -> Self {
        Self {
            source_authority,
            grammar_authority,
            lanes: Mutex::new(HashMap::new()),
            persistence,
            provenance,
            audit: PublicationAuditLog::default(),
        }
    }

    pub fn publish_or_get(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        request: PublicationRequestContext,
    ) -> PublicationOutcome {
        if request.cancellation.is_cancelled() {
            return PublicationOutcome::Cancelled;
        }
        if request.expected_source != *accepted.source().snapshot_id() {
            return PublicationOutcome::Superseded(CurrentSourceEvidence {
                current_source: None,
            });
        }
        if self
            .grammar_authority
            .validate_accepted_current(&self.source_authority, accepted)
            .is_err()
        {
            return PublicationOutcome::Superseded(CurrentSourceEvidence {
                current_source: None,
            });
        }
        let compiler = crate::parse::carrier_compiler_registry()
            .compiler_for_carrier_language(
                accepted.grammar().adapter_id(),
                accepted.grammar().language_id(),
            )
            .cloned();
        let parser_version = parser_version_for(accepted.grammar().adapter_id());
        let artifact_id = FrameworkArtifactId::derive(accepted, parser_version);
        self.audit.push(
            &request,
            &artifact_id,
            PublicationAuditKind::PublicationRequested,
        );

        let (lane, leader, retired_record) = {
            let mut lanes = match self.lanes.lock() {
                Ok(lanes) => lanes,
                Err(_) => return PublicationOutcome::Closed,
            };
            if let Some(lane) = lanes.get(&artifact_id) {
                let lane = Arc::clone(lane);
                let mut state = match lane.state.lock() {
                    Ok(state) => state,
                    Err(_) => return PublicationOutcome::Closed,
                };
                let retired =
                    matches!(&*state, LaneState::Terminal(value) if value.artifact_expired());
                if retired {
                    *state = LaneState::Producing;
                }
                drop(state);
                (lane, retired, retired)
            } else {
                let lane = Arc::new(PublicationLane::vacant());
                let Ok(mut state) = lane.state.lock() else {
                    return PublicationOutcome::Closed;
                };
                *state = LaneState::Producing;
                drop(state);
                lanes.insert(artifact_id.clone(), Arc::clone(&lane));
                (lane, true, false)
            }
        };

        if leader {
            if retired_record {
                self.audit.push(
                    &request,
                    &artifact_id,
                    PublicationAuditKind::LiveRecordRetired,
                );
            }
            self.audit.push(
                &request,
                &artifact_id,
                PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Leader),
            );
            let outcome = match catch_unwind(AssertUnwindSafe(|| {
                self.produce(compiler.as_ref(), accepted, &request, &artifact_id)
            })) {
                Ok(outcome) => outcome,
                Err(_) => PublicationOutcome::WinnerPanicked,
            };
            if !matches!(
                outcome,
                PublicationOutcome::Published(_) | PublicationOutcome::Adopted(_)
            ) {
                self.audit.push(
                    &request,
                    &artifact_id,
                    PublicationAuditKind::TerminalFailure,
                );
            }
            let mut state = match lane.state.lock() {
                Ok(state) => state,
                Err(_) => return PublicationOutcome::Closed,
            };
            *state = LaneState::Terminal(TerminalOutcome::from_outcome(&outcome));
            lane.wake.notify_all();
            outcome
        } else {
            let mut state = match lane.state.lock() {
                Ok(state) => state,
                Err(_) => return PublicationOutcome::Closed,
            };
            if let LaneState::Terminal(outcome) = &*state {
                if self
                    .grammar_authority
                    .validate_accepted_current(&self.source_authority, accepted)
                    .is_err()
                {
                    return PublicationOutcome::Superseded(CurrentSourceEvidence {
                        current_source: None,
                    });
                }
                self.audit
                    .push(&request, &artifact_id, PublicationAuditKind::LiveHit);
                return outcome
                    .outcome()
                    .unwrap_or(PublicationOutcome::RetryExhausted);
            }
            self.audit.push(
                &request,
                &artifact_id,
                PublicationAuditKind::CoordinationLaneEntered(PublicationLaneRole::Waiter),
            );
            loop {
                if request.cancellation.is_cancelled() {
                    self.audit.push(
                        &request,
                        &artifact_id,
                        PublicationAuditKind::WaiterDetachedCancelled,
                    );
                    return PublicationOutcome::Cancelled;
                }
                match &*state {
                    LaneState::Terminal(outcome) => {
                        if self
                            .grammar_authority
                            .validate_accepted_current(&self.source_authority, accepted)
                            .is_err()
                        {
                            return PublicationOutcome::Superseded(CurrentSourceEvidence {
                                current_source: None,
                            });
                        }
                        return outcome
                            .outcome()
                            .unwrap_or(PublicationOutcome::RetryExhausted);
                    }
                    LaneState::Vacant | LaneState::Producing => {}
                }
                let waited = lane.wake.wait_timeout(state, Duration::from_millis(5));
                state = match waited {
                    Ok((state, _)) => state,
                    Err(_) => return PublicationOutcome::Closed,
                };
            }
        }
    }

    fn produce(
        &self,
        compiler: Option<&Arc<dyn verter_compiler::framework_common::CarrierCompiler>>,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
        let Some(compiler) = compiler else {
            return PublicationOutcome::Unsupported(
                RegisteredCarrierUnsupported::NoRegisteredProducer,
            );
        };
        if let Some(candidate) = self.persistence.take_candidate(artifact_id, accepted) {
            self.audit.push(
                request,
                artifact_id,
                PublicationAuditKind::PersistentCandidateFound,
            );
            let rejection =
                match candidate.validate(accepted, current_persisted_carrier_artifact_cohort()) {
                    Ok(()) => {
                        let artifact = candidate.artifact.__rehome_registered(accepted.source());
                        let envelope = Arc::new(FrameworkArtifactEnvelope {
                            id: artifact_id.clone(),
                            source: accepted.source().clone(),
                            artifact: Arc::new(artifact),
                        });
                        self.audit.push(
                            request,
                            artifact_id,
                            PublicationAuditKind::PersistentAdoptionAccepted,
                        );
                        self.audit
                            .push(request, artifact_id, PublicationAuditKind::Adopted);
                        return PublicationOutcome::Adopted(envelope);
                    }
                    Err(rejection) => rejection,
                };
            self.audit.push(
                request,
                artifact_id,
                PublicationAuditKind::PersistentAdoptionRejected(rejection),
            );
            self.audit.push(
                request,
                artifact_id,
                PublicationAuditKind::PersistentCandidateDiscarded,
            );
        }

        use std::sync::atomic::Ordering::Relaxed;
        self.provenance.carrier_parses.fetch_add(1, Relaxed);
        if compiler.adapter_id().is_vue() {
            self.provenance.sfc_parses.fetch_add(1, Relaxed);
        }
        self.audit
            .push(request, artifact_id, PublicationAuditKind::ParserStarted);
        let projection = verter_compiler::framework_common::registered_carrier_projection::__project_registered_carrier_for_store_leader(
            compiler.as_ref(),
            accepted,
        );
        let artifact = Arc::new(projection.into_framework_parse_artifact());
        self.audit
            .push(request, artifact_id, PublicationAuditKind::ParserFinished);
        if self
            .grammar_authority
            .validate_accepted_current(&self.source_authority, accepted)
            .is_err()
        {
            self.audit.push(
                request,
                artifact_id,
                PublicationAuditKind::PublishFenceRejected,
            );
            return PublicationOutcome::Superseded(CurrentSourceEvidence {
                current_source: None,
            });
        }
        self.audit.push(
            request,
            artifact_id,
            PublicationAuditKind::PublishFencePassed,
        );
        let envelope = Arc::new(FrameworkArtifactEnvelope {
            id: artifact_id.clone(),
            source: accepted.source().clone(),
            artifact,
        });
        self.persistence.store_success(
            artifact_id,
            accepted,
            envelope.artifact(),
            current_persisted_carrier_artifact_cohort(),
        );
        self.audit
            .push(request, artifact_id, PublicationAuditKind::Published);
        PublicationOutcome::Published(envelope)
    }

    pub fn audit_snapshot(&self) -> PublicationAuditSnapshot {
        self.audit.snapshot()
    }

    pub fn audit_events(&self) -> Vec<PublicationAuditEvent> {
        self.audit
            .events
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }
}

fn parser_version_for(adapter: &FrameworkAdapterId) -> CarrierParserVersion {
    let cohort = current_persisted_carrier_artifact_cohort();
    if adapter.is_vue() {
        cohort.vue_parser_version()
    } else {
        cohort.svelte_parser_version()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrammarMismatch;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalBlockContentDeferred {
    pub acceptance: &'static str,
}

impl ExternalBlockContentDeferred {
    pub const B23: Self = Self { acceptance: "B-23" };
}

fn _acceptance_error_is_closed(_: CarrierAcceptanceError) {}
