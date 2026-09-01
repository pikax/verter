//! Carrier-only registered publication authority.

pub mod persistence;

use std::collections::HashMap;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use sha2::{Digest, Sha256};
use verter_compiler::framework_common::FrameworkParseArtifact;
use verter_language::carrier_grammar::{
    AcceptedRegisteredCarrierSource, CarrierAcceptanceError, CarrierGrammarAuthority,
    CarrierGrammarFingerprint, GrammarAuthorityNamespaceId,
};
use verter_language::registered_source_authority::{
    FileIncarnation, RegisteredSourceAuthority, RegisteredSourceSnapshot,
    RegisteredSourceSnapshotId, SourceAuthorityNamespaceId, SourceGeneration,
};
use verter_language::{FrameworkAdapterId, LanguageId, ParseKey};
use verter_scheduler::cancellation::CancellationToken;

use crate::carrier_artifact_cohort::current_persisted_carrier_artifact_cohort;
use crate::types::MetaProvenance;
use persistence::{CarrierPersistence, InMemoryCarrierPersistence};

pub(crate) type RegisteredEnvelopeIngest =
    Arc<parking_lot::Mutex<rustc_hash::FxHashMap<String, RegisteredFileStructure>>>;
pub(crate) type SourceAuthorityHandle = Arc<RegisteredSourceAuthority>;
pub(crate) type GrammarAuthorityHandle = Arc<CarrierGrammarAuthority>;
pub(crate) type PublicationStoreHandle = Arc<CarrierPublicationStore>;

/// Host-owned carrier publication handles: the registered-source and
/// grammar authorities, the publication store, and the one-shot validated
/// cross-host envelope ingest (T-B R5 §2 — entries are removed on intake;
/// NOT a cache), grouped so the root `VerterHost` struct stays thin.
pub(crate) struct CarrierPublicationHostHandles {
    pub(crate) source_authority: SourceAuthorityHandle,
    pub(crate) grammar_authority: GrammarAuthorityHandle,
    pub(crate) publication_store: PublicationStoreHandle,
    pub(crate) envelope_ingest: RegisteredEnvelopeIngest,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CarrierParseFailure {
    ParserRejected(Arc<verter_language::SyntaxReject>),
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
    parse_key: ParseKey,
    /// Pre-hashed canonical bytes for the opaque public-token family. Public
    /// block/node/attribute tokens are minted repeatedly from one artifact;
    /// retaining this basis avoids formatting the full identity through its
    /// `Debug` representation and re-hashing that allocation for every local
    /// reference.
    public_token_basis: [u8; 32],
}

impl FrameworkArtifactId {
    fn derive(accepted: &AcceptedRegisteredCarrierSource, parse_key: ParseKey) -> Self {
        let mut artifact = Self {
            authority: accepted.source().authority(),
            source: accepted.source().snapshot_id().clone(),
            grammar_authority: accepted.grammar().authority(),
            grammar_fingerprint: accepted.grammar().fingerprint(),
            adapter_id: accepted.grammar().adapter_id().clone(),
            language_id: accepted.grammar().language_id().clone(),
            parse_key,
            public_token_basis: [0; 32],
        };
        artifact.public_token_basis = framework_artifact_token_basis(&artifact);
        artifact
    }
}

macro_rules! public_structure_token {
    ($name:ident) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub struct $name(Arc<str>);

        impl $name {
            pub fn as_str(&self) -> &str {
                &self.0
            }

            pub fn as_bytes(&self) -> &[u8] {
                self.as_str().as_bytes()
            }

            pub fn is_empty(&self) -> bool {
                self.as_str().is_empty()
            }

            /// Parse an opaque token crossing a wire boundary. This validates
            /// only the bounded token envelope; authority and liveness are
            /// validated by the owning host after capture.
            pub fn parse_untrusted(value: impl Into<Arc<str>>) -> Option<Self> {
                let value = value.into();
                (!value.is_empty() && value.len() <= 256 && !value.chars().any(char::is_control))
                    .then_some(Self(value))
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }
    };
}

public_structure_token!(FrameworkArtifactToken);
public_structure_token!(ArtifactBlockToken);
public_structure_token!(ArtifactNodeToken);
public_structure_token!(ArtifactAttributeToken);
public_structure_token!(ArtifactSourceSpaceToken);

/// An artifact-bound block reference. Construction is owned by
/// [`RegisteredFileStructure`], so a local block id cannot be spliced onto a
/// different carrier artifact.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FrameworkBlockRef {
    artifact: FrameworkArtifactId,
    local: verter_language::parse_artifact::carrier_inventory::ArtifactBlockRef,
}

impl FrameworkBlockRef {
    pub fn artifact_id(&self) -> &FrameworkArtifactId {
        &self.artifact
    }

    pub fn block_id(&self) -> verter_language::parse_artifact::carrier_inventory::BlockId {
        self.local.block_id()
    }

    /// The sealed inventory-minted local ref: the association key analysis
    /// carriers store and consumers full-identity-join against.
    pub fn artifact_block_ref(
        &self,
    ) -> &verter_language::parse_artifact::carrier_inventory::ArtifactBlockRef {
        &self.local
    }
}

/// An artifact-bound markup-node reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactNodeRef {
    artifact: FrameworkArtifactId,
    node: verter_language::parse_artifact::carrier_inventory::MarkupNodeId,
}

impl ArtifactNodeRef {
    pub fn artifact_id(&self) -> &FrameworkArtifactId {
        &self.artifact
    }

    pub fn node_id(&self) -> verter_language::parse_artifact::carrier_inventory::MarkupNodeId {
        self.node
    }
}

/// An artifact-bound carrier-attribute reference.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ArtifactAttributeRef {
    artifact: FrameworkArtifactId,
    attribute: verter_language::parse_artifact::carrier_inventory::AttributeId,
}

impl ArtifactAttributeRef {
    pub fn artifact_id(&self) -> &FrameworkArtifactId {
        &self.artifact
    }

    pub fn attribute_id(&self) -> verter_language::parse_artifact::carrier_inventory::AttributeId {
        self.attribute
    }
}

fn update_len_prefixed(digest: &mut Sha256, bytes: &[u8]) {
    digest.update((bytes.len() as u64).to_le_bytes());
    digest.update(bytes);
}

fn framework_artifact_token_basis(artifact: &FrameworkArtifactId) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"verter.framework-artifact-token-basis.v2\0");
    digest.update(artifact.authority.as_bytes());
    digest.update(artifact.source.canonical_digest().as_bytes());
    digest.update(artifact.source.file_incarnation().get().to_le_bytes());
    digest.update(artifact.source.generation().get().to_le_bytes());
    digest.update(artifact.source.content_hash().as_bytes());
    digest.update(artifact.grammar_authority.as_bytes());
    digest.update(artifact.grammar_fingerprint.as_bytes());
    update_len_prefixed(&mut digest, artifact.adapter_id.as_str().as_bytes());
    update_len_prefixed(&mut digest, artifact.language_id.as_str().as_bytes());
    update_len_prefixed(&mut digest, artifact.parse_key.canonical_bytes());
    digest.finalize().into()
}

fn public_token(domain: &[u8], artifact: &FrameworkArtifactId, local: Option<u32>) -> Arc<str> {
    let mut digest = Sha256::new();
    digest.update(b"verter.structure-token.v2\0");
    digest.update(domain);
    digest.update(artifact.public_token_basis);
    if let Some(local) = local {
        digest.update(local.to_le_bytes());
    }
    Arc::from(base64url_32(digest.finalize().into()))
}

fn base64url_32(bytes: [u8; 32]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(43);
    let mut index = 0;
    while index + 3 <= bytes.len() {
        let value = u32::from(bytes[index]) << 16
            | u32::from(bytes[index + 1]) << 8
            | u32::from(bytes[index + 2]);
        out.push(ALPHABET[((value >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((value >> 12) & 63) as usize] as char);
        out.push(ALPHABET[((value >> 6) & 63) as usize] as char);
        out.push(ALPHABET[(value & 63) as usize] as char);
        index += 3;
    }
    let value = u32::from(bytes[index]) << 16 | u32::from(bytes[index + 1]) << 8;
    out.push(ALPHABET[((value >> 18) & 63) as usize] as char);
    out.push(ALPHABET[((value >> 12) & 63) as usize] as char);
    out.push(ALPHABET[((value >> 6) & 63) as usize] as char);
    out
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
        self.artifact.inventory()
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

    pub fn block_ref(
        &self,
        block: verter_language::parse_artifact::carrier_inventory::BlockId,
    ) -> Option<FrameworkBlockRef> {
        Some(FrameworkBlockRef {
            artifact: self.artifact_id().clone(),
            // Sealed mint: the inventory is the sole authority for the
            // artifact-bound local ref (content-addressed identity).
            local: self.inventory().block_ref(block)?,
        })
    }

    pub fn node_ref(
        &self,
        node: verter_language::parse_artifact::carrier_inventory::MarkupNodeId,
    ) -> Option<ArtifactNodeRef> {
        self.inventory().markup().nodes().get(node.get() as usize)?;
        Some(ArtifactNodeRef {
            artifact: self.artifact_id().clone(),
            node,
        })
    }

    pub fn attribute_ref(
        &self,
        attribute: verter_language::parse_artifact::carrier_inventory::AttributeId,
    ) -> Option<ArtifactAttributeRef> {
        let exists = self
            .inventory()
            .blocks()
            .iter()
            .filter_map(|block| match block {
                verter_language::parse_artifact::carrier_inventory::CarrierBlock::Section {
                    syntax,
                    ..
                } => Some(syntax.attributes.as_ref()),
                verter_language::parse_artifact::carrier_inventory::CarrierBlock::MarkupRoot {
                    ..
                } => None,
            })
            .flatten()
            .chain(
                self.inventory()
                    .markup()
                    .nodes()
                    .iter()
                    .flat_map(|node| node.kind().attributes()),
            )
            .any(|candidate| candidate.id() == attribute);
        exists.then(|| ArtifactAttributeRef {
            artifact: self.artifact_id().clone(),
            attribute,
        })
    }

    pub fn public_artifact_token(&self) -> FrameworkArtifactToken {
        FrameworkArtifactToken(public_token(b"artifact\0", self.artifact_id(), None))
    }

    pub fn public_block_token(&self, block: &FrameworkBlockRef) -> Option<ArtifactBlockToken> {
        (block.artifact == *self.artifact_id()).then(|| {
            ArtifactBlockToken(public_token(
                b"block\0",
                self.artifact_id(),
                Some(block.block_id().get()),
            ))
        })
    }

    pub fn public_node_token(&self, node: &ArtifactNodeRef) -> Option<ArtifactNodeToken> {
        (node.artifact == *self.artifact_id()).then(|| {
            ArtifactNodeToken(public_token(
                b"node\0",
                self.artifact_id(),
                Some(node.node_id().get()),
            ))
        })
    }

    pub fn public_attribute_token(
        &self,
        attribute: &ArtifactAttributeRef,
    ) -> Option<ArtifactAttributeToken> {
        (attribute.artifact == *self.artifact_id()).then(|| {
            ArtifactAttributeToken(public_token(
                b"attribute\0",
                self.artifact_id(),
                Some(attribute.attribute_id().get()),
            ))
        })
    }

    pub fn public_source_space_token(
        &self,
        source_space: verter_language::parse_artifact::carrier_inventory::SourceSpaceId,
    ) -> Option<ArtifactSourceSpaceToken> {
        self.inventory()
            .source_spaces()
            .get(source_space.get() as usize)?;
        Some(ArtifactSourceSpaceToken(public_token(
            b"source-space\0",
            self.artifact_id(),
            Some(source_space.get()),
        )))
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

impl HostSourceRevisionToken {
    pub fn public_token(self) -> String {
        let mut digest = Sha256::new();
        digest.update(b"verter.host-source-revision.v2\0");
        digest.update(self.host_instance.get().to_le_bytes());
        digest.update(self.file_incarnation.get().to_le_bytes());
        digest.update(self.source_generation.get().to_le_bytes());
        base64url_32(digest.finalize().into())
    }
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
        let parse_key = parse_key_for_accepted(accepted);
        let artifact_id = FrameworkArtifactId::derive(accepted, parse_key);
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
                self.produce(accepted, &request, &artifact_id)
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
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
        let language = accepted.source().resolved_file_language();
        let Some(adapter_id) = language.adapter_id() else {
            return PublicationOutcome::Unsupported(
                RegisteredCarrierUnsupported::NoRegisteredProducer,
            );
        };
        let Some(carrier_language_id) = language.carrier_language_id() else {
            return PublicationOutcome::Unsupported(
                RegisteredCarrierUnsupported::NoRegisteredProducer,
            );
        };
        if verter_compiler::framework_common::registered_carrier_projection::registered_frontend_for(
            adapter_id,
            carrier_language_id,
        )
        .is_none()
        {
            return PublicationOutcome::Unsupported(
                RegisteredCarrierUnsupported::NoRegisteredProducer,
            );
        }
        if let Some(candidate) = self.persistence.take_candidate(artifact_id, accepted) {
            self.audit.push(
                request,
                artifact_id,
                PublicationAuditKind::PersistentCandidateFound,
            );
            let rejection = match candidate.validate(
                accepted,
                artifact_id,
                current_persisted_carrier_artifact_cohort(),
            ) {
                Ok(()) => {
                    let artifact = match candidate
                        .artifact
                        .__rehome_registered(accepted, &artifact_id.parse_key)
                    {
                        Ok(artifact) => artifact,
                        // Exhaustive: every `SyntaxReject` arm means the persisted
                        // candidate no longer matches `accepted`'s live identity —
                        // fall back to fresh production. Matched by name (not `_`)
                        // so a new `SyntaxReject` variant forces a decision here
                        // instead of silently inheriting the fallback.
                        Err(
                            verter_language::SyntaxReject::UnsupportedProfile { .. }
                            | verter_language::SyntaxReject::RejectedSyntax { .. }
                            | verter_language::SyntaxReject::UnmappedDiagnostic { .. }
                            | verter_language::SyntaxReject::InvalidCarrierGeometry { .. },
                        ) => {
                            self.audit.push(
                                request,
                                artifact_id,
                                PublicationAuditKind::PersistentAdoptionRejected(
                                    PersistentAdoptionRejection::ParserValidationFailed,
                                ),
                            );
                            self.audit.push(
                                request,
                                artifact_id,
                                PublicationAuditKind::PersistentCandidateDiscarded,
                            );
                            return self.produce_fresh(accepted, request, artifact_id);
                        }
                    };
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

        self.produce_fresh(accepted, request, artifact_id)
    }

    fn produce_fresh(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
        use std::sync::atomic::Ordering::Relaxed;
        self.provenance.carrier_parses.fetch_add(1, Relaxed);
        if accepted.grammar().adapter_id().is_vue() {
            self.provenance.sfc_parses.fetch_add(1, Relaxed);
        }
        self.audit
            .push(request, artifact_id, PublicationAuditKind::ParserStarted);
        let projection = match verter_compiler::framework_common::registered_carrier_projection::project_registered_accepted(
            accepted,
        ) {
                Ok(projection) => projection,
                Err(reject) => {
                    // A reject is a COMPLETED parse attempt (the frontend ran and
                    // produced a definitive typed answer), not an abandoned one —
                    // `ParserFinished` brackets `ParserStarted` on both the
                    // success and the reject path.
                    self.audit
                        .push(request, artifact_id, PublicationAuditKind::ParserFinished);
                    return PublicationOutcome::Failed(CarrierParseFailure::ParserRejected(
                        Arc::new(reject),
                    ));
                }
            };
        let artifact = Arc::new(projection.into_framework_parse_artifact());
        self.audit
            .push(request, artifact_id, PublicationAuditKind::ParserFinished);
        if artifact.parse_key() != &artifact_id.parse_key
            || artifact.adapter_id() != &artifact_id.adapter_id
            || artifact.language_id() != &artifact_id.language_id
        {
            return PublicationOutcome::RegistryMismatch(RegistryMismatch::ProducerVersionMismatch);
        }
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

fn parse_key_for_accepted(accepted: &AcceptedRegisteredCarrierSource) -> ParseKey {
    use verter_language::carrier_grammar::CarrierGrammarConfig;
    let options = match accepted.grammar().canonical_config() {
        CarrierGrammarConfig::Vue { delimiters, .. } => verter_language::ParseOptions {
            delimiters: (
                delimiters.open().to_string(),
                delimiters.close().to_string(),
            ),
            custom_elements: accepted
                .grammar()
                .canonical_config()
                .custom_element_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            svelte_loose: false,
        },
        CarrierGrammarConfig::Svelte => verter_language::ParseOptions::default(),
    };
    let language = accepted.source().resolved_file_language();
    let syntax_profile = verter_language::syntax_profile_id_for(language, &options)
        .expect("accepted carrier grammar has a supported syntax profile");
    let (domain, epoch) = if language.is_vue() {
        (
            verter_language::VUE_SYNTAX_COMPATIBILITY_DOMAIN,
            verter_language::VUE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    } else {
        (
            verter_language::SVELTE_SYNTAX_COMPATIBILITY_DOMAIN,
            verter_language::SVELTE_SYNTAX_COMPATIBILITY_EPOCH,
        )
    };
    verter_language::parse_key_for(
        accepted.source().bytes(),
        language,
        domain,
        epoch,
        &syntax_profile,
    )
    .expect("accepted carrier source has a supported parse identity")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrammarMismatch;

fn _acceptance_error_is_closed(_: CarrierAcceptanceError) {}
