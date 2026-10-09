//! Project-bound tsserver routing for monorepos.
//!
//! One workspace-level `TsserverTypeProvider` cannot serve configured projects
//! that install different TypeScript versions. A pnpm monorepo whose packages
//! pin TypeScript 5.8 and 6.0 side by side has no single correct engine: picking
//! either gives the other package the wrong compiler semantics, and picking the
//! WORKSPACE ROOT (which frequently has no `typescript` at all) resolves to
//! whatever ancestor or configured `tsdk` happens to answer — including a
//! library-less copy that builds a program with no default libs, so valid code
//! reports `Cannot find name 'Math'`.
//!
//! [`ProjectTsserverProvider`] resolves every production operation through the
//! shared `ProjectBinding` → `BoundProject` contract, then lazily owns one
//! hub-managed tsserver engine per `(owning tsconfig, real tsserver.js)` identity.
//! A project whose TypeScript cannot be resolved fails closed with the
//! actionable install message from [`resolve_tsserver`] and NEVER borrows another
//! project's engine.

use std::future::Future;
use std::path::Path;
use std::sync::Arc;

use crate::outbound::Outbound;
use dashmap::DashMap;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineBackend, ProjectBinding,
};
use verter_session::VerterHost;
use verter_type_runtime::discovery::{
    detect_ts_version, resolve_tsserver, tsserver_native_family_major, tsserver_serving_advisory,
    tsserver_serving_tier, ResolvedTsserver, TsserverSource,
};
use verter_type_runtime::provider_hub::{
    AdmissionRefusal, AdmittedRequest, DroppedAdmittedCarrier, DroppedAdmittedState,
    OverlayFileKind, OverlayMutation, OverlayPriority, ProjectWitness,
};
use verter_workspace::{decide_generated_unit_admission_with_basis, CanonicalPath};

use crate::external_ts::TsserverEngineBackend;
use crate::tsgo::project_binding::{
    hub_binding_input, resolve_carrier_with_publication, OwnershipReadinessMode,
    ResolvedPublication,
};
use crate::type_provider::protocol::*;
use crate::type_provider::traits::{
    CarrierActivation, CarrierScriptKind, ProviderFuture, TypeProvider,
};

use super::resilient::{self, TsserverEngineInputs};
use crate::resilient_provider::ProviderHub;

/// The identity of ONE owned tsserver process: the owning configured project
/// plus the REAL `tsserver.js` that serves it. Two projects that resolve the
/// same install share one process; two projects on different TypeScript
/// versions never do.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProjectEngineKey {
    project: String,
    /// The CANONICAL `tsserver.js` from `resolve_tsserver` — on Windows an
    /// extended-length `\\?\D:\…` path. It stays canonical here because this is
    /// an IDENTITY value (the `providers` map key that collapses concurrent cold
    /// demands onto one spawn, and the respawn seed): two demands for the same
    /// install must key equal, and re-normalizing an identity would split it.
    /// The same string is ALSO the tsserver main-script argument, but the
    /// verbatim prefix is stripped there and only there — inside
    /// `TsserverTypeProvider::spawn`'s command builder, at the exec boundary.
    tsserver_path: String,
}

/// Everything needed to spawn (or identify) one project's tsserver.
#[derive(Debug, Clone)]
struct ProjectEngineSpec {
    key: ProjectEngineKey,
    workspace_root: String,
    default_lib_count: usize,
}

/// A cached per-project engine resolution, fenced on the project membership
/// (publication and project generation) it was taken at.
///
/// Resolution walks the filesystem (ancestor `node_modules` probes, a
/// `canonicalize`, a `read_dir` of the install's `lib/`) and, on a total miss,
/// shells out to `npm root -g`. Doing that on every hover would be a per-request
/// filesystem storm, so both the success and the refusal are cached. The
/// basis fence releases the cache whenever the workspace project graph is
/// republished (a tsconfig edit, a config-file change, a workspace-folder
/// change), so a project-graph change re-resolves; a bare `node_modules`
/// mutation that publishes no new snapshot still needs a server reload. A
/// document's content edit changes neither input, so it never re-resolves.
#[derive(Clone)]
struct CachedEngineSpec {
    basis: ResolvedPublication,
    outcome: Result<ProjectEngineSpec, String>,
}

/// A companion path registered by the publish path, mapped back to the authored
/// carrier source and the owning project the publisher resolved it under.
#[derive(Debug, Clone)]
struct RegisteredRoute {
    source: String,
    project: String,
}

/// A read-only query's binding: the serving incarnation, the owning project
/// and — for a generated unit — its membership admission. It stays with the
/// query until its answer is settled.
struct RequestRoute {
    hub: Arc<ProviderHub<dyn TypeProvider>>,
    witness: ProjectWitness,
    admission: Option<AdmittedRequest>,
    path: String,
}

/// Immediate re-issues one WRITE (or its generated-unit admission) takes when
/// the basis drifts under it. Writes only: a write the hub refused on a drifted
/// basis is left to its issuer, and nothing else re-drives a carrier the LSP
/// believes the engine holds. Reads never re-issue — their admission binds the
/// serving incarnation and project membership, which content drift leaves
/// untouched.
const WRITE_DRIFT_REISSUES: usize = 2;

/// Backed-off retries for carrier writes still refused on a drifted basis
/// after their immediate [`WRITE_DRIFT_REISSUES`] (see
/// [`settle_under_fresh_admission`] and
/// [`ProjectTsserverProvider::rearm_admitted_state`]): long enough in total to
/// outlast an edit burst. Writes only, for the same reason.
const WRITE_DRIFT_BACKOFF: [std::time::Duration; 3] = [
    std::time::Duration::from_millis(250),
    std::time::Duration::from_secs(1),
    std::time::Duration::from_secs(4),
];

/// Run one read-only query through its request route, settling it under the
/// route's query admission: exactly one engine call per query.
///
/// The route binds only what the query's answer depends on — the serving
/// engine incarnation, the owning project's membership, and the requested
/// generated unit's admission — never the workspace content generation. An
/// unrelated document's edit landing while the engine answers therefore
/// neither refuses nor re-issues the query; the answer's coordinates are bound
/// by the engine adapter at the request's own wire position. A replaced
/// engine, a moved membership or a withdrawn owner still refuses, each with
/// its own reason.
macro_rules! routed_query {
    ($router:expr, $path:expr, |$route:ident| $query:expr) => {{
        let $route = $router.request_route($path).await?;
        $route.run($query).await
    }};
}

type AdmittedCarrierBatch = (
    Arc<ProviderHub<dyn TypeProvider>>,
    Vec<(AdmittedRequest, CarrierActivation)>,
);

impl RequestRoute {
    fn check(&self) -> Result<(), TypeProviderError> {
        self.hub.check_query(&self.witness).map_err(|reason| {
            project_refusal(&self.path, &format!("request binding expired: {reason:?}"))
        })?;
        if let Some(admission) = &self.admission {
            self.hub
                .check_query_admission(admission)
                .map_err(|reason| {
                    project_refusal(
                        &self.path,
                        &format!("generated admission expired: {reason:?}"),
                    )
                })?;
        }
        Ok(())
    }

    async fn run<T>(
        &self,
        future: impl Future<Output = Result<T, TypeProviderError>>,
    ) -> Result<T, TypeProviderError> {
        self.check()?;
        let result = future.await;
        self.check()?;
        result
    }
}

/// Why one admitted carrier write did not complete.
enum WriteFailure {
    /// No fresh admission could be minted for it.
    Admission(TypeProviderError),
    /// The serving hub refused one of its settlements.
    Refused {
        stage: &'static str,
        reason: AdmissionRefusal,
    },
}

impl WriteFailure {
    /// The settlement refusal of the `stage` write, as `map_err` takes it.
    fn refused(stage: &'static str) -> impl FnOnce(AdmissionRefusal) -> Self {
        move |reason| Self::Refused { stage, reason }
    }

    /// Refused because the basis moved while the engine applied it — the
    /// refusal a fresh admission answers.
    fn basis_drifted(&self) -> bool {
        matches!(
            self,
            Self::Refused {
                reason: AdmissionRefusal::StaleBasis,
                ..
            }
        )
    }
}

impl std::fmt::Display for WriteFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Admission(error) => write!(f, "{error}"),
            Self::Refused { stage, reason } => write!(f, "{stage} refused: {reason:?}"),
        }
    }
}

impl From<WriteFailure> for TypeProviderError {
    fn from(failure: WriteFailure) -> Self {
        match failure {
            WriteFailure::Admission(error) => error,
            refused @ WriteFailure::Refused { .. } => TypeProviderError::new(refused.to_string()),
        }
    }
}

/// Run one admitted carrier write, re-issuing it under a fresh admission while
/// the hub refuses its settlement on a drifted basis.
///
/// A content-only drift (another document's edit or open landed while the
/// engine applied the write) refuses only the settlement: nothing is recorded
/// as applied, and the hub leaves the re-application to its issuer. Surfacing
/// that refusal instead would leave the carrier degraded with nothing to
/// re-drive it — the LSP believes the write was attempted, so its open
/// document never reaches the engine. Each `write` call mints its own
/// admission, so a re-issue binds the live basis. The first re-issues are
/// immediate, like every other basis-drift re-issue; a basis still drifting
/// after them is waited out with [`WRITE_DRIFT_BACKOFF`] so an edit burst
/// cannot spend the budget while it lasts. Any other refusal is returned as is.
async fn settle_under_fresh_admission<T, Fut>(
    mut write: impl FnMut() -> Fut,
) -> Result<T, TypeProviderError>
where
    Fut: Future<Output = Result<T, WriteFailure>>,
{
    let mut reissues = WRITE_DRIFT_REISSUES;
    let mut backoff = WRITE_DRIFT_BACKOFF.iter();
    loop {
        match write().await {
            Err(failure) if failure.basis_drifted() => {
                if reissues > 0 {
                    reissues -= 1;
                } else if let Some(delay) = backoff.next() {
                    tokio::time::sleep(*delay).await;
                } else {
                    return Err(failure.into());
                }
            }
            settled => return settled.map_err(Into::into),
        }
    }
}

/// A project-bound pool of hub-managed tsserver engines.
///
/// Cold at construction: no tsserver starts until a project-bound lifecycle
/// operation or query resolves an owning configured project.
pub struct ProjectTsserverProvider {
    host: Arc<VerterHost>,
    tsdk: Option<String>,
    plugin_path: Option<String>,
    node_path: String,
    client: Outbound,
    /// The witness backend the router mints each operation's [`BoundProject`]
    /// through. It is NOT the publish path's backend — `ensure_project` is a
    /// pure witness mint plus per-backend bookkeeping, and the router only needs
    /// the witness that proves the operation is project-bound.
    witness_backend: TsserverEngineBackend,
    engine_specs: DashMap<String, CachedEngineSpec>,
    providers: DashMap<ProjectEngineKey, Arc<ProviderHub<dyn TypeProvider>>>,
    routes: DashMap<String, RegisteredRoute>,
    /// The recovery re-arm handed to every hub this router creates: a
    /// replacement install drops the old epoch's admitted generated state, and
    /// this hook re-publishes it through fresh admission. Installed after
    /// construction (the hook needs this router behind its `Arc`), before the
    /// first hub can exist — hubs are created lazily on first demand.
    admitted_state_rearm:
        parking_lot::RwLock<Option<crate::resilient_provider::AdmittedStateRearm>>,
    /// Fired every time one of this pool's engines begins serving (each hub
    /// created by [`Self::hub_for_binding`] notifies it on its initial start
    /// and every crash replacement). Exposed through
    /// [`TypeProvider::provider_restart_pulse`] so the pending provider-sync
    /// re-drive can treat an engine (re)start as its retry signal.
    restart_pulse: Arc<tokio::sync::Notify>,
}

impl ProjectTsserverProvider {
    /// Construct a cold project router. No tsserver process starts until a
    /// project-bound lifecycle operation or query requires it.
    ///
    /// # Errors
    /// Returns an error when Node.js — which every tsserver needs — is not on
    /// `PATH` or in the standard locations.
    pub fn new(
        host: Arc<VerterHost>,
        tsdk: Option<String>,
        plugin_path: Option<String>,
        client: Outbound,
    ) -> Result<Self, TypeProviderError> {
        let node_path = super::find_node().ok_or_else(|| {
            TypeProviderError::new("Node.js not found on PATH or standard locations")
        })?;
        Ok(Self {
            host,
            tsdk,
            plugin_path,
            node_path,
            client,
            witness_backend: TsserverEngineBackend::with_default_host_version(),
            engine_specs: DashMap::new(),
            providers: DashMap::new(),
            routes: DashMap::new(),
            admitted_state_rearm: parking_lot::RwLock::new(None),
            restart_pulse: Arc::new(tokio::sync::Notify::new()),
        })
    }

    /// Install the recovery re-arm for every hub this router creates. Called
    /// once, immediately after the router is put behind its `Arc` and before
    /// any hub can have been created (hubs are lazy on first demand).
    pub fn install_admitted_state_rearm(
        &self,
        rearm: Arc<dyn Fn(&DroppedAdmittedState) + Send + Sync>,
    ) {
        *self.admitted_state_rearm.write() = Some(rearm);
    }

    /// Re-publish admitted generated state a replacement engine dropped,
    /// through FRESH admission against the epoch that now serves. Each carrier
    /// re-registers its metadata and, when its last explicit activation was
    /// recorded, re-activates with that exact parsing mode — mirroring the
    /// desired-state replay order for non-admitted carriers.
    ///
    /// A settlement refused on a drifted BASIS (an unrelated document's edit
    /// landed while the engine applied it) is re-issued under a fresh
    /// admission: the hub leaves that re-application to its issuer, and nothing
    /// else re-drives a carrier the LSP still believes the engine holds — its
    /// open documents would wait on a provider that never received them. The
    /// first re-issues are immediate, like every other basis-drift re-issue;
    /// carriers still drifting after them are retried once the whole set has
    /// been re-armed, backing off so a burst of edits cannot spend the budget
    /// while it lasts (and one drifting carrier never delays the others). Any
    /// other refusal (the configuration changed, the project moved, the engine
    /// was replaced) skips that carrier fail-closed: the change that caused it
    /// re-publishes the carrier through the ordinary sync path.
    pub async fn rearm_admitted_state(&self, dropped: &DroppedAdmittedState) {
        let mut drifting = Vec::new();
        for carrier in &dropped.carriers {
            let mut reissues = WRITE_DRIFT_REISSUES;
            loop {
                match self.rearm_admitted_carrier(carrier).await {
                    Ok(()) => break,
                    Err(error) if error.basis_drifted() && reissues > 0 => reissues -= 1,
                    Err(error) if error.basis_drifted() => {
                        drifting.push((carrier, error));
                        break;
                    }
                    Err(error) => {
                        Self::warn_rearm_skipped(carrier, &error);
                        break;
                    }
                }
            }
        }
        for delay in WRITE_DRIFT_BACKOFF {
            if drifting.is_empty() {
                return;
            }
            tokio::time::sleep(delay).await;
            let mut still = Vec::new();
            for (carrier, _) in drifting {
                match self.rearm_admitted_carrier(carrier).await {
                    Ok(()) => {}
                    Err(error) if error.basis_drifted() => still.push((carrier, error)),
                    Err(error) => Self::warn_rearm_skipped(carrier, &error),
                }
            }
            drifting = still;
        }
        for (carrier, error) in &drifting {
            Self::warn_rearm_skipped(carrier, error);
        }
    }

    fn warn_rearm_skipped(carrier: &DroppedAdmittedCarrier, error: &WriteFailure) {
        tracing::warn!(
            companion = %carrier.companion_path,
            "tsserver recovery re-arm skipped (fail-closed; the ordinary \
             carrier sync re-drives it): {error}"
        );
    }

    /// One carrier of [`Self::rearm_admitted_state`], under one fresh admission.
    async fn rearm_admitted_carrier(
        &self,
        carrier: &DroppedAdmittedCarrier,
    ) -> Result<(), WriteFailure> {
        let (hub, admitted) = self
            .admit_registered_unit(
                &carrier.source_path,
                &carrier.companion_path,
                &carrier.project_file_name,
            )
            .await
            .map_err(WriteFailure::Admission)?;
        hub.apply_overlay(
            &admitted,
            OverlayMutation::RegisterCarrierMetadata {
                source_path: carrier.source_path.clone(),
                companion_path: carrier.companion_path.clone(),
                content: carrier.content.clone(),
                project_file_name: carrier.project_file_name.clone(),
            },
        )
        .await
        .map_err(WriteFailure::refused("recovery re-arm registration"))?;
        if let Some(script_kind) = carrier.script_kind {
            hub.apply_overlay(
                &admitted,
                OverlayMutation::ActivateCarrier {
                    source_path: carrier.source_path.clone(),
                    companion_path: carrier.companion_path.clone(),
                    project_file_name: carrier.project_file_name.clone(),
                    script_kind,
                },
            )
            .await
            .map_err(WriteFailure::refused("recovery re-arm activation"))?;
        }
        self.register_route(
            &carrier.source_path,
            &carrier.companion_path,
            &carrier.project_file_name,
        );
        Ok(())
    }

    fn normalized(path: &str) -> String {
        verter_span::path::canonicalize_path(path)
    }

    /// The authored carrier source a provider path routes through, plus the
    /// owning project the publish path registered it under (when known).
    fn source_for_path(&self, path: &str) -> (String, Option<String>) {
        let path = Self::normalized(path);
        if let Some(route) = self.routes.get(&path) {
            return (route.source.clone(), Some(route.project.clone()));
        }
        if let Some(companion) =
            verter_session::framework::descriptor::classify_carrier_companion(&path)
        {
            return (companion.source, None);
        }
        (path, None)
    }

    /// Resolve one provider path to its owning configured project's binding and
    /// exact publication basis.
    ///
    /// Every non-`Bound` state is a DISTINCT fail-closed refusal — never an
    /// inferred project and never another project's engine.
    fn binding_for_path_with_publication(
        &self,
        path: &str,
        fence: PublicationFence,
    ) -> Result<(ProjectBinding, ResolvedPublication), TypeProviderError> {
        let (source, registered_project) = self.source_for_path(path);
        self.binding_for_source_with_expected(&source, registered_project.as_deref(), fence)
    }

    fn binding_for_source_with_expected(
        &self,
        source: &str,
        expected_project: Option<&str>,
        fence: PublicationFence,
    ) -> Result<(ProjectBinding, ResolvedPublication), TypeProviderError> {
        let Some((resolution, _, resolved)) = resolve_carrier_with_publication(
            self.host.as_ref(),
            source,
            Arc::from(""),
            // A PRESENT published snapshot is authoritative — the same gate the
            // OWNED tsgo carrier-diagnostics path uses. The bootstrap-absent case
            // is the `None` arm below; a present-but-cold snapshot must still bind
            // its owner rather than refuse every operation until warm-up finishes.
            OwnershipReadinessMode::PresentSnapshotAuthoritative,
        ) else {
            return Err(project_refusal(
                source,
                "the configured-project snapshot is not published yet",
            ));
        };
        let binding = match resolution {
            CarrierOwnershipResolution::Bound(binding) => binding,
            CarrierOwnershipResolution::NotReady => {
                return Err(project_refusal(
                    source,
                    "configured-project ownership is not ready yet",
                ));
            }
            CarrierOwnershipResolution::NoProject => {
                return Err(project_refusal(
                    source,
                    "no owning tsconfig.json or jsconfig.json was resolved",
                ));
            }
            CarrierOwnershipResolution::Ambiguous { cause, .. } => {
                return Err(project_refusal(
                    source,
                    &format!("configured-project ownership is ambiguous: {cause:?}"),
                ));
            }
        };
        if let Some(expected) = expected_project {
            if Self::normalized(binding.tsconfig_uri()) != expected {
                return Err(project_refusal(
                    source,
                    "the live ProjectBinding no longer matches the registered owning project",
                ));
            }
        }
        if !ResolvedPublication::current(&self.host)
            .is_some_and(|live| fence.admits(&live, &resolved))
        {
            return Err(project_refusal(
                source,
                "project binding raced a workspace change",
            ));
        }
        Ok((binding, resolved))
    }

    /// Mint the operation's [`BoundProject`] witness and resolve the owning
    /// project's engine, reusing the publication-fenced cached resolution.
    fn engine_for_binding(
        &self,
        binding: &ProjectBinding,
        basis: &ResolvedPublication,
    ) -> Result<(BoundProject, ProjectEngineSpec), TypeProviderError> {
        // The witness is minted on EVERY operation (never cached): the
        // project-bound contract requires a live `BoundProject` for each
        // provider op, and the mint is a cheap bookkeeping insert.
        let bound = ensure_bound(&self.witness_backend, binding)?;
        let project = Self::normalized(binding.tsconfig_uri());
        if let Some(cached) = self.engine_specs.get(&project) {
            if PublicationFence::Membership.admits(&cached.basis, basis) {
                return cached
                    .outcome
                    .clone()
                    .map(|spec| (bound, spec))
                    .map_err(TypeProviderError::new);
            }
        }
        let outcome = resolve_engine_spec(&bound, binding, self.tsdk.as_deref());
        self.engine_specs.insert(
            project,
            CachedEngineSpec {
                basis: basis.clone(),
                outcome: outcome.clone(),
            },
        );
        outcome
            .map(|spec| (bound, spec))
            .map_err(TypeProviderError::new)
    }

    async fn hub_for_binding(
        &self,
        binding: &ProjectBinding,
        basis: &ResolvedPublication,
    ) -> Result<Arc<ProviderHub<dyn TypeProvider>>, TypeProviderError> {
        let (_bound, spec) = self.engine_for_binding(binding, basis)?;
        // One hub per engine identity: concurrent cold demands for the same
        // project join ONE establishment, a failed establishment is retried by
        // the next demand, and a crashed engine is recovered by its hub.
        let hub = self
            .providers
            .entry(spec.key.clone())
            .or_insert_with(|| {
                Arc::new(resilient::hub(
                    TsserverEngineInputs::production(
                        self.node_path.clone(),
                        spec.key.tsserver_path.clone(),
                        spec.workspace_root.clone(),
                        self.plugin_path.clone(),
                    ),
                    self.client.clone(),
                    3,
                    self.admitted_state_rearm.read().clone(),
                    Arc::clone(&self.restart_pulse),
                ))
            })
            .clone();
        match hub.establish().await {
            Ok(_) => {}
            // A recovering (or given-up) engine still owns this project: its
            // hub holds lifecycle updates for the replacement and fails
            // queries closed until one serves.
            Err(_) if hub.has_served() => {}
            Err(error) => {
                return Err(TypeProviderError::new(format!(
                    "resolved project {} to {} ({} default libraries), but tsserver failed to \
                     start: {error}",
                    spec.key.project, spec.key.tsserver_path, spec.default_lib_count
                )))
            }
        }
        Ok(hub)
    }

    async fn provider_for_binding(
        &self,
        binding: &ProjectBinding,
        basis: &ResolvedPublication,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        Ok(self.hub_for_binding(binding, basis).await? as Arc<dyn TypeProvider>)
    }

    /// Proof for the complete generated write set, decided against the SAME
    /// publication that resolved the source binding. A republish or provider
    /// replacement refuses before the actor can forward a write.
    async fn admit_generated_write(
        &self,
        source: &str,
        binding: &ProjectBinding,
        resolved: ResolvedPublication,
        units: &[CanonicalPath],
    ) -> Result<(Arc<ProviderHub<dyn TypeProvider>>, AdmittedRequest), TypeProviderError> {
        let hub = self.hub_for_binding(binding, &resolved).await?;
        let input = hub_binding_input(&self.host, source, binding, resolved.clone());
        let witness = hub.bind_project(input).map_err(|reason| {
            project_refusal(source, &format!("hub project binding refused: {reason:?}"))
        })?;
        let admission = hub
            .admit_request_with(&witness, units, || {
                decide_generated_unit_admission_with_basis(
                    resolved.published.snapshot.as_ref(),
                    &CanonicalPath::new(binding.tsconfig_uri()),
                    units,
                    crate::external_ts::carrier_membership_basis,
                )
            })
            .map_err(|reason| {
                project_refusal(
                    source,
                    &format!("hub generated-unit admission refused: {reason:?}"),
                )
            })?;
        Ok((hub, admission))
    }

    /// Admit ONE generated unit against the binding `resolve` yields,
    /// re-admitting when the basis moved DURING admission.
    ///
    /// The publication read, the hub binding and the membership proof are
    /// separate steps; an unrelated document's edit landing between them
    /// refuses the admission on a basis that is already history. Nothing has
    /// reached the engine at that point, so the admission is simply re-run
    /// against the live basis — bounded, and any refusal on an unmoved basis
    /// (or once the budget is spent) is returned as is.
    async fn admit_current_unit(
        &self,
        source: &str,
        unit: &str,
        resolve: impl Fn() -> Result<(ProjectBinding, ResolvedPublication), TypeProviderError>,
    ) -> Result<(Arc<ProviderHub<dyn TypeProvider>>, AdmittedRequest), TypeProviderError> {
        let mut reissues = WRITE_DRIFT_REISSUES;
        loop {
            let before = ResolvedPublication::current(&self.host);
            let admitted = match resolve() {
                Ok((binding, published)) => {
                    self.admit_generated_write(
                        source,
                        &binding,
                        published,
                        &[CanonicalPath::new(unit)],
                    )
                    .await
                }
                Err(refusal) => Err(refusal),
            };
            match admitted {
                Err(_) if reissues > 0 && ResolvedPublication::current(&self.host) != before => {
                    reissues -= 1;
                }
                settled => return settled,
            }
        }
    }

    /// [`Self::admit_current_unit`] for a carrier companion registered under
    /// its owning project.
    async fn admit_registered_unit(
        &self,
        source: &str,
        companion: &str,
        project: &str,
    ) -> Result<(Arc<ProviderHub<dyn TypeProvider>>, AdmittedRequest), TypeProviderError> {
        self.admit_current_unit(source, companion, || {
            self.binding_for_registered_with_publication(source, companion, project)
        })
        .await
    }

    /// Fresh admissions for one bulk-activation group, on the engine that
    /// already serves it. A member whose binding now resolves to another engine
    /// cannot join this group's single bulk call, so the group is refused as a
    /// replaced provider rather than split across engines.
    async fn readmit_carrier_group(
        &self,
        hub: &Arc<ProviderHub<dyn TypeProvider>>,
        group: &[CarrierActivation],
    ) -> Result<Vec<(AdmittedRequest, CarrierActivation)>, WriteFailure> {
        let mut admitted = Vec::with_capacity(group.len());
        for member in group {
            let (owner, admission) = self
                .admit_registered_unit(
                    &member.source_path,
                    &member.companion_path,
                    &member.project_file_name,
                )
                .await
                .map_err(WriteFailure::Admission)?;
            if !Arc::ptr_eq(&owner, hub) {
                return Err(WriteFailure::Refused {
                    stage: "hub carrier batch",
                    reason: AdmissionRefusal::StaleProvider,
                });
            }
            admitted.push((admission, member.clone()));
        }
        Ok(admitted)
    }

    async fn provider_for_path(
        &self,
        path: &str,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let (binding, resolved) =
            self.binding_for_path_with_publication(path, PublicationFence::Exact)?;
        let provider = self.provider_for_binding(&binding, &resolved).await?;
        if ResolvedPublication::current(&self.host).as_ref() != Some(&resolved) {
            return Err(project_refusal(
                path,
                "provider selection raced a workspace change",
            ));
        }
        Ok(provider)
    }

    /// Bind one read-only query: the owning project's engine, a query witness
    /// on its serving incarnation and — for a generated companion — the
    /// membership admission of exactly the requested unit. Every step fences
    /// only the project membership, so an edit landing while the route is bound
    /// cannot refuse it; any refusal is final for this query (it has not reached
    /// the engine).
    async fn request_route(&self, path: &str) -> Result<RequestRoute, TypeProviderError> {
        let (source, _) = self.source_for_path(path);
        let (binding, published) =
            self.binding_for_path_with_publication(path, PublicationFence::Membership)?;
        let hub = self.hub_for_binding(&binding, &published).await?;
        let snapshot = Arc::clone(&published.published.snapshot);
        let witness = hub
            .bind_query(hub_binding_input(&self.host, &source, &binding, published))
            .map_err(|reason| project_refusal(path, &format!("hub binding refused: {reason:?}")))?;
        if verter_session::framework::descriptor::classify_carrier_companion(path).is_none() {
            return Ok(RequestRoute {
                hub,
                witness,
                admission: None,
                path: path.to_string(),
            });
        }
        let units = [CanonicalPath::new(path)];
        let admission = hub
            .admit_query(&witness, &units, || {
                decide_generated_unit_admission_with_basis(
                    snapshot.as_ref(),
                    &CanonicalPath::new(binding.tsconfig_uri()),
                    &units,
                    crate::external_ts::carrier_membership_basis,
                )
            })
            .map_err(|reason| {
                project_refusal(
                    path,
                    &format!("hub generated-unit admission refused: {reason:?}"),
                )
            })?;
        Ok(RequestRoute {
            hub,
            witness,
            admission: Some(admission),
            path: path.to_string(),
        })
    }

    async fn apply_file_write(
        &self,
        path: &str,
        content: &str,
        kind: OverlayFileKind,
        priority: OverlayPriority,
    ) -> Result<(), TypeProviderError> {
        if verter_session::framework::descriptor::classify_carrier_companion(path).is_none() {
            let provider = self.provider_for_path(path).await?;
            return match (kind, priority) {
                (OverlayFileKind::Open, OverlayPriority::Foreground) => {
                    provider.open_file(path, content).await
                }
                (OverlayFileKind::Load, OverlayPriority::Foreground) => {
                    provider.load_file(path, content).await
                }
                (OverlayFileKind::Update, OverlayPriority::Foreground) => {
                    provider.update_file(path, content).await
                }
                (OverlayFileKind::Open, OverlayPriority::Normal) => {
                    provider.open_file_normal(path, content).await
                }
                (OverlayFileKind::Load, OverlayPriority::Normal) => {
                    provider.load_file_normal(path, content).await
                }
                (OverlayFileKind::Update, OverlayPriority::Normal) => {
                    provider.update_file_normal(path, content).await
                }
                (OverlayFileKind::Open, OverlayPriority::Background) => {
                    provider.open_file_background(path, content).await
                }
                (OverlayFileKind::Load, OverlayPriority::Background) => {
                    provider.load_file_background(path, content).await
                }
                (OverlayFileKind::Update, OverlayPriority::Background) => {
                    provider.update_file_background(path, content).await
                }
            };
        }
        let (source, _) = self.source_for_path(path);
        let source = source.as_str();
        settle_under_fresh_admission(move || async move {
            let (hub, admitted) = self
                .admit_current_unit(source, path, || {
                    self.binding_for_path_with_publication(path, PublicationFence::Exact)
                })
                .await
                .map_err(WriteFailure::Admission)?;
            hub.apply_overlay(
                &admitted,
                OverlayMutation::File {
                    path: path.to_string(),
                    content: content.to_string(),
                    kind,
                    priority,
                },
            )
            .await
            .map_err(WriteFailure::refused("hub generated-unit write"))
        })
        .await
    }

    fn register_route(&self, source: &str, companion: &str, project: &str) {
        let route = RegisteredRoute {
            source: Self::normalized(source),
            project: Self::normalized(project),
        };
        self.routes.insert(Self::normalized(source), route.clone());
        self.routes.insert(Self::normalized(companion), route);
    }

    fn binding_for_registered_with_publication(
        &self,
        source: &str,
        companion: &str,
        project: &str,
    ) -> Result<(ProjectBinding, ResolvedPublication), TypeProviderError> {
        let (binding, published) =
            self.binding_for_source_with_expected(source, None, PublicationFence::Exact)?;
        if Self::normalized(binding.tsconfig_uri()) != Self::normalized(project) {
            return Err(project_refusal(
                source,
                "the registered project does not own the source",
            ));
        }
        let _ = companion;
        Ok((binding, published))
    }

    /// Every hub this router has ALLOCATED — including one whose first
    /// establishment is still in flight. Lifecycle updates and teardown must
    /// reach those too: an establishing hub records desired state for the
    /// engine it will install, and a shutdown that skips it abandons the
    /// in-flight establishment (its install is rejected and its engine torn
    /// down) instead of leaking a live engine after teardown returned.
    fn providers_snapshot(&self) -> Vec<Arc<dyn TypeProvider>> {
        self.providers
            .iter()
            .map(|entry| Arc::clone(entry.value()) as Arc<dyn TypeProvider>)
            .collect()
    }
}

/// How much of the live publication must still match the one an operation
/// resolved its binding under.
#[derive(Clone, Copy)]
enum PublicationFence {
    /// Writes: the exact publication and both its content and project
    /// generations.
    Exact,
    /// Reads and engine resolution: only what decides project membership —
    /// the publication identity and the project generation. A document's
    /// content edit does not move it.
    Membership,
}

impl PublicationFence {
    fn admits(self, live: &ResolvedPublication, resolved: &ResolvedPublication) -> bool {
        match self {
            Self::Exact => live == resolved,
            Self::Membership => {
                Arc::ptr_eq(&live.published, &resolved.published)
                    && live.project_generation == resolved.project_generation
            }
        }
    }
}

/// Mint the operation's `BoundProject` witness through the tsserver backend.
fn ensure_bound(
    backend: &TsserverEngineBackend,
    binding: &ProjectBinding,
) -> Result<BoundProject, TypeProviderError> {
    backend
        .ensure_project(binding.ensure_project_request())
        .map_err(|error| {
            TypeProviderError::new(format!(
                "the tsserver backend refused owning project {}: {error:?}",
                binding.tsconfig_uri()
            ))
        })
}

/// Resolve the tsserver that serves ONE owning configured project.
///
/// Discovery starts at the owning project's OWN directory, so a pnpm package
/// resolves its own `node_modules/typescript` (through the `.pnpm` symlink to
/// the real install) instead of whatever the workspace root happens to answer.
fn resolve_engine_spec(
    bound: &BoundProject,
    binding: &ProjectBinding,
    tsdk: Option<&str>,
) -> Result<ProjectEngineSpec, String> {
    let tsconfig_path = verter_type_runtime::file_uri_to_path(bound.project());
    let project_dir = Path::new(&tsconfig_path)
        .parent()
        .ok_or_else(|| format!("owning project path has no directory: {}", bound.project()))?
        .to_string_lossy()
        .into_owned();
    let ResolvedTsserver {
        path,
        source,
        default_lib_count,
        skipped,
    } = resolve_tsserver(tsdk, Some(&project_dir)).map_err(|error| {
        format!(
            "TypeScript semantics are unavailable for owning project {}: {error}",
            bound.project()
        )
    })?;
    if let Some(major) = tsserver_native_family_major(&path) {
        return Err(format!(
            "owning project {} uses TypeScript {major}.x, the native tsgo family; \
             it cannot be served over the Node tsserver protocol",
            bound.project()
        ));
    }
    // A nearer install this resolution passed over swapped the toolchain for this
    // project: the engine is not the one the project pinned. Never silent.
    for rejection in &skipped {
        tracing::warn!(
            project = %bound.project(),
            skipped = %rejection.path.display(),
            serving = %path.display(),
            "a nearer TypeScript install was refused ({}); serving from a different install",
            rejection.reason
        );
    }
    let workspace_root = binding.workspace_root().to_string();
    let tsserver_path = path.to_string_lossy().into_owned();
    tracing::debug!(
        project = %bound.project(),
        tsserver = %tsserver_path,
        ?source,
        version = ?detect_ts_version(&path),
        default_lib_count,
        "resolved project-bound tsserver"
    );
    Ok(ProjectEngineSpec {
        key: ProjectEngineKey {
            project: ProjectTsserverProvider::normalized(binding.tsconfig_uri()),
            tsserver_path,
        },
        workspace_root,
        default_lib_count,
    })
}

fn project_refusal(source: &str, reason: &str) -> TypeProviderError {
    TypeProviderError::new(format!(
        "TypeScript semantics are unavailable for {source}: {reason}. \
         Verter's native analysis remains available."
    ))
}

// ---------------------------------------------------------------------------
// Workspace route-selection probe
// ---------------------------------------------------------------------------

/// One configured project's tsserver resolution, as seen by the startup probe.
#[derive(Debug, Clone)]
pub struct ProbedProject {
    /// The project directory the resolution started from.
    pub project_dir: String,
    /// The resolved install.
    pub resolved: ResolvedTsserver,
    /// The install's `(major, minor)` TypeScript version, when readable.
    pub version: Option<(u32, u32)>,
}

/// What a workspace can supply to the per-project tsserver router.
///
/// SERVING is per-project and lazy; this probe answers only the ROUTE-SELECTION
/// question — "can any configured project in this workspace obtain a servable
/// tsserver, and is the TypeScript here the TS7+ native family?" — because the
/// managed-engine choice must be made at startup, long before the published
/// project graph exists and any individual file's owner can be resolved.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceTsserverProbe {
    /// The lexicographically-first configured project that resolved a SERVABLE
    /// (non-native-family) install. `None` ⇒ no project can be served.
    pub servable: Option<ProbedProject>,
    /// The LOWEST `(major, minor)` among every servable install — the version the
    /// serving-tier advisory is computed from, so a workspace where one package
    /// still runs TypeScript 5.8 is advised even when another runs 6.0.
    pub lowest_servable_version: Option<(u32, u32)>,
    /// `Some(major)` when at least one project resolved an install and EVERY
    /// resolved install is the TS7+ native (tsgo) family — the workspace is never
    /// served over the Node tsserver protocol.
    pub native_family_only: Option<u32>,
    /// The per-project refusals, in probe order (empty when nothing was probed).
    pub refusals: Vec<String>,
}

impl WorkspaceTsserverProbe {
    /// The user-visible advisory for this probe: the serving-tier notice for the
    /// weakest engine that will actually serve, AND — when the serving project
    /// passed over a nearer install — the skipped-tier notice naming it.
    ///
    /// Both ride the SAME `show_message` the serving-tier advisory already used,
    /// so a silently swapped toolchain reaches the user through an existing
    /// surface rather than a new one. `None` only when there is nothing to say.
    #[must_use]
    pub fn advisory(&self) -> Option<String> {
        let tier = self.lowest_servable_version.and_then(|version| {
            tsserver_serving_advisory(version, tsserver_serving_tier(Some(version)))
        });
        let skipped = self
            .servable
            .as_ref()
            .and_then(|probed| probed.resolved.skipped_tier_advisory());
        match (tier, skipped) {
            (Some(tier), Some(skipped)) => Some(format!("{tier} {skipped}")),
            (Some(only), None) | (None, Some(only)) => Some(only),
            (None, None) => None,
        }
    }

    /// An actionable summary of why no project could be served.
    ///
    /// Bounded: every refusal carries discovery's full multi-line candidate
    /// report, so a large monorepo would otherwise render kilobytes into the
    /// status surface and the log. The first few are enough to act on; the
    /// remainder is counted.
    #[must_use]
    pub fn refusal_summary(&self) -> String {
        const SHOWN: usize = 3;
        if self.refusals.is_empty() {
            return "no configured TypeScript project was found to resolve TypeScript from"
                .to_string();
        }
        let shown = self
            .refusals
            .iter()
            .take(SHOWN)
            .cloned()
            .collect::<Vec<_>>();
        let summary = shown.join("; ");
        match self.refusals.len().checked_sub(SHOWN) {
            Some(rest) if rest > 0 => format!("{summary}; and {rest} more configured project(s)"),
            _ => summary,
        }
    }
}

/// Probe every configured project under `workspace_root` for a servable tsserver.
///
/// This is the ROUTE-SELECTION probe: it performs filesystem lookups only (plus,
/// on a total miss, discovery's `npm root -g` fallback) and starts no process.
#[must_use]
pub fn probe_workspace_tsserver(
    workspace_root: &str,
    tsdk: Option<&str>,
) -> WorkspaceTsserverProbe {
    let mut project_dirs: Vec<String> =
        verter_workspace::config::discover_tsconfigs(Path::new(workspace_root))
            .into_iter()
            .map(|entry| entry.root)
            .collect();
    project_dirs.sort_unstable();
    project_dirs.dedup();
    probe_project_dirs(&project_dirs, tsdk)
}

fn probe_project_dirs(project_dirs: &[String], tsdk: Option<&str>) -> WorkspaceTsserverProbe {
    let mut probe = WorkspaceTsserverProbe::default();
    let mut native_majors: Vec<u32> = Vec::new();
    let mut any_resolved = false;
    for project_dir in project_dirs {
        match resolve_tsserver(tsdk, Some(project_dir)) {
            Ok(resolved) => {
                any_resolved = true;
                if let Some(major) = tsserver_native_family_major(&resolved.path) {
                    native_majors.push(major);
                    continue;
                }
                let version = detect_ts_version(&resolved.path);
                if let Some(version) = version {
                    probe.lowest_servable_version = Some(
                        probe
                            .lowest_servable_version
                            .map_or(version, |lowest| lowest.min(version)),
                    );
                }
                if probe.servable.is_none() {
                    probe.servable = Some(ProbedProject {
                        project_dir: project_dir.clone(),
                        resolved,
                        version,
                    });
                }
            }
            Err(error) => probe.refusals.push(format!("{project_dir}: {error}")),
        }
    }
    if probe.servable.is_none() && any_resolved {
        probe.native_family_only = native_majors.into_iter().min();
    }
    probe
}

/// The tier that supplied the probe's servable install — used only in logs.
#[must_use]
pub fn probe_source_label(source: TsserverSource) -> &'static str {
    match source {
        TsserverSource::ProjectLocal => "the owning package's node_modules",
        TsserverSource::ConfiguredTsdk => "the configured typescript.tsdk",
        TsserverSource::Global => "the global npm TypeScript",
    }
}

impl TypeProvider for ProjectTsserverProvider {
    fn load_file_with_disposition<'a>(
        &'a self,
        path: &'a str,
        content: &'a str,
        priority: OverlayPriority,
    ) -> ProviderFuture<'a, verter_type_runtime::traits::FileLoadDisposition> {
        Box::pin(async move {
            if verter_session::framework::descriptor::classify_carrier_companion(path).is_none() {
                return self
                    .provider_for_path(path)
                    .await?
                    .load_file_with_disposition(path, content, priority)
                    .await;
            }
            self.apply_file_write(path, content, OverlayFileKind::Load, priority)
                .await?;
            Ok(verter_type_runtime::traits::disposition_for_applied_bytes(
                &self.applied_content(path),
                content,
            ))
        })
    }

    fn applied_content(&self, path: &str) -> verter_type_runtime::traits::AppliedContent {
        use verter_type_runtime::traits::AppliedContent;
        let normalized = Self::normalized(path);
        let certify = |content: AppliedContent| match content {
            AppliedContent::Uncertified => AppliedContent::NotApplied,
            other => other,
        };
        if let Some(route) = self.routes.get(&normalized) {
            for entry in &self.providers {
                if Self::normalized(&entry.key().project) == route.project {
                    return certify(entry.value().applied_content(path));
                }
            }
            return AppliedContent::NotApplied;
        }
        let mut found = None;
        for entry in self.providers.iter() {
            if let AppliedContent::Applied(bytes) = entry.value().applied_content(path) {
                if found.is_some() {
                    return AppliedContent::NotApplied;
                }
                found = Some(AppliedContent::Applied(bytes));
            }
        }
        found.unwrap_or(AppliedContent::NotApplied)
    }

    fn provider_id(&self) -> &'static str {
        "tsserver"
    }

    fn provider_restart_pulse(&self) -> Option<Arc<tokio::sync::Notify>> {
        Some(Arc::clone(&self.restart_pulse))
    }

    fn supports_completion_resolve(&self) -> bool {
        true
    }

    fn open_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Open,
                OverlayPriority::Foreground,
            )
            .await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Load,
                OverlayPriority::Foreground,
            )
            .await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Update,
                OverlayPriority::Foreground,
            )
            .await
        })
    }

    fn close_file(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move { self.provider_for_path(&path).await?.close_file(&path).await })
    }

    fn get_completions(
        &self,
        path: &str,
        offset: u32,
        trigger_character: Option<&str>,
    ) -> ProviderFuture<'_, CompletionResult> {
        let path = path.to_string();
        let trigger_character = trigger_character.map(str::to_string);
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_completions(
                    &path,
                    offset,
                    trigger_character.as_deref()
                ))
            }
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        Box::pin(async move {
            {
                routed_query!(self, path, |route| route
                    .hub
                    .get_completion_details(path, offset, items))
            }
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_hover(&path, offset))
            }
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_diagnostics(&path))
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_definition(&path, offset))
            }
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .get_type_definition(&path, offset))
            }
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_references(&path, offset))
            }
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .get_rename_locations(&path, offset))
            }
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .get_signature_help(&path, offset))
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
        let path = path.to_string();
        let diagnostics = diagnostics.to_vec();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_code_actions(
                    &path,
                    start_offset,
                    end_offset,
                    &diagnostics
                ))
            }
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_semantic_tokens(&path))
            }
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .get_document_highlights(&path, offset))
            }
        })
    }

    fn get_inlay_hints(
        &self,
        path: &str,
        start_offset: u32,
        end_offset: u32,
    ) -> ProviderFuture<'_, Vec<InlayHint>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route.hub.get_inlay_hints(
                    &path,
                    start_offset,
                    end_offset
                ))
            }
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .resolve_completion(&path, data.clone()))
            }
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let providers = self.providers_snapshot();
        Box::pin(async move {
            // Every allocated hub is shut down — a fully established engine or
            // an establishment still in flight (which it abandons); the FIRST
            // failure is reported only after the rest have been asked to stop,
            // so one wedged tsserver cannot strand its siblings.
            let mut first_error = None;
            for provider in providers {
                if let Err(error) = provider.shutdown().await {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
            first_error.map_or(Ok(()), Err)
        })
    }

    fn notify_carrier_changed(&self, companion_path: &str) -> ProviderFuture<'_, ()> {
        let companion_path = companion_path.to_string();
        Box::pin(async move {
            self.provider_for_path(&companion_path)
                .await?
                .notify_carrier_changed(&companion_path)
                .await
        })
    }

    fn notify_carriers_changed<'a>(
        &'a self,
        companion_paths: &'a [String],
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let mut batches: Vec<(Arc<dyn TypeProvider>, Vec<String>)> = Vec::new();
            for path in companion_paths {
                // Resolve every path against live ownership before grouping by
                // the actual engine instance, not the provider kind or a route cache.
                let provider = self.provider_for_path(path).await?;
                if let Some((_, paths)) = batches
                    .iter_mut()
                    .find(|(existing, _)| Arc::ptr_eq(existing, &provider))
                {
                    paths.push(path.clone());
                } else {
                    batches.push((provider, vec![path.clone()]));
                }
            }
            for (provider, paths) in batches {
                provider.notify_carriers_changed(&paths).await?;
            }
            Ok(())
        })
    }

    fn register_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        content: &str,
        project_file_name: &str,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let content = content.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            let (source, companion, project) = (&source_path, &companion_path, &project_file_name);
            let content = &content;
            settle_under_fresh_admission(move || async move {
                let (hub, admitted) = self
                    .admit_registered_unit(source, companion, project)
                    .await
                    .map_err(WriteFailure::Admission)?;
                hub.apply_overlay(
                    &admitted,
                    OverlayMutation::RegisterCarrier {
                        source_path: source.clone(),
                        companion_path: companion.clone(),
                        content: content.clone(),
                        project_file_name: project.clone(),
                    },
                )
                .await
                .map_err(WriteFailure::refused("hub carrier registration"))
            })
            .await?;
            self.register_route(&source_path, &companion_path, &project_file_name);
            Ok(())
        })
    }

    fn register_carrier_metadata<'a>(
        &'a self,
        source_path: &'a str,
        companion_path: &'a str,
        content: &'a str,
        project_file_name: &'a str,
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            settle_under_fresh_admission(move || async move {
                let (hub, admitted) = self
                    .admit_registered_unit(source_path, companion_path, project_file_name)
                    .await
                    .map_err(WriteFailure::Admission)?;
                hub.apply_overlay(
                    &admitted,
                    OverlayMutation::RegisterCarrierMetadata {
                        source_path: source_path.to_string(),
                        companion_path: companion_path.to_string(),
                        content: content.to_string(),
                        project_file_name: project_file_name.to_string(),
                    },
                )
                .await
                .map_err(WriteFailure::refused("hub carrier metadata"))
            })
            .await?;
            self.register_route(source_path, companion_path, project_file_name);
            Ok(())
        })
    }

    fn activate_carrier_member(
        &self,
        source_path: &str,
        companion_path: &str,
        project_file_name: &str,
        script_kind: CarrierScriptKind,
    ) -> ProviderFuture<'_, ()> {
        let source_path = source_path.to_string();
        let companion_path = companion_path.to_string();
        let project_file_name = project_file_name.to_string();
        Box::pin(async move {
            let (source, companion, project) = (&source_path, &companion_path, &project_file_name);
            settle_under_fresh_admission(move || async move {
                let (hub, admitted) = self
                    .admit_registered_unit(source, companion, project)
                    .await
                    .map_err(WriteFailure::Admission)?;
                hub.apply_overlay(
                    &admitted,
                    OverlayMutation::ActivateCarrier {
                        source_path: source.clone(),
                        companion_path: companion.clone(),
                        project_file_name: project.clone(),
                        script_kind,
                    },
                )
                .await
                .map_err(WriteFailure::refused("hub carrier activation"))
            })
            .await?;
            self.register_route(&source_path, &companion_path, &project_file_name);
            Ok(())
        })
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            let mut batches: Vec<AdmittedCarrierBatch> = Vec::new();
            for member in members {
                let (hub, admitted) = self
                    .admit_registered_unit(
                        &member.source_path,
                        &member.companion_path,
                        &member.project_file_name,
                    )
                    .await?;
                if let Some((_, members)) = batches
                    .iter_mut()
                    .find(|(existing, _)| Arc::ptr_eq(existing, &hub))
                {
                    members.push((admitted, member.clone()));
                } else {
                    batches.push((hub, vec![(admitted, member.clone())]));
                }
            }
            // Preserve each engine's input order and its single-refresh bulk
            // activation contract. Scalar editor opens keep their separate path.
            // A group refused on a drifted basis is re-admitted and re-issued
            // whole on its own engine; the other groups are not re-applied.
            let outcomes = futures_util::future::join_all(batches.into_iter().map(
                |(hub, admitted)| async move {
                    let group: Vec<CarrierActivation> =
                        admitted.iter().map(|(_, member)| member.clone()).collect();
                    let (hub_ref, group_ref) = (&hub, &group);
                    let mut first = Some(admitted);
                    settle_under_fresh_admission(move || {
                        let admitted = first.take();
                        async move {
                            let admitted = match admitted {
                                Some(admitted) => admitted,
                                None => self.readmit_carrier_group(hub_ref, group_ref).await?,
                            };
                            hub_ref
                                .apply_overlay_batch(admitted)
                                .await
                                .map_err(WriteFailure::refused("hub carrier batch"))
                        }
                    })
                    .await?;
                    for member in &group {
                        self.register_route(
                            &member.source_path,
                            &member.companion_path,
                            &member.project_file_name,
                        );
                    }
                    Ok::<(), TypeProviderError>(())
                },
            ))
            .await;
            for outcome in outcomes {
                outcome?;
            }
            Ok(())
        })
    }

    fn resync_open_files(&self) -> ProviderFuture<'_, ()> {
        let providers = self.providers_snapshot();
        Box::pin(async move {
            for provider in providers {
                provider.resync_open_files().await?;
            }
            Ok(())
        })
    }

    fn update_workspace_folders(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let providers = self.providers_snapshot();
        Box::pin(async move {
            for provider in providers {
                provider
                    .update_workspace_folders(added.clone(), removed.clone())
                    .await?;
            }
            Ok(())
        })
    }

    /// The PID of the FIRST engine this router started, or `None` while it is
    /// still cold.
    ///
    /// The wire notification this feeds (`$/verter/typeProviderStarted`) carries
    /// exactly one PID — a single-engine affordance the router cannot honour for
    /// N engines. Orphan containment does not depend on it: every spawned
    /// tsserver arms its own process-group `TreeKill` and registers in the
    /// process-wide engine-tree table, which the client-death monitor terminates
    /// in full.
    fn child_pid(&self) -> Option<u32> {
        self.providers_snapshot()
            .into_iter()
            .find_map(|provider| provider.child_pid())
    }

    fn open_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Open,
                OverlayPriority::Background,
            )
            .await
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Load,
                OverlayPriority::Background,
            )
            .await
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Update,
                OverlayPriority::Background,
            )
            .await
        })
    }

    fn close_file_background(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .close_file_background(&path)
                .await
        })
    }

    fn get_diagnostics_background(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                routed_query!(self, &path, |route| route
                    .hub
                    .get_diagnostics_background(&path))
            }
        })
    }

    fn update_workspace_folders_background(
        &self,
        added: Vec<serde_json::Value>,
        removed: Vec<serde_json::Value>,
    ) -> ProviderFuture<'_, ()> {
        let providers = self.providers_snapshot();
        Box::pin(async move {
            for provider in providers {
                provider
                    .update_workspace_folders_background(added.clone(), removed.clone())
                    .await?;
            }
            Ok(())
        })
    }

    fn open_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Open,
                OverlayPriority::Normal,
            )
            .await
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Load,
                OverlayPriority::Normal,
            )
            .await
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.apply_file_write(
                &path,
                &content,
                OverlayFileKind::Update,
                OverlayPriority::Normal,
            )
            .await
        })
    }

    fn close_file_normal(&self, path: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .close_file_normal(&path)
                .await
        })
    }
}

#[cfg(test)]
#[path = "project_router_tests.rs"]
mod tests;
