//! TEST-ONLY fake tsgo engine for the toolchain validator's capability-smoke
//! tests (`crates/verter_tsgo_api/tests/cases/toolchain_validation.rs`).
//!
//! This is NOT shipped tooling: it exists so the integration tests can drive a
//! deterministic "engine" process without the real tsgo binary. The scenario
//! is selected by the binary's own FILE NAME — the tests hard-link/copy this
//! binary to `verter-tsgo-fake-<scenario>[.exe]` — so parallel tests never
//! share mutable environment:
//!
//! | scenario         | `--version`       | `--lsp --stdio` behavior                |
//! |------------------|-------------------|-----------------------------------------|
//! | `ok`             | `7.0.2`           | handshake ok, `serverInfo` = `7.0.2`    |
//! | `mismatch`       | `7.0.2`           | handshake ok, `serverInfo` = `7.0.9`    |
//! | `noserverinfo`   | `7.0.2`           | initialize result carries no serverInfo |
//! | `exit`           | `7.0.2`           | exits 1 immediately                     |
//! | `v710`           | `7.1.0`           | handshake ok, `serverInfo` = `7.1.0`    |
//! | `rc`             | `7.0.2-rc.1`      | handshake ok, matching serverInfo       |
//! | `nightly`        | `7.0.0-dev.20260703.1` | handshake ok, matching serverInfo  |
//!
//! `custom/initializeAPISession` always returns a dead pipe path — the API
//! capability smoke against a fake is expected to FAIL at the pipe connect
//! (the positive API smoke runs against the real engine, live-gated).

use std::io::{BufRead, BufReader, Write};

/// The scenario encoded in the binary's file name (`verter-tsgo-fake-<x>`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Ok,
    Mismatch,
    NoServerInfo,
    Exit,
    V710,
    Rc,
    Nightly,
}

impl Scenario {
    fn from_argv0() -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        let stem = exe
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        match stem.strip_prefix("verter-tsgo-fake-") {
            Some("mismatch") => Self::Mismatch,
            Some("noserverinfo") => Self::NoServerInfo,
            Some("exit") => Self::Exit,
            Some("v710") => Self::V710,
            Some("rc") => Self::Rc,
            Some("nightly") => Self::Nightly,
            _ => Self::Ok,
        }
    }

    /// The version this fake reports via `--version` (and, unless
    /// `Mismatch`/`NoServerInfo`, via in-band `serverInfo.version`).
    fn probe_version(self) -> &'static str {
        match self {
            Self::V710 => "7.1.0",
            Self::Rc => "7.0.2-rc.1",
            Self::Nightly => "7.0.0-dev.20260703.1",
            _ => "7.0.2",
        }
    }

    /// The `serverInfo.version` the initialize result carries, if any.
    fn server_info_version(self) -> Option<&'static str> {
        match self {
            Self::Mismatch => Some("7.0.9"),
            Self::NoServerInfo => None,
            other => Some(other.probe_version()),
        }
    }
}

fn main() {
    let scenario = Scenario::from_argv0();
    let args: Vec<String> = std::env::args().collect();

    if args.iter().any(|a| a == "--version") {
        println!("Version {}", scenario.probe_version());
        return;
    }
    if args.iter().any(|a| a == "--lsp") {
        if scenario == Scenario::Exit {
            std::process::exit(1);
        }
        serve_lsp(scenario);
        return;
    }
    std::process::exit(2);
}

/// Speak Content-Length-framed JSON-RPC on stdio until EOF or `exit`.
fn serve_lsp(scenario: Scenario) {
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    while let Some(body) = read_frame(&mut reader) {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
            continue;
        };
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = message.get("id").cloned();
        match (method, id) {
            ("initialize", Some(id)) => {
                let mut result = serde_json::json!({ "capabilities": {} });
                if let Some(version) = scenario.server_info_version() {
                    result["serverInfo"] =
                        serde_json::json!({ "name": "verter-fake-tsgo", "version": version });
                }
                write_frame(
                    &mut stdout.lock(),
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                );
            }
            ("custom/initializeAPISession", Some(id)) => {
                write_frame(
                    &mut stdout.lock(),
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "sessionId": "fake", "pipe": "/nonexistent/verter-tsgo-fake-pipe" }
                    }),
                );
            }
            ("shutdown", Some(id)) => {
                write_frame(
                    &mut stdout.lock(),
                    &serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": null }),
                );
            }
            ("exit", _) => return,
            // Notifications (no id) and unknown methods are ignored.
            _ => {}
        }
    }
}

/// Read one `Content-Length`-framed body; `None` on EOF.
fn read_frame(reader: &mut impl BufRead) -> Option<Vec<u8>> {
    let mut content_length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        let read = reader.read_line(&mut line).ok()?;
        if read == 0 {
            return None; // EOF
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if content_length.is_some() {
                break;
            }
            continue;
        }
        if let Some(value) = trimmed.to_ascii_lowercase().strip_prefix("content-length:") {
            content_length = value.trim().parse().ok();
        }
    }
    let mut body = vec![0u8; content_length?];
    reader.read_exact(&mut body).ok()?;
    Some(body)
}

/// Write one `Content-Length`-framed JSON message.
fn write_frame(writer: &mut impl Write, message: &serde_json::Value) {
    let body = serde_json::to_vec(message).expect("serialize frame");
    let _ = write!(writer, "Content-Length: {}\r\n\r\n", body.len());
    let _ = writer.write_all(&body);
    let _ = writer.flush();
}
