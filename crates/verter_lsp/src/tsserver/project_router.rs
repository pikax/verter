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

use dashmap::DashMap;
use tokio::sync::OnceCell;
use tower_lsp_server::Client;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineBackend, ProjectBinding,
};
use verter_session::VerterHost;
use verter_type_runtime::discovery::{
    detect_ts_version, resolve_tsserver, tsserver_native_family_major, tsserver_serving_advisory,
    tsserver_serving_tier, ResolvedTsserver, TsserverSource,
};
use verter_type_runtime::provider_hub::{
    AdmittedRequest, OverlayFileKind, OverlayMutation, OverlayPriority, ProjectWitness,
};
use verter_workspace::{decide_generated_unit_admission, CanonicalPath};

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

/// A cached per-project engine resolution, fenced on the exact publication and
/// workspace generations it was taken at.
///
/// Resolution walks the filesystem (ancestor `node_modules` probes, a
/// `canonicalize`, a `read_dir` of the install's `lib/`) and, on a total miss,
/// shells out to `npm root -g`. Doing that on every hover would be a per-request
/// filesystem storm, so both the success and the refusal are cached. The
/// basis fence releases the cache whenever the workspace project graph is
/// republished (a tsconfig edit, a config-file change, a workspace-folder
/// change), so a project-graph change re-resolves; a bare `node_modules`
/// mutation that publishes no new snapshot still needs a server reload.
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

/// The hub witness stays with the query until its answer is settled.
struct RequestRoute {
    hub: Arc<ProviderHub<dyn TypeProvider>>,
    witness: ProjectWitness,
    admission: Option<AdmittedRequest>,
    path: String,
}

type AdmittedCarrierBatch = (
    Arc<ProviderHub<dyn TypeProvider>>,
    Vec<(AdmittedRequest, CarrierActivation)>,
);

impl RequestRoute {
    fn check(&self) -> Result<(), TypeProviderError> {
        self.hub.check_project(&self.witness).map_err(|reason| {
            project_refusal(&self.path, &format!("request binding expired: {reason:?}"))
        })?;
        if let Some(admission) = &self.admission {
            self.hub.check_admission(admission).map_err(|reason| {
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

/// A project-bound pool of hub-managed tsserver engines.
///
/// Cold at construction: no tsserver starts until a project-bound lifecycle
/// operation or query resolves an owning configured project.
pub struct ProjectTsserverProvider {
    host: Arc<VerterHost>,
    tsdk: Option<String>,
    plugin_path: Option<String>,
    node_path: String,
    client: Arc<OnceCell<Client>>,
    /// The witness backend the router mints each operation's [`BoundProject`]
    /// through. It is NOT the publish path's backend — `ensure_project` is a
    /// pure witness mint plus per-backend bookkeeping, and the router only needs
    /// the witness that proves the operation is project-bound.
    witness_backend: TsserverEngineBackend,
    engine_specs: DashMap<String, CachedEngineSpec>,
    providers: DashMap<ProjectEngineKey, Arc<ProviderHub<dyn TypeProvider>>>,
    routes: DashMap<String, RegisteredRoute>,
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
        client: Arc<OnceCell<Client>>,
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
        })
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
    ) -> Result<(ProjectBinding, ResolvedPublication), TypeProviderError> {
        let (source, registered_project) = self.source_for_path(path);
        self.binding_for_source_with_expected(&source, registered_project.as_deref())
    }

    fn binding_for_source_with_expected(
        &self,
        source: &str,
        expected_project: Option<&str>,
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
        if ResolvedPublication::current(&self.host).as_ref() != Some(&resolved) {
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
            if cached.basis == *basis {
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
                    Arc::clone(&self.client),
                    3,
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
                decide_generated_unit_admission(
                    resolved.published.snapshot.as_ref(),
                    &CanonicalPath::new(binding.tsconfig_uri()),
                    units,
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

    async fn provider_for_path(
        &self,
        path: &str,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let (binding, resolved) = self.binding_for_path_with_publication(path)?;
        let provider = self.provider_for_binding(&binding, &resolved).await?;
        if ResolvedPublication::current(&self.host).as_ref() != Some(&resolved) {
            return Err(project_refusal(
                path,
                "provider selection raced a workspace change",
            ));
        }
        Ok(provider)
    }

    async fn provider_for_request_path(
        &self,
        path: &str,
    ) -> Result<RequestRoute, TypeProviderError> {
        if verter_session::framework::descriptor::classify_carrier_companion(path).is_none() {
            let (source, _) = self.source_for_path(path);
            let (binding, published) = self.binding_for_path_with_publication(path)?;
            let hub = self.hub_for_binding(&binding, &published).await?;
            let witness = hub
                .bind_project(hub_binding_input(&self.host, &source, &binding, published))
                .map_err(|reason| {
                    project_refusal(path, &format!("hub binding refused: {reason:?}"))
                })?;
            return Ok(RequestRoute {
                hub,
                witness,
                admission: None,
                path: path.to_string(),
            });
        }
        let (source, _) = self.source_for_path(path);
        let (binding, published) = self.binding_for_path_with_publication(path)?;
        let (hub, admission) = self
            .admit_generated_write(&source, &binding, published, &[CanonicalPath::new(path)])
            .await?;
        Ok(RequestRoute {
            hub,
            witness: admission.project_witness().clone(),
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
        let (binding, published) = self.binding_for_path_with_publication(path)?;
        let (hub, admitted) = self
            .admit_generated_write(&source, &binding, published, &[CanonicalPath::new(path)])
            .await?;
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
        .map_err(|reason| {
            TypeProviderError::new(format!("hub generated-unit write refused: {reason:?}"))
        })
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
        let (binding, published) = self.binding_for_source_with_expected(source, None)?;
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
    fn provider_id(&self) -> &'static str {
        "tsserver"
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(
                        route
                            .hub
                            .get_completions(&path, offset, trigger_character.as_deref()),
                    )
                    .await
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
                let route = self.provider_for_request_path(path).await?;
                route
                    .run(route.hub.get_completion_details(path, offset, items))
                    .await
            }
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_hover(&path, offset)).await
            }
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_diagnostics(&path)).await
            }
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_definition(&path, offset)).await
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(route.hub.get_type_definition(&path, offset))
                    .await
            }
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_references(&path, offset)).await
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(route.hub.get_rename_locations(&path, offset))
                    .await
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
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_signature_help(&path, offset)).await
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(
                        route
                            .hub
                            .get_code_actions(&path, start_offset, end_offset, &diagnostics),
                    )
                    .await
            }
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path = path.to_string();
        Box::pin(async move {
            {
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_semantic_tokens(&path)).await
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(route.hub.get_document_highlights(&path, offset))
                    .await
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
                let route = self.provider_for_request_path(&path).await?;
                route
                    .run(route.hub.get_inlay_hints(&path, start_offset, end_offset))
                    .await
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
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.resolve_completion(&path, data)).await
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
            let (binding, published) = self.binding_for_registered_with_publication(
                &source_path,
                &companion_path,
                &project_file_name,
            )?;
            let (hub, admitted) = self
                .admit_generated_write(
                    &source_path,
                    &binding,
                    published,
                    &[CanonicalPath::new(&companion_path)],
                )
                .await?;
            hub.apply_overlay(
                &admitted,
                OverlayMutation::RegisterCarrier {
                    source_path: source_path.clone(),
                    companion_path: companion_path.clone(),
                    content,
                    project_file_name: project_file_name.clone(),
                },
            )
            .await
            .map_err(|reason| {
                TypeProviderError::new(format!("hub carrier registration refused: {reason:?}"))
            })?;
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
            let (binding, published) = self.binding_for_registered_with_publication(
                source_path,
                companion_path,
                project_file_name,
            )?;
            let (hub, admitted) = self
                .admit_generated_write(
                    source_path,
                    &binding,
                    published,
                    &[CanonicalPath::new(companion_path)],
                )
                .await?;
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
            .map_err(|reason| {
                TypeProviderError::new(format!("hub carrier metadata refused: {reason:?}"))
            })?;
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
            let (binding, published) = self.binding_for_registered_with_publication(
                &source_path,
                &companion_path,
                &project_file_name,
            )?;
            let (hub, admitted) = self
                .admit_generated_write(
                    &source_path,
                    &binding,
                    published,
                    &[CanonicalPath::new(&companion_path)],
                )
                .await?;
            hub.apply_overlay(
                &admitted,
                OverlayMutation::ActivateCarrier {
                    source_path: source_path.clone(),
                    companion_path: companion_path.clone(),
                    project_file_name: project_file_name.clone(),
                    script_kind,
                },
            )
            .await
            .map_err(|reason| {
                TypeProviderError::new(format!("hub carrier activation refused: {reason:?}"))
            })?;
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
                let (binding, published) = self.binding_for_registered_with_publication(
                    &member.source_path,
                    &member.companion_path,
                    &member.project_file_name,
                )?;
                let (hub, admitted) = self
                    .admit_generated_write(
                        &member.source_path,
                        &binding,
                        published,
                        &[CanonicalPath::new(&member.companion_path)],
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
            for (hub, members) in batches {
                let routes: Vec<_> = members
                    .iter()
                    .map(|(_, member)| {
                        (
                            member.source_path.clone(),
                            member.companion_path.clone(),
                            member.project_file_name.clone(),
                        )
                    })
                    .collect();
                hub.apply_overlay_batch(members).await.map_err(|reason| {
                    TypeProviderError::new(format!("hub carrier batch refused: {reason:?}"))
                })?;
                for (source, companion, project) in routes {
                    self.register_route(&source, &companion, &project);
                }
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
                let route = self.provider_for_request_path(&path).await?;
                route.run(route.hub.get_diagnostics_background(&path)).await
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
