use tower_lsp_server::{LspService, Server};
use verter_host::{HostConfig, VerterHost};
use verter_lsp::{LspConfig, ProjectSyncMode};

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let host = VerterHost::new(HostConfig {
        analysis_level: verter_host::AnalysisLevel::Full,
        ..HostConfig::default()
    });

    let config = LspConfig {
        host,
        type_provider: None,
        project_sync_mode: ProjectSyncMode::TsxOnly,
    };

    let (service, socket) =
        LspService::new(|client| verter_lsp::server::VerterLanguageServer::new(client, config));

    Server::new(stdin, stdout, socket).serve(service).await;
}
