use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::tsgo::composite::{SharedRendezvous, SharedTsgoOverlay, TsgoCompositeProvider};
use verter_lsp::tsgo::ipc::{find_tsgo_binary_canonical, TsgoOwnedProvider, TsgoTypeProvider};
use verter_lsp::tsgo::resilient as tsgo_resilient;
use verter_lsp::tsserver::ipc::TsserverTypeProvider;
use verter_lsp::tsserver::resilient as tsserver_resilient;
use verter_lsp::type_provider::traits::TypeProvider;
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

#[tokio::main]
async fn main() {
    // Initialize tracing (controlled via VERTER_LOG or RUST_LOG env var).
    // ANSI colors are disabled because the output goes to VS Code's debug
    // console which renders escape codes as literal text (e.g. `[2m`, `[0m`).
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_env("VERTER_LOG")
                .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
                .unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_ansi(false)
        .with_writer(std::io::stderr)
        .init();

    tracing::info!(
        "verter-lsp v{} ({}, built {})",
        env!("CARGO_PKG_VERSION"),
        if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
        env!("VERTER_BUILD_DATE"),
    );

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let host = Arc::new(VerterHost::new_standalone(HostConfig {
        analysis_level: verter_session::AnalysisLevel::Full,
        ..HostConfig::default()
    }));

    // Parse CLI arguments
    let args = CliArgs::parse();

    // The LSP binary no longer hosts the MCP HTTP server in-process.
    // `verter_lsp` and `verter_mcp` ship as independent products; IDEs
    // that previously consumed MCP via `--mcp-port` must spawn the
    // standalone `verter_mcp_server` binary in its own process.
    // The `--mcp-port` CLI flag is still parsed so existing IDE
    // configurations remain syntactically valid, but supplying it
    // now triggers a guidance log only.
    let mcp_actual_port: Option<u16> = {
        if args.mcp_port.is_some() {
            tracing::warn!(
                "--mcp-port is no longer honoured by verter-lsp. \
                 The LSP binary no longer embeds the MCP server. \
                 Spawn `verter-mcp-server --transport http --port <port>` \
                 in its own process to expose the HTTP MCP transport."
            );
        }
        None
    };

    // Deferred client cell — populated inside LspService::build, used by
    // ResilientTypeProvider's crash monitor to send user notifications.
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());

    // Provider selection: auto → detect TS version, pick provider
    let (type_provider, provider_kind, suggest_tsgo, type_provider_none_reason) =
        create_type_provider(&args, &client_cell, &host).await;

    let config = LspConfig {
        host,
        type_provider,
        project_sync_mode: ProjectSyncMode::FullProject,
        type_provider_kind: provider_kind,
        suggest_tsgo,
        mcp_port: mcp_actual_port,
        type_provider_none_reason,
        // Production keeps the imported-carrier prewarm (test-only suppression seam).
        suppress_imported_carrier_prewarm: false,
    };

    let client_cell_for_build = Arc::clone(&client_cell);
    let (service, socket) = LspService::build(|client| {
        // Populate the deferred client cell so the crash monitor can send notifications.
        let _ = client_cell_for_build.set(client.clone());
        VerterLanguageServer::new(client, config)
    })
    .custom_method(
        "$/onDidChangeTsOrJsFile",
        VerterLanguageServer::on_did_change_ts_or_js_file,
    )
    .custom_method("$/onFileChanged", VerterLanguageServer::on_file_changed)
    .custom_method(
        "$/verter/watcherStateChanged",
        VerterLanguageServer::on_watcher_state_changed,
    )
    .custom_method("$/getCompiledCode", VerterLanguageServer::get_compiled_code)
    .custom_method(
        "$/verter/getStatistics",
        VerterLanguageServer::get_statistics,
    )
    .custom_method(
        "$/verter/getVirtualFiles",
        VerterLanguageServer::get_virtual_files,
    )
    .custom_method("$/verter/getAnalysis", VerterLanguageServer::get_analysis)
    .custom_method(
        "$/verter/getProjectOverview",
        VerterLanguageServer::get_project_overview,
    )
    .custom_method(
        "$/verter/getBindingTypes",
        VerterLanguageServer::get_binding_types,
    )
    .custom_method(
        "$/verter/getComponentParents",
        VerterLanguageServer::get_component_parents,
    )
    .custom_method(
        "$/verter/documentDropEdit",
        VerterLanguageServer::document_drop_edit,
    )
    .custom_method(
        "$/verter/applyStyleOverrides",
        VerterLanguageServer::apply_style_overrides,
    )
    .custom_method(
        "$/verter/getRouteTree",
        VerterLanguageServer::get_route_tree,
    )
    .custom_method(
        "$/verter/getComponentMeta",
        VerterLanguageServer::get_component_meta,
    )
    .custom_method(
        "$/verter/getComponentMetaSurface",
        VerterLanguageServer::get_component_meta_surface,
    )
    .custom_method(
        "$/verter/getComponentMetaTypeExpansion",
        VerterLanguageServer::get_component_meta_type_expansion,
    )
    .custom_method(
        "$/verter/audit/getRecord",
        VerterLanguageServer::get_audit_record,
    )
    .custom_method(
        "$/verter/audit/getRecent",
        VerterLanguageServer::get_audit_recent,
    )
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
}

/// Parsed CLI arguments.
struct CliArgs {
    /// Type provider mode: "auto" (default), "tsgo", "shared-tsgo", "tsserver",
    /// "extension", "off". "tsgo" and "shared-tsgo" are the SAME routing (SHARED
    /// editor-attach is additive + opt-in on top of the OWNED tsgo baseline); an
    /// unrecognized value falls through to "auto".
    type_provider: String,
    /// Path to TypeScript SDK directory (tsserver.js location).
    tsdk: Option<String>,
    /// Path to the directory containing `@verter/typescript-plugin`.
    plugin_path: Option<String>,
    /// Workspace root (positional argument).
    workspace_root: Option<String>,
    /// MCP HTTP port. Parsed for syntactic compatibility with existing
    /// IDE configurations only — the LSP binary no longer embeds the
    /// MCP server. Supplying this flag now logs a notice that points
    /// consumers at `verter-mcp-server`.
    mcp_port: Option<u16>,
    /// The rendezvous control directory an editor-spawned `verter-relay-shim`
    /// advertised into (`--shared-control-dir`, or `VERTER_SHARED_CONTROL_DIR`).
    /// Present ⇒ SHARED editor-attach is opt-in-eligible; absent ⇒ SHARED is never
    /// attempted (fail-closed, OWNED baseline).
    shared_control_dir: Option<String>,
    /// The shim `--session-key` to discover the advertisement under
    /// (`--shared-session-key`, or `VERTER_SHARED_SESSION_KEY`).
    shared_session_key: Option<String>,
}

impl CliArgs {
    fn parse() -> Self {
        let mut type_provider = "auto".to_string();
        let mut tsdk = None;
        let mut plugin_path = None;
        let mut workspace_root = None;
        let mut mcp_port = None;
        // The SHARED editor-attach rendezvous is opt-in via CLI flag or env — the
        // editor extension supplies it when it spawns a `verter-relay-shim` as its
        // `tsgo`. Absent both, SHARED is never attempted (fail-closed OWNED baseline).
        let mut shared_control_dir = std::env::var("VERTER_SHARED_CONTROL_DIR").ok();
        let mut shared_session_key = std::env::var("VERTER_SHARED_SESSION_KEY").ok();

        for arg in std::env::args().skip(1) {
            if let Some(val) = arg.strip_prefix("--type-provider=") {
                type_provider = val.to_string();
            } else if let Some(val) = arg.strip_prefix("--tsdk=") {
                tsdk = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--plugin-path=") {
                plugin_path = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--shared-control-dir=") {
                shared_control_dir = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--shared-session-key=") {
                shared_session_key = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--mcp-port=") {
                mcp_port = val.parse().ok();
            } else if arg.starts_with("--mcp-lint-preset=") {
                // Accepted for syntactic compatibility with IDE
                // configurations that previously bundled MCP into the
                // LSP binary. Now that MCP ships separately via
                // `verter-mcp-server`, the lint preset is configured
                // on that binary's CLI; the LSP simply ignores the flag.
                continue;
            } else if !arg.starts_with("--") {
                workspace_root = Some(arg);
            }
        }

        Self {
            type_provider,
            tsdk,
            plugin_path,
            workspace_root,
            mcp_port,
            shared_control_dir,
            shared_session_key,
        }
    }

    /// The SHARED editor-attach rendezvous `(control_dir, session_key)`, when BOTH
    /// are supplied — the opt-in evidence that a `verter-relay-shim` may be
    /// discoverable. Absent either, SHARED is never attempted.
    fn shared_rendezvous(&self) -> Option<(&str, &str)> {
        match (&self.shared_control_dir, &self.shared_session_key) {
            (Some(dir), Some(key)) => Some((dir.as_str(), key.as_str())),
            _ => None,
        }
    }
}

/// Create the type provider based on CLI args.
///
/// Auto mode: a workspace whose active TypeScript engine is tsgo/native-preview
/// (TS >= 7) uses the tsgo external engine; otherwise a resolved TS 5.x/6.x
/// tsserver candidate or a composite `tsconfig` selects tsserver, falling back
/// to tsgo.
///
/// Returns `(provider, kind, suggest_tsgo, none_reason)` where `none_reason`
/// explains why no provider could be started (only set when provider is None).
async fn create_type_provider(
    args: &CliArgs,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
    host: &Arc<VerterHost>,
) -> (
    Option<Arc<dyn TypeProvider>>,
    TypeProviderKind,
    bool,
    Option<String>,
) {
    tracing::info!(
        "create_type_provider: type_provider={:?}, tsdk={:?}, workspace_root={:?}",
        args.type_provider,
        args.tsdk,
        args.workspace_root
    );
    let workspace_root = args
        .workspace_root
        .clone()
        .or_else(|| {
            std::env::current_dir()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
        })
        .unwrap_or_else(|| ".".to_string());
    let ws_canonical = verter_span::path::canonicalize_path(&workspace_root);

    match args.type_provider.as_str() {
        "off" => {
            tracing::info!("type provider: disabled by --type-provider=off");
            (
                None,
                TypeProviderKind::None,
                false,
                Some("Disabled by configuration (--type-provider=off)".into()),
            )
        }
        "shared-tsgo" | "tsgo" => {
            // OWNED is the universal baseline — built first, always. SHARED
            // editor-attach is additive + opt-in + fail-closed: when the rendezvous
            // evidence is present, the OWNED provider is WRAPPED in a
            // `TsgoCompositeProvider` whose SHARED overlay binds lazily per query
            // over the published snapshot and overlays ONLY bound carrier
            // diagnostics — it NEVER displaces the OWNED feature/diagnostics
            // surface. Absent the rendezvous, the bare OWNED provider is used.
            // `tsgo` and `shared-tsgo` are the same routing with a clearer name.
            match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Ok(owned) => {
                    let tp = wrap_shared_if_opted_in(owned, args, host, &ws_canonical);
                    (Some(tp), TypeProviderKind::Tsgo, false, None)
                }
                Err(reason) => {
                    tracing::warn!("TSGO unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, false, Some(reason))
                }
            }
        }
        "tsserver" => {
            // tsserver only — no fallback
            match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                Ok(tp) => (Some(tp), TypeProviderKind::Tsserver, false, None),
                Err(reason) => {
                    tracing::warn!("tsserver unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, false, Some(reason))
                }
            }
        }
        "extension" => {
            // Experiment E: extension-hosted TypeScript language service.
            // The extension runs ts.createLanguageService() in-process and
            // responds to $/verter/tsQuery requests over the existing LSP pipe.
            tracing::info!("type provider: extension-hosted (Experiment E)");
            let provider = verter_lsp::extension_provider::ExtensionTypeProvider::new(
                Arc::clone(client_cell),
                &ws_canonical,
            );
            (
                Some(Arc::new(provider) as Arc<dyn TypeProvider>),
                TypeProviderKind::Tsserver,
                false,
                None,
            )
        }
        _ => {
            // "auto" (default): the decision keys on the WORKSPACE's active
            // TypeScript engine, not on whatever find_tsserver resolves (which for
            // a tsgo/TS7 workspace with no workspace tsserver.js would be the
            // editor's bundled/global lower tsserver). A tsgo (TS7) workspace is
            // served by the tsgo external engine; otherwise a TS 5.x/6.x tsserver
            // or a composite tsconfig prefers tsserver (TSGO cannot resolve path
            // aliases from referenced configs).
            let mut tsserver_reason = None;

            let composite_ws = verter_workspace::FilesystemWorkspace::new(
                verter_workspace::FilesystemOptions::default(),
            );
            let has_composite =
                verter_lsp::config::has_solution_style_tsconfig(&composite_ws, &ws_canonical);
            if has_composite {
                tracing::info!(
                    "auto mode: solution-style tsconfig detected at {} \
                     (TSGO cannot resolve path aliases from referenced configs)",
                    ws_canonical
                );
            }

            // The active workspace TypeScript engine (owner: verter_workspace's
            // intrinsic-library discovery) — a tsgo/TS7 workspace routes to the
            // tsgo external engine regardless of any editor-supplied tsserver.
            let workspace_ts = verter_workspace::NativeIntrinsicLibrary::discover(
                std::path::Path::new(&ws_canonical),
            );
            let workspace_tsgo = workspace_ts.active_typescript_is_tsgo();

            // A tsgo workspace is served by tsgo — never resolve a lower tsserver
            // launch path for it.
            let tsserver_path = if workspace_tsgo {
                None
            } else {
                verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(&ws_canonical))
            };
            let tsserver_major = tsserver_path
                .as_deref()
                .and_then(verter_lsp::tsserver::detect_ts_major_version);

            tracing::info!(
                "auto mode: workspace_tsgo={} tsserver={} has_composite={}",
                workspace_tsgo,
                tsserver_major.map_or("none".to_string(), |v| format!("{v}.x")),
                has_composite
            );

            if verter_lsp::config::prefer_tsserver_backend(
                workspace_tsgo,
                tsserver_major,
                has_composite,
            ) {
                match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                    Ok(tp) => return (Some(tp), TypeProviderKind::Tsserver, false, None),
                    Err(reason) => {
                        tracing::warn!("auto mode: tsserver unavailable: {reason}");
                        tsserver_reason = Some(reason);
                    }
                }
            }

            // No tsserver available or not preferred — build the OWNED TSGO
            // baseline, then WRAP it in the SHARED-overlay composite when the
            // rendezvous evidence is present (opt-in; the overlay binds lazily per
            // query and never displaces the OWNED baseline).
            let tsgo_reason = match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Ok(owned) => {
                    let tp = wrap_shared_if_opted_in(owned, args, host, &ws_canonical);
                    return (Some(tp), TypeProviderKind::Tsgo, false, None);
                }
                Err(reason) => {
                    tracing::warn!("auto mode: tsgo unavailable: {reason}");
                    reason
                }
            };

            let reason = if let Some(tsserver_reason) = tsserver_reason {
                format!("tsserver unavailable: {tsserver_reason}; tsgo unavailable: {tsgo_reason}")
            } else {
                format!("tsgo unavailable: {tsgo_reason}")
            };
            tracing::info!("no type provider available — running in verter-only mode ({reason})");
            (None, TypeProviderKind::None, false, Some(reason))
        }
    }
}

/// Resolve the EXPLICIT configured tsconfig binding for an owned tsgo workspace.
///
/// Owned tsgo is PROJECT-BOUND: the `--api` checker requires a real configured
/// project, so this resolves the workspace's explicit `tsconfig.json` and returns
/// its forward-slashed path. A workspace WITHOUT an explicit binding (no
/// `tsconfig.json`) returns `Err` so the owned startup FAILS CLOSED — there is no
/// config-less / inferred-project owned fallback.
fn require_owned_tsconfig(workspace_root: &std::path::Path) -> Result<String, String> {
    let tsconfig = workspace_root.join("tsconfig.json");
    // `is_file()` (not `exists()`): a DIRECTORY named `tsconfig.json` is not a
    // valid configured-project binding and must fail closed, not satisfy the
    // owned-startup precondition.
    if tsconfig.is_file() {
        Ok(tsconfig.to_string_lossy().replace('\\', "/"))
    } else {
        Err(format!(
            "no tsconfig.json file at {} — owned tsgo is project-bound and requires an explicit \
             configured project; it will not start a config-less inferred project",
            workspace_root.display()
        ))
    }
}

/// Try to spawn the OWNED, project-bound dual-surface TSGO provider.
///
/// Owned tsgo is PROJECT-BOUND: it requires an explicit configured tsconfig
/// binding (via [`require_owned_tsconfig`]) and a version-gated `--api` attach
/// BEFORE serving any traffic, and FAILS CLOSED otherwise. The standard LSP
/// handshake (`rootUri`) is retained as transport metadata only.
async fn try_spawn_tsgo(
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, String> {
    // Canonical discovery: explicit `VERTER_TSGO_BIN` override > workspace
    // `node_modules` (the common real-project case a bare PATH/cache search
    // misses) > PATH > npm/npx cache.
    let tsgo_bin = find_tsgo_binary_canonical(Some(std::path::Path::new(workspace_root)))
        .map_err(|err| err.to_string())?;
    tracing::info!("found tsgo binary: {tsgo_bin}");

    // The configured project is mandatory for owned tsgo — resolve it (fail closed
    // when absent) BEFORE spawning, so a config-less workspace never starts a
    // `tsgo --lsp` process it would have to tear down.
    let tsconfig_str = require_owned_tsconfig(std::path::Path::new(workspace_root))?;

    // `root_uri` is the LSP transport's workspace-folder metadata only — NOT the
    // project-binding decision (that is `tsconfig_str` above).
    let root_uri = path_to_file_uri(workspace_root);
    let crash_notify = Arc::new(Notify::new());

    let tp = TsgoTypeProvider::spawn_with_crash_signal(
        &tsgo_bin,
        &root_uri,
        Some(Arc::clone(&crash_notify)),
    )
    .await
    .map_err(|e| format!("found tsgo at {tsgo_bin}, but spawn/initialize failed: {e}"))?;

    // OWNED one-instance dual-surface: attach a version-gated `--api` checker to
    // THIS `tsgo --lsp` process and open the configured project on it (the carrier
    // becomes a member of its real tsconfig — the project-bound membership). The
    // `--api` checker is the project-bound typecheck oracle; the `--lsp` surface
    // serves features + the user-facing diagnostics. A probe / wire-gate / attach
    // failure fails closed rather than silently degrading the typecheck oracle.
    let owned = TsgoOwnedProvider::attach(Arc::new(tp), tsconfig_str.clone(), &tsgo_bin)
        .await
        .map_err(|e| {
            format!(
                "found tsgo at {tsgo_bin} and spawned --lsp, but the version-gated --api \
                 attach failed: {e}"
            )
        })?;
    tracing::info!("TSGO owned dual-surface provider started (--api attached, resilient)");

    let resilient = tsgo_resilient::new_owned(
        owned,
        crash_notify,
        tsgo_bin,
        root_uri,
        tsconfig_str,
        Arc::clone(client_cell),
        3,
    );
    Ok(Arc::new(resilient))
}

/// Wrap the OWNED tsgo provider in a SHARED-overlay [`TsgoCompositeProvider`] when
/// the editor-attach rendezvous evidence is present; otherwise return the bare OWNED
/// provider. SHARED is opt-in and additive — the composite delegates every feature +
/// the diagnostics fallback to OWNED and overlays ONLY successfully-bound SHARED
/// carrier diagnostics, so OWNED is never displaced.
fn wrap_shared_if_opted_in(
    owned: Arc<dyn TypeProvider>,
    args: &CliArgs,
    host: &Arc<VerterHost>,
    workspace_root: &str,
) -> Arc<dyn TypeProvider> {
    match try_attach_shared_tsgo(Arc::clone(&owned), args, host, workspace_root) {
        Some(composite) => composite,
        None => owned,
    }
}

/// Build the SHARED-overlay composite over the OWNED provider (sibling to
/// [`try_spawn_tsgo`]) when the rendezvous evidence is present.
///
/// The composite's [`SharedTsgoOverlay`] binds LAZILY per query: on a carrier
/// diagnostics query it resolves the carrier's owning project through the shared
/// `WorkspaceProjectResolver` over the host's LIVE published snapshot, mints the
/// `BoundProject` witness from the resolved binding, discovers the editor-spawned
/// `verter-relay-shim` advertisement, gates the observed engine version (keyed on the
/// actual attach-gate version, never a hardcoded literal), and overlays the SHARED
/// `--api` carrier diagnostics — falling back to the OWNED baseline for every
/// non-bound / non-SHARED / failed state. Returns `None` when the rendezvous is absent
/// (SHARED is opt-in) so the caller uses the bare OWNED provider.
fn try_attach_shared_tsgo(
    owned: Arc<dyn TypeProvider>,
    args: &CliArgs,
    host: &Arc<VerterHost>,
    workspace_root: &str,
) -> Option<Arc<dyn TypeProvider>> {
    let (control_dir, session_key) = args.shared_rendezvous()?;
    let overlay = SharedTsgoOverlay::new(
        Arc::clone(host),
        SharedRendezvous {
            control_dir: std::path::PathBuf::from(control_dir),
            session_key: session_key.to_string(),
            workspace_root: workspace_root.to_string(),
        },
    );
    tracing::info!(
        "TSGO SHARED editor-attach composite armed (control_dir={control_dir}, \
         session_key={session_key}); the overlay binds lazily per query over the \
         published snapshot and fails closed to the OWNED baseline"
    );
    Some(Arc::new(TsgoCompositeProvider::new(owned, overlay)) as Arc<dyn TypeProvider>)
}

/// Try to spawn tsserver.
async fn try_spawn_tsserver(
    args: &CliArgs,
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, String> {
    let node_path = verter_lsp::tsserver::find_node()
        .ok_or_else(|| "Node.js not found on PATH or standard locations".to_string())?;
    let tsserver_path =
        verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(workspace_root))
            .ok_or_else(|| "TypeScript not installed in workspace".to_string())?;

    tracing::info!(
        "found tsserver: {} (node: {})",
        tsserver_path.display(),
        node_path
    );

    let crash_notify = Arc::new(Notify::new());
    let tsserver_str = tsserver_path.to_string_lossy().to_string();

    // The carrier-publish store dir the LSP publishes carriers into — delivered to
    // the plugin so it reads the SAME store the publish path writes. Derived from
    // the workspace root through the shared store-dir resolver, identical to the
    // dir `TsserverEngineBackend` computes on the publish side.
    let carrier_store_dir =
        verter_lsp::external_ts::default_carrier_store_dir_string(workspace_root);

    match TsserverTypeProvider::spawn(
        &node_path,
        &tsserver_str,
        workspace_root,
        args.plugin_path.as_deref(),
        Some(&carrier_store_dir),
        // verter_lsp-internal backend: the Rust merge layer is the sole
        // companion→source response mapper, so the plugin returns RAW responses.
        false,
        Some(Arc::clone(&crash_notify)),
    )
    .await
    {
        Ok(tp) => {
            tracing::info!("tsserver type provider started (resilient mode)");
            let resilient = tsserver_resilient::new(
                tp,
                crash_notify,
                node_path,
                tsserver_str,
                workspace_root.to_string(),
                args.plugin_path.clone(),
                Arc::clone(client_cell),
                3,
            );
            Ok(Arc::new(resilient))
        }
        Err(e) => Err(format!(
            "found tsserver at {} (node: {node_path}), but spawn/initialize failed: {e}",
            tsserver_path.display()
        )),
    }
}

/// Convert a file path to a `file://` URI.
fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        // Windows: C:/Users/... → file:///C:/Users/...
        format!("file:///{normalized}")
    }
}
