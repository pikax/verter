//! SDK-level conformance test for the rmcp 3.1.4 migration.
//!
//! `mcp_tool_audit_integration.rs` and friends drive tool handlers
//! directly — they never touch the wire protocol, so a breaking change
//! in rmcp's transport, lifecycle negotiation, or `tools/call` wire
//! shape would compile clean and pass every other test in this crate
//! while the real server-over-stdio path silently broke. This test
//! pairs [`verter_mcp::VerterMcpServer`] with a REAL rmcp client over
//! an in-process duplex stream — the same `IntoTransport` +
//! `ServiceExt::serve` machinery the `stdio` transport in `run.rs`
//! uses — and drives the actual JSON-RPC exchange: `initialize`,
//! `tools/list`, `tools/call`, and session cancellation.
//!
//! Discrimination: reverting the `Content` → `ContentBlock` rename (or
//! any other 1.7 → 3.1.4 wire-shape drift this migration fixed) fails
//! to compile. Falling back to rmcp's default (unpinned) protocol-version
//! set fails
//! `supported_protocol_versions_are_pinned_to_the_four_revisions_predating_2026_07_28`
//! and, at handshake time,
//! `initialize_negotiates_server_identity_and_capabilities` (that test
//! drives a client that *requests* 2026-07-28 so the pin is observable).
//! Restoring `Implementation::from_build_env()` as the server identity
//! fails the same initialize test's name/version assertions.
//!
//! `call_tool_round_trips_structured_content` is wire-envelope coverage,
//! not a migration discriminator: the old SDK serializes this same
//! `list_files`/text-content scenario identically.

#![cfg(not(target_arch = "wasm32"))]

use std::sync::Arc;

use rmcp::model::{CallToolRequestParams, ClientInfo, ProtocolVersion};
use rmcp::{ClientHandler, ServerHandler, ServiceExt};
use verter_diagnostics::Linter;
use verter_mcp::config::McpServerConfig;
use verter_mcp::tools::diagnostics::make_lint_config;
use verter_mcp::VerterMcpServer;
use verter_session::{HostConfig, VerterHost};
use verter_workspace::{MemoryOptions, MemoryWorkspace, WorkspaceAccess};

/// `VerterMcpServer::supported_protocol_versions` (`ServerHandler`
/// override): the rmcp 3.1.4 bump introduced protocol revision
/// 2026-07-28 and its default `Cow::Borrowed(ProtocolVersion::KNOWN_VERSIONS)`
/// implementation now advertises it, even though Verter has never been
/// tested against its inline-lifecycle/SEP-2322 semantics. This bump
/// would silently let a client negotiate that revision for per-request
/// (streamable-http) lifecycle checks. Pin the set to the four revisions
/// Verter supported before the migration.
///
/// Discrimination: reverting the `supported_protocol_versions` override
/// (falling back to the trait default) re-admits `V_2026_07_28` and
/// fails this test.
#[test]
fn supported_protocol_versions_are_pinned_to_the_four_revisions_predating_2026_07_28() {
    let supported = ServerHandler::supported_protocol_versions(&build_server());
    assert!(
        !supported.contains(&ProtocolVersion::V_2026_07_28),
        "VerterMcpServer must not advertise the untested 2026-07-28 MCP protocol \
         revision introduced by the rmcp 3.1.4 bump; got {supported:?}"
    );
    assert_eq!(
        supported.as_ref(),
        &[
            ProtocolVersion::V_2024_11_05,
            ProtocolVersion::V_2025_03_26,
            ProtocolVersion::V_2025_06_18,
            ProtocolVersion::V_2025_11_25,
        ],
        "VerterMcpServer must pin to exactly the four protocol revisions it \
         supported before the rmcp 3.1.4 migration"
    );
}

fn build_server() -> VerterMcpServer {
    let workspace: Arc<dyn WorkspaceAccess> =
        Arc::new(MemoryWorkspace::new(MemoryOptions::default()));
    let host = Arc::new(VerterHost::new(HostConfig::default(), workspace));
    let linter = Arc::new(Linter::new(make_lint_config("recommended")));
    VerterMcpServer::new(host, linter, McpServerConfig::default())
}

/// Client that requests the untested 2026-07-28 revision so initialize
/// negotiation actually exercises the server's supported-set pin.
///
/// The default `()` client requests `ProtocolVersion::LATEST`
/// (`2025-11-25`), which sits inside BOTH the pinned set AND rmcp 3.1.4's
/// unpinned `KNOWN_VERSIONS`, so a handshake through it cannot observe
/// whether the pin is in force.
struct ClientRequestingUntestedRevision;

impl ClientHandler for ClientRequestingUntestedRevision {
    fn get_info(&self) -> ClientInfo {
        let mut info = ClientInfo::default();
        info.protocol_version = ProtocolVersion::V_2026_07_28;
        info
    }
}

/// `initialize` (lifecycle negotiation): a real rmcp client, paired
/// with the real server over an in-process duplex transport,
/// completes the handshake and retains the server's declared identity
/// and capabilities — proving `ServerInfo`/`ServerCapabilities` still
/// round-trip through the 3.1.4 wire format. The client deliberately
/// requests 2026-07-28 so the negotiated version is the pin's fallback,
/// not an echo of a client request that happens to already be in the
/// pinned set.
#[tokio::test]
async fn initialize_negotiates_server_identity_and_capabilities() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        let running = build_server()
            .serve(server_transport)
            .await
            .expect("server accepts the client's initialize handshake");
        running.waiting().await.ok();
    });

    let client = ClientRequestingUntestedRevision
        .serve(client_transport)
        .await
        .expect(
            "client completes initialize against the real VerterMcpServer over the duplex transport",
        );

    let peer_info = client
        .peer_info()
        .expect("initialize always retains a server implementation identity");
    assert!(
        peer_info.capabilities.tools.is_some(),
        "VerterMcpServer::get_info enables the tools capability; \
         a real client must observe it after initialize"
    );
    let server_info = peer_info
        .server_info
        .as_ref()
        .expect("initialize must report the server implementation identity");
    assert_eq!(
        server_info.name, "verter-mcp",
        "the negotiated server identity must be Verter's own product identity, \
         not rmcp's default `Implementation::from_build_env()` name"
    );
    assert_eq!(
        server_info.version,
        env!("CARGO_PKG_VERSION"),
        "the negotiated server version must track the workspace-pinned crate version, \
         not rmcp's own crate version"
    );
    assert_ne!(
        peer_info.protocol_version,
        ProtocolVersion::V_2026_07_28,
        "a client requesting the untested 2026-07-28 revision must not have that \
         revision echoed back — the pin must refuse it"
    );
    assert_eq!(
        peer_info.protocol_version,
        ProtocolVersion::V_2025_11_25,
        "an unsupported client request must fall back to the pinned advertised \
         protocol version (2025-11-25), got {:?}",
        peer_info.protocol_version
    );

    client
        .cancel()
        .await
        .expect("client-initiated session cancellation completes cleanly");
    server_task.await.expect("server task exits after cancel");
}

/// `tools/list` + `tools/call`: the real tool router enumerates
/// `list_files`, and a real `tools/call` round-trip for it returns a
/// structured [`ContentBlock::text`] payload over the real 3.1.4 wire
/// transport — proving `ToolRouter`/`#[tool_router]` macro output still
/// produces a valid envelope end-to-end.
///
/// This is not behavioral coverage of the 1.7 -> 3.1.4 migration itself:
/// the old SDK's `Content` type serialized identically for this same
/// `list_files`/text-content scenario, so this test would pass unchanged
/// against either SDK. The `Content` -> `ContentBlock` rename is enforced
/// at compile time (call sites referencing the removed `Content` type
/// fail to build), not by this test's runtime assertions.
#[tokio::test]
async fn call_tool_round_trips_structured_content() {
    let (server_transport, client_transport) = tokio::io::duplex(64 * 1024);

    let server_task = tokio::spawn(async move {
        let running = build_server()
            .serve(server_transport)
            .await
            .expect("server accepts the client's initialize handshake");
        running.waiting().await.ok();
    });

    let client = ().serve(client_transport).await.expect(
        "client completes initialize against the real VerterMcpServer over the duplex transport",
    );

    let tools = client
        .list_tools(None)
        .await
        .expect("tools/list succeeds over the real transport");
    assert!(
        tools.tools.iter().any(|t| t.name == "list_files"),
        "the real tool router must enumerate list_files; got {:?}",
        tools
            .tools
            .iter()
            .map(|t| t.name.clone())
            .collect::<Vec<_>>()
    );

    let result = client
        .call_tool(CallToolRequestParams::new("list_files"))
        .await
        .expect("tools/call for list_files succeeds over the real transport");
    assert!(
        !result.is_error.unwrap_or(false),
        "list_files must not report a tool-level error"
    );
    let text_present = result.content.iter().any(|block| block.as_text().is_some());
    assert!(
        text_present,
        "list_files must return a ContentBlock::text payload; got {:?}",
        result.content
    );

    client
        .cancel()
        .await
        .expect("session cancellation completes");
    server_task.await.expect("server task exits after cancel");
}
