use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::tsgo::composite::{SharedRendezvous, SharedTsgoOverlay, TsgoCompositeProvider};
use verter_lsp::tsgo::ipc::{TsgoOwnedProvider, TsgoTypeProvider};
use verter_lsp::tsgo::resilient as tsgo_resilient;
use verter_lsp::tsserver::ipc::TsserverTypeProvider;
use verter_lsp::tsserver::resilient as tsserver_resilient;
use verter_lsp::type_provider::lazy_managed::LazyManagedTypeProvider;
use verter_lsp::type_provider::traits::TypeProvider;
use verter_lsp::{LspConfig, ProjectSyncMode, TypeProviderKind};
use verter_session::{HostConfig, VerterHost};

/// Start the server on a thread with an explicitly sized stack.
///
/// `#[tokio::main]` would run the runtime — and therefore `Server::serve`, and
/// therefore every request handler `buffer_unordered` polls inline — on the
/// process main thread, whose stack is whatever the platform's linker chose
/// (1 MiB on Windows/MSVC). `verter_lsp::SERVE_THREAD_STACK_BYTES` documents
/// the measured requirement this replaces that default with.
fn main() {
    verter_lsp::run_on_serve_thread(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("multi-thread tokio runtime must build")
            .block_on(serve());
    });
}

async fn serve() {
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

    // Provider selection: identity-based serving order (editor tsgo → editor
    // tsserver plugin → managed tsgo), with explicit operator overrides.
    let (type_provider, provider_kind, type_provider_reason) =
        create_type_provider(&args, &client_cell, &host).await;

    let config = LspConfig {
        host,
        type_provider,
        project_sync_mode: ProjectSyncMode::FullProject,
        type_provider_kind: provider_kind,
        mcp_port: mcp_actual_port,
        type_provider_reason,
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

    Server::new(stdin, stdout, socket)
        .concurrency_level(verter_lsp::LSP_MAX_CONCURRENCY)
        .serve(service)
        .await;
}

/// Parsed CLI arguments.
struct CliArgs {
    /// Type provider mode: "auto" (default), "tsgo", "shared-tsgo", "tsserver",
    /// "extension", "off". `shared-tsgo` selects editor-first attachment with a
    /// lazy managed fallback; `tsgo` is the explicit eager managed override. An
    /// unrecognized value follows the automatic editor-first order.
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
    /// Project-bound receipt written from inside the editor-owned tsserver plugin.
    editor_tsserver_receipt: Option<String>,
    /// Current-session challenge nonce paired with `editor_tsserver_receipt`.
    editor_tsserver_nonce: Option<String>,
}

impl CliArgs {
    fn parse() -> Self {
        Self::parse_with_defaults(
            std::env::args().skip(1),
            std::env::var("VERTER_SHARED_CONTROL_DIR").ok(),
            std::env::var("VERTER_SHARED_SESSION_KEY").ok(),
            std::env::var("VERTER_EDITOR_TSSERVER_RECEIPT").ok(),
            std::env::var("VERTER_EDITOR_TSSERVER_NONCE").ok(),
        )
    }

    #[cfg(test)]
    fn parse_from(args: impl IntoIterator<Item = String>) -> Self {
        Self::parse_with_defaults(args, None, None, None, None)
    }

    fn parse_with_defaults(
        args: impl IntoIterator<Item = String>,
        mut shared_control_dir: Option<String>,
        mut shared_session_key: Option<String>,
        mut editor_tsserver_receipt: Option<String>,
        mut editor_tsserver_nonce: Option<String>,
    ) -> Self {
        let mut type_provider = "auto".to_string();
        let mut tsdk = None;
        let mut plugin_path = None;
        let mut workspace_root = None;
        let mut mcp_port = None;
        // The SHARED editor-attach rendezvous is opt-in via CLI flag or env — the
        // editor extension supplies it when it spawns a `verter-relay-shim` as its
        // `tsgo`. Absent both, SHARED is never attempted (fail-closed OWNED baseline).
        for arg in args {
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
            } else if let Some(val) = arg.strip_prefix("--editor-tsserver-receipt=") {
                editor_tsserver_receipt = Some(val.to_string());
            } else if let Some(val) = arg.strip_prefix("--editor-tsserver-nonce=") {
                editor_tsserver_nonce = Some(val.to_string());
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
            editor_tsserver_receipt,
            editor_tsserver_nonce,
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

    /// Validate the neutral editor-owned tsserver identity/provenance facts.
    fn editor_tsserver_attestation(
        &self,
    ) -> Result<Option<verter_lsp::editor_tsserver::EditorTsserverAttestation>, String> {
        match (&self.editor_tsserver_receipt, &self.editor_tsserver_nonce) {
            (None, None) => Ok(None),
            (Some(receipt), Some(nonce)) => {
                verter_lsp::editor_tsserver::read_editor_tsserver_attestation(
                    std::path::Path::new(receipt),
                    nonce,
                )
                .map(Some)
            }
            _ => Err(
                "editor tsserver attachment requires both receipt path and session nonce".into(),
            ),
        }
    }
}

/// Whether a configured route may adopt an editor-tsserver plugin attestation.
///
/// EXPLICIT selection only. The editor-owned tsserver tier leaves the LSP with
/// no engine of its own and hands carrier rename to the plugin inside VS Code's
/// tsserver; whether that plugin can serve a workspace depends on tsserver's
/// project topology, which cannot be known before carriers are published. The
/// automatic policy therefore never adopts it, and `tsserver` selects the
/// workspace tsserver its setting description advertises.
pub(crate) fn route_consumes_editor_tsserver_attestation(type_provider: &str) -> bool {
    type_provider == "editor-tsserver"
}

/// Create the type provider based on CLI args.
///
/// Auto mode consumes neutral facts supplied by an editor client: an attested
/// Native Preview rendezvous first, then a project-bound editor-tsserver plugin
/// receipt. Without either fact it constructs a stateful managed TSGO fallback
/// that remains cold until the first connected demand.
///
/// Returns `(provider, kind, reason)` where `reason` preserves
/// selected-route provenance or explains why no provider could be started.
async fn create_type_provider(
    args: &CliArgs,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
    host: &Arc<VerterHost>,
) -> (
    Option<Arc<dyn TypeProvider>>,
    TypeProviderKind,
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
    let editor_tsserver = if route_consumes_editor_tsserver_attestation(&args.type_provider) {
        match args.editor_tsserver_attestation() {
            Ok(attestation) => attestation,
            Err(reason) => {
                tracing::warn!(
                    "editor tsserver attestation rejected; continuing to managed fallback: {reason}"
                );
                None
            }
        }
    } else {
        None
    };

    match args.type_provider.as_str() {
        "off" => {
            tracing::info!("type provider: disabled by --type-provider=off");
            (
                None,
                TypeProviderKind::None,
                Some("Disabled by configuration (--type-provider=off)".into()),
            )
        }
        "shared-tsgo" => {
            let has_editor_rendezvous = args.shared_rendezvous().is_some();
            if !has_editor_rendezvous {
                // No editor rendezvous: the managed fallback is all that is left,
                // so its engine is chosen by capability and a route that cannot
                // obtain one reports None rather than an armed empty shell.
                return managed_fallback_topology(args, client_cell, host, &ws_canonical).await;
            }
            // Editor-owned tsgo is authoritative. The managed provider is represented
            // by a stateful lazy fallback: lifecycle/config updates are cached without
            // spawning, and only an observed shared attach/sync/decision failure on a
            // bound query activates it.
            let provider =
                wrap_shared_first_admission(args, host, &ws_canonical, Arc::clone(client_cell));
            (
                Some(provider),
                TypeProviderKind::Tsgo,
                Some(editor_native_preview_reason()),
            )
        }
        "tsgo" => {
            // Explicit managed-tsgo operator override. This does not arm the editor
            // rendezvous and therefore starts the configured managed provider now.
            match try_spawn_tsgo(&ws_canonical, client_cell).await {
                Ok(owned) => {
                    let tp = wrap_owned_admission(owned, host);
                    (Some(tp), TypeProviderKind::Tsgo, None)
                }
                Err(reason) => {
                    tracing::warn!("TSGO unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, Some(reason))
                }
            }
        }
        "editor-tsserver" => {
            if let Some(attestation) = &editor_tsserver {
                return editor_tsserver_topology(attestation);
            }
            // Explicitly selected, but the editor's plugin never proved it is
            // bound to a project in this workspace. There is no editor engine to
            // defer to, so the managed tier serves rather than reporting a
            // connected editor process that does not exist.
            tracing::warn!(
                "--type-provider=editor-tsserver: no attested editor tsserver plugin; \
                 serving from the managed tier"
            );
            managed_fallback_topology(args, client_cell, host, &ws_canonical).await
        }
        "tsserver" => {
            match try_spawn_tsserver(args, &ws_canonical, client_cell).await {
                Ok(tp) => (Some(tp), TypeProviderKind::Tsserver, None),
                Err(TsserverSpawnError::NativeFamily { major }) => {
                    // A TS7+ install is the native (tsgo) engine family — it is
                    // never served over the Node tsserver protocol. Reclassify
                    // the override to the managed TSGO route.
                    tracing::warn!(
                        "--type-provider=tsserver: workspace TypeScript {major}.x is the \
                         native (TSGO) family; serving via managed TSGO"
                    );
                    match try_spawn_tsgo(&ws_canonical, client_cell).await {
                        Ok(owned) => {
                            let tp = wrap_owned_admission(owned, host);
                            (
                                Some(tp),
                                TypeProviderKind::Tsgo,
                                Some(format!(
                                    "workspace TypeScript {major}.x is the native (TSGO) \
                                     family; tsserver override reclassified to managed TSGO"
                                )),
                            )
                        }
                        Err(reason) => {
                            tracing::warn!(
                                "TSGO unavailable — running in verter-only mode: {reason}"
                            );
                            (None, TypeProviderKind::None, Some(reason))
                        }
                    }
                }
                Err(TsserverSpawnError::Unavailable(reason)) => {
                    tracing::warn!("tsserver unavailable — running in verter-only mode: {reason}");
                    (None, TypeProviderKind::None, Some(reason))
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
                Some("extension-hosted TypeScript language service (Experiment E)".into()),
            )
        }
        _ => {
            // Auto serving order is identity-based: the exact editor tsgo, then
            // the managed workspace engine. Managed fallback is constructed
            // lazily and stays cold until a bound demand observes that the
            // editor route cannot serve it.
            if args.shared_rendezvous().is_some() {
                let provider =
                    wrap_shared_first_admission(args, host, &ws_canonical, Arc::clone(client_cell));
                return (
                    Some(provider),
                    TypeProviderKind::Tsgo,
                    Some(editor_native_preview_reason()),
                );
            }

            // The editor route is unavailable. Tier 2 admits tsgo OR tsserver,
            // and a workspace that can supply neither gets an honest `None`.
            managed_fallback_topology(args, client_cell, host, &ws_canonical).await
        }
    }
}

fn editor_native_preview_reason() -> String {
    "attested editor-owned Native Preview Program; managed TSGO remains cold until an observed attach failure".into()
}

fn lazy_managed_tsgo_reason() -> String {
    "editor attachment unavailable; managed TSGO will start only when a connected demand requires it".into()
}

/// The neutral facts the managed-fallback decision is taken from — gathered by
/// [`probe_managed_engine`] with filesystem lookups only (no process spawn).
#[derive(Debug, Clone)]
struct ManagedEngineFacts {
    /// The bound workspace root (named in every diagnostic).
    workspace_root: String,
    /// Whether ANY configured TypeScript project exists under the root. Both
    /// managed engines are project-bound; without one neither may start.
    has_configured_project: bool,
    /// The first tsgo binary the toolchain resolver's tiers name, if any.
    tsgo_candidate: Option<String>,
    /// Resolver tier notes (stale override, untrusted cache, unsupported host).
    tsgo_notes: Vec<String>,
    /// The workspace/tsdk `tsserver.js`, if any.
    tsserver: Option<String>,
    /// The Node runtime tsserver needs, if any.
    node: Option<String>,
}

/// Which managed engine the workspace can actually supply.
#[derive(Debug, PartialEq, Eq)]
enum ManagedEngineChoice {
    /// A tsgo candidate exists — the preferred engine.
    Tsgo {
        /// Human-readable provenance for the selected engine.
        detail: String,
    },
    /// No tsgo, but a Node-backed tsserver exists — the accepted fallback.
    Tsserver {
        /// Human-readable provenance, including WHY tsgo was not used.
        detail: String,
    },
    /// No engine is obtainable. The provider must be `None`, never a shell
    /// that later reports "connected" with nothing behind it.
    None {
        /// The actionable reason, surfaced in status and logs.
        reason: String,
    },
}

/// Choose the managed engine from neutral facts: tsgo preferred, tsserver
/// accepted, nothing claimed that cannot be obtained.
///
/// A project pinned to TypeScript 5.x has no tsgo anywhere but ships a working
/// tsserver; refusing it produced no semantics at all. Route selection is
/// therefore capability-driven, and the chosen engine plus the reason for it are
/// reported honestly.
fn choose_managed_engine(facts: &ManagedEngineFacts) -> ManagedEngineChoice {
    let notes = |prefix: &str| {
        if facts.tsgo_notes.is_empty() {
            String::new()
        } else {
            format!("{prefix}{}", facts.tsgo_notes.join("; "))
        }
    };

    // Both managed engines are project-bound: neither may open a carrier into a
    // config-less inferred project, so a workspace with no configured project
    // obtains no managed engine at all.
    if !facts.has_configured_project {
        return ManagedEngineChoice::None {
            reason: format!(
                "no configured TypeScript project (tsconfig.json) found anywhere under {} — \
                 the managed tsgo and tsserver engines are both project-bound and will not \
                 start a config-less inferred project",
                facts.workspace_root
            ),
        };
    }

    if let Some(tsgo) = &facts.tsgo_candidate {
        return ManagedEngineChoice::Tsgo {
            detail: format!("managed TSGO resolved to {tsgo}"),
        };
    }

    match (&facts.tsserver, &facts.node) {
        (Some(tsserver), Some(node)) => ManagedEngineChoice::Tsserver {
            detail: format!(
                "no supported tsgo engine is available for {}, falling back to the workspace \
                 tsserver {tsserver} (node: {node}){}",
                facts.workspace_root,
                notes(" — ")
            ),
        },
        (Some(tsserver), None) => ManagedEngineChoice::None {
            reason: format!(
                "no TypeScript engine available for {}: no supported tsgo candidate, and the \
                 workspace tsserver {tsserver} cannot run because Node.js was not found on PATH \
                 or in the standard locations{}",
                facts.workspace_root,
                notes(" — ")
            ),
        },
        (None, _) => ManagedEngineChoice::None {
            reason: format!(
                "no TypeScript engine available for {}: no supported tsgo candidate \
                 ({ENV_OVERRIDE_LABEL}, PATH, project-local node_modules, the update cache, the \
                 bundled sidecar) and no workspace tsserver{}",
                facts.workspace_root,
                notes(" — ")
            ),
        },
    }
}

/// The engine-override variable named in no-engine diagnostics.
const ENV_OVERRIDE_LABEL: &str = verter_tsgo_api::toolchain::discovery::ENV_OVERRIDE_VAR;

/// Whether an enumerated candidate can plausibly BE tsgo, judged without
/// spawning it.
///
/// Enumeration is existence-only, so a TypeScript 5.x workspace contributes its
/// `node_modules/.bin/tsc.cmd` shim as a "tsgo candidate". Treating that as an
/// available engine sends the session down the tsgo route, where it dies at the
/// version gate — the exact "no engine at all" outcome the tsserver fallback
/// exists to prevent. Only the PROJECT-LOCAL tier can produce that confusion:
/// the override, `PATH`, the update cache, and the bundled sidecar are operator-
/// or policy-controlled, so they stay plausible and the real validator decides.
///
/// A project-local candidate is plausible when it sits inside the `@typescript`
/// platform-package layout (which only tsgo publishes), or when the workspace's
/// own TypeScript install is the TS7+ native family (so its `.bin` shim is tsgo's).
fn plausible_tsgo_candidate(
    candidate: &std::path::Path,
    provenance: verter_tsgo_api::toolchain::discovery::Provenance,
    workspace_ts_is_native_family: bool,
) -> bool {
    if provenance != verter_tsgo_api::toolchain::discovery::Provenance::ProjectLocal {
        return true;
    }
    workspace_ts_is_native_family
        || candidate
            .components()
            .any(|component| component.as_os_str() == "@typescript")
}

/// Gather [`ManagedEngineFacts`] for `workspace_root` — filesystem lookups only.
///
/// This is the EAGER availability probe: it decides whether a provider may be
/// installed at all, so a route that cannot obtain an engine reports `None` with
/// its reason instead of an armed shell that would log "connected" with nothing
/// behind it. It never spawns a process, so it does not move the managed
/// engine's cold start onto the startup path.
fn probe_managed_engine(workspace_root: &str, tsdk: Option<&str>) -> ManagedEngineChoice {
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Api,
        Some(std::path::PathBuf::from(workspace_root)),
    );
    let enumeration = verter_tsgo_api::toolchain::discovery::enumerate_candidates(&request);
    let workspace_tsserver = verter_lsp::tsserver::find_tsserver(tsdk, Some(workspace_root));
    // A TS7+ workspace install is the native (tsgo) engine family: it is never
    // served over the Node tsserver protocol, and it is what makes a project-local
    // `.bin` shim a plausible tsgo candidate.
    let workspace_ts_is_native_family = workspace_tsserver.as_deref().is_some_and(|path| {
        verter_type_runtime::discovery::tsserver_native_family_major(path).is_some()
    });
    choose_managed_engine(&ManagedEngineFacts {
        workspace_root: workspace_root.to_string(),
        has_configured_project: verter_workspace::config::has_configured_ts_project_anywhere(
            std::path::Path::new(workspace_root),
        ),
        tsgo_candidate: enumeration
            .candidates
            .iter()
            .find(|c| {
                plausible_tsgo_candidate(&c.path, c.provenance, workspace_ts_is_native_family)
            })
            .map(|c| c.path.to_string_lossy().into_owned()),
        tsgo_notes: enumeration.notes,
        tsserver: workspace_tsserver
            .filter(|_| !workspace_ts_is_native_family)
            .map(|p| p.to_string_lossy().into_owned()),
        node: verter_lsp::tsserver::find_node(),
    })
}

/// The managed-fallback topology for a session with NO editor route.
///
/// The engine is chosen by capability (tsgo preferred, tsserver accepted) and a
/// route that cannot obtain one installs NO provider, so the kind reported to
/// the editor is the engine that will actually serve — never a nominal TSGO in
/// front of a Node tsserver, and never a provider with no engine behind it.
async fn managed_fallback_topology(
    args: &CliArgs,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
    host: &Arc<VerterHost>,
    ws_canonical: &str,
) -> (
    Option<Arc<dyn TypeProvider>>,
    TypeProviderKind,
    Option<String>,
) {
    match probe_managed_engine(ws_canonical, args.tsdk.as_deref()) {
        ManagedEngineChoice::Tsgo { detail } => {
            tracing::info!("managed fallback: {detail}");
            let provider =
                wrap_shared_first_admission(args, host, ws_canonical, Arc::clone(client_cell));
            (
                Some(provider),
                TypeProviderKind::Tsgo,
                Some(format!("{}; {detail}", lazy_managed_tsgo_reason())),
            )
        }
        ManagedEngineChoice::Tsserver { detail } => {
            tracing::info!("managed fallback: {detail}");
            match try_spawn_tsserver(args, ws_canonical, client_cell).await {
                Ok(tp) => (Some(tp), TypeProviderKind::Tsserver, Some(detail)),
                Err(TsserverSpawnError::NativeFamily { major }) => {
                    let reason = format!(
                        "workspace TypeScript {major}.x is the native (TSGO) family but no \
                         supported tsgo engine could be resolved — no TypeScript intellisense"
                    );
                    tracing::warn!("{reason}");
                    (None, TypeProviderKind::None, Some(reason))
                }
                Err(TsserverSpawnError::Unavailable(error)) => {
                    let reason =
                        format!("no supported tsgo engine, and tsserver failed to start: {error}");
                    tracing::warn!("{reason}");
                    (None, TypeProviderKind::None, Some(reason))
                }
            }
        }
        ManagedEngineChoice::None { reason } => {
            tracing::warn!("no TypeScript type provider — running in verter-only mode: {reason}");
            (None, TypeProviderKind::None, Some(reason))
        }
    }
}

fn editor_tsserver_topology(
    attestation: &verter_lsp::editor_tsserver::EditorTsserverAttestation,
) -> (
    Option<Arc<dyn TypeProvider>>,
    TypeProviderKind,
    Option<String>,
) {
    tracing::info!(
        "using attested editor-owned tsserver plugin: pid={} projects={:?}",
        attestation.pid,
        attestation.projects
    );
    (
        None,
        TypeProviderKind::EditorTsserver,
        Some(format!(
            "attested editor tsserver process {} across {} project(s)",
            attestation.pid,
            attestation.projects.len()
        )),
    )
}

/// Try to spawn the OWNED, project-bound dual-surface TSGO provider.
///
/// Owned tsgo is PROJECT-BOUND: it requires AT LEAST ONE configured project anywhere
/// in the workspace (a bounded [`has_configured_ts_project_anywhere`][hcp] probe that
/// accepts `packages/*/tsconfig.json` monorepos, not just a root `tsconfig.json`) plus
/// a version-gated `--api` attach BEFORE serving any traffic, and FAILS CLOSED
/// otherwise (no spawn) — never a config-less inferred project. The carrier's OWN
/// owning configured project is resolved PER QUERY by the shared project-binding
/// helper (the always-present host-aware admission layer), so the `--api` checker
/// stores no tsconfig. The standard LSP handshake (`rootUri`) is transport metadata
/// only.
///
/// [hcp]: verter_workspace::config::has_configured_ts_project_anywhere
async fn try_spawn_tsgo(
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, String> {
    // The SPAWN PRECONDITION, checked FIRST: owned tsgo is project-bound, so
    // require AT LEAST ONE configured project ANYWHERE under the workspace
    // (bounded — prunes node_modules; accepts `packages/*/tsconfig.json`
    // monorepos with no root tsconfig) BEFORE the resolver spawns or smokes
    // ANY candidate, so a config-less workspace fails closed with zero
    // candidate processes (a config-less workspace ⇒ no spawn). The per-query
    // per-project binding is resolved later by the admission layer.
    if !verter_workspace::config::has_configured_ts_project_anywhere(std::path::Path::new(
        workspace_root,
    )) {
        return Err(format!(
            "no configured TypeScript project (tsconfig.json) found anywhere under \
             {workspace_root} — owned tsgo is project-bound and requires at least one configured \
             project; it will not start a config-less inferred project"
        ));
    }

    // The 4-tier toolchain resolver (`verter_tsgo_api::toolchain`): shared
    // (`VERTER_TSGO_BIN`, then PATH) → project-local ancestor `node_modules` →
    // temp update cache → bundled sidecar; the first WORKING candidate wins
    // (bounded version probe + support policy + a `--lsp`/`--api` capability
    // smoke per candidate). A resolution failure is actionable (every
    // rejection is listed); an existing-but-invalid bundled sidecar is a
    // product-integrity error.
    let request = verter_tsgo_api::toolchain::discovery::ResolutionRequest::for_environment(
        verter_tsgo_api::toolchain::validation::Capability::Api,
        Some(std::path::PathBuf::from(workspace_root)),
    );
    let resolution = verter_tsgo_api::toolchain::discovery::resolve(&request)
        .await
        .map_err(|err| err.to_string())?;
    let tsgo_bin = resolution.path.to_string_lossy().into_owned();
    tracing::info!(
        "resolved tsgo binary: {tsgo_bin} ({} from {})",
        resolution.version,
        resolution.provenance,
    );

    // `root_uri` is the LSP transport's workspace-folder metadata only — NOT the
    // project-binding decision (that is resolved per query by the admission layer).
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
    // THIS `tsgo --lsp` process. The checker stores NO configured project — each
    // carrier's owning tsconfig is supplied per query (the binding the admission
    // layer resolves), so ONE process serves every configured project. The `--lsp`
    // surface serves features + the user-facing diagnostics (gated on the carrier's
    // resolved `BoundProject` by the admission layer). A probe / wire-gate / attach
    // failure fails closed rather than silently degrading the typecheck oracle.
    let lsp = Arc::new(tp);
    let owned = match TsgoOwnedProvider::attach(Arc::clone(&lsp), &tsgo_bin).await {
        Ok(owned) => owned,
        Err(error) => {
            let teardown = lsp.shutdown().await;
            return Err(format!(
                "found tsgo at {tsgo_bin} and spawned --lsp, but the version-gated --api \
                 attach failed: {error}; managed child teardown: {}",
                teardown
                    .err()
                    .map_or_else(|| "reaped".to_string(), |error| error.to_string())
            ));
        }
    };
    tracing::info!("TSGO owned dual-surface provider started (--api attached, resilient)");

    let resilient = tsgo_resilient::new_owned(
        owned,
        crash_notify,
        tsgo_bin,
        root_uri,
        Arc::clone(client_cell),
        3,
    );
    Ok(Arc::new(resilient))
}

/// Wrap the OWNED tsgo provider in the ALWAYS-present host-aware admission /
/// composition layer ([`TsgoCompositeProvider`]) — never a bare OWNED provider.
///
/// The layer ALWAYS holds OWNED + the host, so the OWNED carrier-diagnostics gate can
/// resolve each carrier's owning configured project over the host's published snapshot
/// (a non-bound carrier fails closed to no external-TS diagnostics, never a `tsgo --lsp`
/// inferred fall-through). The SHARED editor-attach overlay is OPTIONAL — present only
/// under the rendezvous evidence, additive + fail-closed (it never displaces OWNED and
/// never bypasses the OWNED gate).
fn wrap_owned_admission(
    owned: Arc<dyn TypeProvider>,
    host: &Arc<VerterHost>,
) -> Arc<dyn TypeProvider> {
    Arc::new(TsgoCompositeProvider::new(owned, Arc::clone(host), None)) as Arc<dyn TypeProvider>
}

/// Build the shared-first provider without starting a managed process. The lazy
/// fallback records every lifecycle/configuration update and invokes `try_spawn_tsgo`
/// at most once, only after the composite has observed that the editor route cannot
/// serve a bound demand.
fn wrap_shared_first_admission(
    args: &CliArgs,
    host: &Arc<VerterHost>,
    workspace_root: &str,
    client_cell: Arc<OnceCell<tower_lsp_server::Client>>,
) -> Arc<dyn TypeProvider> {
    let workspace_root_owned = workspace_root.to_string();
    let fallback = Arc::new(LazyManagedTypeProvider::new(move || {
        let workspace_root = workspace_root_owned.clone();
        let client_cell = Arc::clone(&client_cell);
        async move {
            try_spawn_tsgo(&workspace_root, &client_cell)
                .await
                .map_err(verter_lsp::type_provider::protocol::TypeProviderError::new)
        }
    })) as Arc<dyn TypeProvider>;
    let shared = try_attach_shared_tsgo(args, host, workspace_root);
    Arc::new(TsgoCompositeProvider::new(
        fallback,
        Arc::clone(host),
        shared,
    )) as Arc<dyn TypeProvider>
}

/// Build the OPTIONAL SHARED editor-attach [`SharedTsgoOverlay`] when the rendezvous
/// evidence is present; `None` otherwise (SHARED is opt-in).
///
/// The overlay binds LAZILY per query: for a carrier ALREADY resolved to a bound
/// configured project by the admission layer's OWNED gate, it discovers the
/// editor-spawned `verter-relay-shim` advertisement, gates the observed engine version
/// (keyed on the actual attach-gate version, never a hardcoded literal), and overlays
/// the SHARED `--api` carrier diagnostics OVER OWNED through the SAME already-resolved
/// binding — falling back to the OWNED baseline for every non-SHARED / failed state.
fn try_attach_shared_tsgo(
    args: &CliArgs,
    host: &Arc<VerterHost>,
    workspace_root: &str,
) -> Option<SharedTsgoOverlay> {
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
        "TSGO SHARED editor-attach overlay armed (control_dir={control_dir}, \
         session_key={session_key}); the overlay binds lazily per query over the \
         published snapshot and fails closed to the OWNED baseline"
    );
    Some(overlay)
}

/// Why the tsserver route could not produce a tsserver provider.
#[derive(Debug)]
enum TsserverSpawnError {
    /// The resolved candidate belongs to a TS7+ install — the native (tsgo)
    /// engine family. It must classify as tsgo for serving-order purposes,
    /// never spawn under the Node tsserver protocol.
    NativeFamily { major: u32 },
    /// tsserver genuinely unavailable (missing node/TypeScript, spawn failure).
    Unavailable(String),
}

/// Try to spawn tsserver.
///
/// The native-family gate runs BEFORE any binary lookup or process spawn: a
/// TS7+ (tsgo-family) candidate returns [`TsserverSpawnError::NativeFamily`]
/// without touching node.
async fn try_spawn_tsserver(
    args: &CliArgs,
    workspace_root: &str,
    client_cell: &Arc<OnceCell<tower_lsp_server::Client>>,
) -> Result<Arc<dyn TypeProvider>, TsserverSpawnError> {
    let tsserver_path =
        verter_lsp::tsserver::find_tsserver(args.tsdk.as_deref(), Some(workspace_root))
            .ok_or_else(|| {
                TsserverSpawnError::Unavailable("TypeScript not installed in workspace".to_string())
            })?;
    if let Some(major) =
        verter_type_runtime::discovery::tsserver_native_family_major(&tsserver_path)
    {
        return Err(TsserverSpawnError::NativeFamily { major });
    }
    let node_path = verter_lsp::tsserver::find_node().ok_or_else(|| {
        TsserverSpawnError::Unavailable("Node.js not found on PATH or standard locations".into())
    })?;

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
        Err(e) => Err(TsserverSpawnError::Unavailable(format!(
            "found tsserver at {} (node: {node_path}), but spawn/initialize failed: {e}",
            tsserver_path.display()
        ))),
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

#[cfg(test)]
#[path = "main_tests.rs"]
mod tests;
