//! MCP CSS analysis request charges one top-level parse.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use verter_css_syntax::parse_style_ir_thread_invocations;
use verter_diagnostics::Linter;
use verter_mcp::config::McpServerConfig;
use verter_mcp::server::FilePathParams;
use verter_mcp::tools::diagnostics::make_lint_config;
use verter_mcp::VerterMcpServer;
use verter_session::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

#[tokio::test]
async fn mcp_boundary_charges_one_top_level_parse_entry() {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let before = parse_style_ir_thread_invocations();
    let _ = host
        .upsert(UpsertRequest {
            canonical_id: Some("/c.vue".into()),
            input_id: "/c.vue".into(),
            source: Arc::from(
                "<template><div class=\"card\">x</div></template>\
                 <style>.card { color: red; }</style>",
            ),
            file_language: FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert");
    let linter = Arc::new(Linter::new(make_lint_config("recommended")));
    let server = VerterMcpServer::new(host, linter, McpServerConfig::default());
    let result = server
        .analyze_css(Parameters(FilePathParams {
            path: "/c.vue".to_string(),
        }))
        .await
        .expect("analyze_css");
    let text_present = result
        .content
        .iter()
        .any(|c| c.as_text().map(|t| !t.text.is_empty()).unwrap_or(false));
    assert!(text_present, "MCP CSS analysis must return content");
    let charged = parse_style_ir_thread_invocations() - before;
    assert!(
        charged <= 1,
        "MCP CSS analysis must not charge a second parse_style_ir, got {charged}"
    );
}
