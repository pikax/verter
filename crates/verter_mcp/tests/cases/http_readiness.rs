//! The standalone HTTP transport announces its OS-assigned port through a
//! stable stdout readiness record — and genuinely serves MCP at the announced
//! URL.
//!
//! Hosts that spawn `verter-mcp --transport http --port 0` (the VS Code
//! extension among them) learn the bound port ONLY from this record: human
//! `tracing` output goes to stderr and is not port identity. The record must
//! therefore be the FIRST stdout line, byte-equal to the canonical encoding,
//! and must name a port that is genuinely SERVING — not merely bound, since
//! the OS listener backlog accepts TCP connections even for a launcher that
//! parked right after announcing. The full contract (readiness record + MCP
//! `initialize` round-trip) lives in the shared support module, driven here
//! against the distributed `verter-mcp` binary and, from
//! `crates/verter_mcp_server/tests/`, against the identically-wired
//! `verter-mcp-server` entry binary.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use verter_mcp::readiness::parse_http_ready_record;

#[path = "../support/http_serving_contract.rs"]
mod http_serving_contract;

use http_serving_contract::{
    assert_http_launcher_binds_announces_and_serves, KillOnDrop, READINESS_DEADLINE,
};

#[test]
fn http_transport_emits_bound_port_record_first_and_serves_mcp_at_announced_url() {
    assert_http_launcher_binds_announces_and_serves(env!("CARGO_BIN_EXE_verter-mcp"));
}

/// How long the contained server gets to notice its client died and exit.
/// The pid monitors are event-driven (pidfd/kqueue/WaitForSingleObject), so
/// the real latency is milliseconds; the ceiling only bounds a regression.
const CONTAINMENT_EXIT_DEADLINE: Duration = Duration::from_secs(30);

/// `--client-pid` binds the server's lifetime to the named host process,
/// exactly like `verter-lsp`'s containment: an HTTP server that outlives a
/// hard-killed editor host is worse than an ordinary orphan — it keeps a
/// bound listener alive (and, under a fixed `verter.mcp.port`, blocks the
/// next launch with EADDRINUSE).
#[test]
fn client_pid_containment_exits_the_server_when_the_client_dies() {
    // Client stand-in: a second verter-mcp on the stdio transport with a held
    // stdin pipe — it blocks reading MCP framing, portably, from the same
    // binary the test already builds.
    let client = Command::new(env!("CARGO_BIN_EXE_verter-mcp"))
        .args(["--transport", "stdio"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn client stand-in");
    let client_pid = client.id();
    let mut client = KillOnDrop(client);

    let mut server = Command::new(env!("CARGO_BIN_EXE_verter-mcp"))
        .args([
            "--transport",
            "http",
            "--port",
            "0",
            "--client-pid",
            &client_pid.to_string(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn contained verter-mcp");
    let stdout = server.stdout.take().expect("stdout is piped");
    let mut server = KillOnDrop(server);

    // Wait for readiness FIRST: the guard is armed before the record prints,
    // so a record proves containment was accepted (a dead-on-arrival monitor
    // refuses to start and never prints one).
    let (sender, receiver) = mpsc::channel::<std::io::Result<String>>();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let result = reader.read_line(&mut line).map(|_| line);
        let _ = sender.send(result);
    });
    let first_line = receiver
        .recv_timeout(READINESS_DEADLINE)
        .expect("contained verter-mcp printed nothing on stdout within the readiness deadline")
        .expect("reading contained verter-mcp stdout failed");
    parse_http_ready_record(&first_line)
        .expect("contained verter-mcp must still announce readiness");

    // Kill the client; the server must exit on its own.
    client.0.kill().expect("kill client stand-in");
    client.0.wait().expect("reap client stand-in");

    let deadline = std::time::Instant::now() + CONTAINMENT_EXIT_DEADLINE;
    loop {
        match server.0.try_wait().expect("poll contained verter-mcp") {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                panic!(
                    "verter-mcp survived its dead client for {}s — client containment is not armed",
                    CONTAINMENT_EXIT_DEADLINE.as_secs()
                );
            }
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}
