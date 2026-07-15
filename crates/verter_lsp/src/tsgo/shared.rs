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
//! This provider serves both the SHARED project-bound `--api` diagnostic oracle
//! and the full interactive `--lsp` feature surface. Feature requests traverse
//! the relay's typed read-only control method under reserved request IDs and are
//! parsed by the same `TsgoTypeProvider` response code as an owned connection.
//! The facade owns no process and never sends a second initialize, shutdown, or exit.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex as SyncMutex;

use verter_tsgo_api::api_attach::ApiAttachClient;
use verter_tsgo_api::control::messages::FeatureRequestMethod;
use verter_tsgo_api::control::{Advertisement, ControlClient};
use verter_tsgo_api::gate::{self, ObservedEngine};
use verter_tsgo_api::jsonrpc::framing::{encode_message, MessageFramer};
use verter_tsgo_api::jsonrpc::JsonRpcConnection;
use verter_tsgo_api::proto::types::OpaqueHandle;
use verter_tsgo_api::transport::pipe_attach::connect_attach_pipe;

use verter_session::external_ts::{
    compose_eligibility, decide_live, AttachFact, BindingFact, CarrierOwnershipResolution,
    ComponentModeDecision, ConfigPathProbe, EditorBindingFact, EligibilityFacts,
    EngineSessionCandidates, EngineSessionFacts, EngineWarmCache, LiveDecision,
    LiveDecisionRequest, LiveProjectInput, OwnedSessionFacts, ProjectIdentitySource, ProxyFact,
    ReferenceInput, ServeMode, SharedSessionFacts, VersionGateFact,
};
use verter_session::file_artifact_store::ProjectIdentity;

use verter_type_runtime::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation, TypeProviderError,
};
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};
use verter_type_runtime::tsgo::{
    position_carrier_diagnostics, select_configured_project_carrier, TsgoTypeProvider,
};

use super::shared_support::{
    language_id_for, parent_dir, path_to_file_uri, resolve_editor_binding, slash,
};

#[path = "carrier_sync.rs"]
mod carrier_sync;
pub(crate) use carrier_sync::*;

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
    /// real [`CarrierOwnershipResolution::Bound`] is the ONLY value that yields a
    /// [`BindingFact::Bound`]; every other state fails closed to OWNED.
    pub resolution: CarrierOwnershipResolution,
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
/// session, proxy, OWNED session — all stable across carriers for one editor session)
/// plus the STABLE editor-binding evidence (workspace root + witness root URI) and the
/// composite warm cache, so the serve mode is RE-DECIDED per query for the queried
/// carrier's resolved binding at the CURRENT snapshot/config generation
/// ([`Self::decide`]) — never frozen at construction — with the editor-binding fact
/// RECOMPUTED from that evidence for the carrier's OWN resolved project identity rather
/// than reused from the first establishment. The decision is
/// memoized per reference-closure in the warm cache (keyed on the representative
/// component project, the canonical tsconfig, the snapshot/config generation, the full
/// serving-engine identity — mode + observed version + wire pin + reconnect/editor
/// generation — AND the carrier's editor-binding project identity), so a new published
/// snapshot re-decides (a superseded-generation entry is unreachable under the new
/// generation) while a same-generation repeat reuses the warm serving state. The
/// editor-binding identity is a key dimension (the per-carrier discrimination axis), so a later carrier
/// that recomputes a DIFFERENT editor binding keys a distinct warm slot instead of
/// reusing the first establishment's.
pub struct SharedModeController {
    version_gate: VersionGateFact,
    attach: AttachFact,
    proxy: ProxyFact,
    /// The resolved workspace root — the STABLE editor-binding evidence retained
    /// (with `witness_root_uri`) so each per-query decision RECOMPUTES the
    /// editor-binding fact for the carrier's OWN resolved project identity, rather than
    /// reusing a single first-establishment fact for every later carrier.
    ///
    /// Both `workspace_root` and `witness_root_uri` are SESSION-LEVEL constants — the
    /// single rendezvous workspace root and the editor's `initialize` `rootUri`, fixed
    /// for the whole editor session — so they are captured ONCE at establishment. They
    /// are NOT the per-carrier discrimination axis: that is the resolved `project_identity`
    /// (`node_identity`), which is why [`resolve_editor_binding`] recomputes the fact per
    /// carrier from the CURRENT `node_identity` against this fixed evidence and keys the
    /// fact on `project_identity`, never on the workspace root (see
    /// `editor_binding_fact_keys_on_project_identity_not_workspace_root` and
    /// `controller_recomputes_editor_binding_per_decided_binding`).
    workspace_root: Arc<str>,
    /// The initialize-witness `root_uri` — the other half of the stable editor-binding
    /// evidence (`None` when the editor advertised no workspace root).
    witness_root_uri: Option<Arc<str>>,
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
    /// a same-generation repeat reuses the warm state. The editor-binding fact is
    /// RECOMPUTED for this carrier's own `node_identity` from the retained stable
    /// evidence (workspace root + witness root URI), never reused from the first
    /// establishment — so a later carrier resolving a DIFFERENT configured project
    /// under the same session decides over its OWN editor binding. Pure over the
    /// retained facts + evidence + the warm cache — no engine contact.
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
        // Recompute the editor-binding fact for THIS carrier's resolved project
        // identity from the retained stable evidence — never reuse a first-
        // establishment fact for a later carrier that resolved a different project.
        let (editor_binding, editor_bound_identity) = resolve_editor_binding(
            node_identity,
            &self.workspace_root,
            self.witness_root_uri.as_deref(),
        );
        let mut guard = self.warm_cache.lock();
        decide_shared_serve(
            self.version_gate.clone(),
            self.attach.clone(),
            binding_fact,
            self.proxy,
            editor_binding,
            node_identity,
            &tsconfig_dir,
            canonical_tsconfig,
            references,
            self.owned_session.clone(),
            config_generation,
            editor_bound_identity,
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
    control: Arc<ControlClient>,
    /// Full interactive TypeProvider facade over the SAME editor-owned LSP
    /// connection. It owns no process and sends no initialize/shutdown/exit; its
    /// requests are multiplexed through the relay's typed feature control method.
    features: Arc<TsgoTypeProvider>,
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
        if !hello.capabilities.carrier_injection
            || !hello.capabilities.api_session
            || !hello.capabilities.wait_initialized
            || !hello.capabilities.feature_requests
        {
            return Err(EstablishError::Handshake(format!(
                "relay capabilities incomplete for editor-session reuse: {:?}",
                hello.capabilities
            )));
        }
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
            CarrierOwnershipResolution::Bound(b) => b.env_dims().project_identity,
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
        // Retain the STABLE editor-binding EVIDENCE (workspace root + witness root URI),
        // not the computed fact: the live controller recomputes the fact per query for
        // each carrier's own resolved project identity ([`SharedModeController::decide`]).
        let controller_workspace_root: Arc<str> = Arc::from(params.workspace_root);
        let controller_witness_root_uri: Option<Arc<str>> =
            witness.root_uri.as_deref().map(Arc::from);

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
            CarrierOwnershipResolution::Bound(b) => redirect_on_references(b),
            CarrierOwnershipResolution::NoProject
            | CarrierOwnershipResolution::Ambiguous { .. }
            | CarrierOwnershipResolution::NotReady => Vec::new(),
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

        let control = Arc::new(control);
        let features = Arc::new(start_feature_bridge(Arc::clone(&control)));
        let controller = SharedModeController {
            version_gate,
            attach,
            proxy,
            workspace_root: controller_workspace_root,
            witness_root_uri: controller_witness_root_uri,
            owned_session,
            observed_version: Arc::clone(&observed_version),
            warm_cache,
            establishment: decision,
        };

        Ok(Self {
            control,
            features,
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
            BindingFact::from_resolution(&CarrierOwnershipResolution::Bound(binding.clone())),
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

    /// Full pull diagnostics from the exact editor-owned LSP session. Unlike the
    /// ordinary provider facade this is strict: a relay failure is returned to the
    /// composite, never converted into a legitimate empty report. The composite calls
    /// this only after the `--api` membership proof has established that the carrier is
    /// a root of its resolved configured project.
    pub async fn full_diagnostics_for_carrier(
        &self,
        path: &str,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        self.features.get_diagnostics_strict(path).await
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
        // text the shared Program actually accepted). A reserved/uncertain
        // (`PossiblyOpenUnsynced`) or never-injected carrier yields `None`: FAIL CLOSED to
        // OWNED (an `Err`) rather than positioning an (even empty) SHARED result against an
        // absent barrier-synced basis.
        let content =
            require_synced_carrier_content(self.sync.synced_content(&engine_carrier, carrier))?;
        Ok(Some(position_carrier_diagnostics(
            &diags,
            Some(content),
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
    /// `carrier_did_close`); the ordering + local-slot consistency ([`reserve_carrier_capturing`] /
    /// [`sync_commit`] — a first-open barrier failure marks the slot `PossiblyOpenUnsynced`
    /// and best-effort retracts the possibly-open Program file; a `didChange` failure fails
    /// closed to the non-serveable `OpenUnsyncedContent` slot; a close transitions the slot to
    /// the non-serveable `PossiblyOpenUnsynced` shell BEFORE its barrier and removes it only on a
    /// SUCCESSFUL close — a failed/timed-out close leaves the shell to reconcile) live in the
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
    /// transitions the local slot to the non-serveable `PossiblyOpenUnsynced` shell BEFORE
    /// the bounded close barrier and removes it only on a SUCCESSFUL close (a failed/timed-out
    /// close leaves the shell to reconcile). The shim mirrors this on its own side — its
    /// control server removes a carrier from its open-overlay set only on a confirmed
    /// `didClose` — so a close that does not confirm there leaves the overlay tracked
    /// shim-side, where the shim's session-end drain retracts it on transport close.
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

/// Build the normal tsgo LSP feature parser over a local duplex transport whose
/// peer forwards only the closed read-only feature set through the authenticated
/// relay control channel. This reuses the production response parsing and
/// byte/UTF-16 conversion without giving the non-owning provider a process handle
/// or a raw editor wire.
fn start_feature_bridge(control: Arc<ControlClient>) -> TsgoTypeProvider {
    let (provider_side, bridge_side) = tokio::io::duplex(1024 * 1024);
    let (provider_read, provider_write) = tokio::io::split(provider_side);
    let (bridge_read, bridge_write) = tokio::io::split(bridge_side);
    tokio::spawn(run_feature_bridge(bridge_read, bridge_write, control));
    TsgoTypeProvider::from_initialized_transport(provider_read, provider_write)
}

async fn run_feature_bridge<R, W>(mut read: R, write: W, control: Arc<ControlClient>)
where
    R: tokio::io::AsyncRead + Unpin + Send + 'static,
    W: tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let write = Arc::new(tokio::sync::Mutex::new(write));
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let count = match read.read(&mut chunk).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        framer.push(&chunk[..count]);
        loop {
            let message = match framer.next_message() {
                Ok(Some(message)) => message,
                Ok(None) => break,
                Err(_) => return,
            };
            let Some(id) = message.get("id").cloned().filter(|id| !id.is_null()) else {
                // The facade emits no lifecycle notifications. A notification on
                // this read-only bridge is ignored rather than forwarded.
                continue;
            };
            let Some(method) = message.get("method").and_then(|value| value.as_str()) else {
                continue;
            };
            let method = FeatureRequestMethod::from_lsp_method(method);
            let params = message
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let control = Arc::clone(&control);
            let write = Arc::clone(&write);
            tokio::spawn(async move {
                let response = match method {
                    Some(method) => match control.feature_request(method, params).await {
                        Ok(result) => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "result": result,
                        }),
                        Err(error) => serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": id,
                            "error": { "code": -32014, "message": error.to_string() },
                        }),
                    },
                    None => serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": {
                            "code": -32601,
                            "message": "method is not in Verter's read-only editor feature set"
                        },
                    }),
                };
                let frame = encode_message(&response);
                let mut writer = write.lock().await;
                let _ = writer.write_all(&frame).await;
                let _ = writer.flush().await;
            });
        }
    }
}

impl TypeProvider for TsgoSharedProvider {
    fn provider_id(&self) -> &'static str {
        // The SHARED dual-surface attach IS the tsgo provider — the editor's engine
        // served non-owningly; every engine-identifying branch treats it as tsgo.
        "tsgo"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    // ── Carrier lifecycle: inject through the shim's gated control channel (NOT
    //    an OWNED `--lsp` didOpen — Verter holds no editor↔tsgo wire). ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.inject_carrier(&path, &content).await?;
            self.features.load_file(&path, &content).await?;
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        self.open_file(path, content)
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.inject_carrier(&path, &content).await?;
            self.features.load_file(&path, &content).await?;
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.close_carrier_overlay(&path).await?;
            self.features.forget_cached_content(&path).await;
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

    // Interactive features reuse the exact editor-owned LSP session through the
    // relay's typed read-only feature multiplexer.

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        self.features
            .get_completions(path, offset, trigger_character)
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        self.features.get_completion_details(path, offset, items)
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        self.features.resolve_completion(path, data)
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        self.features.get_hover(path, offset)
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.features.get_definition(path, offset)
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.features.get_type_definition(path, offset)
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        self.features.get_references(path, offset)
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        self.features.get_rename_locations(path, offset)
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        self.features.get_signature_help(path, offset)
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        self.features
            .get_code_actions(path, start_offset, end_offset, diagnostics)
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        self.features.get_semantic_tokens(path)
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        self.features.get_document_highlights(path, offset)
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        self.features
            .get_inlay_hints(path, start_offset, end_offset)
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
