use std::sync::Arc;

use tokio::sync::{Notify, OnceCell};
use tower_lsp_server::{LspService, Server};
use tracing_subscriber::EnvFilter;
use verter_host::{HostConfig, VerterHost};
use verter_lsp::server::VerterLanguageServer;
use verter_lsp::tsgo::ipc::{find_tsgo_binary, TsgoTypeProvider};
use verter_lsp::tsgo::resilient::ResilientTypeProvider;
use verter_lsp::tsgo::traits::TypeProvider;
use verter_lsp::{LspConfig, ProjectSyncMode};

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

    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let host = VerterHost::new(HostConfig {
        analysis_level: verter_host::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    // Deferred client cell — populated inside LspService::build, used by
    // ResilientTypeProvider's crash monitor to send user notifications.
    let client_cell: Arc<OnceCell<tower_lsp_server::Client>> = Arc::new(OnceCell::new());

    // Try to find and spawn TSGO for TypeScript type checking.
    let type_provider: Option<Arc<dyn TypeProvider>> = match find_tsgo_binary() {
        Some(tsgo_bin) => {
            tracing::info!("found tsgo binary: {tsgo_bin}");

            // Derive workspace root from CLI arg or current directory.
            let workspace_root = std::env::args()
                .nth(1)
                .or_else(|| {
                    std::env::current_dir()
                        .ok()
                        .map(|p| p.to_string_lossy().to_string())
                })
                .unwrap_or_else(|| ".".to_string());

            let root_uri = path_to_file_uri(&workspace_root);

            let crash_notify = Arc::new(Notify::new());
            match TsgoTypeProvider::spawn_with_crash_signal(
                &tsgo_bin,
                &root_uri,
                Some(Arc::clone(&crash_notify)),
            )
            .await
            {
                Ok(tp) => {
                    tracing::info!("TSGO type provider started (resilient mode)");
                    let resilient = ResilientTypeProvider::new(
                        tp,
                        crash_notify,
                        tsgo_bin,
                        root_uri,
                        Arc::clone(&client_cell),
                        3, // max_restarts
                    );
                    Some(Arc::new(resilient))
                }
                Err(e) => {
                    tracing::warn!("TSGO unavailable: {e}");
                    None
                }
            }
        }
        None => {
            tracing::info!("tsgo binary not found — running in verter-only mode");
            None
        }
    };

    let config = LspConfig {
        host,
        type_provider,
        project_sync_mode: ProjectSyncMode::TsxOnly,
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
    .finish();

    Server::new(stdin, stdout, socket).serve(service).await;
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
