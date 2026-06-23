//! Stdio-launch handshake smoke for the real `verter-lsp` binary.
//!
//! This is the authoritative AUTOMATED check of the shared editor-client launch
//! path: Lapce and Zed have no headless extension-test harness, so a GUI launch
//! cannot be exercised in CI. Instead this test spawns the REAL `verter-lsp`
//! binary as a child process over stdio, drives a minimal LSP `initialize` /
//! `initialized` / `shutdown` / `exit` handshake, and asserts the server returns
//! a populated `ServerCapabilities` — exactly the contract a real editor client
//! relies on after it hands the host a launch command.
//!
//! Hermeticity: the child is launched with `--type-provider=off`, so `initialize`
//! is toolchain-free (no tsgo / tsserver / node lookup) and runs identically on
//! every host and in CI. The workspace root is a fresh temp dir; no third-party
//! corpus or `node_modules` is touched.
//!
//! Discrimination (Stub Prevention): the test FAILS — never skips, never passes
//! vacuously — if the handshake is broken. A missing/late `initialize` response,
//! an `initialize` result without a non-empty `capabilities` object carrying a
//! concrete capability key, a malformed frame, or a child that crashes/exits
//! non-zero during initialize all make it FAIL. Every blocking read is bounded by
//! a hard deadline (a reader thread + `recv_timeout`), so a hung server kills the
//! child and fails with a clear message rather than hanging the suite.
//!
//! The argv and `initializationOptions` are built through the SHARED
//! `verter_editor_client` launch contract, so the smoke also pins that contract:
//! the same `build_server_args` / `build_initialization_options` / `resolve_server`
//! the Lapce and Zed clients consume.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use verter_editor_client::{
    build_initialization_options, build_server_args, resolve_server, DiscoveryInputs, ServerSource,
};

/// Hard ceiling on any single framed read. A correctly-behaving server answers
/// `initialize` well within this window even on a cold, debug-built binary; a
/// broken/hung server trips it, and the test kills the child and FAILS.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// A framed message read off the child's stdout: either a parsed JSON-RPC value
/// or a fatal reader-side condition (EOF / malformed frame / IO error). The
/// reader thread sends these over a channel so the test side can apply a hard
/// `recv_timeout` to every read.
enum ReaderEvent {
    /// A successfully parsed Content-Length-framed JSON-RPC message.
    Message(Value),
    /// The reader hit a fatal condition (EOF, a malformed header/frame, or an IO
    /// error). The string explains which, so a failure is actionable.
    Fatal(String),
}

/// Encode a JSON-RPC message as an LSP `Content-Length`-framed payload and write
/// it to the child's stdin.
fn write_message(stdin: &mut impl Write, message: &Value) {
    let body = serde_json::to_vec(message).expect("serialize JSON-RPC message");
    write!(stdin, "Content-Length: {}\r\n\r\n", body.len()).expect("write LSP frame header");
    stdin.write_all(&body).expect("write LSP frame body");
    stdin.flush().expect("flush LSP frame");
}

/// Spawn a reader thread that parses `Content-Length`-framed JSON-RPC messages
/// off `stdout` and forwards each as a [`ReaderEvent`] over the returned channel.
///
/// The thread treats EOF and any malformed framing as a [`ReaderEvent::Fatal`] —
/// it never silently stops, so the test side always learns why a response did not
/// arrive (rather than blocking forever waiting on a dead stream).
fn spawn_reader(stdout: impl Read + Send + 'static) -> Receiver<ReaderEvent> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            // Parse the header block, extracting Content-Length.
            let mut content_length: Option<usize> = None;
            loop {
                let mut line = String::new();
                match reader.read_line(&mut line) {
                    Ok(0) => {
                        let _ = tx.send(ReaderEvent::Fatal(
                            "stdout closed (EOF) before a complete frame header".to_string(),
                        ));
                        return;
                    }
                    Ok(_) => {}
                    Err(err) => {
                        let _ = tx.send(ReaderEvent::Fatal(format!("stdout read error: {err}")));
                        return;
                    }
                }
                let trimmed = line.trim_end_matches(['\r', '\n']);
                if trimmed.is_empty() {
                    // Blank line terminates the header block.
                    break;
                }
                if let Some(value) = trimmed.strip_prefix("Content-Length:") {
                    match value.trim().parse::<usize>() {
                        Ok(len) => content_length = Some(len),
                        Err(_) => {
                            let _ = tx.send(ReaderEvent::Fatal(format!(
                                "malformed Content-Length header: {trimmed:?}"
                            )));
                            return;
                        }
                    }
                }
            }

            let len = match content_length {
                Some(len) => len,
                None => {
                    let _ = tx.send(ReaderEvent::Fatal(
                        "frame header had no Content-Length".to_string(),
                    ));
                    return;
                }
            };

            let mut body = vec![0u8; len];
            if let Err(err) = reader.read_exact(&mut body) {
                let _ = tx.send(ReaderEvent::Fatal(format!(
                    "failed reading {len}-byte frame body: {err}"
                )));
                return;
            }

            match serde_json::from_slice::<Value>(&body) {
                Ok(value) => {
                    if tx.send(ReaderEvent::Message(value)).is_err() {
                        // Test side dropped the receiver; nothing more to do.
                        return;
                    }
                }
                Err(err) => {
                    let _ = tx.send(ReaderEvent::Fatal(format!(
                        "frame body was not valid JSON: {err}"
                    )));
                    return;
                }
            }
        }
    });
    rx
}

/// Best-effort kill + reap so a failed assertion never leaks the child process.
fn kill_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

/// Block for the next message with a hard deadline. On timeout or a fatal reader
/// condition the child is killed and the test FAILS with a clear reason — this is
/// the discrimination guarantee (a broken/hung handshake can never hang or pass).
fn next_message(rx: &Receiver<ReaderEvent>, child: &mut Child, context: &str) -> Value {
    match rx.recv_timeout(READ_TIMEOUT) {
        Ok(ReaderEvent::Message(value)) => value,
        Ok(ReaderEvent::Fatal(reason)) => {
            kill_child(child);
            panic!("{context}: reader reported a fatal condition: {reason}");
        }
        Err(RecvTimeoutError::Timeout) => {
            kill_child(child);
            panic!(
                "{context}: timed out after {}s waiting for a framed response \
                 (server did not answer — broken handshake)",
                READ_TIMEOUT.as_secs()
            );
        }
        Err(RecvTimeoutError::Disconnected) => {
            kill_child(child);
            panic!("{context}: reader thread ended without sending a message");
        }
    }
}

/// Drive the minimal handshake against the real binary and assert the
/// `initialize` result carries a populated, concretely-keyed `capabilities`.
#[test]
fn verter_lsp_initialize_handshake_returns_capabilities() {
    // A fresh, unique workspace root — no fixtures, no node_modules.
    let tmp = tempfile::tempdir().expect("create temp workspace root");
    let root = tmp.path().to_string_lossy().into_owned();

    // ── Pin the shared launch contract ──────────────────────────────────────
    // The argv we launch with is built by the SAME shared contract the editor
    // clients use, so this smoke also guards it. `--type-provider=off` keeps the
    // handshake hermetic; the contract round-trips `off` (it is one of the two
    // SDK-free emittable providers) rather than clamping it to tsgo.
    let launch_settings = json!({ "typeProvider": "off" });
    let args = build_server_args(Some(&root), &launch_settings);
    assert_eq!(
        args.first().map(String::as_str),
        Some("--type-provider=off"),
        "the shared contract must emit --type-provider=off first: {args:?}"
    );
    assert_eq!(
        args.last().map(String::as_str),
        Some(root.as_str()),
        "the shared contract must place the workspace root last: {args:?}"
    );
    // Cheap discovery-contract pin: an explicit override resolves to Override.
    let resolved = resolve_server(&DiscoveryInputs {
        override_path: Some(env!("CARGO_BIN_EXE_verter-lsp")),
        ..Default::default()
    })
    .expect("an explicit override must resolve");
    assert_eq!(
        resolved,
        ServerSource::Override(env!("CARGO_BIN_EXE_verter-lsp").to_string()),
        "an explicit override path must resolve to ServerSource::Override"
    );

    // The initializationOptions are produced by the shared builder too.
    let init_options = build_initialization_options(&launch_settings);
    assert!(
        init_options.get("frameworks").is_none(),
        "the shared init-options builder must drop `frameworks`: {init_options:?}"
    );

    // ── Spawn the real binary over stdio ────────────────────────────────────
    // stderr is discarded (`Stdio::null()`) rather than piped: the test never
    // inspects it, and an undrained pipe would deadlock the handshake. The child
    // inherits this process's environment, so under `RUST_LOG`/`VERTER_LOG=debug`
    // (CI or a dev shell) a piped-but-undrained stderr could fill its small OS
    // buffer before `initialize` is answered, blocking the server on the stderr
    // write and tripping a spurious read timeout. Nulling stderr removes that
    // failure mode while keeping the handshake assertions fully discriminating.
    let mut child = Command::new(env!("CARGO_BIN_EXE_verter-lsp"))
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn the verter-lsp binary");

    let mut stdin = child.stdin.take().expect("child stdin piped");
    let stdout = child.stdout.take().expect("child stdout piped");
    let rx = spawn_reader(stdout);

    // ── initialize ──────────────────────────────────────────────────────────
    let root_uri = path_to_file_uri(&root);
    let initialize_request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "processId": null,
            "rootUri": root_uri,
            "workspaceFolders": [ { "uri": root_uri, "name": "verter-smoke" } ],
            "capabilities": {},
            "initializationOptions": init_options,
        }
    });
    write_message(&mut stdin, &initialize_request);

    // Read framed messages until the `initialize` response (id == 1) arrives.
    // Server-to-client requests/notifications (e.g. window/logMessage) before the
    // response are tolerated; each read is hard-bounded so a no-response server
    // trips the deadline and FAILS.
    let mut initialize_result: Option<Value> = None;
    for _ in 0..64 {
        let message = next_message(&rx, &mut child, "awaiting initialize response");
        if message.get("id").and_then(Value::as_i64) == Some(1) {
            if let Some(error) = message.get("error") {
                kill_child(&mut child);
                panic!("initialize returned a JSON-RPC error: {error}");
            }
            initialize_result = message.get("result").cloned();
            break;
        }
    }

    let result = match initialize_result {
        Some(result) => result,
        None => {
            kill_child(&mut child);
            panic!("never received an initialize response with id == 1");
        }
    };

    // The capabilities object must be present and NON-EMPTY.
    let capabilities = result
        .get("capabilities")
        .and_then(Value::as_object)
        .unwrap_or_else(|| {
            kill_child(&mut child);
            panic!("initialize result had no `capabilities` object: {result}");
        });
    assert!(
        !capabilities.is_empty(),
        "initialize must advertise a non-empty capabilities object: {result}"
    );

    // Assert on CONCRETE capability keys the server actually emits (see
    // `crates/verter_lsp/src/capabilities.rs::server_capabilities`). A broken
    // handshake that returned `{}` or a stub fails here.
    for key in [
        "textDocumentSync",
        "hoverProvider",
        "completionProvider",
        "definitionProvider",
    ] {
        assert!(
            capabilities.contains_key(key),
            "capabilities must include `{key}`: {result}"
        );
    }

    // ── initialized ─────────────────────────────────────────────────────────
    write_message(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "initialized", "params": {} }),
    );

    // ── shutdown / exit ─────────────────────────────────────────────────────
    write_message(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "id": 2, "method": "shutdown", "params": null }),
    );
    // Read until the shutdown response (id == 2) — also hard-bounded.
    let mut got_shutdown = false;
    for _ in 0..64 {
        let message = next_message(&rx, &mut child, "awaiting shutdown response");
        if message.get("id").and_then(Value::as_i64) == Some(2) {
            assert!(
                message.get("error").is_none(),
                "shutdown returned a JSON-RPC error: {message}"
            );
            got_shutdown = true;
            break;
        }
    }
    assert!(
        got_shutdown,
        "never received a shutdown response with id == 2"
    );

    write_message(
        &mut stdin,
        &json!({ "jsonrpc": "2.0", "method": "exit", "params": null }),
    );
    // Dropping stdin signals EOF in case the server waits on it.
    drop(stdin);

    // The launch + handshake contract this smoke pins is complete: the binary
    // launched (through the shared argv), answered `initialize` with populated
    // capabilities, and answered `shutdown`. We send `exit` as the spec-correct
    // graceful signal, then DETERMINISTICALLY reap so the suite never leaks a
    // child and never hangs.
    //
    // Process termination after `exit` is intentionally NOT asserted here: it is a
    // separate server behavior (verter-lsp does not currently terminate the
    // process on `exit` — a runtime/host-teardown issue tracked independently),
    // not part of the editor-client launch path this test guards. Asserting a
    // clean exit would make the smoke flaky for a reason orthogonal to the
    // handshake it discriminates on. A short grace window lets a well-behaved
    // build exit cleanly; otherwise we force-kill.
    let grace_deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() >= grace_deadline {
                    kill_child(&mut child);
                    break;
                }
                thread::sleep(Duration::from_millis(25));
            }
            Err(_) => {
                kill_child(&mut child);
                break;
            }
        }
    }
}

/// Convert a filesystem path to a `file://` URI (mirroring the binary's own
/// `path_to_file_uri`): a POSIX absolute path becomes `file://<path>`, a Windows
/// path (`C:\...`) becomes `file:///C:/...`.
///
/// Kept local rather than reusing the binary's helper: that helper lives in
/// `verter_lsp`'s private `uri` module / `main.rs`, neither of which is reachable
/// from this integration-test crate, and the `rootUri` it produces is not
/// load-bearing for the handshake (the hermetic `off` init never resolves it).
fn path_to_file_uri(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    if normalized.starts_with('/') {
        format!("file://{normalized}")
    } else {
        format!("file:///{normalized}")
    }
}
