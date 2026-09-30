//! Host-aware tsgo admission and shared-first serving order.
//!
//! [`TsgoCompositeProvider`] is the production tsgo [`TypeProvider`] shape. It holds
//! a managed provider (eager for an explicit managed mode, statefully lazy for an editor
//! integration), the live host/project authority, and an optional [`SharedTsgoOverlay`].
//!
//! **Project-bound gate.** A carrier companion resolves its owning configured project
//! once through the shared
//! [`project_binding`](crate::tsgo::project_binding) helper — published snapshot →
//! [`WorkspaceProjectResolver`](verter_session::external_ts::WorkspaceProjectResolver)
//! → [`ProjectBinding`](verter_session::external_ts::ProjectBinding) →
//! [`BoundProject`](verter_session::external_ts::BoundProject) witness. Non-bound states
//! fail closed to the feature's empty external answer; they never reach an engine's
//! inferred-project self-discovery. The SHARED route's engagement and every
//! generated-unit write carry witnesses issued by the overlay's
//! [`ProviderHub`](verter_type_runtime::provider_hub::ProviderHub) — the one
//! admission authority of the shared route.
//!
//! **Serving order.** For a bound carrier, an armed editor rendezvous is established,
//! synchronized, and live-revalidated first. Diagnostics and every read-only feature use
//! that exact editor-owned Program. Only an observed attach/sync/decision failure or the
//! bounded shared deadline admits the managed provider. With
//! [`crate::type_provider::lazy_managed::LazyManagedTypeProvider`] this means a successful
//! shared session never creates or queries a duplicate semantic engine. Carrier diagnostics
//! are served only through the exact configured project's `--api` semantic + syntactic
//! channels; a raw companion LSP pull is never mixed in because it can bind to a broader
//! project with different JavaScript policy.
//!
//! Lifecycle/configuration calls still flow to the managed slot so a lazy fallback can
//! cache the latest desired state without spawning; the shared overlay records carrier
//! content independently and injects it only when a bound demand engages.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use verter_semantic::resolver_core::normalize_canonical_id;
use verter_session::external_ts::{
    AmbiguityCause, CarrierOwnershipResolution, GeneratedUnitAdmissionFact, ProjectBinding,
    ServeMode,
};
use verter_session::framework::descriptor::classify_carrier_companion;
use verter_session::VerterHost;
use verter_workspace::traits::WorkspaceRead;
use verter_workspace::workspace_snapshot::ProjectPayload;
use verter_workspace::{CanonicalPath, GeneratedUnitAdmission, GeneratedUnitNonAdmissionReason};

use verter_tsgo_api::control::Advertisement;
use verter_type_runtime::protocol::{
    Completion, CompletionResolveData, CompletionResolveResult, CompletionResult, HoverInfo,
    InlayHint, ProviderDiagnosticContext, RenameLocation, SemanticToken, SignatureHelp,
    TypeCodeAction, TypeDiagnostic, TypeDocumentHighlight, TypeLocation, TypeProviderError,
};
use verter_type_runtime::provider_hub::{
    HubPolicy, ProviderEstablisher, ProviderHub, TracingNotifier,
};
use verter_type_runtime::traits::{ProviderFuture, TypeProvider};

use crate::tsgo::project_binding::{self, AdmissionEpoch};
use crate::tsgo::shared::{EstablishSharedParams, TsgoSharedProvider};
use verter_type_runtime::provider_hub::overlay::{
    GeneratedUnitWritePermit, HubAdmittedTransport, LazyOverlayCore, OverlayPriority,
    OverlaySyncState, OverlayTransport, ServingTransport,
};
use verter_type_runtime::provider_hub::ProviderEpoch;

/// The bound on the lazy SHARED-attach establishment: a slow or never-initializing
/// editor tsgo cannot stall a carrier diagnostics query beyond this — on elapse the
/// overlay yields no SHARED result and the composite admits managed fallback
/// (fail-closed). Concurrent queries during establishment reuse the one bounded
/// attempt (singleflight); a failed attempt re-arms on a fresh advertisement/editor
/// generation OR a fresh workspace/config generation (see
/// [`LazyTransport`](verter_type_runtime::provider_hub::LazyTransport)). Establishment is
/// reached only from a query path, never the managed lifecycle path — so opting into
/// SHARED never trips the managed
/// foreground-sync budget.
const SHARED_ESTABLISH_TIMEOUT: Duration = Duration::from_secs(15);

/// The OUTER production deadline bounding the ENTIRE SHARED overlay contribution to a
/// single diagnostics query — establishment + whole-dirty-set injection + the per-query
/// control re-decision + both diagnostic channels, as one unit. On elapse the composite
/// admits the managed fallback. Every shared sub-operation is awaited inside this bound,
/// so no relay/control/`--api` stall can escape it. It exceeds
/// [`SHARED_ESTABLISH_TIMEOUT`] so a legitimate first establishment reaches its own
/// singleflight decision.
const SHARED_OVERLAY_TIMEOUT: Duration = Duration::from_secs(20);

/// The bound on a SHARED carrier retract issued from `close_file` lifecycle:
/// a slow or never-answering relay close cannot hang or delay the composite close beyond
/// this — on elapse the retract is abandoned (fail-closed) and the composite close
/// returns promptly. The retract is best-effort and the transport is torn down / evicted
/// on a broken connection anyway, so a dropped retract only leaves a soon-cleaned
/// lingering document, never a wrong result. Symmetric with the open/change lifecycle,
/// which only records content off the managed critical path.
const SHARED_CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

/// The client label the SHARED overlay presents on the control hello.
const SHARED_CLIENT_LABEL: &str = "verter_lsp";

/// The rendezvous evidence a SHARED editor-attach is established from: the control
/// directory the editor's `verter-relay-shim` advertised into, the session key it
/// published under, and the workspace root (the editor-binding witness base).
#[derive(Debug, Clone)]
pub struct SharedRendezvous {
    /// The rendezvous control directory the editor's shim advertised into.
    pub control_dir: PathBuf,
    /// The `--session-key` the shim published under.
    pub session_key: String,
    /// The workspace root the editor bound the carrier to.
    pub workspace_root: String,
}

/// The SHARED carrier-diagnostics overlay: resolves the queried carrier's owning
/// project per query over the host's live published snapshot and, only for a
/// resolved binding, serves the SHARED `--api` carrier diagnostics through the
/// lazily-established relay-attach transport.
///
/// Cheap to clone (one `Arc`).
#[derive(Clone)]
pub struct SharedTsgoOverlay<P: SharedAttach = TsgoSharedProvider> {
    inner: Arc<OverlayInner<P>>,
}

/// The typed terminal reason an armed SHARED route refused to engage. Every refusal
/// carries the carrier/project witness used for the query; variants that crossed the
/// transport boundary also carry the exact transport epoch and overlay sync state.
/// This remains an internal routing result (not a wire error): the composite logs it and
/// follows its existing fail-closed fallback policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SharedEngageFailure {
    kind: SharedEngageFailureKind,
    source: String,
    config: String,
    generation: u64,
    transport_epoch: Option<ProviderEpoch>,
    sync_state: Option<OverlaySyncState>,
}

/// Exhaustive engagement refusal classes. These replace the former three-way `None`
/// collapse and retain diagnostic-operation refusals at the same observable boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SharedEngageFailureKind {
    /// The configured project that owns the carrier SOURCE does not admit the generated
    /// units this serve would write (`units` are the offenders, sorted). Decided BEFORE
    /// the transport is touched: nothing was written.
    GeneratedUnitsNotAdmitted {
        reason: GeneratedUnitNonAdmissionReason,
        units: Vec<String>,
    },
    /// No generated unit is recorded for the carrier, so there is no write set to prove
    /// admitted. Decided BEFORE the transport is touched: nothing was written.
    GeneratedUnitAdmissionUnproven,
    TransportUnavailable,
    QueriedCarrierNotSynced,
    LiveDecisionNotShared {
        reason: String,
    },
    ProjectDiagnosticsUnavailable,
    ProjectDiagnosticsFailed {
        error: String,
    },
}

impl SharedEngageFailureKind {
    /// Whether this refusal is the generated-unit admission gate. That answer holds for
    /// every request until the membership or the write set changes, so it is reported
    /// once per (carrier, generation) by the gate itself — a caller does not repeat it
    /// per request.
    pub(crate) fn is_generated_unit_refusal(&self) -> bool {
        matches!(
            self,
            Self::GeneratedUnitsNotAdmitted { .. } | Self::GeneratedUnitAdmissionUnproven
        )
    }
}

impl std::fmt::Display for SharedEngageFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "kind={:?} source={} config={} generation={} transport_epoch={:?} sync_state={:?}",
            self.kind,
            self.source,
            self.config,
            self.generation,
            self.transport_epoch,
            self.sync_state
        )
    }
}

/// The provider-side surface of a shared attach: what the overlay's
/// [`ProviderHub`] establishes and the composite serves through. ONE
/// production implementation ([`TsgoSharedProvider`] — the relay-shim
/// control channel + directly-connected `--api` checker); test doubles
/// implement the same contract so the REAL gates (establishment door,
/// hub-issued admission, epoch fencing) run against a real hub in tests.
pub trait SharedAttach: TypeProvider + Sized + Send + Sync + 'static {
    /// Attach to the editor's already-running engine using the pre-resolved
    /// demand (the non-owning handshake; fails closed to the OWNED baseline).
    fn establish_shared_attach<'a>(
        params: EstablishSharedParams<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, crate::tsgo::shared::EstablishError>> + Send + 'a>>;

    /// Re-decide the serve mode for a per-query resolved binding at the
    /// current generation through the live controller.
    fn redecide_for_binding(
        &self,
        binding: &ProjectBinding,
        generated_units: GeneratedUnitAdmissionFact,
        generation: u64,
    ) -> verter_session::external_ts::LiveDecision;

    /// The project-bound `--api` diagnostics oracle for a carrier in its OWN
    /// per-query resolved configured project.
    fn overlay_diagnostics_in_project<'a>(
        &'a self,
        path: &'a str,
        tsconfig: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>>;

    /// Whether the attach is still live (the hub watcher's death signal).
    fn attach_is_alive(&self) -> bool;
}

impl SharedAttach for TsgoSharedProvider {
    fn establish_shared_attach<'a>(
        params: EstablishSharedParams<'a>,
    ) -> Pin<Box<dyn Future<Output = Result<Self, crate::tsgo::shared::EstablishError>> + Send + 'a>>
    {
        Box::pin(TsgoSharedProvider::establish_shared(params))
    }

    fn redecide_for_binding(
        &self,
        binding: &ProjectBinding,
        generated_units: GeneratedUnitAdmissionFact,
        generation: u64,
    ) -> verter_session::external_ts::LiveDecision {
        TsgoSharedProvider::redecide_for_binding(self, binding, generated_units, generation)
    }

    fn overlay_diagnostics_in_project<'a>(
        &'a self,
        path: &'a str,
        tsconfig: &'a str,
    ) -> ProviderFuture<'a, Option<Vec<TypeDiagnostic>>> {
        Box::pin(TsgoSharedProvider::overlay_diagnostics_in_project(
            self, path, tsconfig,
        ))
    }

    fn attach_is_alive(&self) -> bool {
        TsgoSharedProvider::is_alive(self)
    }
}

/// A provider that passed the exact-epoch synchronization and live project-binding
/// decision barriers, plus the witnesses needed if the following diagnostics operation
/// itself refuses.
struct EngagedSharedProvider<P: SharedAttach> {
    provider: Arc<P>,
    /// The hub that owns the serving incarnation — diagnostics settle against
    /// its serving epoch, feature calls route through its guarded forwarding.
    hub: Arc<ProviderHub<P>>,
    source: String,
    config: String,
    generation: u64,
    transport_epoch: ProviderEpoch,
    sync_state: OverlaySyncState,
}

/// A feature route selected after admission. SHARED retains the owning hub and
/// the exact serving epoch until the feature call finishes; the hub settles
/// the call against that epoch, so a replacement landing mid-call can neither
/// attribute a stale answer to the fresh incarnation nor serve one from a
/// retired one.
enum FeatureProviderSelection<P: SharedAttach> {
    Managed(Arc<dyn TypeProvider>),
    Shared {
        /// The owning hub: invoking features through it guards the query
        /// (crash quarantine) and settles the answer against the serving
        /// epoch the selection captured.
        hub: Arc<ProviderHub<P>>,
        managed: Arc<dyn TypeProvider>,
        core: Arc<OverlayInner<P>>,
        provider_path: String,
        transport_epoch: ProviderEpoch,
    },
}

impl<P: SharedAttach> FeatureProviderSelection<P> {
    async fn invoke<R, F, Fut>(self, invoke: F) -> Result<R, TypeProviderError>
    where
        F: Fn(Arc<dyn TypeProvider>) -> Fut,
        Fut: Future<Output = Result<R, TypeProviderError>>,
    {
        match self {
            Self::Managed(provider) => invoke(provider).await,
            Self::Shared {
                hub,
                managed,
                core,
                provider_path,
                transport_epoch,
            } => {
                let still_serving = || hub.serving_epoch() == Some(transport_epoch);
                // The pre-call check closes replacements that land after
                // selection: content must be confirmed synced into the exact
                // captured epoch AND that epoch must still be the hub's
                // serving one.
                if !core
                    .hub
                    .overlay_sync_state(&provider_path, transport_epoch)
                    .is_synced()
                    || !still_serving()
                {
                    return invoke(managed).await;
                }
                let hub_handle: Arc<dyn TypeProvider> = hub.clone();
                let shared_result = invoke(hub_handle).await;
                // A settled answer the captured epoch still owns is returned
                // as-is (an engine error propagates exactly as before); any
                // replacement or content desync converts success OR error
                // into the managed fallback.
                if still_serving()
                    && core
                        .hub
                        .overlay_sync_state(&provider_path, transport_epoch)
                        .is_synced()
                {
                    shared_result
                } else {
                    invoke(managed).await
                }
            }
        }
    }
}

/// Run one shared diagnostics operation only while its selected epoch remains
/// the exact content-synchronized epoch. Returns the stale sync witness to
/// callers that need to construct a typed refusal before activating their own
/// fallback policy. The hub-epoch settle (a replacement landing mid-call) is
/// the caller's — it holds the owning hub.
async fn observe_epoch_bound<T, R, SharedCall, SharedFuture>(
    core: &LazyOverlayCore<T>,
    provider_path: &str,
    transport_epoch: ProviderEpoch,
    shared_call: SharedCall,
) -> Result<R, OverlaySyncState>
where
    T: OverlayTransport,
    SharedCall: FnOnce() -> SharedFuture,
    SharedFuture: Future<Output = R>,
{
    let before = core.sync_state_for_epoch(provider_path, transport_epoch);
    if !before.is_synced() {
        return Err(before);
    }
    let result = shared_call().await;
    let after = core.sync_state_for_epoch(provider_path, transport_epoch);
    if after.is_synced() {
        Ok(result)
    } else {
        Err(after)
    }
}

struct OverlayInner<P: SharedAttach> {
    /// The host — the live published-snapshot + per-project R21 env-dims authority
    /// the per-query binding resolution reads from.
    host: Arc<VerterHost>,
    /// The rendezvous evidence the attach is lazily established from.
    rendezvous: SharedRendezvous,
    /// The ProviderHub that OWNS the shared attach: it establishes lazily
    /// through its discriminant re-arm door, mints the serving epoch every
    /// witness and admitted request binds to, retires the epoch fail-closed
    /// when the attach dies, and enforces generated-unit admission on every
    /// provider-visible write. This is the ONE serving/lifecycle/admission
    /// authority of the shared route — there is no second transport cell,
    /// epoch mint, or admission cache beside it.
    hub: Arc<ProviderHub<P>>,
    /// The latest bound demand the query path captured for the establisher:
    /// the pre-resolved carrier binding (+ admission fact + generation) the
    /// attach is established with. Written immediately before each
    /// establishment demand; the singleflight reuses the first caller's
    /// demand, exactly as concurrent demands previously joined one
    /// establishment.
    attach_demand: Arc<StdMutex<Option<SharedAttachDemand>>>,
    /// The source-resolution half of [`SharedTsgoOverlay::injection_is_shadow_safe`],
    /// per SOURCE for ONE workspace content generation. A carrier's companions (IDE,
    /// API, testing, sidecar) share their source's answer, and resolving project
    /// ownership is the expensive half; the generation is what any change of answer
    /// advances, so an older generation's entries are dropped wholesale.
    source_shadow_safety: parking_lot::Mutex<(u64, std::collections::HashMap<String, bool>)>,
    /// The monotonic sweep generation: it advances whenever the host's admission epoch
    /// (the published root by identity, the content generation, the project generation)
    /// differs from the last one observed. Every per-unit write decision the overlay core
    /// caches is keyed on it, so a changed `include`/`files`/`exclude`, a changed owner,
    /// or a changed file set re-decides every unit.
    sweep_generation: parking_lot::Mutex<(Option<AdmissionEpoch>, u64)>,
    /// The carriers whose admission refusal was already reported at ONE sweep generation.
    reported_refusals: parking_lot::Mutex<(u64, std::collections::HashSet<String>)>,
}

/// The pre-resolved binding evidence one SHARED attach establishment runs with.
#[derive(Clone)]
struct SharedAttachDemand {
    binding: ProjectBinding,
    generated_units: GeneratedUnitAdmissionFact,
    generation: u64,
}

/// How often the attach liveness watcher polls the serving incarnation — the
/// signal path from a dead attach (control `verter/fatal` / a closed pipe) to
/// the hub's crash signal. The hub retires the epoch fail-closed on the
/// signal; the re-arm door holds re-establishment until a fresh generation
/// discriminant (a reconnect).
const SHARED_LIVENESS_POLL_INTERVAL: Duration = Duration::from_millis(500);

/// The SHARED attach establishment strategy for the overlay's
/// [`ProviderHub`]: it attaches to the editor's already-running engine using
/// the LATEST bound demand the query path captured (a binding resolved BEFORE
/// any hub interaction — a non-binding carrier never demands an attach), and
/// wires the attach's liveness into the hub's crash signal.
struct SharedAttachBackend<P: SharedAttach> {
    rendezvous: SharedRendezvous,
    demand: Arc<StdMutex<Option<SharedAttachDemand>>>,
    attach: std::marker::PhantomData<fn() -> P>,
}

impl<P: SharedAttach> ProviderEstablisher<P> for SharedAttachBackend<P> {
    fn log_name(&self) -> &'static str {
        "TSGO(shared attach)"
    }

    fn user_label(&self) -> &'static str {
        "tsgo (editor-owned)"
    }

    fn restarting_error(&self) -> &'static str {
        "the editor-owned tsgo attach is re-arming"
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn establish<'a>(
        &'a self,
        crash_signal: Arc<tokio::sync::Notify>,
    ) -> verter_type_runtime::provider_hub::EstablishFuture<'a, P> {
        let Some(demand) = self.demand.lock().unwrap().clone() else {
            return Box::pin(async {
                Err(TypeProviderError::new(
                    "no bound carrier demand captured for the shared attach",
                ))
            });
        };
        let rendezvous = &self.rendezvous;
        Box::pin(async move {
            let tsconfig_path = demand.binding.tsconfig_uri().to_string();
            let params = EstablishSharedParams {
                control_dir: &rendezvous.control_dir,
                session_key: &rendezvous.session_key,
                workspace_root: &rendezvous.workspace_root,
                tsconfig_path: &tsconfig_path,
                resolution: CarrierOwnershipResolution::Bound(demand.binding.clone()),
                generated_units: demand.generated_units,
                config_generation: demand.generation,
                client_label: SHARED_CLIENT_LABEL,
            };
            let provider = P::establish_shared_attach(params).await.map_err(|error| {
                TypeProviderError::new(format!(
                    "SHARED editor route not established ({error}); managed fallback is \
                         eligible"
                ))
            })?;
            let provider = Arc::new(provider);
            // Liveness → crash signal: the hub that owns this incarnation owns
            // its death detection. The watcher exits once it observes death
            // (the signal it raises is inert for an already-replaced epoch).
            let watched = Arc::clone(&provider);
            tokio::spawn(async move {
                loop {
                    if !P::attach_is_alive(watched.as_ref()) {
                        crash_signal.notify_one();
                        return;
                    }
                    tokio::time::sleep(SHARED_LIVENESS_POLL_INTERVAL).await;
                }
            });
            Ok(provider)
        })
    }
}

impl<P: SharedAttach> SharedTsgoOverlay<P> {
    /// Build the overlay over the host and the rendezvous evidence. The
    /// shared attach is established lazily on the first bound carrier query
    /// (never the lifecycle path) through the overlay's [`ProviderHub`];
    /// the observed engine version is taken from the attach gate at that
    /// point.
    ///
    /// Must be called on a tokio runtime: the hub spawns its single-writer
    /// actor at construction.
    #[must_use]
    pub fn new(host: Arc<VerterHost>, rendezvous: SharedRendezvous) -> Self {
        let attach_demand: Arc<StdMutex<Option<SharedAttachDemand>>> =
            Arc::new(StdMutex::new(None));
        let hub = Arc::new(ProviderHub::new(
            SharedAttachBackend::<P> {
                rendezvous: rendezvous.clone(),
                demand: Arc::clone(&attach_demand),
                attach: std::marker::PhantomData,
            },
            // Logging-only notifications: the shared route's wire contract
            // attests its managed fallback stays cold until an observed
            // attach failure, and the editor-owned attach has no child pid
            // to announce — the attach itself stays off the
            // `$/verter/typeProviderStarted` channel.
            Arc::new(TracingNotifier),
            HubPolicy::lazy_attach(SHARED_ESTABLISH_TIMEOUT),
        ));
        Self {
            inner: Arc::new(OverlayInner {
                host,
                rendezvous,
                hub,
                attach_demand,
                source_shadow_safety: parking_lot::Mutex::default(),
                sweep_generation: parking_lot::Mutex::default(),
                reported_refusals: parking_lot::Mutex::default(),
            }),
        }
    }

    /// Build the overlay over a TEST attach hub: every production gate
    /// (establishment door, hub-issued admission, epoch fencing) runs against
    /// the given [`ProviderHub`], whose establisher hands back the test's
    /// [`SharedAttach`] double. The rendezvous is a stub the tests never
    /// attach through (they establish the hub directly).
    #[cfg(test)]
    pub(crate) fn over_test_hub(host: Arc<VerterHost>, hub: Arc<ProviderHub<P>>) -> Self {
        Self {
            inner: Arc::new(OverlayInner {
                host,
                rendezvous: SharedRendezvous {
                    control_dir: PathBuf::from("/test/no-relay"),
                    session_key: "test-session".to_string(),
                    workspace_root: "/test".to_string(),
                },
                hub,
                attach_demand: Arc::new(StdMutex::new(None)),
                source_shadow_safety: parking_lot::Mutex::default(),
                sweep_generation: parking_lot::Mutex::default(),
                reported_refusals: parking_lot::Mutex::default(),
            }),
        }
    }

    /// Bytes the shared hub's serving incarnation accepted. Desired-only
    /// `record_content` is not an application receipt.
    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        verter_type_runtime::traits::TypeProvider::applied_content(self.inner.hub.as_ref(), path)
    }

    /// Record the carrier's current content for the SHARED overlay — a cheap in-memory
    /// insert off the managed lifecycle critical path. It never establishes the SHARED
    /// transport, so opting into SHARED cannot trip the managed file-lifecycle timing
    /// (the foreground TSX sync is budgeted far below the SHARED establishment bound).
    /// The query path ([`Self::engage_diagnostics`]) establishes the transport and
    /// injects the recorded content lazily. A non-carrier path is ignored.
    fn record_content(&self, provider_path: &str, content: &str, priority: OverlayPriority) {
        if carrier_source_of(provider_path).is_none() {
            return;
        }
        self.inner
            .hub
            .overlay_state()
            .record_content_at_priority(provider_path, content, priority);
    }

    /// Retract a carrier overlay off the managed `close_file` critical path — drop its
    /// recorded content and, if a serving incarnation exists, issue the
    /// retract bounded + fail-closed (a slow/dead relay cannot hang or delay the managed
    /// close; a dead attach is retired by its hub anyway). The retract never triggers —
    /// or head-of-line-blocks on — an establishment (the non-establishing hub
    /// `serving()` read), and routes through the per-carrier gate so it is
    /// correctly ordered w.r.t. any in-flight injection.
    async fn feed_close(&self, provider_path: &str) {
        if carrier_source_of(provider_path).is_none() {
            return;
        }
        let hub = Arc::clone(&self.inner.hub);
        self.inner
            .hub
            .overlay_state()
            .retract_bounded(provider_path, SHARED_CLOSE_TIMEOUT, move || {
                hub.serving().map(|(provider, epoch)| {
                    Arc::new(HubAdmittedTransport::new(provider, Arc::clone(&hub), epoch))
                        as Arc<HubAdmittedTransport<P>>
                })
            })
            .await;
    }

    /// Establish, synchronize, and revalidate the exact editor-owned provider for a
    /// carrier already resolved to a configured project. A typed `Err` is an observed
    /// attach, synchronization, or live-decision failure and is the only condition that
    /// admits the managed fallback tier.
    ///
    /// The carrier binding is passed in PRE-RESOLVED (the composite gate resolved it
    /// ONCE via the shared [`project_binding`] helper): SHARED reuses the SAME binding
    /// (for its per-query re-decision + the attach), the SAME generation (for the
    /// re-arm discriminant), and the SAME already-minted `BoundProject`
    /// (`carrier.bound().project()` — the version-independent owning tsconfig) for the
    /// `--api` overlay target. There is NO second resolution and NO witness re-mint.
    async fn engage_provider(
        &self,
        provider_path: &str,
        carrier: &project_binding::BoundCarrier,
    ) -> Result<EngagedSharedProvider<P>, SharedEngageFailure> {
        let source = carrier_source_of(provider_path).unwrap_or_else(|| provider_path.to_string());
        let config = carrier.binding().tsconfig_uri().to_string();
        let generation = carrier.generation();
        let refusal = |kind, transport_epoch, sync_state| SharedEngageFailure {
            kind,
            source: source.clone(),
            config: config.clone(),
            generation,
            transport_epoch,
            sync_state,
        };
        // FIRST, before the attach is established or a single overlay is written:
        // prove that EVERY generated unit this serve would write is admitted to the
        // carrier's owning configured project — the resolver-side gate, over the exact
        // snapshot the binding was resolved at. Owning the carrier source is not that
        // proof — `src/**/*.vue` owns `Foo.vue` and admits no `Foo.vue.tsx` — and an
        // unadmitted unit written into the editor's engine lands in an inferred project.
        // A missing proof selects the managed route here, with its reason, rather than
        // after a SHARED attempt that already wrote. (The HUB-issued admission that
        // authorizes each physical write is minted per unit by the write gate below,
        // against the serving epoch the attach lands under.)
        let sweep_generation = self.sweep_generation();
        let generated_units = Self::recorded_units_of(self.inner.hub.overlay_state(), &source);
        let resolver_admission = if generated_units.is_empty() {
            None
        } else {
            Some(carrier.admit_generated_units(&generated_units))
        };
        let admission_refusal = match &resolver_admission {
            None => Some(SharedEngageFailureKind::GeneratedUnitAdmissionUnproven),
            Some(GeneratedUnitAdmission::NotAdmitted(not_admitted)) => {
                Some(SharedEngageFailureKind::GeneratedUnitsNotAdmitted {
                    reason: not_admitted.reason(),
                    units: not_admitted
                        .offending()
                        .iter()
                        .map(|(unit, _)| unit.as_str().to_string())
                        .collect(),
                })
            }
            Some(GeneratedUnitAdmission::Admitted(_)) => None,
        };
        if let Some(kind) = admission_refusal {
            // Units a PREVIOUS admission wrote (the membership narrowed since) leave the
            // editor's engine now. Non-establishing: no attach, nothing to withdraw.
            self.withdraw_unadmitted(&source, sweep_generation).await;
            if self.first_refusal_report(&source, sweep_generation) {
                tracing::info!(
                    source = %source,
                    config = %config,
                    generation,
                    refusal_kind = ?kind,
                    "editor-owned tsgo did not engage; activating managed fallback: the \
                     generated units are not proven admitted to the owning configured project"
                );
            }
            return Err(refusal(kind, None, None));
        }
        let generated_units_fact = match &resolver_admission {
            Some(admission @ GeneratedUnitAdmission::Admitted(_)) => {
                GeneratedUnitAdmissionFact::from_admission(admission)
            }
            // NotAdmitted was refused above; an unproven write set attaches
            // with the fail-closed fact.
            _ => GeneratedUnitAdmissionFact::Unproven,
        };

        // Lazily establish (once) the SHARED attach through the overlay's hub —
        // at QUERY time, off the managed lifecycle critical path (SHARED is never
        // fabricated; the binding is the gate's resolved one). The hub mints the
        // serving epoch every witness and admitted request below binds to.
        let engage_started = std::time::Instant::now();
        let established = self
            .ensure_serving(carrier, generated_units_fact)
            .await
            .ok_or_else(|| refusal(SharedEngageFailureKind::TransportUnavailable, None, None))?;
        let transport_ready = engage_started.elapsed();

        // Re-decide the serve mode through the live controller at the resolved
        // snapshot/config generation, reusing the SAME binding and the admission fact
        // proven above — BEFORE any overlay is written, so a not-SHARED decision admits
        // managed having written nothing.
        let decision = established.transport.provider().redecide_for_binding(
            carrier.binding(),
            generated_units_fact,
            carrier.generation(),
        );
        if decision.mode() != ServeMode::Shared {
            return Err(refusal(
                SharedEngageFailureKind::LiveDecisionNotShared {
                    reason: format!("{:?}", decision.decision().owned_reason()),
                },
                Some(established.epoch),
                None,
            ));
        }

        self.inject_editor_demand(
            self.inner.hub.overlay_state(),
            &established,
            provider_path,
            sweep_generation,
        )
        .await;
        tracing::debug!(
            provider_path,
            transport_ms = transport_ready.as_millis() as u64,
            total_ms = engage_started.elapsed().as_millis() as u64,
            "shared engage: attach ensured and editor-demand overlays injected"
        );

        // Admit managed when the queried carrier's current content is not
        // confirmed synced into the shared Program (its dirty injection failed) — never
        // serve SHARED diagnostics computed against stale/absent content (a prior synced
        // slot). Only a carrier whose current content is confirmed synced is served.
        let sync_state = self
            .inner
            .hub
            .overlay_sync_state(provider_path, established.epoch);
        if !sync_state.is_synced() {
            return Err(refusal(
                SharedEngageFailureKind::QueriedCarrierNotSynced,
                Some(established.epoch),
                Some(sync_state),
            ));
        }

        Ok(EngagedSharedProvider {
            provider: Arc::clone(established.transport.provider()),
            hub: Arc::clone(&self.inner.hub),
            source,
            config,
            generation,
            transport_epoch: established.epoch,
            sync_state,
        })
    }

    /// The monotonic sweep generation for the host's CURRENT admission epoch — see
    /// [`OverlayInner::sweep_generation`].
    fn sweep_generation(&self) -> u64 {
        let epoch = AdmissionEpoch::current(&self.inner.host);
        let mut state = self.inner.sweep_generation.lock();
        if state.0.as_ref() != Some(&epoch) {
            state.0 = Some(epoch);
            state.1 += 1;
        }
        state.1
    }

    /// The generated units recorded for the carrier `source` in `core` — its proposed
    /// write set. Grouping is the descriptor companion authority in reverse
    /// ([`carrier_source_of`]), so every companion family (IDE, import surface, testing,
    /// sidecar, declaration) of the source is included and nothing else.
    fn recorded_units_of<T: OverlayTransport>(
        core: &LazyOverlayCore<T>,
        source: &str,
    ) -> Vec<CanonicalPath> {
        core.recorded_paths()
            .iter()
            .filter(|unit| carrier_source_of(unit).as_deref() == Some(source))
            .map(|unit| CanonicalPath::new(unit))
            .collect()
    }

    /// The HUB-ISSUED write gate of the sweep: a [`GeneratedUnitWritePermit`]
    /// carrying the ProviderHub's [`AdmittedRequest`] for the `companion`'s
    /// carrier's whole recorded write set, or `None` — the unit is skipped
    /// and, if a prior sweep wrote it, retracted.
    ///
    /// The gate composes THREE independent refusals, each fail-closed:
    ///
    /// 1. **Shadow-safety** ([`Self::injection_is_shadow_safe`]) — no real
    ///    user file is displaced. A workspace-side fact, memoized per source
    ///    per content generation.
    /// 2. **Project binding** — the carrier source's owning configured
    ///    project, bound to the serving epoch and the exact publication
    ///    basis through [`ProviderHub::bind_project`]. A warm current
    ///    witness is reused ([`ProviderHub::bound_project`]); a drifted or
    ///    absent one re-resolves through the ONE shared resolver and
    ///    re-binds. This is the ONLY binding/witness warmth — the hub owns
    ///    it; no admission cache survives beside it.
    /// 3. **Generated-unit admission** — the workspace membership proof that
    ///    EVERY unit of the carrier's write set is a member of the owning
    ///    project, consumed by the hub ([`ProviderHub::admit_request_with`])
    ///    which validates the proof's project, publication identity, and
    ///    exact unit set. The hub's request cache is the one warm admission
    ///    authority; a publication, membership change, or provider
    ///    replacement invalidates it.
    ///
    /// The permit can only be minted from the hub's admitted request, so the
    /// queried carrier, its companions, and every neighbour the sweep reaches
    /// are held to the same rule — a recorded carrier is never written on the
    /// strength of its priority or a stale epoch's authorization.
    fn generated_unit_write_permit<T: OverlayTransport>(
        &self,
        core: &LazyOverlayCore<T>,
        companion: &str,
    ) -> Option<GeneratedUnitWritePermit> {
        if !self.injection_is_shadow_safe(companion) {
            return None;
        }
        let source = carrier_source_of(companion)?;
        let units = Self::recorded_units_of(core, &source);
        if units.is_empty() {
            return None;
        }
        let witness = match self
            .inner
            .hub
            .bound_project(&normalize_canonical_id(&source))
        {
            Some(witness) => witness,
            None => self.bind_carrier_witness(&source)?,
        };
        let admission = self
            .inner
            .hub
            .admit_request_with(&witness, &units, || {
                // Cold path: a membership query needs the resolver's bound
                // carrier (the snapshot basis + the owning tsconfig). A
                // fail-closed resolution (no owner) fails the write closed —
                // never a fabricated proof.
                match project_binding::resolve_carrier_bound(&self.inner.host, &source).into_bound()
                {
                    Some(carrier) => {
                        carrier.admit_generated_units(&Self::recorded_units_of(core, &source))
                    }
                    None => GeneratedUnitAdmission::unresolved_owner(&units),
                }
            })
            .ok()?;
        Some(GeneratedUnitWritePermit::admitted(admission))
    }

    /// Resolve the carrier `source`'s owning project through the ONE shared
    /// resolver and bind it to the hub's serving incarnation — the cold path
    /// of [`Self::generated_unit_write_permit`]'s binding gate. `None` when
    /// the resolver fails closed (no snapshot, no project, ambiguous,
    /// scratch, or a mint refusal): no witness, no write.
    fn bind_carrier_witness(
        &self,
        source: &str,
    ) -> Option<verter_type_runtime::provider_hub::ProjectWitness> {
        let host = &self.inner.host;
        let (resolution, _, published) = project_binding::resolve_carrier_with_publication(
            host.as_ref(),
            source,
            Arc::from(""),
            project_binding::OwnershipReadinessMode::PresentSnapshotAuthoritative,
        )?;
        match resolution {
            CarrierOwnershipResolution::Bound(binding) => {
                let input = project_binding::hub_binding_input(host, source, &binding, published);
                self.inner.hub.bind_project(input).ok()
            }
            // Every non-bound resolution is a fail-closed no-witness state.
            _ => None,
        }
    }

    /// Inject the recorded content of the EDITOR-DEMAND carrier set into `established`.
    ///
    /// Every open carrier (dirty-tracked — only what changed since the last injection) so
    /// the queried carrier's `--api` diagnostics see the current text AND its companion
    /// family / imported carriers are members of the SHARED Program (else its imports
    /// spuriously fail with TS2307) — the normal open→diagnostics flow, now that the
    /// lifecycle only RECORDS content off-path. Best-effort: a failed inject admits the
    /// managed fallback.
    ///
    /// Two independent bounds apply to every recorded unit:
    ///
    /// * SCOPE — the request pays only for the carriers the editor is working with: every
    ///   carrier an editor lifecycle lane recorded (the open documents plus the import
    ///   closure the background import publication delivers), plus the queried carrier's
    ///   own companion family whatever lane recorded it. That last clause is
    ///   load-bearing: the caller's `is_synced` gate is unconditional, so a queried
    ///   carrier scoped out would fail closed and admit the managed fallback. The
    ///   workspace scan's BACKGROUND bulk is deliberately excluded; see
    ///   [`LazyOverlayCore::inject_all_dirty`] for why.
    /// * WRITE PERMIT — scope never authorizes a write. Each unit needs
    ///   [`Self::generated_unit_write_permit`]; a unit without one is skipped and, if a
    ///   previous sweep wrote it, retracted.
    ///
    /// `sweep_generation` keys the overlay core's per-unit decision cache: a clean unit is
    /// re-decided only when it advances, and it advances on any publication, content, or
    /// project-generation change — so neither a real user file appearing at a companion
    /// path (`carrier_never_shadows_real_user_file`) nor a narrowed `include` is answered
    /// from a stale decision.
    async fn inject_editor_demand(
        &self,
        core: &LazyOverlayCore<HubAdmittedTransport<P>>,
        established: &ServingTransport<HubAdmittedTransport<P>>,
        provider_path: &str,
        sweep_generation: u64,
    ) {
        let queried_source = carrier_source_of(provider_path);
        let _ = self
            .inner
            .hub
            .synchronize(
                established.epoch,
                sweep_generation,
                |companion, priority| {
                    priority >= OverlayPriority::Normal
                        || carrier_source_of(companion) == queried_source
                },
                |companion| self.generated_unit_write_permit(core, companion),
            )
            .await;
    }

    /// Withdraw the carrier `source`'s units from a live serving incarnation after
    /// its write set stopped being admitted. Never establishes: with no serving
    /// incarnation nothing was written, so there is nothing to withdraw. The sweep's
    /// own no-permit arm does the work — it retracts exactly the units a previous
    /// sweep committed.
    async fn withdraw_unadmitted(&self, source: &str, sweep_generation: u64) {
        // Non-establishing: the hub's serving read never demands an attach —
        // with no serving incarnation nothing was written, so there is
        // nothing to withdraw.
        let Some((_, epoch)) = self.inner.hub.serving() else {
            return;
        };
        let core = self.inner.hub.overlay_state();
        let _ = self
            .inner
            .hub
            .synchronize(
                epoch,
                sweep_generation,
                |companion, _| carrier_source_of(companion).as_deref() == Some(source),
                |companion| self.generated_unit_write_permit(core, companion),
            )
            .await;
    }

    /// Whether this is the FIRST admission refusal reported for `source` at
    /// `sweep_generation`. The refusal is a property of the membership and the write set,
    /// not of the request, so it is reported once until either changes.
    fn first_refusal_report(&self, source: &str, sweep_generation: u64) -> bool {
        let mut reported = self.inner.reported_refusals.lock();
        if reported.0 != sweep_generation {
            *reported = (sweep_generation, std::collections::HashSet::new());
        }
        reported.1.insert(source.to_string())
    }

    /// Project-bound diagnostics from the exact editor-owned Program. `Some([])` is an
    /// authoritative clean result; `None` or an error admits only the managed provider's
    /// project-bound capability, never a raw companion LSP pull.
    async fn engage_diagnostics(
        &self,
        provider_path: &str,
        carrier: &project_binding::BoundCarrier,
    ) -> Result<Vec<TypeDiagnostic>, SharedEngageFailure> {
        let engaged = self.engage_provider(provider_path, carrier).await?;
        let diagnostics_result = observe_epoch_bound(
            self.inner.hub.overlay_state(),
            provider_path,
            engaged.transport_epoch,
            || {
                engaged
                    .provider
                    .overlay_diagnostics_in_project(provider_path, carrier.bound().project())
            },
        )
        .await
        // Settle against the owning hub: an attach replaced mid-call never
        // serves its answer as the fresh incarnation's.
        .and_then(|diagnostics| match self.inner.hub.serving_epoch() {
            Some(epoch) if epoch == engaged.transport_epoch => Ok(diagnostics),
            active => Err(OverlaySyncState::TransportEpochMismatch {
                expected: engaged.transport_epoch,
                active: active.unwrap_or(engaged.transport_epoch),
            }),
        })
        .map_err(|sync_state| SharedEngageFailure {
            kind: SharedEngageFailureKind::QueriedCarrierNotSynced,
            source: engaged.source.clone(),
            config: engaged.config.clone(),
            generation: engaged.generation,
            transport_epoch: Some(engaged.transport_epoch),
            sync_state: Some(sync_state),
        })?;
        match diagnostics_result {
            Ok(Some(diagnostics)) => Ok(diagnostics),
            Ok(None) => Err(SharedEngageFailure {
                kind: SharedEngageFailureKind::ProjectDiagnosticsUnavailable,
                source: engaged.source,
                config: engaged.config,
                generation: engaged.generation,
                transport_epoch: Some(engaged.transport_epoch),
                sync_state: Some(engaged.sync_state),
            }),
            Err(error) => Err(SharedEngageFailure {
                kind: SharedEngageFailureKind::ProjectDiagnosticsFailed {
                    error: error.to_string(),
                },
                source: engaged.source,
                config: engaged.config,
                generation: engaged.generation,
                transport_epoch: Some(engaged.transport_epoch),
                sync_state: Some(engaged.sync_state),
            }),
        }
    }

    /// Whether injecting the recorded `companion_path` overlay is shadow-safe — i.e. no
    /// REAL user file is displaced. TWO independent gates, either of which fails closed:
    ///
    /// 1. **Disk-occupancy at the EXACT injected path (defense-in-depth).** The injected
    ///    companion paths — IDE (`Foo.vue.tsx` / `.jsx` / `Foo.svelte.tsx`), DECLARATION
    ///    (`Foo.d.vue.ts` / `Foo.d.svelte.ts`), API (`Foo.vue.verter.ts`), testing-API,
    ///    sidecar, and any other companion [`carrier_source_of`] admits — all live in the
    ///    USER namespace. A REAL user file at the exact path SHARED is about to inject
    ///    generated content at is a collision Verter must NEVER overlay-shadow
    ///    (`carrier_never_shadows_real_user_file`), for EVERY companion type. This exact-
    ///    path occupancy gate closes the whole class uniformly, independent of what source
    ///    [`carrier_source_of`] derives AND of what gate (2)'s conflict pass enumerated, so
    ///    a stale VFS snapshot, a future companion form, or a direct overlay call can never
    ///    slip a shadow past ([`real_file_occupies_injected_path`]).
    /// 2. **Source-resolution shadow-safety.** For a genuine virtual companion (no real
    ///    file at its path), the source is resolved and its shadow-cause honoured. The
    ///    resolver's UNCONDITIONAL carrier-path-conflict pass (`carrier_path_conflict` over
    ///    `carrier_companion_identities_for_source`) enumerates EVERY descriptor-owned
    ///    companion family — IDE, declaration, import-surface API, testing-API, sidecar —
    ///    so a REAL user file at ANY of those companion paths (not just the IDE companion)
    ///    downgrades the source to `Ambiguous(CarrierPathOccupiedByRealFile)`; a same-stem
    ///    rune module beside the source downgrades it to `Ambiguous(SameStemRuneModule)`.
    ///    Either downgrade means skipped and managed serves. A genuine generated companion (a
    ///    clean binding, `NoProject`, `NotReady`, or a MultipleOwners ambiguity,
    ///    none of which sit a REAL file at a companion path) displaces no user file.
    ///
    /// Shadow-safety is NECESSARY for a write, not sufficient: it says no real file is
    /// displaced, not that a configured project admits the unit. The write gate
    /// ([`Self::generated_unit_write_permit`]) additionally requires the generated-unit
    /// admission proof, so a shadow-safe companion of a carrier with no owning project is
    /// still never written.
    ///
    /// A not-a-companion path or a not-yet-ready snapshot is conservatively NOT injected.
    fn injection_is_shadow_safe(&self, companion_path: &str) -> bool {
        // (1) Disk-occupancy at the EXACT injected path — the defense-in-depth gate that
        //     covers every companion type (IDE / declaration / API / testing / sidecar)
        //     uniformly at the injected path.
        // Every host read runs under a semantic-activity guard (see
        // `crate::documents::guarded_host`).
        let _activity = self.inner.host.semantic_activity();
        let ws_read = self.inner.host.workspace_read();
        if real_file_occupies_injected_path(ws_read.as_ref(), companion_path) {
            return false;
        }
        // (2) Source-resolution shadow-safety: the source's descriptor carrier-companion
        //     conflict (across EVERY companion family) or a same-stem rune module beside
        //     it, for a genuine virtual companion. The empty `ts_version` is safe — the
        //     shadow-safety decision is version-independent (it reads the resolution KIND,
        //     not the binding).
        let Some(source) = carrier_source_of(companion_path) else {
            return false;
        };
        let generation = ws_read.content_generation();
        {
            let mut memo = self.inner.source_shadow_safety.lock();
            if memo.0 != generation {
                *memo = (generation, std::collections::HashMap::new());
            }
            if let Some(safe) = memo.1.get(&source) {
                return *safe;
            }
        }
        let Some((resolution, _)) = project_binding::resolve_carrier(
            self.inner.host.as_ref(),
            &source,
            Arc::from(""),
            project_binding::OwnershipReadinessMode::PresentSnapshotAuthoritative,
        ) else {
            // No resolution yet is not an answer: never remembered.
            return false;
        };
        let safe = injection_shadow_safe(&resolution);
        let mut memo = self.inner.source_shadow_safety.lock();
        // Only an answer for the generation it was computed under is kept.
        if memo.0 == generation {
            memo.1.insert(source, safe);
        }
        safe
    }

    /// Lazily establish (once) the SHARED attach for the carrier's ALREADY-resolved
    /// binding (resolved once by the composite gate at `generation`) through the
    /// overlay's [`ProviderHub`] — the singleflight, bounded, discriminant-re-arming,
    /// retire-on-death authority that OWNS the serving epoch. Only a bound carrier
    /// reaches here (the gate resolved the binding before calling
    /// [`Self::engage_diagnostics`]), so a transient non-binding carrier never poisons
    /// the re-arm gate. Concurrent queries reuse the ONE in-flight establishment; a
    /// slow/broken attach is bounded by [`SHARED_ESTABLISH_TIMEOUT`] (then managed is
    /// admitted, never a stall); a failed ATTACH re-arms on a fresh advertisement/editor
    /// generation OR a fresh workspace/config generation — never a retry storm within
    /// one discriminant; a dead attach retires the epoch fail-closed.
    ///
    /// On success the core OBSERVES the hub-minted serving epoch (a re-attachment
    /// resets the injection markers so the open set replays into the fresh
    /// incarnation), and the identity-bound [`ServingTransport`] is returned so
    /// injection is attributed to the EXACT serving epoch, not a re-read of the core's
    /// active epoch.
    async fn ensure_serving(
        &self,
        carrier: &project_binding::BoundCarrier,
        generated_units: GeneratedUnitAdmissionFact,
    ) -> Option<ServingTransport<HubAdmittedTransport<P>>> {
        let generation = carrier.generation();
        *self.inner.attach_demand.lock().unwrap() = Some(SharedAttachDemand {
            binding: carrier.binding().clone(),
            generated_units,
            generation,
        });
        let hub = Arc::clone(&self.inner.hub);
        let epoch = hub
            .establish_rearming(|| self.probe_establishment_discriminant(generation))
            .await
            .ok()?;
        let (provider, serving_epoch) = hub.serving()?;
        // Observe the serving epoch BEFORE handing the incarnation to the
        // injection path — a re-attachment (new epoch) resets the markers so
        // the open set replays into it.
        if epoch != serving_epoch {
            return None;
        }
        Some(ServingTransport {
            transport: Arc::new(HubAdmittedTransport::new(provider, hub, serving_epoch)),
            epoch: serving_epoch,
        })
    }

    /// The re-arm discriminant for a failed SHARED establishment at config
    /// `generation`: BOTH the shim advertisement nonce (a cheap FS read) AND the
    /// workspace/config generation, composed by [`compose_establishment_discriminant`].
    /// `None` when no advertisement is observable (a flapping / absent shim never
    /// storms establishment — the hub's re-arm door holds the fail-closed state
    /// while no discriminant is observable).
    fn probe_establishment_discriminant(&self, generation: u64) -> Option<String> {
        let nonce = Advertisement::find_for_session_key(
            &self.inner.rendezvous.control_dir,
            &self.inner.rendezvous.session_key,
        )
        .ok()
        .map(|(_, adv)| adv.nonce)?;
        Some(compose_establishment_discriminant(&nonce, generation))
    }
}

/// The carrier SOURCE (`Foo.vue`) a provider companion path projects from, or `None`
/// when `provider_path` is not a framework-carrier companion.
///
/// Routes through the descriptor companion-classification authority
/// ([`classify_carrier_companion`]): every companion family reverse-maps to its TRUE
/// carrier source — the IDE `Foo.vue.tsx` / `Foo.vue.jsx`, the extension-middle
/// declaration `Foo.d.vue.ts`, the `.verter.ts` import-surface API, and the
/// testing-API / sidecar surfaces. A declaration companion maps to `Foo.vue` (the
/// descriptor inverts the `.d.` infix), never the intermediate `Foo.d.vue` stem. A
/// plain `.ts`/`.tsx` file (no carrier stem) yields `None` (the managed provider serves
/// it). Backslash paths normalize to the same forward-slashed source.
fn carrier_source_of(provider_path: &str) -> Option<String> {
    classify_carrier_companion(provider_path).map(|companion| companion.source)
}

/// Whether a carrier companion whose SOURCE resolved to `resolution` is shadow-safe to
/// inject into the SHARED Program. A real user file occupying a descriptor
/// carrier-companion path (or a same-stem rune module beside the source) downgrades the
/// source to an `Ambiguous` real-file-shadow cause — Verter must NEVER overlay-shadow it
/// (`carrier_never_shadows_real_user_file`). Every other resolution (a clean binding,
/// `NoProject`, `NotReady`, or a MultipleOwners ambiguity — none of which sit a
/// REAL file at the companion path) leaves a GENUINE virtual companion safe to inject as
/// a supporting Program member. Typed over [`CarrierOwnershipResolution`] — never a path-shape or
/// substring check.
fn injection_shadow_safe(resolution: &CarrierOwnershipResolution) -> bool {
    !matches!(
        resolution,
        CarrierOwnershipResolution::Ambiguous {
            cause: AmbiguityCause::CarrierPathOccupiedByRealFile
                | AmbiguityCause::SameStemRuneModule,
            ..
        }
    )
}

/// Whether a REAL user file already occupies the EXACT path the SHARED overlay is about
/// to inject generated carrier content at. The injected companion paths — IDE
/// (`Foo.vue.tsx` / `.jsx` / `Foo.svelte.tsx`), DECLARATION (`Foo.d.vue.ts` /
/// `Foo.d.svelte.ts`), API (`Foo.vue.verter.ts`), and any other companion
/// [`carrier_source_of`] admits — all live in the USER namespace, so a real user file at
/// that exact path is a shadow collision Verter must NEVER overlay-shadow
/// (`carrier_never_shadows_real_user_file`).
///
/// This exact-path occupancy probe is an INDEPENDENT injection-boundary guard, NOT an
/// ownership classifier: it maps nothing back to a source (that is [`carrier_source_of`]'s
/// role, which reverse-maps every companion — including the extension-middle declaration
/// `Foo.d.vue.ts` -> the real `Foo.vue` — through the descriptor authority). It fails the
/// injection closed the instant a real file sits at the injected path, uniformly across
/// every companion type and independent of what the source-resolution conflict pass
/// enumerated — defense-in-depth so a stale VFS snapshot, a future companion form, or a
/// direct overlay call can never overlay-shadow a user file even if the source-side
/// conflict pass did not flag it. Probes the shared workspace/VFS authority
/// ([`WorkspaceRead::file_exists`] — the same disk-occupancy machinery the resolver's
/// carrier-path-conflict pass uses, never a private disk reimplementation) over the
/// NORMALIZED path, so a non-canonical (backslash / uppercase-drive) injected path cannot
/// evade the probe on a case-insensitive FS.
fn real_file_occupies_injected_path(ws: &dyn WorkspaceRead, injected_path: &str) -> bool {
    ws.file_exists(&normalize_canonical_id(injected_path))
}

/// Compose the SHARED-establishment re-arm discriminant from the shim advertisement
/// `nonce` and the workspace/config `generation`. A change to EITHER field yields a
/// distinct discriminant, so a failed establishment re-arms on a reconnect (fresh
/// nonce) OR a fresh published snapshot (fresh generation) — never nonce-only (the
/// transport-cell-poisoning fix's re-arm rail). The `\u{1f}` unit separator can never
/// appear in the hex nonce or the decimal generation, so the composition is injective.
fn compose_establishment_discriminant(nonce: &str, generation: u64) -> String {
    format!("{nonce}\u{1f}{generation}")
}

/// Every carrier TS FEATURE provider call the composite GATES on a resolved
/// `BoundProject` admission. Each variant maps 1:1 to exactly one gated `TypeProvider`
/// feature method on [`TsgoCompositeProvider`]; the enum is the EXHAUSTIVE registry of
/// gated features (no wildcard arm).
///
/// TWO distinct enforcement layers — do not conflate them:
/// * COMPILE-TIME: [`Self::name`]'s wildcard-free `match` makes only the
///   VARIANT→`name()` mapping exhaustive — a new variant must be named there or the
///   crate fails to compile. It does NOT tie variants to methods.
/// * BEHAVIOR-ENFORCED: the typed `owned_binding_gate` integration suite invokes every
///   feature group through [`TypeProvider`]. It proves that a denied carrier never calls
///   the managed provider, while bound carriers and plain TypeScript inputs delegate.
///   Runtime routing enters through [`TsgoCompositeProvider::feature_provider`], which
///   resolves the bound-carrier witness once and applies shared-first/fallback ordering.
///
/// Provenance class (informs the DENIED shape the HANDLER layer composes, NOT a
/// composite-runtime branch — every denied feature serves its own type's empty/none
/// external default):
/// * EXTERNAL-ONLY — no native sub-answer to merge; denied ⇒ empty/none.
/// * MIXED — the LSP handler merges a native sub-answer; denied ⇒ the external default
///   (empty/none) so the handler merge preserves the native side.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProviderFeature {
    // ── EXTERNAL-ONLY (denied ⇒ empty/none; no owned call, no native sub-answer) ──
    /// `get_type_definition`.
    TypeDefinition,
    /// `get_signature_help`.
    SignatureHelp,
    /// `get_semantic_tokens`.
    SemanticTokens,
    // ── MIXED (denied ⇒ external default so the handler merge preserves the native) ──
    /// `get_hover`.
    Hover,
    /// `get_definition`.
    Definition,
    /// `get_references`.
    References,
    /// `get_document_highlights`.
    DocumentHighlights,
    /// `get_inlay_hints`.
    InlayHints,
    /// `get_completions` (MIXED).
    Completions,
    /// `get_completion_details` (MIXED).
    CompletionDetails,
    /// `resolve_completion` (EXTERNAL-ONLY — denied ⇒ no provider enrichment).
    ResolveCompletion,
    /// `get_rename_locations` (MIXED — the LSP handler's incomplete-rename safety gates
    /// stay a separate layer this admission does not touch).
    RenameLocations,
    /// `get_code_actions` (MIXED — the LSP `handle_code_action` handler contributes
    /// native Verter carrier code-actions (organize-imports, extract-component,
    /// macro/component/event actions, action-engine fixes) and MERGES the provider's
    /// `getCodeFixes` quickfixes over them; denied ⇒ the empty external default so the
    /// handler merge preserves the native side).
    CodeActions,
}

impl ProviderFeature {
    /// The stable feature name, for admission observability. EXHAUSTIVE match (no
    /// wildcard): the ONLY compile-time-enforced property is that every variant is named
    /// here (a new variant fails to compile until it is). The public feature behavior is
    /// pinned by the typed `owned_binding_gate` integration suite (see the type doc).
    fn name(self) -> &'static str {
        match self {
            ProviderFeature::TypeDefinition => "type_definition",
            ProviderFeature::SignatureHelp => "signature_help",
            ProviderFeature::SemanticTokens => "semantic_tokens",
            ProviderFeature::Hover => "hover",
            ProviderFeature::Definition => "definition",
            ProviderFeature::References => "references",
            ProviderFeature::DocumentHighlights => "document_highlights",
            ProviderFeature::InlayHints => "inlay_hints",
            ProviderFeature::Completions => "completions",
            ProviderFeature::CompletionDetails => "completion_details",
            ProviderFeature::ResolveCompletion => "resolve_completion",
            ProviderFeature::RenameLocations => "rename_locations",
            ProviderFeature::CodeActions => "code_actions",
        }
    }
}

/// The always-present host-aware admission and serving-order layer.
pub struct TsgoCompositeProvider {
    /// Managed fallback. It is eager in an explicit managed mode and statefully lazy
    /// when the editor-owned route is armed.
    managed: Arc<dyn TypeProvider>,
    /// Live published-snapshot + per-project R21 env-dims authority.
    host: Arc<VerterHost>,
    /// Exact editor-session route, present only with rendezvous evidence.
    /// Its [`ProviderHub`] is the ONE hub authority of the shared route: a
    /// bound carrier's feature engagement and every generated-unit write
    /// carry its witnesses.
    shared: Option<SharedTsgoOverlay>,
    /// Compiler-lifted file-check directive for each generated companion.
    /// The configured-project diagnostics API is not available for every
    /// generated JSX root, so the fallback must retain the authored override
    /// without reaching into an opaque managed provider.
    file_check_directives: dashmap::DashMap<String, FileCheckDirective>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileCheckDirective {
    Check,
    NoCheck,
}

fn leading_file_check_directive(content: &str) -> Option<FileCheckDirective> {
    let first_line = content.split_once('\n').map_or(content, |(line, _)| line);
    let first_line = first_line.strip_suffix('\r').unwrap_or(first_line);
    match first_line {
        "// @ts-check" => Some(FileCheckDirective::Check),
        "// @ts-nocheck" => Some(FileCheckDirective::NoCheck),
        _ => None,
    }
}

fn effective_javascript_check_policy(
    configured_check_js: Option<bool>,
    directive: Option<FileCheckDirective>,
) -> Option<bool> {
    match directive {
        Some(FileCheckDirective::Check) => Some(true),
        Some(FileCheckDirective::NoCheck) => Some(false),
        None => configured_check_js,
    }
}

enum ManagedWrite {
    Open,
    Load,
    Update,
}

impl TsgoCompositeProvider {
    /// Build the layer over a managed provider slot, the binding authority, and an
    /// optional exact-editor route.
    #[must_use]
    pub fn new(
        managed: Arc<dyn TypeProvider>,
        host: Arc<VerterHost>,
        shared: Option<SharedTsgoOverlay>,
    ) -> Self {
        Self {
            managed,
            host,
            shared,
            file_check_directives: dashmap::DashMap::new(),
        }
    }

    fn record_file_check_directive(&self, path: &str, content: &str) {
        let path = normalize_canonical_id(path);
        match leading_file_check_directive(content) {
            Some(directive) => {
                self.file_check_directives.insert(path, directive);
            }
            None => {
                self.file_check_directives.remove(&path);
            }
        }
    }

    fn clear_file_check_directive(&self, path: &str) {
        self.file_check_directives
            .remove(&normalize_canonical_id(path));
    }

    fn effective_javascript_check_policy(
        &self,
        path: &str,
        configured_project: &str,
    ) -> Option<bool> {
        let path = normalize_canonical_id(path);
        let directive = self
            .file_check_directives
            .get(&path)
            .map(|entry| *entry.value());
        effective_javascript_check_policy(
            self.configured_project_check_js(configured_project),
            directive,
        )
    }

    /// Select the provider for a feature query while preserving the serving order.
    ///
    /// A NON-carrier path (plain `.ts`/`.tsx`, `carrier_source_of == None`) is UNGATED:
    /// it delegates to managed unchanged. A carrier companion admits through the
    /// generation-scoped [`CarrierAdmissionCache`] (the ONE shared `resolve_carrier_bound`
    /// resolver, memoized): only a resolved `BoundProject` admits. Every non-bound state —
    /// and, by the cache's construction, any never-produced state — FAILS CLOSED: the
    /// caller serves its type's empty/none external default, NEVER a `tsgo --lsp`
    /// self-discovery fall-through. `feature` ties each method to its [`ProviderFeature`]
    /// variant (the method↔variant registry) and labels the fail-closed trace.
    async fn feature_provider(
        &self,
        feature: ProviderFeature,
        path: &str,
    ) -> Option<FeatureProviderSelection<TsgoSharedProvider>> {
        // NON-carrier path (plain `.ts`/`.tsx`): not gated. In a carrier-only LSP
        // client this is not normally queried; an explicit request uses managed.
        let Some(source) = carrier_source_of(path) else {
            return Some(FeatureProviderSelection::Managed(Arc::clone(&self.managed)));
        };

        let carrier = project_binding::resolve_carrier_bound(&self.host, &source).into_bound();
        let Some(carrier) = carrier else {
            tracing::trace!(
                feature = feature.name(),
                source = %source,
                "carrier feature denied — no BoundProject; serving the external default \
                 (fail-closed, never a `--lsp` self-discovery fall-through)"
            );
            return None;
        };

        if let Some(shared) = &self.shared {
            match tokio::time::timeout(
                SHARED_OVERLAY_TIMEOUT,
                shared.engage_provider(path, &carrier),
            )
            .await
            {
                Ok(Ok(engaged)) => {
                    // Reports ONLY what this site observed. It cannot see the
                    // managed provider's session-long activation cell, so it must
                    // not claim the fallback stayed cold: managed may have
                    // activated earlier and the editor route recovered after.
                    // `LazyManagedTypeProvider` records its own activation.
                    tracing::info!(
                        feature = feature.name(),
                        source = %source,
                        "editor-owned tsgo served carrier feature"
                    );
                    return Some(FeatureProviderSelection::Shared {
                        hub: engaged.hub,
                        managed: Arc::clone(&self.managed),
                        core: Arc::clone(&shared.inner),
                        provider_path: path.to_string(),
                        transport_epoch: engaged.transport_epoch,
                    });
                }
                // The admission gate reported this once for the carrier's generation.
                Ok(Err(refusal)) if refusal.kind.is_generated_unit_refusal() => tracing::trace!(
                    feature = feature.name(),
                    source = %source,
                    refusal_kind = ?refusal.kind,
                    "editor-owned tsgo route refused on generated-unit admission; serving managed"
                ),
                Ok(Err(refusal)) => tracing::info!(
                    feature = feature.name(),
                    source = %source,
                    refusal = %refusal,
                    refusal_kind = ?refusal.kind,
                    config = %refusal.config,
                    generation = refusal.generation,
                    transport_epoch = ?refusal.transport_epoch,
                    sync_state = ?refusal.sync_state,
                    "editor-owned tsgo attach did not engage; activating managed fallback"
                ),
                Err(_) => tracing::warn!(
                    feature = feature.name(),
                    source = %source,
                    "editor-owned tsgo attach timed out; activating managed fallback"
                ),
            }
        }

        Some(FeatureProviderSelection::Managed(Arc::clone(&self.managed)))
    }

    /// Shared-first diagnostics entry used by both foreground and background queries.
    ///
    /// A NON-carrier path (plain `.ts`/`.tsx`) is NOT gated — it delegates to managed
    /// unchanged. A carrier companion resolves its owning project ONCE via the shared
    /// [`project_binding`] helper: a NON-bound state yields NO external-TS diagnostics
    /// (fail closed — never a `tsgo --lsp` inferred / own-discovery fall-through), and a
    /// BOUND carrier first attempts the editor route and activates managed only after an
    /// observed shared failure.
    async fn diagnostics_gated(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        // NON-carrier path: not gated — delegate to managed unchanged.
        let Some(source) = carrier_source_of(path) else {
            return self.managed_diagnostics(path, background).await;
        };

        // Carrier companion: resolve the owning project ONCE. A non-bound state yields
        // NO external-TS diagnostics for the carrier (fail closed — NEVER a `tsgo --lsp`
        // inferred / own-discovery fall-through for the carrier).
        let Some(carrier) =
            project_binding::resolve_carrier_bound(&self.host, &source).into_bound()
        else {
            return Ok(Vec::new());
        };

        // SHARED is authoritative and attempted FIRST. Only an observed failure or
        // timeout admits the managed provider. This is deliberately not a union: a
        // successful editor-owned route must not start or query a duplicate engine.
        if let Some(shared) = &self.shared {
            match tokio::time::timeout(
                SHARED_OVERLAY_TIMEOUT,
                shared.engage_diagnostics(path, &carrier),
            )
            .await
            {
                Ok(Ok(diagnostics)) => {
                    tracing::info!(
                        source = %source,
                        "editor-owned tsgo served carrier diagnostics; managed fallback remained cold"
                    );
                    return Ok(diagnostics);
                }
                // The admission gate reported this once for the carrier's generation.
                Ok(Err(refusal)) if refusal.kind.is_generated_unit_refusal() => tracing::trace!(
                    source = %source,
                    refusal_kind = ?refusal.kind,
                    "editor-owned tsgo diagnostics refused on generated-unit admission; serving managed"
                ),
                Ok(Err(refusal)) => tracing::info!(
                    source = %source,
                    refusal = %refusal,
                    refusal_kind = ?refusal.kind,
                    config = %refusal.config,
                    generation = refusal.generation,
                    transport_epoch = ?refusal.transport_epoch,
                    sync_state = ?refusal.sync_state,
                    "editor-owned tsgo diagnostics did not engage; activating managed fallback"
                ),
                Err(_) => tracing::warn!(
                    source = %source,
                    "editor-owned tsgo diagnostics timed out; activating managed fallback"
                ),
            }
        }

        // JavaScript carriers must preserve the bound project's checkJs/@ts-check
        // policy. The rich `--lsp` pull treats the generated JSX as a standalone
        // inferred file and can therefore report diagnostics that the configured
        // project intentionally disables. A project-bound result (including an empty
        // one) is authoritative for JSX; an unavailable/partial capability falls back
        // to the rich route so valid diagnostics are not silently erased.
        let javascript_carrier = path
            .rsplit_once('.')
            .is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("jsx"));
        if javascript_carrier {
            match self
                .managed
                .get_diagnostics_in_project(path, carrier.bound().project())
                .await
            {
                Ok(Some(diagnostics)) => return Ok(diagnostics),
                Ok(None) => tracing::debug!(
                    source = %source,
                    project = %carrier.bound().project(),
                    "configured-project JavaScript diagnostics unavailable; evaluating policy-safe fallback"
                ),
                Err(error) => tracing::warn!(
                    source = %source,
                    project = %carrier.bound().project(),
                    error = %error,
                    "configured-project JavaScript diagnostics failed; evaluating policy-safe fallback"
                ),
            }

            // The pinned TSGO API cannot expose some generated companions as roots
            // of nested configured projects. A raw LSP fallback would bind that JSX
            // to an inferred project and re-enable semantic diagnostics even when
            // the authoritative owning project has checking disabled. Preserve the
            // effective per-file policy in that unavailable-capability case:
            // compiler-lifted `@ts-check` overrides `checkJs: false`, while
            // `@ts-nocheck` overrides `checkJs: true`.
            if self.effective_javascript_check_policy(path, carrier.bound().project())
                == Some(false)
            {
                return Ok(Vec::new());
            }
        }

        // The BoundProject witness above remains the admission authority. Once
        // admitted, use the same managed LSP surface that owns the didOpen/didChange
        // overlay; never use it for an unbound carrier.
        self.managed_diagnostics(path, background).await
    }

    /// Read `checkJs` from the same immutable published snapshot that minted the
    /// carrier's configured-project witness. `None` retains the conservative rich
    /// fallback; only an observed configured project with checking disabled can
    /// suppress the wrong inferred-project semantic result.
    fn configured_project_check_js(&self, configured_project: &str) -> Option<bool> {
        let _activity = self.host.semantic_activity();
        let workspace = self.host.workspace_read();
        let published = workspace.published_root()?;
        let configured_project = normalize_canonical_id(configured_project);
        published
            .snapshot
            .projects
            .iter()
            .find_map(|project| match &project.payload {
                ProjectPayload::Configured {
                    tsconfig_path,
                    compiler_options,
                    ..
                } if normalize_canonical_id(tsconfig_path.as_str()) == configured_project => {
                    Some(compiler_options.check_js)
                }
                _ => None,
            })
    }

    /// Managed diagnostics for `path` on the requested lane.
    async fn managed_diagnostics(
        &self,
        path: &str,
        background: bool,
    ) -> Result<Vec<TypeDiagnostic>, TypeProviderError> {
        if background {
            self.managed.get_diagnostics_background(path).await
        } else {
            self.managed.get_diagnostics(path).await
        }
    }

    /// Record the carrier's content into the SHARED overlay (a cheap in-memory insert
    /// off the managed lifecycle critical path) — a no-op when SHARED is not opted in.
    async fn forward_managed(
        &self,
        path: &str,
        content: &str,
        priority: verter_type_runtime::provider_hub::OverlayPriority,
        kind: ManagedWrite,
    ) -> Result<verter_type_runtime::traits::FileLoadDisposition, TypeProviderError> {
        use verter_type_runtime::traits::FileLoadDisposition;
        let disposition = match kind {
            ManagedWrite::Open => {
                self.managed
                    .open_file_with_disposition(path, content, priority)
                    .await?
            }
            ManagedWrite::Load => {
                self.managed
                    .load_file_with_disposition(path, content, priority)
                    .await?
            }
            ManagedWrite::Update => {
                self.managed
                    .update_file_with_disposition(path, content, priority)
                    .await?
            }
        };
        if disposition != FileLoadDisposition::Shadowed {
            self.record_file_check_directive(path, content);
            let shared_priority = match priority {
                verter_type_runtime::provider_hub::OverlayPriority::Foreground => {
                    OverlayPriority::Interactive
                }
                verter_type_runtime::provider_hub::OverlayPriority::Normal => {
                    OverlayPriority::Normal
                }
                verter_type_runtime::provider_hub::OverlayPriority::Background => {
                    OverlayPriority::Background
                }
            };
            self.shared_record(path, content, shared_priority);
        }
        Ok(disposition)
    }

    fn shared_record(&self, path: &str, content: &str, priority: OverlayPriority) {
        if let Some(shared) = &self.shared {
            shared.record_content(path, content, priority);
        }
    }

    /// Retract a carrier from the SHARED overlay off the managed close critical path — a
    /// no-op when SHARED is not opted in.
    async fn shared_feed_close(&self, path: &str) {
        if let Some(shared) = &self.shared {
            shared.feed_close(path).await;
        }
    }
}

impl std::fmt::Debug for TsgoCompositeProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TsgoCompositeProvider")
            .field("managed", &self.managed.provider_id())
            .field("shared", &self.shared.is_some())
            .finish_non_exhaustive()
    }
}

impl TypeProvider for TsgoCompositeProvider {
    fn load_file_with_disposition<'a>(
        &'a self,
        path: &'a str,
        content: &'a str,
        priority: verter_type_runtime::provider_hub::OverlayPriority,
    ) -> ProviderFuture<'a, verter_type_runtime::traits::FileLoadDisposition> {
        Box::pin(async move {
            self.forward_managed(path, content, priority, ManagedWrite::Load)
                .await
        })
    }

    fn open_file_with_disposition<'a>(
        &'a self,
        path: &'a str,
        content: &'a str,
        priority: verter_type_runtime::provider_hub::OverlayPriority,
    ) -> ProviderFuture<'a, verter_type_runtime::traits::FileLoadDisposition> {
        Box::pin(async move {
            self.forward_managed(path, content, priority, ManagedWrite::Open)
                .await
        })
    }

    fn update_file_with_disposition<'a>(
        &'a self,
        path: &'a str,
        content: &'a str,
        priority: verter_type_runtime::provider_hub::OverlayPriority,
    ) -> ProviderFuture<'a, verter_type_runtime::traits::FileLoadDisposition> {
        Box::pin(async move {
            self.forward_managed(path, content, priority, ManagedWrite::Update)
                .await
        })
    }

    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        use verter_type_runtime::traits::AppliedContent;
        match self.managed.applied_content(path) {
            AppliedContent::Applied(bytes) => AppliedContent::Applied(bytes),
            other => match self
                .shared
                .as_ref()
                .map(|shared| shared.applied_content(path))
            {
                Some(AppliedContent::Applied(bytes)) => AppliedContent::Applied(bytes),
                _ => other,
            },
        }
    }

    fn provider_id(&self) -> &'static str {
        // The composite IS the tsgo provider — the SHARED overlay is an internal
        // implementation detail of the ONE provider; every engine-identifying branch
        // treats it as tsgo.
        self.managed.provider_id()
    }

    fn supports_completion_resolve(&self) -> bool {
        self.managed.supports_completion_resolve()
    }

    // ── Carrier lifecycle: record desired state in the managed slot, then record the
    //    carrier for SHARED. A lazy managed slot does not spawn here. This path never
    //    awaits SHARED establishment.
    //    The query path (`get_diagnostics`) establishes the transport and injects the
    //    recorded content lazily; a bound carrier's `--api` diagnostics then see it. ──

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Foreground,
                ManagedWrite::Open,
            )
            .await?;
            Ok(())
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Foreground,
                ManagedWrite::Load,
            )
            .await?;
            Ok(())
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Foreground,
                ManagedWrite::Update,
            )
            .await?;
            Ok(())
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file(&path).await?;
            self.clear_file_check_directive(&path);
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Background,
                ManagedWrite::Open,
            )
            .await?;
            Ok(())
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Background,
                ManagedWrite::Load,
            )
            .await?;
            Ok(())
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Background,
                ManagedWrite::Update,
            )
            .await?;
            Ok(())
        })
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file_background(&path).await?;
            self.clear_file_check_directive(&path);
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Normal,
                ManagedWrite::Open,
            )
            .await?;
            Ok(())
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Normal,
                ManagedWrite::Load,
            )
            .await?;
            Ok(())
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.forward_managed(
                &path,
                &content,
                verter_type_runtime::provider_hub::OverlayPriority::Normal,
                ManagedWrite::Update,
            )
            .await?;
            Ok(())
        })
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.managed.close_file_normal(&path).await?;
            self.clear_file_check_directive(&path);
            self.shared_feed_close(&path).await;
            Ok(())
        })
    }

    // ── Diagnostics: gate on a resolved BoundProject, serve SHARED first, and admit
    //    managed only after observed shared failure. The two SHARED diagnostic channels
    //    are composed inside one editor-owned session. ──

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, false).await })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move { self.diagnostics_gated(&path, true).await })
    }

    // ── Features: `feature_provider` gates each carrier on BoundProject and preserves
    //    shared-first ordering. A non-bound carrier serves its empty external default. ──

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        // MIXED: a denied carrier serves the empty external default (native completions
        // preserved by the handler merge); a non-carrier path is ungated.
        let path = path.to_string();
        let trigger_character = trigger_character.map(str::to_string);
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::Completions, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        let trigger_character = trigger_character.clone();
                        async move {
                            provider
                                .get_completions(&path, offset, trigger_character.as_deref())
                                .await
                        }
                    })
                    .await
            } else {
                Ok(CompletionResult {
                    items: Vec::new(),
                    is_incomplete: false,
                })
            }
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        // MIXED: a denied carrier serves the empty external default.
        let path = path.to_string();
        let items = items.to_vec();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::CompletionDetails, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        let items = items.clone();
                        async move { provider.get_completion_details(&path, offset, &items).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        // EXTERNAL-ONLY: a denied carrier suppresses provider enrichment (None) — no
        // owned resolve call.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::ResolveCompletion, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        let data = data.clone();
                        async move { provider.resolve_completion(&path, data).await }
                    })
                    .await
            } else {
                Ok(None)
            }
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        // MIXED: a denied carrier serves the None external default (the handler merge
        // preserves any native sub-answer); a non-carrier path is ungated.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self.feature_provider(ProviderFeature::Hover, &path).await {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_hover(&path, offset).await }
                    })
                    .await
            } else {
                Ok(None)
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::Definition, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_definition(&path, offset).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // EXTERNAL-ONLY: a denied carrier serves the empty external default with NO owned
        // delegation (never a `--lsp` self-discovery fall-through); a non-carrier path is
        // ungated.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::TypeDefinition, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_type_definition(&path, offset).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::References, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_references(&path, offset).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        // MIXED: a denied carrier serves the empty external default (native rename only);
        // the LSP handler's existing incomplete-rename safety gates — a SEPARATE layer
        // this admission does not touch — still block unsafe partial edits. Never a
        // `--lsp` self-discovery fall-through after admission failure.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::RenameLocations, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_rename_locations(&path, offset).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        // EXTERNAL-ONLY: a denied carrier serves `None` with NO owned delegation.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::SignatureHelp, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_signature_help(&path, offset).await }
                    })
                    .await
            } else {
                Ok(None)
            }
        })
    }

    fn get_code_actions(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
        diagnostics: &[ProviderDiagnosticContext],
    ) -> ProviderFuture<'_, Vec<TypeCodeAction>> {
        // MIXED: a denied carrier serves the empty external default (the LSP
        // `handle_code_action` handler's native Verter carrier code-actions —
        // organize-imports, extract-component, macro/component/event actions,
        // action-engine fixes — are preserved by its merge); a non-carrier path is
        // ungated. Never a `--lsp` self-discovery fall-through after admission failure.
        let path = path.to_string();
        let diagnostics = diagnostics.to_vec();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::CodeActions, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        let diagnostics = diagnostics.clone();
                        async move {
                            provider
                                .get_code_actions(&path, start_offset, end_offset, &diagnostics)
                                .await
                        }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        // EXTERNAL-ONLY: a denied carrier serves the empty external default with NO owned
        // delegation.
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::SemanticTokens, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_semantic_tokens(&path).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::DocumentHighlights, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move { provider.get_document_highlights(&path, offset).await }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        // MIXED: a denied carrier serves the empty external default (native preserved by
        // the handler merge).
        let path = path.to_string();
        Box::pin(async move {
            if let Some(selection) = self
                .feature_provider(ProviderFeature::InlayHints, &path)
                .await
            {
                selection
                    .invoke(|provider| {
                        let path = path.clone();
                        async move {
                            provider
                                .get_inlay_hints(&path, start_offset, end_offset)
                                .await
                        }
                    })
                    .await
            } else {
                Ok(Vec::new())
            }
        })
    }

    // ── Config / workspace lifecycle: record/delegate through the managed slot. ──

    fn configure_paths(&self, base_url: &str, paths: serde_json::Value) -> ProviderFuture<'_, ()> {
        self.managed.configure_paths(base_url, paths)
    }

    fn configure_paths_background(
        &self,
        base_url: &str,
        paths: serde_json::Value,
    ) -> ProviderFuture<'_, ()> {
        self.managed.configure_paths_background(base_url, paths)
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        self.managed.notify_carrier_changed(companion_path)
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        self.managed.register_carrier_member(
            source_path,
            companion_path,
            content,
            project_file_name,
        )
    }

    fn activate_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        project_file_name: &str,
        script_kind: verter_type_runtime::CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        self.managed.activate_carrier_member(
            source_path,
            companion_path,
            project_file_name,
            script_kind,
        )
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [verter_type_runtime::CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        self.managed.activate_carrier_members(members)
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        self.managed.resync_open_files()
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.managed.update_workspace_folders(added, removed)
    }

    fn notify_watched_files_changed<'a>(
        &'a self,
        changes: &'a [verter_type_runtime::WatchedFileChange],
    ) -> ProviderFuture<'a, ()> {
        self.managed.notify_watched_files_changed(changes)
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        self.managed
            .update_workspace_folders_background(added, removed)
    }

    fn child_pid(&self) -> Option<u32> {
        self.managed.child_pid()
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        Box::pin(async move {
            if let Some(shared) = &self.shared {
                shared.shutdown().await;
            }
            self.managed.shutdown().await
        })
    }
}

impl<P: SharedAttach> SharedTsgoOverlay<P> {
    /// Tear the SHARED attach down (best-effort) through its hub, then let
    /// managed shutdown remain the composite authority. Bounded: a slow/dead
    /// SHARED teardown must never block past this bound; on elapse the
    /// teardown is abandoned (a dead attach is retired by its hub anyway).
    /// The hub's shutdown is non-establishing, so this never triggers an
    /// attach.
    async fn shutdown(&self) {
        use verter_type_runtime::traits::TypeProvider as _;
        let _ = tokio::time::timeout(SHARED_CLOSE_TIMEOUT, self.inner.hub.shutdown()).await;
    }
}

#[cfg(test)]
#[path = "composite_tests.rs"]
mod composite_tests;
