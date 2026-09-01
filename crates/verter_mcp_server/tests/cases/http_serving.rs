//! The `verter-mcp-server` entry binary serves MCP over HTTP at the URL it
//! announces.
//!
//! `verter-mcp-server` exists so editors can spawn the MCP server without
//! `verter_lsp` ever depending on `verter_mcp`; it must behave identically to
//! the distributed `verter-mcp` binary. Both entry points delegate to
//! `verter_mcp::run::run`, but delegation is wiring that can drift — so the
//! SAME behavioural contract that pins `verter-mcp`
//! (`crates/verter_mcp/tests/cases/http_readiness.rs`) is driven here against
//! THIS crate's own binary. The contract body is `#[path]`-included from the
//! owning crate's shared support module, so the two spawn tests cannot assert
//! diverging contracts either.

#[path = "../../../verter_mcp/tests/support/http_serving_contract.rs"]
mod http_serving_contract;

use http_serving_contract::assert_http_launcher_binds_announces_and_serves;

#[test]
fn http_transport_emits_bound_port_record_first_and_serves_mcp_at_announced_url() {
    let binary = verter_test_support::cargo_test_binary_path!("verter-mcp-server");
    assert_http_launcher_binds_announces_and_serves(binary.to_string_lossy().as_ref());
}
