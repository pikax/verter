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
use persistence::{
    CarrierStableUnitStore, InMemoryStableUnitStore, RetainedStableUnit, StableUnitKey,
};

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

/// How many distinct immutable carrier stable units one publication store
/// keeps addressable before evicting the least recently used one.
///
/// A unit is one already-parsed carrier artifact, addressed by the content /
/// grammar / parse identity of its source rather than by the registered
/// snapshot generation it came from. The bound exists so that a long editing
/// session — which registers a new generation on every keystroke and revisits
/// earlier bytes on every undo — reuses its open files' units without the
/// store growing once per generation ever seen. Override with
/// [`CarrierPublicationStore::with_stable_unit_retention`].
pub const DEFAULT_STABLE_UNIT_RETENTION: usize = 64;

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

/// One in-flight parse of one immutable stable unit, shared by every
/// publication lane whose generation carries identical bytes.
///
/// Publication lanes are keyed by [`FrameworkArtifactId`], which is
/// snapshot-bound by design: two generations of identical bytes own two
/// lanes and would each run the parser. The stable key drops the
/// incarnation/generation axes, so concurrent generations of one unit join
/// one parse. The shared product is the snapshot-independent parse
/// artifact; each lane still validates currency, builds its own envelope
/// for its own snapshot, and retains independently.
#[derive(Clone)]
enum StableParseOutcome {
    Parsed(Arc<FrameworkParseArtifact>),
    Rejected(Arc<verter_language::SyntaxReject>),
    Panicked,
}

enum StableParseState {
    Producing,
    Ready(StableParseOutcome),
}

struct StableParseLane {
    state: Mutex<StableParseState>,
    wake: Condvar,
}

impl StableParseLane {
    fn producing() -> Self {
        Self {
            state: Mutex::new(StableParseState::Producing),
            wake: Condvar::new(),
        }
    }
}

/// What a lane leader does with a retained candidate: adopt it, or discard
/// it and proceed to fresh production.
enum StableAdoption {
    Adopted(PublicationOutcome),
    Discarded,
}

/// What the stable-key singleflight hands back: the shared parse product
/// (the caller runs its own currency fence and publishes its own
/// envelope), or an already-terminal outcome.
enum StableShared {
    Parsed(Arc<FrameworkParseArtifact>),
    Done(PublicationOutcome),
}

pub struct CarrierPublicationStore {
    source_authority: Arc<RegisteredSourceAuthority>,
    grammar_authority: Arc<CarrierGrammarAuthority>,
    lanes: Mutex<HashMap<FrameworkArtifactId, Arc<PublicationLane>>>,
    stable_parses: Mutex<HashMap<StableUnitKey, Arc<StableParseLane>>>,
    units: Arc<dyn CarrierStableUnitStore>,
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
            Arc::new(InMemoryStableUnitStore::default()),
            Arc::new(MetaProvenance::default()),
        )
    }

    /// Same as [`Self::new`] with an explicit stable-unit retention count —
    /// how many distinct immutable carrier units this store keeps addressable
    /// before evicting the least recently used one. Zero retains nothing.
    pub fn with_stable_unit_retention(
        source_authority: Arc<RegisteredSourceAuthority>,
        grammar_authority: Arc<CarrierGrammarAuthority>,
        retention: usize,
    ) -> Self {
        Self::with_dependencies(
            source_authority,
            grammar_authority,
            Arc::new(InMemoryStableUnitStore::with_capacity(retention)),
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
            Arc::new(InMemoryStableUnitStore::default()),
            provenance,
        )
    }

    pub(crate) fn with_dependencies(
        source_authority: Arc<RegisteredSourceAuthority>,
        grammar_authority: Arc<CarrierGrammarAuthority>,
        units: Arc<dyn CarrierStableUnitStore>,
        provenance: Arc<MetaProvenance>,
    ) -> Self {
        Self {
            source_authority,
            grammar_authority,
            lanes: Mutex::new(HashMap::new()),
            stable_parses: Mutex::new(HashMap::new()),
            units,
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
            let mut lanes = self
                .lanes
                .lock()
                .expect("publication lane table lock poisoned by a worker panic");
            if let Some(lane) = lanes.get(&artifact_id) {
                let lane = Arc::clone(lane);
                let mut state = lane
                    .state
                    .lock()
                    .expect("publication lane lock poisoned by a worker panic");
                let retired =
                    matches!(&*state, LaneState::Terminal(value) if value.artifact_expired());
                if retired {
                    *state = LaneState::Producing;
                }
                drop(state);
                (lane, retired, retired)
            } else {
                let lane = Arc::new(PublicationLane::vacant());
                let mut state = lane
                    .state
                    .lock()
                    .expect("publication lane lock poisoned by a worker panic");
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
            let mut state = lane
                .state
                .lock()
                .expect("publication lane lock poisoned by a worker panic");
            *state = LaneState::Terminal(TerminalOutcome::from_outcome(&outcome));
            lane.wake.notify_all();
            outcome
        } else {
            let mut state = lane
                .state
                .lock()
                .expect("publication lane lock poisoned by a worker panic");
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
                    Err(_) => panic!("publication lane lock poisoned by a worker panic"),
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
        if let Some(candidate) = self.units.retained(artifact_id, accepted) {
            match self.adopt_retained(candidate, accepted, request, artifact_id) {
                StableAdoption::Adopted(outcome) => {
                    return self.fence_adopted(outcome, accepted, request, artifact_id);
                }
                StableAdoption::Discarded => {}
            }
        }

        self.produce_fresh(accepted, request, artifact_id)
    }

    /// An adopted envelope is served under the SAME publish fence as a
    /// freshly published one. `publish_shared` retains the parse product
    /// regardless of currency and then fences the ENVELOPE on it; adoption
    /// serves an envelope too, so a lane whose generation lost currency
    /// while it was adopting must be superseded rather than serve a stale
    /// snapshot. Every adoption site — the initial retained read and the
    /// stable leader's double-checked read — routes through here, so the
    /// same losing generation never publishes `Superseded` on one path and
    /// `Adopted` on another.
    fn fence_adopted(
        &self,
        outcome: PublicationOutcome,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
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
        outcome
    }

    /// Adopt a retained stable unit for this lane's own snapshot: the
    /// envelope is bound to the CALLER's snapshot, never the generation the
    /// unit was parsed from. A discard falls through to fresh production.
    fn adopt_retained(
        &self,
        candidate: RetainedStableUnit,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> StableAdoption {
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
                        self.units.discard(artifact_id, accepted);
                        self.audit.push(
                            request,
                            artifact_id,
                            PublicationAuditKind::PersistentCandidateDiscarded,
                        );
                        return StableAdoption::Discarded;
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
                return StableAdoption::Adopted(PublicationOutcome::Adopted(envelope));
            }
            Err(rejection) => rejection,
        };
        self.units.discard(artifact_id, accepted);
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
        StableAdoption::Discarded
    }

    /// Run the parser exactly once per stable unit across concurrent lanes.
    /// The first lane leader to insert the stable key parses; every other
    /// lane leader with identical bytes waits for that product and then runs
    /// its own currency fence and envelope publication below.
    fn shared_parse(
        &self,
        key: &StableUnitKey,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> StableShared {
        let lane = {
            let mut inflight = self
                .stable_parses
                .lock()
                .expect("stable-parse table lock poisoned by a worker panic");
            if let Some(lane) = inflight.get(key) {
                Arc::clone(lane)
            } else {
                let lane = Arc::new(StableParseLane::producing());
                inflight.insert(key.clone(), Arc::clone(&lane));
                drop(inflight);
                return self.lead_stable_parse(&lane, key, accepted, request, artifact_id);
            }
        };
        self.await_stable_parse(&lane, request, artifact_id)
    }

    fn lead_stable_parse(
        &self,
        lane: &Arc<StableParseLane>,
        key: &StableUnitKey,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> StableShared {
        // Double-checked retained read: `produce` missed before the insert
        // above, but a previous leader may have retained and unlisted
        // between that miss and this leader's insert. Adopt the retained
        // unit instead of parsing it a second time. Guarded by the same
        // catch_unwind discipline as the parse/publish steps below: a
        // panic here must still reach `finish_stable_parse`, or this
        // stable key is left listed forever against a dead `Producing`
        // lane and every later publication of this unit joins it and
        // blocks until its own request cancellation.
        let double_check = catch_unwind(AssertUnwindSafe(|| {
            self.units
                .retained(artifact_id, accepted)
                .map(|candidate| self.adopt_retained(candidate, accepted, request, artifact_id))
        }));
        match double_check {
            Ok(Some(StableAdoption::Adopted(outcome))) => {
                if let PublicationOutcome::Adopted(envelope) = &outcome {
                    self.finish_stable_parse(
                        lane,
                        key,
                        StableParseOutcome::Parsed(Arc::clone(envelope.artifact())),
                    );
                } else {
                    self.finish_stable_parse(lane, key, StableParseOutcome::Panicked);
                }
                // The retained product is currency-independent and still
                // serves joiners above; only this lane's envelope is fenced.
                return StableShared::Done(self.fence_adopted(
                    outcome,
                    accepted,
                    request,
                    artifact_id,
                ));
            }
            Ok(Some(StableAdoption::Discarded)) | Ok(None) => {}
            Err(_) => {
                self.finish_stable_parse(lane, key, StableParseOutcome::Panicked);
                return StableShared::Done(PublicationOutcome::WinnerPanicked);
            }
        }
        let shared = match catch_unwind(AssertUnwindSafe(|| {
            self.parse_stable_unit(accepted, request, artifact_id)
        })) {
            Ok(shared) => shared,
            Err(_) => StableParseOutcome::Panicked,
        };
        match shared {
            StableParseOutcome::Parsed(artifact) => {
                // Publish before unlisting so joiners observe the product,
                // but keep the entry listed until this leader retains below:
                // a newcomer in between either joins this lane or lands on
                // the double-checked retained read — never on a second
                // parse of the same unit.
                *lane
                    .state
                    .lock()
                    .expect("stable-parse lane lock poisoned by a worker panic") =
                    StableParseState::Ready(StableParseOutcome::Parsed(Arc::clone(&artifact)));
                lane.wake.notify_all();
                let outcome = match catch_unwind(AssertUnwindSafe(|| {
                    self.publish_shared(&artifact, accepted, request, artifact_id)
                })) {
                    Ok(outcome) => outcome,
                    Err(payload) => {
                        // Never leak the in-flight key when publication
                        // panics: the outer `publish_or_get` converts the
                        // resumed unwind into `WinnerPanicked`.
                        self.finish_stable_parse(lane, key, StableParseOutcome::Parsed(artifact));
                        std::panic::resume_unwind(payload);
                    }
                };
                self.stable_parses
                    .lock()
                    .expect("stable-parse table lock poisoned by a worker panic")
                    .remove(key);
                StableShared::Done(outcome)
            }
            StableParseOutcome::Rejected(reject) => {
                let outcome = PublicationOutcome::Failed(CarrierParseFailure::ParserRejected(
                    Arc::clone(&reject),
                ));
                self.finish_stable_parse(lane, key, StableParseOutcome::Rejected(reject));
                StableShared::Done(outcome)
            }
            StableParseOutcome::Panicked => {
                self.finish_stable_parse(lane, key, StableParseOutcome::Panicked);
                StableShared::Done(PublicationOutcome::WinnerPanicked)
            }
        }
    }

    fn await_stable_parse(
        &self,
        lane: &Arc<StableParseLane>,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> StableShared {
        let mut state = lane
            .state
            .lock()
            .expect("stable-parse lane lock poisoned by a worker panic");
        loop {
            if request.cancellation.is_cancelled() {
                self.audit.push(
                    request,
                    artifact_id,
                    PublicationAuditKind::WaiterDetachedCancelled,
                );
                return StableShared::Done(PublicationOutcome::Cancelled);
            }
            match &*state {
                StableParseState::Ready(outcome) => {
                    return match outcome.clone() {
                        StableParseOutcome::Parsed(artifact) => StableShared::Parsed(artifact),
                        StableParseOutcome::Rejected(reject) => StableShared::Done(
                            PublicationOutcome::Failed(CarrierParseFailure::ParserRejected(reject)),
                        ),
                        StableParseOutcome::Panicked => {
                            StableShared::Done(PublicationOutcome::WinnerPanicked)
                        }
                    };
                }
                StableParseState::Producing => {}
            }
            let waited = lane.wake.wait_timeout(state, Duration::from_millis(5));
            state = match waited {
                Ok((state, _)) => state,
                Err(_) => panic!("stable-parse lane lock poisoned by a worker panic"),
            };
        }
    }

    fn finish_stable_parse(
        &self,
        lane: &Arc<StableParseLane>,
        key: &StableUnitKey,
        outcome: StableParseOutcome,
    ) {
        // Publish before unlisting: a newcomer that misses the entry must
        // land on the double-checked retained read (or a fresh in-flight
        // lane), never on a removed-but-unpublished parse.
        *lane
            .state
            .lock()
            .expect("stable-parse lane lock poisoned by a worker panic") =
            StableParseState::Ready(outcome);
        lane.wake.notify_all();
        self.stable_parses
            .lock()
            .expect("stable-parse table lock poisoned by a worker panic")
            .remove(key);
    }

    /// The one parser run for this stable unit: provenance counters and the
    /// `ParserStarted`/`ParserFinished` bracket fire once here, on the
    /// stable leader only.
    fn parse_stable_unit(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> StableParseOutcome {
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
                    return StableParseOutcome::Rejected(Arc::new(reject));
                }
            };
        let artifact = Arc::new(projection.into_framework_parse_artifact());
        self.audit
            .push(request, artifact_id, PublicationAuditKind::ParserFinished);
        StableParseOutcome::Parsed(artifact)
    }

    fn produce_fresh(
        &self,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
        let key = StableUnitKey::new(artifact_id, accepted);
        let artifact = match self.shared_parse(&key, accepted, request, artifact_id) {
            StableShared::Parsed(artifact) => artifact,
            StableShared::Done(outcome) => return outcome,
        };
        self.publish_shared(&artifact, accepted, request, artifact_id)
    }

    /// Per-lane publication of a shared stable-unit product: this lane's own
    /// currency fence, its own snapshot-bound envelope, and its own retain.
    /// Every lane leader runs this — the stable leader and each joiner — so
    /// sharing a parse never shares currency or provenance.
    fn publish_shared(
        &self,
        artifact: &Arc<FrameworkParseArtifact>,
        accepted: &AcceptedRegisteredCarrierSource,
        request: &PublicationRequestContext,
        artifact_id: &FrameworkArtifactId,
    ) -> PublicationOutcome {
        if artifact.parse_key() != &artifact_id.parse_key
            || artifact.adapter_id() != &artifact_id.adapter_id
            || artifact.language_id() != &artifact_id.language_id
        {
            return PublicationOutcome::RegistryMismatch(RegistryMismatch::ProducerVersionMismatch);
        }
        // The stable unit's identity is byte- and grammar-bound, independent
        // of snapshot currency: retain the verified parse product even when
        // this lane's own generation lost currency mid-parse, so a revisit
        // of these bytes adopts instead of re-parsing. Only the envelope
        // below stays fenced on currency.
        self.units.retain(
            artifact_id,
            accepted,
            artifact,
            current_persisted_carrier_artifact_cohort(),
        );
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
        // Rebind this lane's own snapshot into the envelope's embedded
        // registered geometry, exactly like the retained-adoption path's
        // `__rehome_registered` above: a parse shared from another lane
        // leader still carries THAT leader's source-space snapshot id, so
        // publishing it unrebound here would pair this lane's own
        // `envelope.source` with a stale embedded snapshot identity. The
        // artifact retained above stays the original, generation-independent
        // parse product — only the published envelope is rehomed.
        //
        // The stable leader publishing its own freshly-parsed artifact is
        // already bound to `accepted`'s own snapshot (it was parsed from
        // exactly this `accepted` in `parse_stable_unit`): rehoming there is
        // an identity no-op that still pays a full inventory geometry copy
        // plus a structure-hash recompute. Skip it whenever the embedded
        // registered snapshot already matches this lane's own — every other
        // route (a joiner sharing a different lane's parse, or an adopted
        // retained unit) still rehomes.
        let already_homed = matches!(
            artifact.inventory().source_spaces().first().map(|space| &space.identity),
            Some(verter_language::SourceSpaceIdentity::RegisteredSnapshot { snapshot })
                if snapshot == accepted.source().snapshot_id()
        );
        let rehomed = if already_homed {
            Arc::clone(artifact)
        } else {
            match artifact.__rehome_registered(accepted, &artifact_id.parse_key) {
                Ok(rehomed) => Arc::new(rehomed),
                // Exhaustive: every `SyntaxReject` arm means this lane's own
                // `accepted` no longer matches the shared artifact's identity —
                // matched by name (not `_`) so a new variant forces a decision
                // here instead of silently inheriting a fallback.
                Err(
                    verter_language::SyntaxReject::UnsupportedProfile { .. }
                    | verter_language::SyntaxReject::RejectedSyntax { .. }
                    | verter_language::SyntaxReject::UnmappedDiagnostic { .. }
                    | verter_language::SyntaxReject::InvalidCarrierGeometry { .. },
                ) => {
                    return PublicationOutcome::RegistryMismatch(
                        RegistryMismatch::ProducerVersionMismatch,
                    );
                }
            }
        };
        let envelope = Arc::new(FrameworkArtifactEnvelope {
            id: artifact_id.clone(),
            source: accepted.source().clone(),
            artifact: rehomed,
        });
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
