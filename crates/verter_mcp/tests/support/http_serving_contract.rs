//! Shared behavioural contract for the standalone MCP HTTP launcher: it must
//! bind, announce the bound port as the first stdout line, AND actually serve
//! MCP over HTTP at the announced URL.
//!
//! Two shipped entry binaries run the same launcher body
//! (`verter_mcp::run::run`): `verter-mcp` (the distributed name) and
//! `verter-mcp-server` (the standalone entry editors spawn so `verter_lsp`
//! never needs a dependency edge to `verter_mcp`). Each crate's consolidated
//! integration-test binary `#[path]`-includes THIS file and drives the
//! contract against its own `CARGO_BIN_EXE_*`, so the serving property is
//! pinned per entry binary and cannot silently drift between the twins.
//!
//! The HTTP round-trip is the load-bearing half of the contract. A launcher
//! that binds the listener and prints the readiness record but hangs (or
//! panics) BEFORE constructing and running the HTTP service still accepts TCP
//! connections through the OS listener backlog, so a bare `TcpStream::connect`
//! cannot distinguish "serving" from "parked after announce". Only a completed
//! MCP response over HTTP proves the service is running.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use verter_mcp::readiness::{http_ready_record, parse_http_ready_record};

/// Kill the spawned server even when an assertion fails mid-test.
pub struct KillOnDrop(pub Child);

impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// How long the spawned debug binary gets to bind and print its record.
pub const READINESS_DEADLINE: Duration = Duration::from_secs(60);

/// How long the announced endpoint gets to answer the MCP `initialize`
/// request. Same generous ceiling as readiness: the real latency is
/// milliseconds and the bound only contains a regression (or a loaded CI
/// machine) — never a tight wall-clock race.
pub const SERVE_DEADLINE: Duration = Duration::from_secs(60);

/// The MCP `initialize` request POSTed to the announced endpoint. The protocol
/// version is one the pinned `rmcp` server accepts; the client identity is a
/// test-only marker.
const INITIALIZE_BODY: &str = concat!(
    r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"#,
    r#""protocolVersion":"2025-06-18","capabilities":{},"#,
    r#""clientInfo":{"name":"verter-mcp-http-serving-contract","version":"0.0.0"}}}"#
);

/// Spawn `<bin> --transport http --port 0` and assert the full launcher
/// contract:
///
/// 1. the FIRST stdout line is byte-equal to the canonical readiness record
///    and names a real OS-assigned port;
/// 2. an MCP `initialize` POST to the ANNOUNCED `/mcp` URL completes with a
///    `200` streamable-HTTP response carrying an `Mcp-Session-Id` and the
///    initialize result (`serverInfo`) — the service is genuinely serving,
///    not merely bound.
pub fn assert_http_launcher_binds_announces_and_serves(bin: &str) {
    let mut child = Command::new(bin)
        .args(["--transport", "http", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap_or_else(|error| panic!("spawn {bin}: {error}"));
    let stdout = child.stdout.take().expect("stdout is piped");
    let _guard = KillOnDrop(child);

    // A blocking read_line with no deadline would hang the in-process test
    // surface on a regression that never prints; read on a helper thread and
    // bound the wait.
    let (sender, receiver) = mpsc::channel::<std::io::Result<String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });

    let first_line = receiver
        .recv_timeout(READINESS_DEADLINE)
        .expect("the launcher printed nothing on stdout within the readiness deadline")
        .expect("reading launcher stdout failed");

    let record = parse_http_ready_record(&first_line)
        .unwrap_or_else(|| panic!("first stdout line is not a readiness record: {first_line:?}"));
    assert!(
        record.port > 0,
        "readiness record must carry a real OS-assigned port"
    );
    assert_eq!(
        record.url,
        format!("http://127.0.0.1:{}/mcp", record.port),
        "readiness record URL must point at the /mcp endpoint on the bound port"
    );

    // The first stdout line is EXACTLY the canonical record — no banner, no
    // tracing noise, no prefix a host-side parser would have to skip.
    assert_eq!(
        first_line.trim_end_matches(['\r', '\n']),
        http_ready_record(record.port),
        "first stdout line must be byte-equal to the canonical readiness record"
    );

    // The announced URL genuinely SERVES: POST an MCP `initialize` and require
    // a completed response. A fabricated port refuses the connection; a
    // parked-after-announce launcher accepts it (listener backlog) but never
    // sends a byte back, so every assertion below discriminates.
    let mut stream = TcpStream::connect(("127.0.0.1", record.port))
        .expect("the port named in the readiness record must accept connections");
    stream
        .set_write_timeout(Some(SERVE_DEADLINE))
        .expect("set write timeout");
    // Short read timeout so the loop re-checks the overall deadline; the
    // TOTAL wait is bounded by SERVE_DEADLINE below.
    stream
        .set_read_timeout(Some(Duration::from_secs(1)))
        .expect("set read timeout");

    let request = format!(
        "POST /mcp HTTP/1.1\r\n\
         Host: 127.0.0.1:{port}\r\n\
         Accept: application/json, text/event-stream\r\n\
         Content-Type: application/json\r\n\
         Content-Length: {len}\r\n\
         \r\n\
         {body}",
        port = record.port,
        len = INITIALIZE_BODY.len(),
        body = INITIALIZE_BODY,
    );
    stream
        .write_all(request.as_bytes())
        .expect("write the MCP initialize request to the announced endpoint");

    let deadline = Instant::now() + SERVE_DEADLINE;
    let mut response: Vec<u8> = Vec::new();
    let mut headers_checked = false;
    let mut chunk = [0u8; 4096];
    loop {
        if !headers_checked {
            if let Some(header_end) = find_subslice(&response, b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&response[..header_end]).into_owned();
                let status_line = head.lines().next().unwrap_or_default().to_owned();
                assert!(
                    status_line.starts_with("HTTP/1.1 200"),
                    "the announced /mcp endpoint must answer the MCP initialize \
                     POST with 200, got status line {status_line:?}\nheaders:\n{head}"
                );
                assert!(
                    head.to_ascii_lowercase().contains("\r\nmcp-session-id:"),
                    "the 200 response must carry an Mcp-Session-Id header — the \
                     streamable-HTTP session layer is what proves the MCP \
                     service (not merely an HTTP router) is serving\nheaders:\n{head}"
                );
                headers_checked = true;
            }
        }
        // The initialize result is streamed as an SSE `data:` event; its
        // `serverInfo` field is spec-mandated and proves the MCP server
        // processed the request end-to-end.
        if headers_checked && find_subslice(&response, b"serverInfo").is_some() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "no complete MCP initialize response from the announced /mcp \
             endpoint within {}s — the launcher bound and announced but is not \
             serving HTTP; got {} response byte(s): {:?}",
            SERVE_DEADLINE.as_secs(),
            response.len(),
            String::from_utf8_lossy(&response[..response.len().min(2048)]),
        );
        match stream.read(&mut chunk) {
            Ok(0) => panic!(
                "the announced /mcp endpoint closed the connection before a \
                 complete MCP initialize response; got {} byte(s): {:?}",
                response.len(),
                String::from_utf8_lossy(&response[..response.len().min(2048)]),
            ),
            Ok(read) => response.extend_from_slice(&chunk[..read]),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => panic!("reading the /mcp response failed: {error}"),
        }
    }
}

/// First index of `needle` in `haystack`, if any.
fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}
