//! The engine-backend layer of the project-bound external-TS contract.
//!
//! `EngineBackend` is the per-engine seam (a synchronous in-process tsserver
//! plugin vs an asynchronous out-of-process tsgo `--api` client both implement
//! it). This module defines the TRAIT and its data-transfer objects only — no
//! concrete engine lives here; the first real backend is a separate concern.
//!
//! ## The certified engine binding — the sole route to a production result
//!
//! A config-less / inferred-project operation for a production carrier source is
//! **not representable**. The ops that PRODUCE external-TS results
//! (`publish_snapshot` / `query` / `diagnostics`) are reachable only through a
//! [`CertifiedTypeEngineBinding`](crate::semantic_capability::CertifiedTypeEngineBinding),
//! and that binding is obtainable ONLY by [`certify`]-ing a [`BoundProject`]
//! witness — which is itself obtainable ONLY by calling
//! [`EngineBackend::ensure_project`] with an [`EnsureProject`] request, which in
//! turn can be minted ONLY from a resolved
//! [`ProjectBinding`](super::ProjectBinding) (its fields are private and it is
//! constructed only inside the contract module). `NoProject` / `Ambiguous`
//! carry no witness, so they cannot reach any production op. A `SyntheticScratch`
//! buffer carries a SEPARATE, clearly-labelled [`ScratchProject`] witness usable
//! ONLY for non-cross-file features (it never warms a project cache). Therefore
//! constructing a production provider op without a `ProjectBinding` is a COMPILE
//! error, not a runtime fallthrough.
//!
//! The [`BoundProject`] alone is NOT that route. It proves a project was
//! ensured; it says nothing about whether the engine's capability
//! interpretation was OBSERVED (an unobserved engine composes a default,
//! assumed profile), which serving session the answer belongs to, or which
//! input basis it is attributed to. Certification is what composes those three,
//! so a production-result op taking a bare `&BoundProject` would be a second,
//! uncertified route to an engine answer — the route this plane replaces. The
//! op signature is the enforcement: `&BoundProject` does not coerce to
//! `&CertifiedTypeEngineBinding`.

use std::sync::Arc;

use verter_identity::canonical::Canonical;
use verter_identity::encoding::{CanonicalEncode, CanonicalEncoder};
use verter_identity::identity::InputBasisId;
use verter_semantic::analysis::types::Hash16;

use crate::file_artifact_store::ProjectIdentity;
use crate::semantic_capability::CertifiedTypeEngineBinding;

/// The orthogonal environment dimensions a cache value actually depends on.
///
/// Per the project's fact-based-cache R21 rule this is NEVER a single bundled
/// `compiler_env_hash`: each cache layer keys only on the subset it depends on.
/// Reuses the established `Hash16` env-hash representation and the
/// [`ProjectIdentity`] newtype — no parallel env-hash types are introduced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct EnvDims {
    /// Parse-environment hash (parser options that affect the AST shape).
    pub parse_env_hash: Hash16,
    /// Module-resolution-environment hash (`paths`/`baseUrl`/`moduleResolution`/…).
    pub resolve_env_hash: Hash16,
    /// Lib-environment hash (`lib`/`target`/default-lib selection).
    pub lib_env_hash: Hash16,
    /// The project's identity (distinct configured Program identity).
    pub project_identity: ProjectIdentity,
}

/// The carrier role published in a snapshot file. Mirrors
/// [`super::CarrierRole`]; kept distinct so the wire DTO does not depend on the
/// registry layer's enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SnapshotRole {
    /// The `{name}.vue.tsx` / `{name}.svelte.tsx` interactive IDE carrier.
    CarrierIde,
    /// The redirect-reached `{name}.vue.verter.ts` public-API carrier.
    CarrierApi,
    /// A self-file shadow / rune-module surface.
    Shadow,
    /// A genuine non-carrier `.ts`/`.tsx` synced verbatim for context.
    Real,
}

/// The TypeScript `ScriptKind` of a snapshot file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScriptKind {
    Ts,
    Tsx,
    Js,
    Jsx,
}

/// Whether a snapshot file is open in an editor buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OpenState {
    /// Open in an editor (live buffer).
    Open,
    /// Closed (served from the published store but not editor-open).
    Closed,
}

/// The IDE/TSC feature a [`Query`] requests.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QueryFeature {
    Hover,
    Definition,
    TypeDefinition,
    References,
    Rename,
    Completion,
    SignatureHelp,
    DocumentHighlights,
    SemanticTokens,
    InlayHints,
}

/// `ensure_project` request — the project association DTO.
///
/// The fields are private and there is NO public constructor: the ONLY way to
/// obtain an `EnsureProject` is [`super::ProjectBinding::ensure_project_request`].
/// This is the first link in the bound-project witness
/// type-state chain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnsureProject {
    workspace_root: Arc<str>,
    tsconfig_uri: Arc<str>,
    ts_version: Arc<str>,
    project_identity: ProjectIdentity,
    env_dims: EnvDims,
}

impl EnsureProject {
    /// Crate-internal constructor used by the resolved-binding layer
    /// (`super::ProjectBinding`). Not `pub`: external code cannot fabricate an
    /// `EnsureProject`, so it cannot reach `ensure_project` without a binding.
    pub(super) fn new(
        workspace_root: Arc<str>,
        tsconfig_uri: Arc<str>,
        ts_version: Arc<str>,
        project_identity: ProjectIdentity,
        env_dims: EnvDims,
    ) -> Self {
        Self {
            workspace_root,
            tsconfig_uri,
            ts_version,
            project_identity,
            env_dims,
        }
    }

    /// The owning workspace root.
    #[must_use]
    pub fn workspace_root(&self) -> &str {
        &self.workspace_root
    }

    /// The owning tsconfig URI (the project the carrier is a member of).
    #[must_use]
    pub fn tsconfig_uri(&self) -> &str {
        &self.tsconfig_uri
    }

    /// The negotiated TypeScript version string.
    #[must_use]
    pub fn ts_version(&self) -> &str {
        &self.ts_version
    }

    /// The configured project's identity.
    #[must_use]
    pub fn project_identity(&self) -> ProjectIdentity {
        self.project_identity
    }

    /// The orthogonal env dimensions for this project.
    #[must_use]
    pub fn env_dims(&self) -> &EnvDims {
        &self.env_dims
    }
}

/// Content-free carrier ownership facts published beside a standard source map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotStructureStamp {
    pub schema_version: u32,
    pub artifact_token: Arc<str>,
    pub script_content_ranges: Vec<[u32; 2]>,
    /// Parser-identified markup opening-tag spans (elements plus recovered/
    /// unknown nodes retaining an opening span), in arena order. The plugin's
    /// bounded cursor-attribute classification lexes ONLY inside one of these
    /// parser-supplied spans — it never rediscovers `<` from raw source.
    pub markup_opening_ranges: Vec<[u32; 2]>,
}

/// One file in a published snapshot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotFile {
    pub source_uri: Arc<str>,
    pub provider_uri: Arc<str>,
    pub role: SnapshotRole,
    pub script_kind: ScriptKind,
    pub content: Arc<str>,
    pub content_hash: Hash16,
    pub map_hash: Hash16,
    /// The serialized `CodeTransform` source-map JSON (the `ProviderPositionMapper`
    /// JSON) the on-disk publish store writes to `maps/blake3-<map_hash>.json`.
    /// `None` when the carrier has no source map (a zero `map_hash`) OR when the
    /// publish path has not threaded the map JSON yet — a publisher that carries
    /// only the `map_hash` identity (the in-memory rename-mapping path) leaves this
    /// `None`, and the store then advertises no on-disk map blob (no broken
    /// pointer), the fail-closed two-phase rule applied to maps as well as content.
    pub map_json: Option<Arc<str>>,
    /// Content-free carrier ownership facts published beside, not inside, the
    /// standard source map so every source-map consumer sees unchanged JSON.
    pub structure: Option<SnapshotStructureStamp>,
    pub version: u64,
    pub open_state: OpenState,
}

/// `publish_snapshot` request: a per-project atomic delta batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublishSnapshot {
    /// The owning project (tsconfig URI).
    pub project: Arc<str>,
    pub files: Vec<SnapshotFile>,
    pub resolution_map_version: u64,
    pub fs_generation: u64,
}

/// The role discriminant a snapshot file's carrier role hashes through.
/// Exhaustive over [`SnapshotRole`], so a new role cannot alias an old one's
/// basis.
const fn snapshot_role_discriminant(role: SnapshotRole) -> u32 {
    match role {
        SnapshotRole::CarrierIde => 1,
        SnapshotRole::CarrierApi => 2,
        SnapshotRole::Shadow => 3,
        SnapshotRole::Real => 4,
    }
}

/// The script-kind discriminant a snapshot file hashes through. Exhaustive over
/// [`ScriptKind`].
const fn script_kind_discriminant(kind: ScriptKind) -> u32 {
    match kind {
        ScriptKind::Ts => 1,
        ScriptKind::Tsx => 2,
        ScriptKind::Js => 3,
        ScriptKind::Jsx => 4,
    }
}

/// The open-state discriminant a snapshot file hashes through. Exhaustive over
/// [`OpenState`].
const fn open_state_discriminant(state: OpenState) -> u32 {
    match state {
        OpenState::Open => 1,
        OpenState::Closed => 2,
    }
}

/// One published file's contribution to the snapshot basis.
struct SnapshotFileBasis<'a> {
    file: &'a SnapshotFile,
}

impl CanonicalEncode for SnapshotFileBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.external_ts.snapshot_file_basis.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, &self.file.source_uri);
        encoder.field_str(2, &self.file.provider_uri);
        encoder.field_enum_discriminant(3, snapshot_role_discriminant(self.file.role));
        encoder.field_enum_discriminant(4, script_kind_discriminant(self.file.script_kind));
        encoder.field_bytes(5, &self.file.content_hash);
        encoder.field_bytes(6, &self.file.map_hash);
        encoder.field_u64(7, self.file.version);
        encoder.field_enum_discriminant(8, open_state_discriminant(self.file.open_state));
        encoder.field_option(9, self.file.map_json.as_ref().map(|json| json.as_bytes()));
    }
}

/// Descriptor a [`PublishSnapshot`]'s basis hashes through: the identity the
/// store keys its own rows on — project, resolution-map version, FS generation,
/// and each published file's carrier identity and mapped bytes. Content and
/// geometry are covered by identity, never re-hashed.
struct PublishSnapshotBasis<'a> {
    snapshot: &'a PublishSnapshot,
}

impl CanonicalEncode for PublishSnapshotBasis<'_> {
    const DOMAIN_TAG: &'static str = "verter.session.external_ts.publish_snapshot_basis.v1";

    fn encode_fields(&self, encoder: &mut CanonicalEncoder) {
        encoder.field_str(1, &self.snapshot.project);
        encoder.field_u64(2, self.snapshot.resolution_map_version);
        encoder.field_u64(3, self.snapshot.fs_generation);
        // Each file enters as its own canonical digest, so the set is sorted by
        // identity and a repeated file collapses to one observation.
        let files = self
            .snapshot
            .files
            .iter()
            .map(|file| Canonical::from_encodable(&SnapshotFileBasis { file }).digest())
            .collect::<Vec<_>>();
        encoder.field_sorted_set(4, files.iter().map(|digest| digest.as_bytes()));
    }
}

impl PublishSnapshot {
    /// The basis a result published from THIS snapshot is attributed to.
    ///
    /// Derived from the snapshot's own identity, never supplied beside it: a
    /// caller cannot name one snapshot's basis and publish another's. The
    /// certification the production-result ops take is minted over exactly this
    /// value, and [`CertifiedTypeEngineBinding::publish_admitted`] admits
    /// exactly the snapshot it names — so a result from a superseded basis is
    /// refused before it can warm, never discovered afterwards.
    #[must_use]
    pub fn input_basis(&self) -> InputBasisId {
        InputBasisId::from_canonical(&PublishSnapshotBasis { snapshot: self })
    }
}

/// `query` request: a single carrier-offset feature query that fails closed on
/// carrier-identity / version mismatch.
///
/// §2.1: "every query carries the expected carrier identity (content-hash +
/// source-map id) and fails closed on mismatch (retry once, then no result)."
/// The carrier identity (`content_hash` + `map_hash`) is therefore part of the
/// query, reusing the same [`Hash16`] newtype [`SnapshotFile`] carries (no
/// parallel hash type), so a backend can refuse a query whose mapped position
/// was computed against carrier content / a source-map the live snapshot no
/// longer has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub project: Arc<str>,
    pub provider_uri: Arc<str>,
    pub carrier_offset: u32,
    pub feature: QueryFeature,
    /// The carrier content hash the caller's mapped position was computed
    /// against (fail-closed on mismatch). Same [`Hash16`] as [`SnapshotFile`].
    pub content_hash: Hash16,
    /// The `CodeTransform` source-map hash the caller's position was mapped
    /// through (fail-closed on mismatch). Same [`Hash16`] as [`SnapshotFile`].
    pub map_hash: Hash16,
    /// The snapshot version the caller's mapped position was computed against;
    /// the backend fails closed if the live snapshot differs.
    pub required_version: u64,
}

/// `diagnostics` request: whole-file or whole-project diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostics {
    pub project: Arc<str>,
    /// `None` ⇒ the whole project; `Some` ⇒ the named files only.
    pub files: Option<Vec<Arc<str>>>,
    /// The snapshot version the diagnostics must reflect (fail-closed gate).
    pub required_snapshot: u64,
}

/// Negotiated engine capabilities (never assumed — handshaked per engine).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EngineCapabilities {
    /// The engine exposes a static module-resolution-map endpoint (the
    /// non-fail-closed dissolution of the carrier-path conflict cases). The
    /// shipped tsgo `--api` does NOT.
    pub static_module_resolution_map: bool,
    /// The engine supports an async / cancellable query lane.
    pub async_cancellable_queries: bool,
    /// The engine version string it reported during the handshake.
    pub reported_version: Option<Arc<str>>,
}

/// An error a backend operation can fail with. Closed for the contract; backends
/// map their native errors into it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineError {
    /// The project could not be ensured (config parse / spawn failure).
    EnsureFailed(Arc<str>),
    /// The required snapshot version was not present (fail-closed).
    SnapshotMismatch { required: u64, actual: u64 },
    /// The backend is unavailable (process down, handshake failed).
    Unavailable(Arc<str>),
}

/// A seal token proving a [`BoundProject`] is being constructed by a real
/// backend's `ensure_project` (which itself required an [`EnsureProject`] minted
/// from a resolved binding). The token's constructor is `pub(super)` so only
/// this crate's contract module can mint it — a foreign `EngineBackend` impl in
/// another crate still receives the token through `ensure_project`'s signature
/// and cannot fabricate one.
#[derive(Debug)]
pub struct BoundProjectSeal(());

impl BoundProjectSeal {
    /// Mint the seal. Crate-internal: the witness chain stays closed. The seal
    /// never leaves this module — a foreign backend mints its [`BoundProject`]
    /// through [`BoundProject::from_ensured`] (which requires an [`EnsureProject`],
    /// itself mintable only from a resolved [`ProjectBinding`](super::ProjectBinding)),
    /// so the bound-project witness type-state holds across crates.
    pub(super) fn new() -> Self {
        Self(())
    }
}

/// The witness that a configured project has been ensured on a backend.
///
/// This is the gate for every production external-TS op. Its ONLY constructor
/// ([`BoundProject::sealed`]) requires a [`BoundProjectSeal`], minted only inside
/// this contract module after a resolved [`ProjectBinding`] produced an
/// [`EnsureProject`]. Holding a `BoundProject` is therefore PROOF that a
/// `ProjectBinding` existed — a config-less production op is uninstantiable.
#[derive(Debug, Clone)]
pub struct BoundProject {
    project: Arc<str>,
    capabilities: EngineCapabilities,
    env_dims: EnvDims,
}

impl BoundProject {
    /// Construct the witness. Requires the seal, so only the backend's
    /// `ensure_project` (reached through a [`ProjectBinding`]-minted
    /// [`EnsureProject`]) can produce one.
    #[must_use]
    pub fn sealed(
        _seal: BoundProjectSeal,
        project: Arc<str>,
        capabilities: EngineCapabilities,
        env_dims: EnvDims,
    ) -> Self {
        Self {
            project,
            capabilities,
            env_dims,
        }
    }

    /// Mint the witness directly from an [`EnsureProject`] request — the path a
    /// real [`EngineBackend`] in ANOTHER crate uses inside its `ensure_project`.
    ///
    /// This PRESERVES the bound-project witness type-state: an
    /// `EnsureProject` is itself mintable ONLY from a resolved [`ProjectBinding`]
    /// (its constructor is `pub(super)`), so requiring one here means a foreign
    /// backend still cannot fabricate a `BoundProject` without a binding. The
    /// project URI and env dims are READ FROM the request (the backend cannot
    /// substitute a mismatched project), and the seal is minted internally — the
    /// raw [`BoundProjectSeal`] never leaves the contract module. The backend
    /// supplies only its negotiated [`EngineCapabilities`].
    #[must_use]
    pub fn from_ensured(request: &EnsureProject, capabilities: EngineCapabilities) -> Self {
        Self::sealed(
            BoundProjectSeal::new(),
            Arc::clone(&request.tsconfig_uri),
            capabilities,
            *request.env_dims(),
        )
    }

    /// The owning project (tsconfig URI) this witness is bound to.
    #[must_use]
    pub fn project(&self) -> &str {
        &self.project
    }

    /// The owning project URI as the shared handle, for witnesses that retain
    /// the project's identity past this witness's own lifetime (the certified
    /// engine binding). Shares the same allocation; mints no copy.
    #[must_use]
    pub fn project_arc(&self) -> Arc<str> {
        Arc::clone(&self.project)
    }

    /// The backend's negotiated capabilities for this project.
    #[must_use]
    pub fn capabilities(&self) -> &EngineCapabilities {
        &self.capabilities
    }

    /// The env dimensions of the bound project.
    #[must_use]
    pub fn env_dims(&self) -> &EnvDims {
        &self.env_dims
    }
}

/// A seal for the scratch witness, mirroring [`BoundProjectSeal`] but for the
/// synthetic-scratch lane.
#[derive(Debug)]
pub struct ScratchProjectSeal(());

impl ScratchProjectSeal {
    /// Mint the scratch seal. See [`BoundProjectSeal::new`] for why this is
    /// `dead_code`-allowed in the additive contract.
    #[allow(dead_code)]
    pub(super) fn new() -> Self {
        Self(())
    }
}

/// The SEPARATE, clearly-labelled witness for a `SyntheticScratch` buffer
/// (untitled buffers / files outside any tsconfig).
///
/// It is usable ONLY for non-cross-file features and NEVER warms a project
/// cache. It is a DISTINCT type from [`BoundProject`]: a production cross-file op
/// takes `&BoundProject` and will not accept a `&ScratchProject`, so the scratch
/// lane cannot masquerade as production project semantics.
#[derive(Debug, Clone)]
pub struct ScratchProject {
    label: Arc<str>,
}

impl ScratchProject {
    /// Construct the scratch witness (seal-gated, like [`BoundProject::sealed`]).
    #[must_use]
    pub fn sealed(_seal: ScratchProjectSeal, label: Arc<str>) -> Self {
        Self { label }
    }

    /// A human-readable label identifying this scratch buffer.
    #[must_use]
    pub fn label(&self) -> &str {
        &self.label
    }
}

/// The per-engine backend seam. ONE implementor per engine kind.
///
/// No concrete backend exists in this module — it is the trait + its DTOs. The
/// production-result ops (`publish_snapshot` / `query` / `diagnostics`) take a
/// [`CertifiedTypeEngineBinding`], so they are unreachable without a resolved
/// [`ProjectBinding`] AND an observed capability profile, serving session and
/// input basis. `ensure_project` is the sole witness factory, and
/// [`CertifiedTypeEngineBinding::certify`] the sole route from that witness to
/// the binding these ops accept.
pub trait EngineBackend {
    /// Ensure the configured project's Program exists on the backend and return
    /// the [`BoundProject`] witness. This is the SOLE way to obtain the witness;
    /// the [`EnsureProject`] argument can only be minted from a resolved
    /// [`ProjectBinding`].
    fn ensure_project(&self, request: EnsureProject) -> Result<BoundProject, EngineError>;

    /// Publish a per-project atomic snapshot delta. Requires the certified
    /// binding over the snapshot's own [`PublishSnapshot::input_basis`] —
    /// [`CertifiedTypeEngineBinding::publish_admitted`] is the publication rule
    /// this op is reached through, so a stale basis fails closed instead of
    /// warming a superseded result.
    fn publish_snapshot(
        &self,
        project: &CertifiedTypeEngineBinding,
        snapshot: PublishSnapshot,
    ) -> Result<(), EngineError>;

    /// Answer a single feature query against a carrier offset. Requires the
    /// certified binding; fails closed on a version mismatch.
    fn query(
        &self,
        project: &CertifiedTypeEngineBinding,
        query: Query,
    ) -> Result<QueryOutcome, EngineError>;

    /// Compute diagnostics for a file set or the whole project. Requires the
    /// certified binding.
    fn diagnostics(
        &self,
        project: &CertifiedTypeEngineBinding,
        request: Diagnostics,
    ) -> Result<DiagnosticsOutcome, EngineError>;

    /// The backend's negotiated capabilities.
    fn capabilities(&self) -> EngineCapabilities;
}

/// The opaque outcome of a [`EngineBackend::query`]. The concrete per-feature
/// payload is mapped back through `ProviderPositionMapper` by the caller; the
/// contract only models presence vs fail-closed-empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryOutcome {
    /// The backend produced a result (the engine-native payload rides an
    /// out-of-band channel the contract does not model yet).
    Answered,
    /// The required snapshot was not yet synced — fail closed (no result).
    NoResultVersionMismatch,
}

/// The opaque outcome of [`EngineBackend::diagnostics`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiagnosticsOutcome {
    /// Diagnostics were produced for the requested snapshot version.
    Produced,
    /// The required snapshot was not present — fail closed.
    NoResultVersionMismatch,
}
