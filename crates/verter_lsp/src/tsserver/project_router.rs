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
//! resilient tsserver process per `(owning tsconfig, real tsserver.js)` identity.
//! A project whose TypeScript cannot be resolved fails closed with the
//! actionable install message from [`resolve_tsserver`] and NEVER borrows another
//! project's engine.

use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::Client;
use verter_session::external_ts::{
    BoundProject, CarrierOwnershipResolution, EngineBackend, ProjectBinding,
};
use verter_session::VerterHost;
use verter_type_runtime::discovery::{
    detect_ts_version, resolve_tsserver, tsserver_native_family_major, tsserver_serving_advisory,
    tsserver_serving_tier, ResolvedTsserver, TsserverSource,
};

use crate::external_ts::TsserverEngineBackend;
use crate::tsgo::project_binding::{resolve_carrier, OwnershipReadinessMode};
use crate::type_provider::protocol::*;
use crate::type_provider::traits::{
    CarrierActivation, CarrierScriptKind, ProviderFuture, TypeProvider,
};

use super::ipc::TsserverTypeProvider;
use super::resilient;

/// The identity of ONE owned tsserver process: the owning configured project
/// plus the REAL `tsserver.js` that serves it. Two projects that resolve the
/// same install share one process; two projects on different TypeScript
/// versions never do.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ProjectEngineKey {
    project: String,
    tsserver_path: String,
}

/// Everything needed to spawn (or identify) one project's tsserver.
#[derive(Debug, Clone)]
struct ProjectEngineSpec {
    key: ProjectEngineKey,
    workspace_root: String,
    default_lib_count: usize,
}

/// A cached per-project engine resolution, fenced on the published-snapshot
/// generation it was taken at.
///
/// Resolution walks the filesystem (ancestor `node_modules` probes, a
/// `canonicalize`, a `read_dir` of the install's `lib/`) and, on a total miss,
/// shells out to `npm root -g`. Doing that on every hover would be a per-request
/// filesystem storm, so both the success and the refusal are cached. The
/// generation fence releases the cache whenever the workspace project graph is
/// republished (a tsconfig edit, a config-file change, a workspace-folder
/// change), so a project-graph change re-resolves; a bare `node_modules`
/// mutation that publishes no new snapshot still needs a server reload.
#[derive(Debug, Clone)]
struct CachedEngineSpec {
    generation: u64,
    outcome: Result<ProjectEngineSpec, String>,
}

/// A companion path registered by the publish path, mapped back to the authored
/// carrier source and the owning project the publisher resolved it under.
#[derive(Debug, Clone)]
struct RegisteredRoute {
    source: String,
    project: String,
}

/// A project-bound pool of resilient tsserver processes.
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
    providers: DashMap<ProjectEngineKey, Arc<OnceCell<Arc<dyn TypeProvider>>>>,
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
    /// the published-snapshot generation the decision was taken at.
    ///
    /// Every non-`Bound` state is a DISTINCT fail-closed refusal — never an
    /// inferred project and never another project's engine.
    fn binding_for_path(&self, path: &str) -> Result<(ProjectBinding, u64), TypeProviderError> {
        let (source, registered_project) = self.source_for_path(path);
        let Some((resolution, generation)) = resolve_carrier(
            self.host.as_ref(),
            &source,
            Arc::from(""),
            // A PRESENT published snapshot is authoritative — the same gate the
            // OWNED tsgo carrier-diagnostics path uses. The bootstrap-absent case
            // is the `None` arm below; a present-but-cold snapshot must still bind
            // its owner rather than refuse every operation until warm-up finishes.
            OwnershipReadinessMode::PresentSnapshotAuthoritative,
        ) else {
            return Err(project_refusal(
                &source,
                "the configured-project snapshot is not published yet",
            ));
        };
        let binding = match resolution {
            CarrierOwnershipResolution::Bound(binding) => binding,
            CarrierOwnershipResolution::NotReady => {
                return Err(project_refusal(
                    &source,
                    "configured-project ownership is not ready yet",
                ));
            }
            CarrierOwnershipResolution::NoProject => {
                return Err(project_refusal(
                    &source,
                    "no owning tsconfig.json or jsconfig.json was resolved",
                ));
            }
            CarrierOwnershipResolution::Ambiguous { cause, .. } => {
                return Err(project_refusal(
                    &source,
                    &format!("configured-project ownership is ambiguous: {cause:?}"),
                ));
            }
        };
        if let Some(expected) = registered_project {
            if Self::normalized(binding.tsconfig_uri()) != expected {
                return Err(project_refusal(
                    &source,
                    "the live ProjectBinding no longer matches the registered owning project",
                ));
            }
        }
        Ok((binding, generation))
    }

    /// Mint the operation's [`BoundProject`] witness and resolve the owning
    /// project's engine, reusing the generation-fenced cached resolution.
    fn engine_for_binding(
        &self,
        binding: &ProjectBinding,
        generation: u64,
    ) -> Result<(BoundProject, ProjectEngineSpec), TypeProviderError> {
        // The witness is minted on EVERY operation (never cached): the
        // project-bound contract requires a live `BoundProject` for each
        // provider op, and the mint is a cheap bookkeeping insert.
        let bound = ensure_bound(&self.witness_backend, binding)?;
        let project = Self::normalized(binding.tsconfig_uri());
        if let Some(cached) = self.engine_specs.get(&project) {
            if cached.generation == generation {
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
                generation,
                outcome: outcome.clone(),
            },
        );
        outcome
            .map(|spec| (bound, spec))
            .map_err(TypeProviderError::new)
    }

    async fn provider_for_binding(
        &self,
        binding: &ProjectBinding,
        generation: u64,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let (_bound, spec) = self.engine_for_binding(binding, generation)?;
        // One `OnceCell` per engine identity: concurrent cold demands for the
        // same project collapse onto ONE spawn, and a failed spawn leaves the
        // cell uninitialized so the next demand retries rather than latching.
        let cell = self
            .providers
            .entry(spec.key.clone())
            .or_insert_with(|| Arc::new(OnceCell::new()))
            .clone();
        let provider = cell
            .get_or_try_init(|| self.spawn_project_provider(spec))
            .await?;
        Ok(Arc::clone(provider))
    }

    async fn provider_for_path(
        &self,
        path: &str,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let (binding, generation) = self.binding_for_path(path)?;
        self.provider_for_binding(&binding, generation).await
    }

    async fn spawn_project_provider(
        &self,
        spec: ProjectEngineSpec,
    ) -> Result<Arc<dyn TypeProvider>, TypeProviderError> {
        let crash_notify = Arc::new(Notify::new());
        let carrier_store_dir =
            crate::external_ts::default_carrier_store_dir_string(&spec.workspace_root);
        let provider = TsserverTypeProvider::spawn(
            &self.node_path,
            &spec.key.tsserver_path,
            &spec.workspace_root,
            self.plugin_path.as_deref(),
            Some(&carrier_store_dir),
            // verter_lsp-internal backend: the Rust merge layer is the sole
            // companion→source response mapper, so the plugin returns RAW responses.
            false,
            Some(Arc::clone(&crash_notify)),
        )
        .await
        .map_err(|error| {
            TypeProviderError::new(format!(
                "resolved project {} to {} ({} default libraries), but tsserver failed to start: \
                 {error}",
                spec.key.project, spec.key.tsserver_path, spec.default_lib_count
            ))
        })?;
        let provider = resilient::new(
            provider,
            crash_notify,
            self.node_path.clone(),
            spec.key.tsserver_path.clone(),
            spec.workspace_root.clone(),
            self.plugin_path.clone(),
            Arc::clone(&self.client),
            3,
        );
        let provider: Arc<dyn TypeProvider> = Arc::new(provider);
        tracing::info!(
            project = %spec.key.project,
            tsserver = %spec.key.tsserver_path,
            default_lib_count = spec.default_lib_count,
            "project-bound tsserver started"
        );
        // Announce THIS engine's child pid. The router is cold at construction,
        // so `initialized()` has no pid to report; and with N engines a single
        // startup announcement could only ever name one of them. Announcing each
        // engine as it starts keeps the editor's orphan-cleanup set complete.
        self.announce_started(&provider);
        Ok(provider)
    }

    /// Send `$/verter/typeProviderStarted` for a freshly started engine.
    fn announce_started(&self, provider: &Arc<dyn TypeProvider>) {
        let Some(pid) = provider.child_pid() else {
            // No pid, no notification: the contract carries a real child process
            // id, and fabricating one would name a process that does not exist.
            tracing::warn!("project-bound tsserver started without a reportable child pid");
            return;
        };
        let client = Arc::clone(&self.client);
        tokio::spawn(async move {
            if let Some(client) = client.get() {
                client
                    .send_notification::<crate::server::protocol_types::TypeProviderStarted>(
                        crate::server::protocol_types::TypeProviderStartedParams {
                            pid,
                            kind: "tsserver".to_string(),
                        },
                    )
                    .await;
            }
        });
    }

    fn register_route(&self, source: &str, companion: &str, project: &str) {
        let route = RegisteredRoute {
            source: Self::normalized(source),
            project: Self::normalized(project),
        };
        self.routes.insert(Self::normalized(source), route.clone());
        self.routes.insert(Self::normalized(companion), route);
    }

    /// Register a publish-path route and resolve its binding in one step.
    fn binding_for_registered(
        &self,
        source: &str,
        companion: &str,
        project: &str,
    ) -> Result<(ProjectBinding, u64), TypeProviderError> {
        self.register_route(source, companion, project);
        self.binding_for_path(source)
    }

    /// Every tsserver process this router has actually started.
    fn providers_snapshot(&self) -> Vec<Arc<dyn TypeProvider>> {
        self.providers
            .iter()
            .filter_map(|entry| entry.value().get().cloned())
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
    /// The serving-tier advisory for the weakest engine that will actually
    /// serve, or `None` when every servable install is current-generation.
    #[must_use]
    pub fn advisory(&self) -> Option<String> {
        let version = self.lowest_servable_version?;
        tsserver_serving_advisory(version, tsserver_serving_tier(Some(version)))
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
            self.provider_for_path(&path)
                .await?
                .open_file(&path, &content)
                .await
        })
    }

    fn load_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .load_file(&path, &content)
                .await
        })
    }

    fn update_file(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .update_file(&path, &content)
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
            self.provider_for_path(&path)
                .await?
                .get_completions(&path, offset, trigger_character.as_deref())
                .await
        })
    }

    fn get_completion_details<'a>(
        &'a self,
        path: &'a str,
        offset: u32,
        items: &'a [Completion],
    ) -> ProviderFuture<'a, Vec<Completion>> {
        Box::pin(async move {
            self.provider_for_path(path)
                .await?
                .get_completion_details(path, offset, items)
                .await
        })
    }

    fn get_hover(&self, path: &str, offset: u32) -> ProviderFuture<'_, Option<HoverInfo>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_hover(&path, offset)
                .await
        })
    }

    fn get_diagnostics(&self, path: &str) -> ProviderFuture<'_, Vec<TypeDiagnostic>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_diagnostics(&path)
                .await
        })
    }

    fn get_definition(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_definition(&path, offset)
                .await
        })
    }

    fn get_type_definition(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_type_definition(&path, offset)
                .await
        })
    }

    fn get_references(&self, path: &str, offset: u32) -> ProviderFuture<'_, Vec<TypeLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_references(&path, offset)
                .await
        })
    }

    fn get_rename_locations(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<RenameLocation>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_rename_locations(&path, offset)
                .await
        })
    }

    fn get_signature_help(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Option<SignatureHelp>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_signature_help(&path, offset)
                .await
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
            self.provider_for_path(&path)
                .await?
                .get_code_actions(&path, start_offset, end_offset, &diagnostics)
                .await
        })
    }

    fn get_semantic_tokens(&self, path: &str) -> ProviderFuture<'_, Vec<SemanticToken>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_semantic_tokens(&path)
                .await
        })
    }

    fn get_document_highlights(
        &self,
        path: &str,
        offset: u32,
    ) -> ProviderFuture<'_, Vec<TypeDocumentHighlight>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .get_document_highlights(&path, offset)
                .await
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
            self.provider_for_path(&path)
                .await?
                .get_inlay_hints(&path, start_offset, end_offset)
                .await
        })
    }

    fn resolve_completion(
        &self,
        path: &str,
        data: CompletionResolveData,
    ) -> ProviderFuture<'_, Option<CompletionResolveResult>> {
        let path = path.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .resolve_completion(&path, data)
                .await
        })
    }

    fn shutdown(&self) -> ProviderFuture<'_, ()> {
        let providers = self.providers_snapshot();
        Box::pin(async move {
            // Every started engine is shut down; the FIRST failure is reported
            // only after the rest have been asked to stop, so one wedged
            // tsserver cannot strand its siblings.
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
            for path in companion_paths {
                self.notify_carrier_changed(path).await?;
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
            let (binding, generation) =
                self.binding_for_registered(&source_path, &companion_path, &project_file_name)?;
            self.provider_for_binding(&binding, generation)
                .await?
                .register_carrier_member(
                    &source_path,
                    &companion_path,
                    &content,
                    &project_file_name,
                )
                .await
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
            let (binding, generation) =
                self.binding_for_registered(source_path, companion_path, project_file_name)?;
            self.provider_for_binding(&binding, generation)
                .await?
                .register_carrier_metadata(source_path, companion_path, content, project_file_name)
                .await
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
            let (binding, generation) =
                self.binding_for_registered(&source_path, &companion_path, &project_file_name)?;
            self.provider_for_binding(&binding, generation)
                .await?
                .activate_carrier_member(
                    &source_path,
                    &companion_path,
                    &project_file_name,
                    script_kind,
                )
                .await
        })
    }

    fn activate_carrier_members<'a>(
        &'a self,
        members: &'a [CarrierActivation],
    ) -> ProviderFuture<'a, ()> {
        Box::pin(async move {
            for member in members {
                self.activate_carrier_member(
                    &member.source_path,
                    &member.companion_path,
                    &member.project_file_name,
                    member.script_kind,
                )
                .await?;
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
            self.provider_for_path(&path)
                .await?
                .open_file_background(&path, &content)
                .await
        })
    }

    fn load_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .load_file_background(&path, &content)
                .await
        })
    }

    fn update_file_background(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .update_file_background(&path, &content)
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
            self.provider_for_path(&path)
                .await?
                .get_diagnostics_background(&path)
                .await
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
            self.provider_for_path(&path)
                .await?
                .open_file_normal(&path, &content)
                .await
        })
    }

    fn load_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .load_file_normal(&path, &content)
                .await
        })
    }

    fn update_file_normal(&self, path: &str, content: &str) -> ProviderFuture<'_, ()> {
        let path = path.to_string();
        let content = content.to_string();
        Box::pin(async move {
            self.provider_for_path(&path)
                .await?
                .update_file_normal(&path, &content)
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
mod tests {
    use super::*;
    use std::path::PathBuf;
    use verter_session::external_ts::EnvDims;
    use verter_session::file_artifact_store::ProjectIdentity;
    use verter_workspace::workspace_snapshot::{ProjectId, SnapshotGeneration};

    fn write_typescript(root: &Path, version: &str) -> PathBuf {
        let lib = root.join("node_modules/typescript/lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
        std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
        std::fs::write(
            root.join("node_modules/typescript/package.json"),
            format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
        )
        .unwrap();
        lib.join("tsserver.js").canonicalize().unwrap()
    }

    /// A pnpm-shaped install: the package's `node_modules/typescript` is a
    /// SYMLINK into a workspace-level `.pnpm` store, exactly as pnpm lays a
    /// monorepo out. Returns the REAL (canonical) `tsserver.js`.
    #[cfg(unix)]
    fn link_pnpm_typescript(workspace: &Path, package: &Path, version: &str) -> PathBuf {
        use std::os::unix::fs::symlink;
        let store = workspace
            .join("node_modules/.pnpm")
            .join(format!("typescript@{version}"))
            .join("node_modules/typescript");
        let lib = store.join("lib");
        std::fs::create_dir_all(&lib).unwrap();
        std::fs::write(lib.join("tsserver.js"), "// tsserver").unwrap();
        std::fs::write(lib.join("lib.es5.d.ts"), "interface Array<T> {}").unwrap();
        std::fs::write(
            store.join("package.json"),
            format!(r#"{{ "name": "typescript", "version": "{version}" }}"#),
        )
        .unwrap();
        std::fs::create_dir_all(package.join("node_modules")).unwrap();
        symlink(&store, package.join("node_modules/typescript")).unwrap();
        lib.join("tsserver.js").canonicalize().unwrap()
    }

    fn write_tsconfig(project: &Path) {
        std::fs::create_dir_all(project).unwrap();
        std::fs::write(project.join("tsconfig.json"), r#"{ "include": ["src"] }"#).unwrap();
    }

    fn binding(workspace: &Path, project: &Path, id: u32) -> ProjectBinding {
        ProjectBinding::new_for_test(
            workspace.to_string_lossy().into_owned(),
            project.join("tsconfig.json").to_string_lossy().into_owned(),
            "",
            EnvDims {
                parse_env_hash: [id as u8; 16],
                resolve_env_hash: [id as u8; 16],
                lib_env_hash: [id as u8; 16],
                project_identity: ProjectIdentity([id as u8; 16]),
            },
            Vec::new(),
            ProjectId(id),
            SnapshotGeneration(1),
        )
    }

    fn engine_spec(
        backend: &TsserverEngineBackend,
        binding: &ProjectBinding,
        tsdk: Option<&str>,
    ) -> Result<ProjectEngineSpec, String> {
        let bound = ensure_bound(backend, binding).expect("the witness mint is infallible");
        resolve_engine_spec(&bound, binding, tsdk)
    }

    /// @ai-generated - Pins distinct engine identity for different owning projects.
    ///
    /// The whole point of the router: two packages in ONE workspace, pinned to
    /// DIFFERENT TypeScript versions, resolve to two DIFFERENT `tsserver.js`
    /// installs — so they can never share one process.
    ///
    /// The fixture also plants a THIRD, unrelated TypeScript at the WORKSPACE
    /// ROOT. Resolving from the workspace root (the behaviour this router
    /// replaced) would hand BOTH packages that root install — the assertions
    /// below fail in exactly that case, so this test discriminates the
    /// per-project resolution from the workspace-level one.
    #[cfg(unix)]
    #[test]
    fn different_projects_keep_their_own_typescript_engines() {
        let workspace = tempfile::tempdir().unwrap();
        let project_a = workspace.path().join("packages/a");
        let project_b = workspace.path().join("packages/b");
        std::fs::create_dir_all(&project_a).unwrap();
        std::fs::create_dir_all(&project_b).unwrap();
        let root_install = write_typescript(workspace.path(), "5.0.4");
        // pnpm layout: package symlinks into the workspace `.pnpm` store, so the
        // resolution must canonicalize to the REAL versioned install — tsserver
        // finds its `lib.*.d.ts` relative to its own script path.
        let expected_a = link_pnpm_typescript(workspace.path(), &project_a, "5.8.3");
        let expected_b = link_pnpm_typescript(workspace.path(), &project_b, "6.0.2");
        let backend = TsserverEngineBackend::with_default_host_version();

        let spec_a =
            engine_spec(&backend, &binding(workspace.path(), &project_a, 0), None).unwrap();
        let spec_b =
            engine_spec(&backend, &binding(workspace.path(), &project_b, 1), None).unwrap();

        assert_eq!(Path::new(&spec_a.key.tsserver_path), expected_a);
        assert_eq!(Path::new(&spec_b.key.tsserver_path), expected_b);
        assert_ne!(
            spec_a.key.tsserver_path, spec_b.key.tsserver_path,
            "two packages pinned to different TypeScript versions must not share an engine"
        );
        assert_ne!(spec_a.key, spec_b.key);
        for spec in [&spec_a, &spec_b] {
            assert_ne!(
                Path::new(&spec.key.tsserver_path),
                root_install,
                "a package must be served by its OWN install, never the workspace root's"
            );
        }
        assert!(spec_a.default_lib_count > 0 && spec_b.default_lib_count > 0);
    }

    /// @ai-generated - NEGATIVE CONTROL: a project with no resolvable TypeScript
    /// fails closed with the actionable install message and is NEVER served by a
    /// sibling project's engine.
    #[cfg(unix)]
    #[test]
    fn project_without_typescript_fails_closed_and_never_borrows_a_sibling_engine() {
        let workspace = tempfile::tempdir().unwrap();
        let served = workspace.path().join("packages/served");
        let bare = workspace.path().join("packages/bare");
        std::fs::create_dir_all(&served).unwrap();
        std::fs::create_dir_all(&bare).unwrap();
        let served_tsserver = link_pnpm_typescript(workspace.path(), &served, "6.0.2");
        let backend = TsserverEngineBackend::with_default_host_version();

        let served_spec =
            engine_spec(&backend, &binding(workspace.path(), &served, 0), None).unwrap();
        assert_eq!(Path::new(&served_spec.key.tsserver_path), served_tsserver);

        // The bare package's ancestor walk escapes the tempdir, so the assertion
        // is conditional on the machine genuinely having no ambient TypeScript
        // above it; when one exists the meaningful invariant is still checked —
        // the refusal (or the resolution) is NEVER the sibling's engine.
        match engine_spec(&backend, &binding(workspace.path(), &bare, 1), None) {
            Err(message) => {
                assert!(
                    message.contains("no usable TypeScript installation was found"),
                    "the refusal names the missing install: {message}"
                );
                assert!(
                    message.contains("npm install -D typescript"),
                    "the refusal carries the actionable install command: {message}"
                );
                assert!(
                    !message.contains(&served_spec.key.tsserver_path),
                    "the refusal must not point at the sibling project's engine: {message}"
                );
            }
            Ok(spec) => assert_ne!(
                spec.key.tsserver_path, served_spec.key.tsserver_path,
                "a project must never be served by another project's resolved engine"
            ),
        }
    }

    /// @ai-generated - The route-selection probe reports the workspace as
    /// servable when ANY configured project can obtain TypeScript, and computes
    /// the advisory from the LOWEST serving version (not the first one found).
    #[cfg(unix)]
    #[test]
    fn workspace_probe_serves_on_any_project_and_advises_on_the_lowest_version() {
        let workspace = tempfile::tempdir().unwrap();
        let legacy = workspace.path().join("packages/legacy");
        let current = workspace.path().join("packages/current");
        let bare = workspace.path().join("packages/bare");
        write_tsconfig(&legacy);
        write_tsconfig(&current);
        write_tsconfig(&bare);
        link_pnpm_typescript(workspace.path(), &legacy, "5.8.3");
        link_pnpm_typescript(workspace.path(), &current, "6.0.2");

        let probe = probe_workspace_tsserver(&workspace.path().to_string_lossy(), None);

        let servable = probe.servable.as_ref().expect("a servable project exists");
        assert!(
            servable.resolved.default_lib_count > 0,
            "a library-less install is never reported servable"
        );
        // `packages/bare` sorts first and cannot resolve locally; the probe must
        // keep walking rather than reporting the workspace unservable.
        assert_eq!(probe.lowest_servable_version, Some((5, 8)));
        let advisory = probe.advisory().expect("a 5.8 package is advised");
        assert!(
            advisory.contains("5.8"),
            "the advisory names 5.8: {advisory}"
        );
        assert!(probe.native_family_only.is_none());
    }

    /// @ai-generated - A workspace whose ONLY resolvable TypeScript is the TS7+
    /// native family is never served over the Node tsserver protocol.
    #[cfg(unix)]
    #[test]
    fn workspace_probe_reports_native_family_only() {
        let workspace = tempfile::tempdir().unwrap();
        let native = workspace.path().join("packages/native");
        write_tsconfig(&native);
        link_pnpm_typescript(workspace.path(), &native, "7.0.0");

        let probe = probe_project_dirs(&[native.to_string_lossy().into_owned()], None);

        assert!(probe.servable.is_none());
        assert_eq!(probe.native_family_only, Some(7));
    }

    /// @ai-generated - Guards the non-pnpm (plain `node_modules`) layout too.
    #[test]
    fn plain_node_modules_install_resolves_for_its_own_project() {
        let workspace = tempfile::tempdir().unwrap();
        let project = workspace.path().join("packages/plain");
        std::fs::create_dir_all(&project).unwrap();
        let expected = write_typescript(&project, "6.0.2");
        let backend = TsserverEngineBackend::with_default_host_version();

        let spec = engine_spec(&backend, &binding(workspace.path(), &project, 0), None).unwrap();

        assert_eq!(Path::new(&spec.key.tsserver_path), expected);
    }
}
