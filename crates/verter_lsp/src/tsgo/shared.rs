//! The SHARED editor-attach tsgo provider.
//!
//! `TsgoSharedProvider` serves external-TypeScript results by ATTACHING to the
//! editor's already-running `tsgo` engine — never a Verter-spawned engine. It
//! composes three landed substrates:
//!
//! - the relay-shim CONTROL protocol ([`verter_tsgo_api::control::ControlClient`]):
//!   the editor spawns a `verter-relay-shim` as its `tsgo`; a `verter_lsp` process
//!   discovers the shim's advertisement, verifies the nonce + editor binding, and
//!   drives carrier injection (`carrierDidOpenSynced`) through the shim's gated
//!   injection channel. Verter NEVER touches the raw editor↔tsgo wire — the shim
//!   owns the relay + the carrier egress taint.
//! - the directly-connected `--api` checker ([`verter_tsgo_api::api_attach::ApiAttachClient`]):
//!   the shim mints an `--api` session (`initializeApiSession`) and returns its
//!   pipe; `verter_lsp` connects it DIRECTLY and OWNS the `--api` queries
//!   (`updateSnapshot`, `getSemanticDiagnostics`) — the shim stays dumb.
//! - the live session DECISION layer ([`verter_session::external_ts`]): SHARED is
//!   served ONLY when the queried component's decision is
//!   [`ServeMode::Shared`] — composed from five provenance-typed eligibility facts
//!   (version gate, attach liveness, project binding, proxy availability, editor
//!   binding). Incomplete or partial evidence yields OWNED (fail-open is
//!   forbidden); OWNED is the universal baseline the caller falls back to.
//!
//! Map-back is the OWNED two-step, reused verbatim: an `--api` UTF-16 diagnostic
//! offset maps to a carrier BYTE span through the shared
//! [`verter_type_runtime::tsgo::position_carrier_diagnostics`] authority (built
//! over the carrier text Verter itself injected), and the LSP feature layer maps
//! that carrier span to the `.vue`/`.svelte` source through the document's
//! `ProviderPositionMapper`. There is NO forged `(0,0)` fallback — a carrier whose
//! content is unavailable fails closed.
//!
//! ## Serving scope
//!
//! This provider is the SHARED DIAGNOSTICS / project-bound typecheck oracle
//! (proven live against the real 7.0.1-rc engine). Interactive features
//! (hover/completion/definition/…) over the SHARED path — where the editor's own
//! `--lsp` surface serves `.ts` features directly and `.vue`-carrier feature
//! mapping is layered on top — are a residual supervised full-DX concern
//! and are not served here; SHARED is opt-in and fail-closed, so absent that
//! surface the caller uses the OWNED provider (full features) as the baseline.

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex as SyncMutex;
use tokio::sync::Mutex as AsyncMutex;

use verter_span::path::canonicalize_path;
use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::control::{Advertisement, ControlClient};
use verter_tsgo_api::gate::{self, ObservedEngine};
use verter_tsgo_api::jsonrpc::JsonRpcConnection;
use verter_tsgo_api::proto::types::OpaqueHandle;
use verter_tsgo_api::transport::pipe_attach::connect_attach_pipe;

use verter_session::external_ts::{
    compose_eligibility, decide_live, AttachFact, BindingFact, ComponentModeDecision,
    ConfigPathProbe, EditorBindingFact, EligibilityFacts, EngineSessionCandidates,
    EngineSessionFacts, EngineWarmCache, LiveDecision, LiveDecisionRequest, LiveProjectInput,
    OwnedSessionFacts, ProjectIdentitySource, ProjectResolution, ProxyFact, ReferenceInput,
    ServeMode, SharedSessionFacts, VersionGateFact,
};
use verter_session::file_artifact_store::ProjectIdentity;

use verter_type_runtime::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation, TypeProviderError,
};
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};
use verter_type_runtime::tsgo::{position_carrier_diagnostics, select_configured_project_carrier};

/// Why establishing a SHARED attach did not yield a SHARED provider. Every
/// variant fails CLOSED to the OWNED baseline (the caller falls through to
/// `try_spawn_tsgo`) — SHARED is never fabricated from incomplete evidence.
#[derive(Debug)]
pub enum EstablishError {
    /// No shim advertisement was discoverable for the workspace's session key —
    /// no editor-spawned relay to attach to.
    NoShim(String),
    /// The control handshake (`hello` / `waitInitialized`) failed or was refused.
    Handshake(String),
    /// The in-band engine-version witness did not clear the fail-closed wire gate.
    VersionGate(String),
    /// Minting or connecting the `--api` session failed.
    ApiSession(String),
    /// Every precondition held EXCEPT that the composed decision was not SHARED —
    /// the component is served OWNED (the reason is carried for diagnostics).
    NotShared(ComponentModeDecision),
}

impl std::fmt::Display for EstablishError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EstablishError::NoShim(m) => write!(f, "no relay-shim advertisement: {m}"),
            EstablishError::Handshake(m) => write!(f, "control handshake failed: {m}"),
            EstablishError::VersionGate(m) => write!(f, "engine version gate not green: {m}"),
            EstablishError::ApiSession(m) => write!(f, "--api session unavailable: {m}"),
            EstablishError::NotShared(decision) => write!(
                f,
                "component decided OWNED ({:?}) — SHARED not served",
                decision.owned_reason()
            ),
        }
    }
}

impl std::error::Error for EstablishError {}

/// The inputs to a live SHARED attach establishment.
pub struct EstablishSharedParams<'a> {
    /// The rendezvous control directory the editor's shim advertised into.
    pub control_dir: &'a Path,
    /// The `--session-key` the shim published under.
    pub session_key: &'a str,
    /// The workspace root (the editor-binding witness the advertisement is matched
    /// against, and the base for the resolved binding).
    pub workspace_root: &'a str,
    /// The forward-slashed configured tsconfig path opened on the `--api` side.
    pub tsconfig_path: &'a str,
    /// The resolved project binding evidence for the carrier's owning project. A
    /// real [`ProjectResolution::ProjectBinding`] is the ONLY value that yields a
    /// [`BindingFact::Bound`]; every other state fails closed to OWNED.
    pub resolution: ProjectResolution,
    /// The published snapshot / config generation the binding was resolved at — the
    /// warm-key `config_generation` dimension for the establishment decision, so a
    /// same-generation first per-query re-decision reuses the establishment's warm entry
    /// instead of keying a dead slot. NOT the editor-session generation (that is already
    /// the `EngineIdentity.editor_session_generation` dimension).
    pub config_generation: u64,
    /// A free-form client label for the control hello (diagnostics only).
    pub client_label: &'a str,
}

/// The [`ConfigPathProbe`] for the live decision's reference canonicalization. A
/// realpath/symlink resolution needs on-disk existence, which this pure decision
/// site does not perform, so it fails closed (`None`) — the reference resolver then
/// uses the collapsed (`.`/`..`-normalized, slash-folded) path as the canonical form.
/// It is invoked for every redirect-ON reference the queried binding threads in.
struct NoReferenceProbe;
impl ConfigPathProbe for NoReferenceProbe {
    fn realpath(&self, _canonical: &str) -> Option<String> {
        None
    }
}

/// The [`ProjectIdentitySource`] for the live decision's references: it maps a
/// resolved reference's canonical config path to a stable content-derived identity
/// (blake3 → the 16-byte key), so two reference URIs denoting the same config
/// collapse to ONE identity. A referenced project absent from the single-project
/// decision snapshot therefore keys a distinct, present-nowhere identity — the
/// closure's absent member the fail-closed `IncompleteComponent` rule catches. Never
/// a fabricated zero.
struct PathHashIdentitySource;
impl ProjectIdentitySource for PathHashIdentitySource {
    fn identity_for(&self, canonical_config_path: &str) -> ProjectIdentity {
        stable_project_identity(canonical_config_path)
    }
}

/// A stable content identity for a canonical path (blake3 → the 16-byte
/// [`ProjectIdentity`] key). Deterministic across processes so the same canonical
/// path always keys the same identity.
fn stable_project_identity(canonical: &str) -> ProjectIdentity {
    let digest = blake3::hash(canonical.as_bytes());
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest.as_bytes()[..16]);
    ProjectIdentity(bytes)
}

/// Compose the five provenance-typed eligibility facts and decide the ONE serve
/// mode for the queried carrier's redirect-ON reference-connected component
/// through the shared live decision layer.
///
/// This is the SOLE mode oracle: SHARED requires ALL-positive evidence (version
/// gate cleared ∧ attach live ∧ project bound ∧ proxy available ∧ editor binding
/// matched) AND a fully-resolved redirect-ON reference closure; any missing/negative
/// fact — or a redirect-ON reference whose referenced project is not a proven-eligible
/// member of the decision snapshot — composes to OWNED. A SHARED result caches its
/// serving state under the composite warm key; OWNED is never warmed.
///
/// `references` are the queried project's redirect-ON project references (from the
/// resolved `ProjectBinding`), threaded into the decision snapshot so mode is decided
/// over the whole reference-connected component, never per single tsconfig: a project
/// that references another (redirect-ON) whose eligibility the snapshot cannot prove
/// fails CLOSED to OWNED (`IncompleteComponent`) rather than serving one endpoint of a
/// cross-project edge SHARED while the other is OWNED.
///
/// Pure over its typed inputs + the warm cache — no engine contact — so the
/// fail-open negatives can drive it discriminatingly (each fact missing in turn
/// yields OWNED).
#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn decide_shared_serve(
    version_gate: VersionGateFact,
    attach: AttachFact,
    binding: BindingFact,
    proxy: ProxyFact,
    editor_binding: EditorBindingFact,
    node_identity: ProjectIdentity,
    tsconfig_dir: &str,
    canonical_tsconfig: Arc<str>,
    references: &[ReferenceInput],
    owned_session: OwnedSessionFacts,
    config_generation: u64,
    editor_binding_identity: ProjectIdentity,
    warm_cache: &mut EngineWarmCache,
) -> LiveDecision {
    // The SHARED session candidate exists IFF the attach is live — a would-be
    // SHARED decision with no live attach fails closed to OWNED rather than
    // fabricating SHARED session facts.
    let shared_session = match &attach {
        AttachFact::Live(facts) => Some(facts.clone()),
        AttachFact::NotLive => None,
    };
    let facts = EligibilityFacts {
        version_gate,
        attach,
        binding,
        proxy,
        editor_binding,
    };
    // Compose once here so an eligibility misread is caught before the request is
    // built; `decide_live` recomposes from the same facts internally.
    let _ = compose_eligibility(&facts);

    let engines = EngineSessionCandidates {
        owned: owned_session,
        shared: shared_session,
    };
    let project = LiveProjectInput {
        identity: node_identity,
        tsconfig_dir,
        canonical_tsconfig,
        facts,
        references,
    };
    let request = LiveDecisionRequest {
        root: node_identity,
        projects: std::slice::from_ref(&project),
        engines: &engines,
        config_generation,
        editor_binding: editor_binding_identity,
    };
    decide_live(
        &request,
        &NoReferenceProbe,
        &PathHashIdentitySource,
        warm_cache,
    )
}

/// The live shared-mode controller — the re-decidable replacement for a frozen
/// per-provider decision.
///
/// It retains the establishment-level eligibility facts (version gate, attach
/// session, proxy, editor binding, OWNED session — all stable across carriers for
/// one editor session) plus the composite warm cache, so the serve mode is RE-DECIDED
/// per query for the queried carrier's resolved binding at the CURRENT snapshot/config
/// generation ([`Self::decide`]) — never frozen at construction. The decision is
/// memoized per reference-closure in the warm cache (keyed on the snapshot/config
/// generation, the representative project, the editor generation, and the observed
/// engine version), so a new published snapshot re-decides (a superseded-generation
/// entry is unreachable under the new generation) while a same-generation repeat
/// reuses the warm serving state.
pub struct SharedModeController {
    version_gate: VersionGateFact,
    attach: AttachFact,
    proxy: ProxyFact,
    editor_binding: EditorBindingFact,
    editor_bound_identity: ProjectIdentity,
    owned_session: OwnedSessionFacts,
    /// The observed engine version (from the attach version gate) — a warm-key
    /// dimension carried on the session facts.
    observed_version: Arc<str>,
    /// The EngineIdentity-keyed warm cache of SHARED serving state.
    warm_cache: Arc<SyncMutex<EngineWarmCache>>,
    /// The establishment decision (the attach-time gate). The FROZEN
    /// single-decision field is replaced by this controller; this is the initial
    /// state, re-decided per query via [`Self::decide`].
    establishment: LiveDecision,
}

impl SharedModeController {
    /// Re-decide the serve mode for a carrier's resolved `binding_fact` /
    /// `node_identity` / `canonical_tsconfig` at `config_generation` (the
    /// snapshot/config generation). The generation is a warm-key dimension, so a
    /// fresh published snapshot re-decides COLD (a superseded-generation SHARED
    /// serving state is unreachable under the new generation and never reused) while
    /// a same-generation repeat reuses the warm state. Pure over the retained facts
    /// + the warm cache — no engine contact.
    #[must_use]
    pub fn decide(
        &self,
        binding_fact: BindingFact,
        node_identity: ProjectIdentity,
        canonical_tsconfig: Arc<str>,
        references: &[ReferenceInput],
        config_generation: u64,
    ) -> LiveDecision {
        let tsconfig_dir = parent_dir(&canonical_tsconfig);
        let mut guard = self.warm_cache.lock();
        decide_shared_serve(
            self.version_gate.clone(),
            self.attach.clone(),
            binding_fact,
            self.proxy,
            self.editor_binding,
            node_identity,
            &tsconfig_dir,
            canonical_tsconfig,
            references,
            self.owned_session.clone(),
            config_generation,
            self.editor_bound_identity,
            &mut guard,
        )
    }

    /// The establishment (attach-time) decision.
    #[must_use]
    pub fn establishment_decision(&self) -> &LiveDecision {
        &self.establishment
    }

    /// The observed engine version the attach gate accepted.
    #[must_use]
    pub fn observed_version(&self) -> &Arc<str> {
        &self.observed_version
    }

    /// The warm cache of SHARED serving state.
    #[must_use]
    pub fn warm_cache(&self) -> &Arc<SyncMutex<EngineWarmCache>> {
        &self.warm_cache
    }
}

/// The SHARED editor-attach tsgo provider.
pub struct TsgoSharedProvider {
    /// The relay-shim control client — the SOLE carrier-injection path (through
    /// the shim's gated injection channel; Verter never mutates leak policy).
    control: ControlClient,
    /// The directly-connected `--api` checker (the SHARED diagnostics / typecheck
    /// oracle).
    api: ApiAttachClient,
    /// The live shared-mode controller (re-decides the serve mode per query; the
    /// frozen single-decision field is replaced by it).
    controller: SharedModeController,
    /// The configured tsconfig path (forward-slashed) opened on the `--api` side —
    /// the representative project resolved at establishment. Per-query diagnostics
    /// open the carrier's OWN resolved tsconfig via
    /// [`TsgoSharedProvider::overlay_diagnostics_in_project`].
    tsconfig_path: String,
    /// The in-band engine-version witness the wire gate accepted — flowed to the
    /// `--api` `updateSnapshot` rail (never a hardcoded literal).
    observed_version: String,
    /// The per-carrier ORDERED injection state ([`CarrierSyncState`]): the
    /// barrier-SYNCED carrier slots (the ONLY content served / positioned from), the
    /// per-carrier async gates that serialize each carrier's wire send + barrier +
    /// commit (so a concurrent open/change never desyncs the overlay), and the
    /// latest-pending coalescing cells. The UTF-16 diagnostic index is built ONLY from
    /// a slot's barrier-SYNCED content — never the optimistically-reserved in-flight
    /// injection — so the local overlay view never diverges from the Program on a sync
    /// timeout.
    sync: CarrierSyncState,
    /// The current `--api` snapshot context `(snapshot_handle, project_id)`,
    /// refreshed on demand.
    snapshot: SyncMutex<Option<(OpaqueHandle, String)>>,
}

impl std::fmt::Debug for TsgoSharedProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoSharedProvider")
            .field("tsconfig", &self.tsconfig_path)
            .field("mode", &self.controller.establishment_decision().mode())
            .field(
                "serving",
                &self.controller.establishment_decision().serving(),
            )
            .finish_non_exhaustive()
    }
}

impl TsgoSharedProvider {
    /// Establish a live SHARED attach and build the provider, or fail CLOSED.
    ///
    /// The full non-owning handshake: discover the shim advertisement under
    /// `params.session_key`, verify the nonce + editor binding on `hello`, take the
    /// in-band `serverInfo.version` witness on `waitInitialized` and gate it, mint
    /// the `--api` session and connect its pipe DIRECTLY, then compose the five
    /// eligibility facts from the live evidence + the resolved binding and decide
    /// the serve mode. Only a SHARED decision yields a provider; any missing
    /// positive fact (or a non-SHARED decision) returns [`EstablishError`] and the
    /// caller falls through to the OWNED baseline.
    pub async fn establish_shared(
        params: EstablishSharedParams<'_>,
    ) -> Result<Self, EstablishError> {
        // 1. Discover the editor-spawned shim's advertisement (never "attach to
        //    the first live shim" — keyed on the session key).
        let (_, adv) = Advertisement::find_for_session_key(params.control_dir, params.session_key)
            .map_err(|e| EstablishError::NoShim(e.to_string()))?;

        // 2. Connect + hello (nonce verified) + waitInitialized (in-band witness).
        let mut control = ControlClient::connect(&adv.endpoint)
            .await
            .map_err(|e| EstablishError::Handshake(format!("connect control endpoint: {e}")))?;
        let hello = control
            .hello(&adv.nonce, params.client_label)
            .await
            .map_err(|e| EstablishError::Handshake(format!("hello: {e}")))?;
        let witness = control
            .wait_initialized()
            .await
            .map_err(|e| EstablishError::Handshake(format!("waitInitialized: {e}")))?;

        // 3. Gate the IN-BAND engine-version witness (fail-closed wire gate).
        let observed_raw = witness.server_info_version.clone().ok_or_else(|| {
            EstablishError::VersionGate(
                "the initialize witness carried no serverInfo.version".into(),
            )
        })?;
        let clearance = gate::validate(&ObservedEngine::from_in_band_server_info(&observed_raw))
            .map_err(|e| EstablishError::VersionGate(e.to_string()))?;
        let observed_version: Arc<str> = Arc::from(clearance.observed_version);

        // 4. Mint the `--api` session and connect its pipe DIRECTLY.
        let api_session = control
            .initialize_api_session()
            .await
            .map_err(|e| EstablishError::ApiSession(format!("initializeApiSession: {e}")))?;
        let endpoint = api_session
            .endpoint()
            .ok_or_else(|| EstablishError::ApiSession("no minted --api endpoint".into()))?;
        let (read, write) = connect_attach_pipe(endpoint)
            .await
            .map_err(|e| EstablishError::ApiSession(format!("connect --api pipe: {e}")))?;
        let api = ApiAttachClient::new(JsonRpcConnection::connect(read, write));
        api.initialize()
            .await
            .map_err(|e| EstablishError::ApiSession(format!("--api initialize: {e}")))?;

        // 5. Compose the eligibility evidence from the live handshake + the
        //    resolved binding, then decide the serve mode.
        let binding_fact = BindingFact::from_resolution(&params.resolution);
        let node_identity = match &params.resolution {
            ProjectResolution::ProjectBinding(b) => b.env_dims().project_identity,
            _ => stable_project_identity(params.tsconfig_path),
        };
        // The editor-binding witness, keyed on the resolved PROJECT identity (never a
        // bare workspace-root hash): two distinct configured projects under the SAME
        // `rootUri` produce DISTINCT editor-binding facts, so eligibility from one can
        // never spill to another. Routes through the ONE `editor_binding_matches`
        // primitive via `EditorBindingFact::evaluate`.
        let (editor_binding_fact, editor_bound) = resolve_editor_binding(
            node_identity,
            params.workspace_root,
            witness.root_uri.as_deref(),
        );

        // The live attach produced real SHARED-session facts (the sealed provenance
        // type), so attach-liveness is witnessed by the api session, never a bare flag.
        let shared_session = SharedSessionFacts::new(EngineSessionFacts {
            observed_version: Arc::clone(&observed_version),
            wire_pin: api_session.wire_pin,
            editor_session_generation: hello.editor_session_generation,
        });
        // The OWNED baseline session-candidate facts for the mode decision. In the
        // SHARED-attach scenario there is exactly ONE engine (the editor's tsgo), so
        // these are DERIVED from the SAME attach evidence as `shared_session` — the
        // api-session wire pin + the editor session generation — NOT independently
        // observed OWNED-spawn facts: the decision compares the two candidates over the
        // same engine identity. (`OwnedSessionFacts` names the candidate ROLE in the
        // decision, not a separate spawned process.)
        let owned_session = OwnedSessionFacts::new(EngineSessionFacts {
            observed_version: Arc::clone(&observed_version),
            wire_pin: api_session.wire_pin,
            editor_session_generation: hello.editor_session_generation,
        });

        let tsconfig_dir = parent_dir(params.tsconfig_path);
        let canonical_tsconfig: Arc<str> = Arc::from(params.tsconfig_path);
        let warm_cache = Arc::new(SyncMutex::new(EngineWarmCache::new()));

        // The establishment-level eligibility facts (stable across carriers for this
        // editor session), retained by the live controller for per-query re-decision.
        let version_gate = VersionGateFact::Cleared {
            observed_version: Arc::clone(&observed_version),
        };
        let attach = AttachFact::Live(shared_session);
        let proxy = ProxyFact::Available;

        // The queried project's redirect-ON references (from the resolved binding),
        // threaded so the establishment gate decides over the whole reference-connected
        // component, never per single tsconfig — a references-bearing project whose
        // closure the snapshot cannot prove eligible fails closed to OWNED here.
        let references = match &params.resolution {
            ProjectResolution::ProjectBinding(b) => redirect_on_references(b),
            ProjectResolution::NoProject
            | ProjectResolution::Ambiguous(_)
            | ProjectResolution::SyntheticScratch(_) => Vec::new(),
        };

        // The establishment (attach-time) decision through the ONE shared live
        // decision layer — the initial gate.
        let decision = {
            let mut guard = warm_cache.lock();
            decide_shared_serve(
                version_gate.clone(),
                attach.clone(),
                binding_fact,
                proxy,
                editor_binding_fact,
                node_identity,
                &tsconfig_dir,
                Arc::clone(&canonical_tsconfig),
                &references,
                owned_session.clone(),
                params.config_generation,
                editor_bound,
                &mut guard,
            )
        };

        if decision.mode() != ServeMode::Shared {
            // Fail closed: tear the control session down (best-effort) and hand the
            // OWNED decision back so the caller uses the baseline provider.
            let _ = control.detach(true).await;
            return Err(EstablishError::NotShared(decision.decision().clone()));
        }

        let controller = SharedModeController {
            version_gate,
            attach,
            proxy,
            editor_binding: editor_binding_fact,
            editor_bound_identity: editor_bound,
            owned_session,
            observed_version: Arc::clone(&observed_version),
            warm_cache,
            establishment: decision,
        };

        Ok(Self {
            control,
            api,
            controller,
            tsconfig_path: params.tsconfig_path.to_string(),
            observed_version: observed_version.to_string(),
            sync: CarrierSyncState::new(),
            snapshot: SyncMutex::new(None),
        })
    }

    /// The decided serve mode (always [`ServeMode::Shared`] for a constructed
    /// provider — the OWNED baseline is refused at construction).
    #[must_use]
    pub fn serve_mode(&self) -> ServeMode {
        self.controller.establishment_decision().mode()
    }

    /// The establishment SHARED serving decision. The per-query serve mode is
    /// re-decided through the live controller ([`Self::redecide_for_binding`]).
    #[must_use]
    pub fn decision(&self) -> &LiveDecision {
        self.controller.establishment_decision()
    }

    /// The live shared-mode controller (the re-decidable replacement for a frozen
    /// per-provider decision).
    #[must_use]
    pub fn controller(&self) -> &SharedModeController {
        &self.controller
    }

    /// The observed engine version the attach gate accepted — the witness the
    /// composite's per-query `BoundProject` mint keys on (never a hardcoded literal).
    #[must_use]
    pub fn observed_version(&self) -> &str {
        &self.observed_version
    }

    /// Whether this SHARED attach is still LIVE — the transport-eviction signal the
    /// composite's [`LazyTransport`](crate::tsgo::transport_cell::LazyTransport) reads
    /// to evict a dead `Live` transport. Dead when the control attach reported
    /// `verter/fatal` or its connection closed (the shim's relay/engine is gone), OR
    /// the `--api` checker connection closed. A dead attach fails closed to OWNED and
    /// re-establishes on a fresh advertisement/config generation.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        self.control.is_alive() && !self.api.connection().is_closed()
    }

    /// Re-decide the serve mode for a per-query resolved `binding` at the current
    /// snapshot/config `generation`, through the live controller — the re-decidable
    /// replacement for the frozen establishment decision.
    #[must_use]
    pub fn redecide_for_binding(
        &self,
        binding: &verter_session::external_ts::ProjectBinding,
        generation: u64,
    ) -> LiveDecision {
        let references = redirect_on_references(binding);
        self.controller.decide(
            BindingFact::from_resolution(&ProjectResolution::ProjectBinding(binding.clone())),
            binding.env_dims().project_identity,
            Arc::from(binding.tsconfig_uri()),
            &references,
            generation,
        )
    }

    /// The warm cache the SHARED serving state is keyed into (for reconnect
    /// supersession handling / inspection).
    #[must_use]
    pub fn warm_cache(&self) -> &Arc<SyncMutex<EngineWarmCache>> {
        self.controller.warm_cache()
    }

    /// The `--api` SEMANTIC diagnostics for a carrier, positioned to carrier BYTE
    /// offsets — the SHARED project-bound typecheck oracle.
    ///
    /// Refreshes the configured-project snapshot, requires the carrier is a Program
    /// ROOT of the configured project (fail closed to `Ok(vec![])` for a
    /// non-member, never a wrong-project result), gets its `--api` diagnostics, and
    /// maps their UTF-16 offsets to carrier bytes through the SAME
    /// [`position_carrier_diagnostics`] authority the OWNED path uses — over the
    /// carrier text Verter injected. The `.vue`/`.svelte` map-back is the feature
    /// layer's `ProviderPositionMapper` step (never a forged `(0,0)`).
    pub async fn semantic_diagnostics_for_carrier(
        &self,
        path: &str,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        let carrier = slash(path);
        Ok(self
            .api_carrier_diagnostics(&carrier, &self.tsconfig_path)
            .await?
            .unwrap_or_default())
    }

    /// The SHARED `--api` carrier diagnostics for the OVERLAY consumer, in the
    /// carrier's OWN per-query resolved configured project (`tsconfig`), with the
    /// SERVED-vs-not signal the composite needs to fail closed:
    ///
    /// - `Ok(Some(diags))` — SHARED served the carrier in its configured project
    ///   (overlay these, even when empty: SHARED is the diagnostics authority for a
    ///   served carrier).
    /// - `Ok(None)` — SHARED did NOT serve (the project would not open, or the
    ///   carrier is not a Program root of it): the caller falls back to OWNED.
    /// - `Err(_)` — the `--api` call itself failed, or a diagnostic could not be
    ///   positioned (no forged `(0,0)`): the caller falls back to OWNED.
    pub async fn overlay_diagnostics_in_project(
        &self,
        path: &str,
        tsconfig: &str,
    ) -> Result<Option<Vec<TypeDiagnostic>>, TypeProviderError> {
        let carrier = slash(path);
        self.api_carrier_diagnostics(&carrier, tsconfig).await
    }

    /// The shared `--api` carrier-diagnostics core: open `tsconfig` on the checker,
    /// require the carrier is a Program ROOT of that configured project (fail closed
    /// to `Ok(None)` for a non-member / non-openable project — never a wrong-project
    /// result), get its `--api` diagnostics, and map their UTF-16 offsets to carrier
    /// bytes through the SAME [`position_carrier_diagnostics`] authority the OWNED
    /// path uses — over the carrier text Verter injected. The `.vue`/`.svelte`
    /// map-back is the feature layer's `ProviderPositionMapper` step (never a forged
    /// `(0,0)`).
    async fn api_carrier_diagnostics(
        &self,
        carrier: &str,
        tsconfig: &str,
    ) -> Result<Option<Vec<TypeDiagnostic>>, TypeProviderError> {
        let snap = match self
            .api
            .update_snapshot_open_project(tsconfig, &self.observed_version)
            .await
        {
            Ok(snap) => snap,
            Err(err) => {
                tracing::warn!(
                    "shared tsgo `--api` could not open the configured project `{tsconfig}`: {err}"
                );
                return Ok(None);
            }
        };
        let Some((project_id, engine_carrier)) =
            select_configured_project_carrier(&snap, tsconfig, carrier)
        else {
            return Ok(None);
        };
        *self.snapshot.lock() = Some((snap.snapshot, project_id.clone()));

        let diags = self
            .api
            .get_semantic_diagnostics(&snap.snapshot, &project_id, &engine_carrier)
            .await
            .map_err(|e| {
                TypeProviderError::new(format!("shared --api getSemanticDiagnostics: {e}"))
            })?;

        // The engine may report the carrier under a different canonicalization than
        // the key Verter injected under; look up by the engine's form first, then
        // fall back to the injected key. Serve ONLY the barrier-SYNCED content (the
        // text the shared Program actually accepted) — a reserved-but-not-yet-synced
        // carrier yields `None` (fail-closed, no unaccepted text positioned against).
        let content = self.sync.synced_content(&engine_carrier, carrier);
        Ok(Some(position_carrier_diagnostics(
            &diags,
            content,
            &engine_carrier,
        )?))
    }

    /// Drive one ORDERED per-carrier lifecycle op (inject or close) through the shim's
    /// gated control channel, committing to the diagnostic index ONLY once the sync
    /// barrier confirms the shared Program accepted it — driven as the ORDERED
    /// per-carrier state machine ([`CarrierSyncState::drive`]).
    ///
    /// The per-carrier wire send + barrier + commit is SERIALIZED (a per-carrier async
    /// gate spanning the barrier), so a `didChange` can never race ahead of the first
    /// `didOpen`, a `didClose` can never interleave with an in-flight/queued injection
    /// (no op after a committed close; a close supersedes an older queued injection), a
    /// stale/timed-out earlier op can never retract/overwrite content a later op already
    /// committed, and a burst of edits COALESCES to the latest op (the gate holder always
    /// drains the newest pending). This method supplies the wire sink (the shim CONTROL
    /// channel: `carrier_did_open_synced` / `carrier_did_change_synced` /
    /// `carrier_did_close`); the ordering + local-slot consistency ([`reserve_carrier`] /
    /// [`sync_commit`] — a first-open barrier failure best-effort RETRACTS the
    /// possibly-open Program file and drops the local slot; a `didChange` failure keeps
    /// the PRIOR synced content; a close drops the slot after its barrier) live in the
    /// state machine.
    async fn drive_carrier(&self, path: &str, kind: PendingKind) -> Result<(), TypeProviderError> {
        let carrier = slash(path);
        let uri = path_to_file_uri(&carrier);
        let language_id = language_id_for(&carrier);

        self.sync
            .drive(&carrier, kind, |op| async {
                match op {
                    CarrierWireOp::Open { version, content } => self
                        .control
                        .carrier_did_open_synced(&uri, language_id, version, &content)
                        .await
                        .map_err(|e| {
                            TypeProviderError::new(format!("shared carrier didOpen: {e}"))
                        }),
                    CarrierWireOp::Change { version, content } => self
                        .control
                        .carrier_did_change_synced(&uri, version, &content)
                        .await
                        .map_err(|e| {
                            TypeProviderError::new(format!("shared carrier didChange: {e}"))
                        }),
                    CarrierWireOp::Close => {
                        self.control.carrier_did_close(&uri).await.map_err(|e| {
                            TypeProviderError::new(format!("shared carrier didClose: {e}"))
                        })
                    }
                }
            })
            .await
    }

    /// Inject (or refresh) a carrier overlay — the [`PendingKind::Inject`] entry of the
    /// ordered per-carrier state machine.
    async fn inject_carrier(&self, path: &str, content: &str) -> Result<(), TypeProviderError> {
        self.drive_carrier(path, PendingKind::Inject(Arc::from(content)))
            .await
    }

    /// Close a carrier overlay — the [`PendingKind::Close`] entry of the ordered
    /// per-carrier state machine. Routed through the SAME gate as injection so the
    /// `didClose` is ordered w.r.t. an in-flight/queued injection (never a reopen after
    /// a committed close) and supersedes an older queued injection; the state machine
    /// drops the local slot after the close barrier.
    async fn close_carrier_overlay(&self, path: &str) -> Result<(), TypeProviderError> {
        self.drive_carrier(path, PendingKind::Close).await
    }
}

/// The redirect-ON [`ReferenceInput`]s a resolved [`ProjectBinding`] carries — its
/// resolved project references, each a real potential redirect-ON graph edge. Threaded
/// into the decision snapshot so the serve mode is decided over the whole
/// reference-connected component (a referenced project the snapshot cannot prove
/// eligible fails the component closed to OWNED), never per single tsconfig.
fn redirect_on_references(
    binding: &verter_session::external_ts::ProjectBinding,
) -> Vec<ReferenceInput> {
    binding
        .references()
        .iter()
        .map(|r| ReferenceInput::redirect_on(Arc::clone(r)))
        .collect()
}

/// A resolved carrier wire operation an injection sink performs — the ordered state
/// machine ([`CarrierSyncState::drive`]) decides the action + version and hands the
/// sink the COALESCED content; the sink maps it onto the shim CONTROL channel.
enum CarrierWireOp {
    /// The carrier's FIRST reservation — send `didOpen` (version 1) with its content.
    Open { version: i64, content: Arc<str> },
    /// An already-reserved carrier — send `didChange` at a monotonic version.
    Change { version: i64, content: Arc<str> },
    /// Send `didClose` — remove the carrier's overlay from the shared Program. Issued
    /// BOTH for a top-level carrier close AND for the best-effort retract of a
    /// possibly-open Program file after a first-open barrier failed (both end the doc);
    /// the retract variant's result is ignored (the local slot is dropped regardless).
    Close,
}

/// The lifecycle op a coalesced submission resolves to — an injection carrying its
/// coalesced content, or a close.
#[derive(Clone)]
enum PendingKind {
    /// Inject (open/change) the carrier at the coalesced content.
    Inject(Arc<str>),
    /// Close (retract) the carrier overlay.
    Close,
}

/// The latest-pending coalescing cell for one carrier. A gate holder drains the NEWEST
/// submitted lifecycle op (`latest_*` — an inject-with-content OR a close) rather than
/// replaying each intermediate submission, and skips entirely when the newest has
/// already been committed (`latest_seq <= committed_seq`) by an earlier gate holder — so
/// a burst of edits (and a trailing close) reaches the Program in ~one barrier and the
/// LATEST op always wins: a close SUPERSEDES an older queued injection, and a newer
/// injection SUPERSEDES an older close (a genuine reopen).
struct PendingSubmission {
    /// The newest submitted op + its global submission sequence.
    latest_seq: u64,
    latest_kind: PendingKind,
    /// The highest submission sequence a gate holder has SUCCESSFULLY committed
    /// (barrier-synced). Advanced only on a successful commit.
    committed_seq: u64,
}

/// The per-carrier ORDERED lifecycle state machine.
///
/// Concurrent open/change/close on the SAME carrier URI (the host has multiple provider
/// sync paths — did_change eager, the debounced coordinator, foreground / background /
/// import — plus the close path, and does NOT serialize per carrier) used to desync the
/// SHARED overlay: a `didChange` could race ahead of the first `didOpen`, a first-open
/// timeout could retract a slot a concurrent change had promoted, or a `didClose` could
/// interleave with an in-flight injection and reopen a closed carrier (an op after a
/// committed close). This state machine SERIALIZES each carrier's wire send + barrier +
/// commit behind a per-carrier ASYNC gate (a `tokio::sync::Mutex`, correctly held across
/// the barrier await — never a sync lock across `.await`), COALESCES a burst of edits
/// (and a trailing close) to the latest op, and keeps the local slot view consistent
/// with the shared Program on failure/timeout. Open, change, AND close all flow through
/// the SAME gate + coalescing cell, so the newest submission always wins: a close
/// supersedes an older queued injection (no op after a committed close) and a newer
/// injection supersedes an older close (a genuine reopen). Fail-closed: a broken
/// connection surfaces as an `Err` the composite treats as OWNED.
struct CarrierSyncState {
    /// The barrier-SYNCED carrier slots (the ONLY content served / positioned from),
    /// keyed by forward-slashed carrier path.
    injected: SyncMutex<HashMap<String, CarrierSlot>>,
    /// Per-carrier async gates serializing each carrier's wire send + barrier + commit
    /// so lifecycle ops commit in submission order (an Open barrier before any Change;
    /// a close after a committed injection, never reopening it).
    gates: SyncMutex<HashMap<String, Arc<AsyncMutex<()>>>>,
    /// Per-carrier latest-pending coalescing cells.
    pending: SyncMutex<HashMap<String, PendingSubmission>>,
    /// A GLOBAL monotonic `didChange` version counter. LSP requires only that each
    /// `didChange` version exceed the document's previous version; a single monotonic
    /// counter guarantees that for EVERY carrier. `didOpen` uses version 1; this starts
    /// at 2.
    next_version: AtomicI64,
    /// A GLOBAL monotonic submission-sequence counter (the coalescing discriminant).
    next_seq: AtomicU64,
}

impl CarrierSyncState {
    fn new() -> Self {
        Self {
            injected: SyncMutex::new(HashMap::new()),
            gates: SyncMutex::new(HashMap::new()),
            pending: SyncMutex::new(HashMap::new()),
            next_version: AtomicI64::new(2),
            next_seq: AtomicU64::new(1),
        }
    }

    /// The last barrier-SYNCED content for a carrier (by the engine's canonicalization
    /// first, then the injected key) — the ONLY content served / positioned from.
    fn synced_content(&self, engine_carrier: &str, carrier: &str) -> Option<Arc<str>> {
        synced_content(&self.injected, engine_carrier, carrier)
    }

    /// The per-carrier async gate (get-or-insert). A brief sync lock fetches the `Arc`;
    /// the caller then awaits the async gate — the sync lock is never held across the
    /// await.
    fn gate_for(&self, carrier: &str) -> Arc<AsyncMutex<()>> {
        Arc::clone(self.gates.lock().entry(carrier.to_string()).or_default())
    }

    /// Record `kind` as the carrier's latest pending submission at `seq` (a later
    /// submission overwrites an earlier one — the coalescing target). An inject and a
    /// close share the one cell, so the newest op supersedes an older one of EITHER
    /// kind (a close supersedes a queued inject; a reopen supersedes an older close).
    fn record_pending(&self, carrier: &str, seq: u64, kind: PendingKind) {
        let mut pending = self.pending.lock();
        match pending.get_mut(carrier) {
            Some(p) if seq > p.latest_seq => {
                p.latest_seq = seq;
                p.latest_kind = kind;
            }
            Some(_) => {}
            None => {
                pending.insert(
                    carrier.to_string(),
                    PendingSubmission {
                        latest_seq: seq,
                        latest_kind: kind,
                        committed_seq: 0,
                    },
                );
            }
        }
    }

    /// The newest pending op still needing a sync — `None` when the latest has already
    /// been committed (an earlier gate holder synced this-or-newer op).
    fn take_drainable(&self, carrier: &str) -> Option<(u64, PendingKind)> {
        let pending = self.pending.lock();
        let p = pending.get(carrier)?;
        (p.latest_seq > p.committed_seq).then(|| (p.latest_seq, p.latest_kind.clone()))
    }

    /// Mark `seq` (and everything before it) as committed (barrier-synced), so a later
    /// gate holder for the same-or-older content skips the redundant sync.
    fn mark_committed(&self, carrier: &str, seq: u64) {
        if let Some(p) = self.pending.lock().get_mut(carrier) {
            p.committed_seq = p.committed_seq.max(seq);
        }
    }

    /// Prune a carrier's per-carrier gate + pending state on a committed close, but ONLY
    /// when NO newer op is queued for the carrier (`latest_seq <= up_to_seq`). The
    /// injected slot is already dropped by the close; this keeps the `gates` / `pending`
    /// maps tracking the CURRENT open set rather than the cumulative touched set.
    ///
    /// Race-safety (common case): a queued op records its pending submission
    /// (`record_pending`, a NEWER `latest_seq`) BEFORE it fetches the gate Arc (`gate_for`),
    /// so a newer pending op is the exact witness that another op holds/awaits this
    /// carrier's gate — observing it (`latest_seq > up_to_seq`) SKIPS the prune, so the
    /// common newer-op-queued path never orphans a queued op onto a fresh Arc. The `pending`
    /// and `gates` locks are taken TOGETHER, in the SAME order `drive` acquires them
    /// (`record_pending` locks `pending` in step 1, `gate_for` locks `gates` in step 2 —
    /// never the reverse). No deadlock: no code path holds `gates` then locks `pending`.
    ///
    /// NARROW residual (NOT fully race-safe): because an in-flight op COALESCES the
    /// carrier's NEWEST pending op (`take_drainable`), it can commit an OLDER
    /// waiter's already-recorded op and then prune at that op's own `drain_seq` — observing
    /// no strictly-newer pending and removing the gate Arc WHILE that older waiter still
    /// holds a reference to it and is blocked on `gate.lock()`. A subsequent close/reopen
    /// then mints a FRESH gate Arc, transiently splitting the carrier across two gates. The
    /// window is BOUNDED and self-converging (the coalesced waiter finds nothing drainable
    /// and releases the old gate) and any resulting inconsistency falls back to OWNED;
    /// making the prune waiter-aware / generation-stamped is deferred (tracked as ROW E5 in
    /// `docs/arch/external-ts-engine-architecture.md`).
    fn prune_carrier_state_if_idle(&self, carrier: &str, up_to_seq: u64) {
        let mut pending = self.pending.lock();
        let has_newer = pending
            .get(carrier)
            .is_some_and(|p| p.latest_seq > up_to_seq);
        if has_newer {
            return;
        }
        let mut gates = self.gates.lock();
        pending.remove(carrier);
        gates.remove(carrier);
    }

    /// Ordered per-carrier lifecycle op: serialize + coalesce + commit, driving the
    /// wire send + barrier through `sink`. Open, change, AND close all flow through
    /// this ONE gate + coalescing cell.
    ///
    /// The submission is recorded as the carrier's latest pending op; the caller then
    /// acquires the per-carrier gate (a later op BLOCKS here until the in-flight op's
    /// barrier completes — ordered commits, no `didChange` ahead of `didOpen`, no
    /// `didClose` interleaved with an in-flight injection), drains the NEWEST pending op
    /// (coalescing a burst — the newest op wins, so a close supersedes an older queued
    /// injection and a reopen supersedes an older close), then performs it:
    ///
    /// - [`PendingKind::Inject`]: decide Open vs Change by slot presence (reserved under
    ///   the gate — no TOCTOU), send the wire op + await its barrier, then commit the
    ///   local slot consistently. A first-open barrier failure retracts the
    ///   possibly-open Program file and drops the slot; a `didChange` failure keeps the
    ///   prior synced content.
    /// - [`PendingKind::Close`]: send `didClose` ONLY when the carrier is currently
    ///   reserved (open in the Program) and drop the local slot; a never-opened carrier
    ///   (or one an earlier gate holder already closed) is a no-op — the slot presence
    ///   is the authoritative open/closed decision under the gate.
    ///
    /// Returns the barrier `Result` (a broken connection is an `Err` the caller fails
    /// closed on).
    async fn drive<S, Fut>(
        &self,
        carrier: &str,
        kind: PendingKind,
        sink: S,
    ) -> Result<(), TypeProviderError>
    where
        S: Fn(CarrierWireOp) -> Fut,
        Fut: Future<Output = Result<(), TypeProviderError>>,
    {
        // 1. Record this submission as the carrier's latest pending op.
        let seq = self.next_seq.fetch_add(1, Ordering::Relaxed);
        self.record_pending(carrier, seq, kind);

        // 2. Acquire the per-carrier gate — a later op BLOCKS here until the in-flight
        //    op's barrier completes (ordered commits; no didChange ahead of didOpen; no
        //    didClose interleaved with an in-flight injection).
        let gate = self.gate_for(carrier);
        let _guard = gate.lock().await;

        // 3. Drain the NEWEST pending op. If an earlier gate holder already committed
        //    this-or-newer op, this call is a no-op (coalesced away).
        let Some((drain_seq, drain_kind)) = self.take_drainable(carrier) else {
            return Ok(());
        };

        match drain_kind {
            PendingKind::Inject(drain_content) => {
                // Decide Open vs Change by slot presence (under the gate — no TOCTOU).
                let action = reserve_carrier(&self.injected, carrier);
                let op = match action {
                    InjectAction::Open => CarrierWireOp::Open {
                        version: 1,
                        content: Arc::clone(&drain_content),
                    },
                    InjectAction::Change => CarrierWireOp::Change {
                        version: self.next_version.fetch_add(1, Ordering::Relaxed),
                        content: Arc::clone(&drain_content),
                    },
                };

                // Wire send + barrier — the ONLY await under the gate for an inject.
                let result = sink(op).await;

                // Commit the local slot consistently with the shared Program. A first-open
                // barrier that COMPLETES with a failure best-effort RETRACTS the possibly
                // open Program file BEFORE dropping the local slot, so both sides stay
                // consistent. This reconciliation runs only when the barrier COMPLETES:
                // unlike the Close arm (which drops its slot UP FRONT to survive an outer
                // cancellation), a first-open reserves the slot BEFORE the barrier, so an
                // OUTER overlay deadline that cancels this future mid-barrier — before the
                // commit below runs — can leave a reserved unsynced slot (bounded, fail
                // closed to OWNED). Making the first-open reservation cancellation-safe
                // (dropping/retracting the reserved slot on an outer-deadline cancel) is
                // deferred (tracked as ROW F1 in `docs/arch/external-ts-engine-architecture.md`).
                let commit = sync_commit(action, result.is_ok());
                if matches!(commit, SyncCommit::RetractOpen) {
                    let _ = sink(CarrierWireOp::Close).await;
                }
                apply_local_sync_commit(&self.injected, carrier, drain_content, commit);
                if result.is_ok() {
                    self.mark_committed(carrier, drain_seq);
                }
                result
            }
            PendingKind::Close => {
                // Drop the local slot UP FRONT — BEFORE the wire barrier — so a bounded /
                // cancelled off-path close (the timeout-safe retract) still leaves the
                // carrier reading not-synced: the local view can never outlive a cancelled
                // wire close (a `tokio::time::timeout` that cancels this future mid-barrier
                // must not skip the slot drop). Slot PRESENCE (captured here) is the
                // authoritative open/closed decision under the gate: send `didClose` ONLY
                // when the carrier was actually reserved (open in the Program); a
                // never-opened carrier (or one an earlier gate holder already closed) sends
                // nothing.
                let was_open = take_carrier_slot(&self.injected, carrier);
                let result = if was_open {
                    // Wire send + barrier — the ONLY await under the gate for a close.
                    sink(CarrierWireOp::Close).await
                } else {
                    Ok(())
                };
                if result.is_ok() {
                    self.mark_committed(carrier, drain_seq);
                }
                // Prune the carrier's per-carrier gate + pending cell (the slot is already
                // dropped) when THIS close is the latest op — no newer submission is
                // queued — so the per-carrier maps track the CURRENT open set, not the
                // cumulative touched set across a long opt-in session. Skipped when a newer
                // op is pending (that op owns the gate Arc; pruning it would orphan the
                // queued op onto a fresh Arc and split ordering).
                self.prune_carrier_state_if_idle(carrier, drain_seq);
                result
            }
        }
    }
}

/// A tracked carrier overlay slot. The last barrier-SYNCED content (the ONLY content
/// served / positioned from) is tracked SEPARATELY from the optimistically-reserved
/// in-flight injection, so the local overlay view never diverges from the shared
/// Program on a sync timeout.
struct CarrierSlot {
    /// The last content the shim's sync barrier confirmed the shared Program
    /// ACCEPTED. `None` while a first-open is in flight (reserved but not yet synced),
    /// and only ever set to content a barrier accepted ([`promote_synced`]). The
    /// UTF-16 diagnostic index is built from THIS — never the optimistic reservation.
    synced: Option<Arc<str>>,
}

/// Whether a carrier injection must send a `didOpen` (the carrier's FIRST reservation)
/// or a `didChange` (a carrier already reserved).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InjectAction {
    /// The carrier slot was absent — this caller reserved it and must send `didOpen`.
    Open,
    /// The carrier slot was already reserved — send `didChange`.
    Change,
}

/// Atomically reserve the carrier slot and decide the injection action.
///
/// A SINGLE lock acquisition inspects the entry: exactly one caller sees the absent
/// slot, inserts a pending slot (`synced: None`), and returns [`InjectAction::Open`];
/// every concurrent caller sees the reserved slot and returns [`InjectAction::Change`].
/// This is the reserve-before-await that closes the inject TOCTOU — no window between
/// "is it open?" and the wire send in which two first-opens both send `didOpen`
/// version 1. Reservation NEVER touches `synced` (the reserved text is not served
/// until its barrier is confirmed accepted — see [`promote_synced`]).
fn reserve_carrier(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
) -> InjectAction {
    use std::collections::hash_map::Entry;
    match injected.lock().entry(carrier.to_string()) {
        Entry::Occupied(_) => InjectAction::Change,
        Entry::Vacant(vacant) => {
            vacant.insert(CarrierSlot { synced: None });
            InjectAction::Open
        }
    }
}

/// The local-slot action after an injection's sync barrier resolves — the
/// consistency oracle that keeps the local overlay view aligned with the shared
/// Program. PURE over `(action, barrier_ok)`; the caller applies it via
/// [`apply_local_sync_commit`] and (for [`SyncCommit::RetractOpen`]) issues the wire
/// `didClose`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SyncCommit {
    /// The barrier SYNCED — promote the reserved text to the slot's authoritative
    /// synced content (the only content served / positioned from).
    Promote,
    /// A FIRST-OPEN barrier FAILED/timed out — the Program MAY hold the didOpen, so
    /// retract it (`didClose`) and drop the local slot (both sides end closed).
    RetractOpen,
    /// A `didChange` barrier FAILED/timed out — keep the PRIOR synced content the
    /// Program still holds; never serve the reserved-but-unaccepted new text.
    KeepPriorSynced,
}

/// Map an injection's sync-barrier outcome to the local-slot action that keeps the
/// local view consistent with the shared Program: any success promotes; a first-open
/// failure retracts the possibly-open Program file; a `didChange` failure keeps the
/// prior synced content.
fn sync_commit(action: InjectAction, barrier_ok: bool) -> SyncCommit {
    match (action, barrier_ok) {
        (_, true) => SyncCommit::Promote,
        (InjectAction::Open, false) => SyncCommit::RetractOpen,
        (InjectAction::Change, false) => SyncCommit::KeepPriorSynced,
    }
}

/// Apply the local-slot half of a [`SyncCommit`] (promote the synced content / drop
/// the slot / keep the prior synced). The wire `didClose` retract for
/// [`SyncCommit::RetractOpen`] is the caller's separate control call.
fn apply_local_sync_commit(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
    text: Arc<str>,
    commit: SyncCommit,
) {
    match commit {
        SyncCommit::Promote => promote_synced(injected, carrier, text),
        SyncCommit::RetractOpen => drop_carrier_slot(injected, carrier),
        SyncCommit::KeepPriorSynced => {}
    }
}

/// Promote the barrier-SYNCED `text` to the slot's authoritative synced content
/// (called only after the sync barrier ACCEPTED the injection).
fn promote_synced(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    carrier: &str,
    text: Arc<str>,
) {
    if let Some(slot) = injected.lock().get_mut(carrier) {
        slot.synced = Some(text);
    }
}

/// Drop a carrier's local slot (a first-open barrier failed — the caller separately
/// retracts the possibly-open Program file, keeping the two consistent).
fn drop_carrier_slot(injected: &SyncMutex<HashMap<String, CarrierSlot>>, carrier: &str) {
    injected.lock().remove(carrier);
}

/// Remove a carrier's local slot and report whether it was present (open in the
/// Program) — one lock acquisition. Called by the close arm SYNCHRONOUSLY, before the
/// wire barrier, so a bounded / cancelled close still drops the slot: the local view can
/// never outlive a cancelled wire close (the timeout-safe close).
fn take_carrier_slot(injected: &SyncMutex<HashMap<String, CarrierSlot>>, carrier: &str) -> bool {
    injected.lock().remove(carrier).is_some()
}

/// The last barrier-SYNCED content for a carrier — by the engine's canonicalization
/// first, then the injected key — the ONLY content served / positioned from. A
/// reserved-but-not-yet-synced slot returns `None` (fail-closed — no unaccepted text).
fn synced_content(
    injected: &SyncMutex<HashMap<String, CarrierSlot>>,
    engine_carrier: &str,
    carrier: &str,
) -> Option<Arc<str>> {
    let injected = injected.lock();
    injected
        .get(engine_carrier)
        .or_else(|| injected.get(carrier))
        .and_then(|slot| slot.synced.clone())
}

/// Forward-slash-normalize a path for engine comparison.
fn slash(p: &str) -> String {
    p.replace('\\', "/")
}

/// A distinct identity from `id` (one byte flipped) — the fail-closed
/// editor-binding mismatch witness (never a forged match).
fn distinct_identity(id: ProjectIdentity) -> ProjectIdentity {
    let mut bytes = id.0;
    bytes[0] ^= 0xFF;
    ProjectIdentity(bytes)
}

/// The editor-binding-identity fact + the bound identity, keyed on the resolved
/// PROJECT identity — never a bare workspace-root hash.
///
/// The editor-binding EVIDENCE is the initialize witness `root_uri`: the editor bound
/// the carrier to the workspace Verter resolved iff the witness `rootUri` canonicalizes
/// to the resolved `workspace_root`. When it matches, the fact is
/// `Matched(project_identity)`; a missing witness root, or a DIFFERENT workspace,
/// yields a distinct identity ⇒ `Mismatch` (fail closed — never a forged match).
///
/// Because the fact is keyed on `project_identity`, two DISTINCT configured projects
/// under the SAME `rootUri` produce DISTINCT `Matched` facts, so SHARED eligibility
/// established for one project can never spill to a sibling project of the same
/// workspace. Keying on the workspace-root hash (the prior behaviour) made those two
/// facts EQUAL — the eligibility-spill defect this closes.
fn resolve_editor_binding(
    project_identity: ProjectIdentity,
    workspace_root: &str,
    witness_root_uri: Option<&str>,
) -> (EditorBindingFact, ProjectIdentity) {
    let editor_bound = match witness_root_uri {
        Some(root_uri)
            if canonicalize_path(&file_uri_to_path(root_uri))
                == canonicalize_path(workspace_root) =>
        {
            project_identity
        }
        _ => distinct_identity(project_identity),
    };
    (
        EditorBindingFact::evaluate(&project_identity, &editor_bound),
        editor_bound,
    )
}

/// The parent directory of a forward-slashed path (the tsconfig dir base).
fn parent_dir(path: &str) -> String {
    let slashed = slash(path);
    match slashed.rfind('/') {
        Some(i) => slashed[..i].to_string(),
        None => String::new(),
    }
}

/// Minimal `file://` URI decode (drive-form + POSIX). Shared shape with the rest
/// of the carrier path handling; the shim's egress layer canonicalizes on match.
fn file_uri_to_path(uri: &str) -> String {
    verter_span::uri::file_uri_to_path(uri)
}

/// Convert a forward-slashed path to a `file://` URI.
fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}

/// The LSP language id for a carrier companion by extension: `.tsx`/`.jsx` are
/// the JSX IDE carriers, `.ts`/`.js` the plain companions.
fn language_id_for(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".tsx") {
        "typescriptreact"
    } else if lower.ends_with(".jsx") {
        "javascriptreact"
    } else if lower.ends_with(".js") || lower.ends_with(".mjs") || lower.ends_with(".cjs") {
        "javascript"
    } else {
        "typescript"
    }
}

/// The error every SHARED interactive-feature `TypeProvider` method returns. SHARED tsgo
/// is the DIAGNOSTICS-ONLY project-bound typecheck oracle (see the module docs);
/// interactive features (hover / definition / references / completion / …) are served by
/// the OWNED baseline, and the composite ([`crate::tsgo::composite::TsgoCompositeProvider`])
/// delegates EVERY feature method to OWNED — so these methods are UNREACHABLE through the
/// production composite. Returning a LOUD error (rather than a silent empty result) makes
/// any accidental production wiring of the raw SHARED provider as a feature backend fail
/// visibly instead of silently serving no results — the raw feature surface is
/// deliberately non-production, not a hollow silent stub.
const SHARED_FEATURE_NOT_SERVED: &str =
    "shared tsgo is diagnostics-only; interactive features are served by the OWNED baseline \
     (the composite delegates every feature to OWNED) — this raw SHARED feature method is \
     not reachable in production";

impl TypeProvider for TsgoSharedProvider {
    fn provider_id(&self) -> &'static str {
        // The SHARED dual-surface attach IS the tsgo provider — the editor's engine
        // served non-owningly; every engine-identifying branch treats it as tsgo.
        "tsgo"
    }

    // ── Carrier lifecycle: inject through the shim's gated control channel (NOT
    //    an OWNED `--lsp` didOpen — Verter holds no editor↔tsgo wire). ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.inject_carrier(&path, &content).await?;
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.inject_carrier(&path, &content).await?;
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.close_carrier_overlay(&path).await?;
            Ok(())
        })
    }

    // ── Diagnostics: the SHARED project-bound `--api` typecheck oracle. ──

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            match self.semantic_diagnostics_for_carrier(&path).await {
                Ok(diags) => Ok(diags),
                Err(e) => {
                    tracing::warn!("shared tsgo diagnostics for {path}: {e}");
                    Ok(Vec::new())
                }
            }
        })
    }

    // ── Interactive features: the SHARED path is DIAGNOSTICS-ONLY. Interactive
    //    features are served by the OWNED baseline (the composite delegates EVERY
    //    feature method to OWNED), so these raw SHARED methods are UNREACHABLE in
    //    production. They FAIL LOUDLY ([`SHARED_FEATURE_NOT_SERVED`]) rather than
    //    silently return an empty/`None` result — a deliberately non-production
    //    surface, never a silent hollow feature stub that could mask an accidental
    //    wiring of the raw SHARED provider as a feature backend. ──

    fn get_completions(
        &self,
        _path: &str,
        _offset: u32,
        _trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_completion_details<'a>(
        &'a self,
        _path: &'a str,
        _offset: u32,
        _items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn resolve_completion(
        &self,
        _path: &str,
        _data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_hover(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_definition(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_type_definition(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_references(&self, _path: &str, _offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_rename_locations(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_signature_help(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_code_actions(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
        _diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_semantic_tokens(&self, _path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_document_highlights(
        &self,
        _path: &str,
        _offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn get_inlay_hints(
        &self,
        _path: &str,
        _start_offset: u32,
        _end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        Box::pin(async move { Err(TypeProviderError::new(SHARED_FEATURE_NOT_SERVED)) })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            // Retract Verter's carriers + tear the control session down; close the
            // `--api` connection. Best-effort — a dead shim is the intended effect.
            let _ = self.control.detach(true).await;
            let _ = self.api.close().await;
            Ok(())
        })
    }
}

#[cfg(test)]
#[path = "shared_tests.rs"]
mod shared_tests;
