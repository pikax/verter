//! Throwaway instrumentation: measure MCP tool handler future sizes.
//!
//! Run with:
//!   cargo test -p verter_mcp future_size_measure -- --nocapture --ignored
//!   cargo test -p verter_mcp future_size_measure --release -- --nocapture --ignored
//!
//! Not a gate. Numbers are printed and copied into docs/arch/future/* findings.

use std::mem::{size_of, size_of_val};
use std::sync::Arc;

use rmcp::handler::server::wrapper::Parameters;
use verter_diagnostics::Linter;
use verter_session::{HostConfig, VerterHost};

use crate::config::McpServerConfig;
use crate::server::{AnalyzeFileParams, CompileFileParams, FilePathParams, VerterMcpServer};

fn report(label: &str, bytes: usize) {
    eprintln!(
        "[future-size] {label}: {bytes} B ({:.1} KiB)",
        bytes as f64 / 1024.0
    );
}

fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "debug"
    } else {
        "release"
    }
}

fn make_server() -> VerterMcpServer {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let linter = Arc::new(Linter::default());
    VerterMcpServer::new(host, linter, McpServerConfig::default())
}

#[tokio::test]
#[ignore = "throwaway instrumentation — run manually"]
async fn measure_mcp_tool_future_sizes() {
    eprintln!("=== mcp tool futures profile={} ===", profile());

    let server = make_server();
    let path = "/tmp/verter-future-size/App.vue".to_string();

    // Public tool methods (what Verter owns). The rmcp `#[tool]` macro wraps
    // call sites in `Box::pin` at the router boundary — these sizes are the
    // *unboxed* method futures before that box.
    {
        let fut = server.analyze_file(Parameters(AnalyzeFileParams {
            path: path.clone(),
            sections: None,
        }));
        report("VerterMcpServer::analyze_file", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = server.get_component_api(Parameters(FilePathParams { path: path.clone() }));
        report("VerterMcpServer::get_component_api", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = server.get_framework_surface(Parameters(FilePathParams { path: path.clone() }));
        report("VerterMcpServer::get_framework_surface", size_of_val(&fut));
        drop(fut);
    }
    {
        let fut = server.compile_file(Parameters(CompileFileParams {
            path: path.clone(),
            production: None,
            source_map: None,
            vapor: None,
        }));
        report("VerterMcpServer::compile_file", size_of_val(&fut));
        drop(fut);
    }

    // Outer BoxFuture slot (what rmcp DynService / tool router holds).
    type BoxFut = std::pin::Pin<
        Box<
            dyn std::future::Future<Output = Result<rmcp::model::CallToolResult, rmcp::ErrorData>>
                + Send,
        >,
    >;
    report("BoxFuture tool slot (rmcp boxes)", size_of::<BoxFut>());
    report(
        "size_of Pin<Box<dyn Future + Send>>",
        size_of::<std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>>(),
    );

    // The #[tool] macro rewrites `async fn` tools into
    // `fn … -> Pin<Box<dyn Future + Send + '_>> { Box::pin(async move { body }) }`,
    // so the 16 B figures above are the *outer* slots. The production tool bodies
    // contain ZERO `.await` points — they call sync host APIs — so the unboxed
    // state machine is only argument capture. Measure that shape synthetically:
    {
        let path = path.clone();
        let host = Arc::clone(&server_host_for_size(&server));
        let fut = async move {
            let _canonical = path.clone();
            let _ = host.as_ref();
            // Mirror: resolve path, run sync audit/host closure, return text.
            Ok::<String, String>(String::new())
        };
        report(
            "synthetic MCP tool body (path+Arc host, no await)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        // Heavier capture: path + optional sections + Arc host (analyze_file-ish).
        let path = path.clone();
        let sections: Option<Vec<String>> = Some(vec!["script".into(), "template".into()]);
        let host = Arc::clone(&server_host_for_size(&server));
        let fut = async move {
            let _ = (path, sections, host);
            Ok::<(), ()>(())
        };
        report(
            "synthetic analyze_file-ish capture (path+sections+host)",
            size_of_val(&fut),
        );
        drop(fut);
    }
    {
        // compile_file-ish: path + three Option<bool> + host.
        let path = path.clone();
        let host = Arc::clone(&server_host_for_size(&server));
        let fut = async move {
            let _ = (path, None::<bool>, None::<bool>, None::<bool>, host);
            Ok::<(), ()>(())
        };
        report("synthetic compile_file-ish capture", size_of_val(&fut));
        drop(fut);
    }

    eprintln!("=== notes ===");
    eprintln!(
        "[future-size] rmcp #[tool] already Box::pins every tool method; \
         service loop spawns one task per inbound request (no Verter-owned \
         concurrency cap). Tool bodies are await-free sync host work."
    );
}

/// Borrow the server's host Arc for synthetic sizing without polling tools.
fn server_host_for_size(server: &VerterMcpServer) -> Arc<VerterHost> {
    // VerterMcpServer fields are private; reconstruct a sibling host is fine
    // for size-of-capture measurements (Arc is always 8 B on this target).
    let _ = server;
    Arc::new(VerterHost::new_standalone(HostConfig::default()))
}
