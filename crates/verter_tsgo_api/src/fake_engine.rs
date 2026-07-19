//! TEST-ONLY fake tsgo engine, behind the `test-fake-engine` feature (never
//! built into a default `cargo build` / release — see `Cargo.toml`). The thin
//! feature-gated `verter_tsgo_fake_engine` bin (and mirror shims in consumer
//! crates' test lanes) just calls [`main`].
//!
//! It exists so the integration tests can drive a deterministic "engine"
//! process without the real tsgo binary. The scenario is selected by the
//! binary's own FILE NAME — the tests copy/hard-link the shim binary to
//! `verter-tsgo-fake-<scenario>[.exe]` — so parallel tests never share mutable
//! environment:
//!
//! | scenario         | `--version`              | `--lsp --stdio` behavior                         |
//! |------------------|--------------------------|--------------------------------------------------|
//! | `ok`             | `7.0.2`                  | handshake ok, `serverInfo` = `7.0.2`; DEAD api pipe |
//! | `mismatch`       | `7.0.2`                  | handshake ok, `serverInfo` = `7.0.9`             |
//! | `noserverinfo`   | `7.0.2`                  | initialize result carries no serverInfo          |
//! | `exit`           | `7.0.2`                  | exits 1 immediately                              |
//! | `v710`           | `7.1.0`                  | handshake ok, `serverInfo` = `7.0.9`-style match |
//! | `rc`             | `7.0.2-rc.1`             | handshake ok, matching serverInfo                |
//! | `nightly`        | `7.0.0-dev.20260703.1`   | handshake ok, matching serverInfo                |
//! | `apiok`          | `7.0.2`                  | full surface: handshake + WORKING `--api` pipe (integer snapshot handle, staged project echoed) |
//! | `apihollow`      | `7.0.2`                  | full surface BUT `updateSnapshot` returns a hollow `projects: []` |
//! | `declfail`       | `7.0.2`                  | full surface; `--project <cfg>` exits 2 with NO output |
//! | `hang-version`   | hangs forever            | (unused)                                         |
//! | `hold-pipe`      | prints `7.0.2`, then spawns a child that HOLDS the stdout/stderr pipes open and exits 0 |
//! | `hang-lsp`       | `7.0.2`                  | hangs forever (never answers `initialize`)       |
//! | `hang-api`       | `7.0.2`                  | full `--lsp`+pipe surface, but standalone `--api` never writes a frame |
//!
//! The `hold-pipe` grandchild writes its pid to `<exe>.child.pid` so the wedge
//! tests can prove the bounded probe leaves NO live descendant after the tree
//! kill.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;

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
    ApiOk,
    ApiHollow,
    DeclFail,
    HangVersion,
    HoldPipe,
    HangLsp,
    HangApi,
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
            Some("apiok") => Self::ApiOk,
            Some("apihollow") => Self::ApiHollow,
            Some("declfail") => Self::DeclFail,
            Some("hang-version") => Self::HangVersion,
            Some("hold-pipe") => Self::HoldPipe,
            Some("hang-lsp") => Self::HangLsp,
            Some("hang-api") => Self::HangApi,
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

    /// Whether this scenario serves a WORKING `--api` attach pipe (the LSP
    /// handshake mints a real pipe and the fake speaks `initialize` /
    /// `updateSnapshot` on it).
    fn serves_api_pipe(self) -> bool {
        matches!(
            self,
            Self::ApiOk | Self::ApiHollow | Self::DeclFail | Self::HangApi
        )
    }

    /// The `projects` array `updateSnapshot` returns for `config` (the tsconfig
    /// the client leased via `openProjects`). `ApiHollow` returns a hollow
    /// empty vector with a VALID integer snapshot handle.
    fn snapshot_projects(self, config: &str) -> serde_json::Value {
        if self == Self::ApiHollow {
            return serde_json::json!([]);
        }
        serde_json::json!([{
            "id": "p.fake",
            "configFileName": config,
            "compilerOptions": {},
            "rootFiles": [],
        }])
    }
}

/// The fake engine entry point (the shim bins call this).
pub fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Internal grandchild mode (spawned by the `hold-pipe` scenario): hold the
    // inherited stdout/stderr pipes open forever so a "bounded" probe that only
    // bounds `child.wait()` wedges on its reader joins. Writes its pid to
    // `<exe>.child.pid` so tests can prove the tree kill reaps it.
    if args.iter().any(|a| a == "--verter-fake-hold-pipe") {
        let pid_file = hold_pipe_pid_file();
        let _ = std::fs::write(&pid_file, std::process::id().to_string());
        hang_forever();
    }

    let scenario = Scenario::from_argv0();

    if args.iter().any(|a| a == "--version") {
        match scenario {
            Scenario::HangVersion => hang_forever(),
            Scenario::HoldPipe => {
                println!("Version {}", scenario.probe_version());
                let _ = std::io::stdout().flush();
                spawn_pipe_holder();
                return;
            }
            _ => {
                println!("Version {}", scenario.probe_version());
                return;
            }
        }
    }
    // The standalone `--api` MessagePack stdio surface: only `hang-api` is
    // defined here (it never writes a frame); every other scenario refuses.
    if args.iter().any(|a| a == "--api") {
        if scenario == Scenario::HangApi {
            hang_forever();
        }
        std::process::exit(2);
    }
    // The declaration-stage CLI surface (`tsc --project <cfg> --declaration`):
    // `declfail` dies INSTANTLY with no output — an engine that validated but
    // fails the real invocation.
    if args.iter().any(|a| a == "--project") {
        std::process::exit(2);
    }
    if args.iter().any(|a| a == "--lsp") {
        match scenario {
            Scenario::Exit => std::process::exit(1),
            Scenario::HangLsp => hang_forever(),
            _ => serve_lsp(scenario),
        }
        return;
    }
    std::process::exit(2);
}

/// The pid file the `hold-pipe` grandchild writes (`<exe>.child.pid`).
fn hold_pipe_pid_file() -> PathBuf {
    let exe = std::env::current_exe().unwrap_or_default();
    let mut name = exe.into_os_string();
    name.push(".child.pid");
    PathBuf::from(name)
}

/// Never return (the process is expected to be killed by the bounded probe /
/// smoke under test).
fn hang_forever() -> ! {
    loop {
        std::thread::park();
    }
}

/// Spawn the pipe-holding grandchild (inherits this process's stdout/stderr so
/// the pipes stay open after this process exits) and return immediately.
fn spawn_pipe_holder() {
    let exe = std::env::current_exe().expect("current exe");
    let _ = std::process::Command::new(exe)
        .arg("--verter-fake-hold-pipe")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .expect("spawn the pipe-holding grandchild");
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
                let pipe = if scenario.serves_api_pipe() {
                    start_api_pipe_server(scenario)
                } else {
                    // The API capability smoke against a non-API fake is
                    // expected to FAIL at the pipe connect.
                    "/nonexistent/verter-tsgo-fake-pipe".to_string()
                };
                write_frame(
                    &mut stdout.lock(),
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "sessionId": "fake", "pipe": pipe }
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

/// Bind the `--api` attach pipe, spawn the thread serving it, and return the
/// pipe path the client connects to. The server accepts ONE connection and
/// answers `initialize` + `updateSnapshot` per the scenario.
fn start_api_pipe_server(scenario: Scenario) -> String {
    let (pipe_path, listener) = bind_api_pipe();
    let thread_path = pipe_path.clone();
    std::thread::spawn(move || {
        let (reader, writer) = accept_api_pipe(listener);
        serve_api_connection(scenario, reader, writer);
        cleanup_api_pipe(&thread_path);
    });
    pipe_path
}

/// The accepted-connection IO types differ per OS (UnixListener yields a
/// stream; the Windows named pipe is materialized as a `File`), unified here.
type PipeIo = (Box<dyn Read + Send>, Box<dyn Write + Send>);

#[cfg(unix)]
type ApiPipeListener = std::os::unix::net::UnixListener;

#[cfg(unix)]
fn bind_api_pipe() -> (String, ApiPipeListener) {
    let path = std::env::temp_dir().join(format!("verter-fake-api-{}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = std::os::unix::net::UnixListener::bind(&path).expect("bind fake --api UDS");
    (path.to_string_lossy().into_owned(), listener)
}

#[cfg(unix)]
fn accept_api_pipe(listener: ApiPipeListener) -> PipeIo {
    let (stream, _) = listener.accept().expect("accept fake --api UDS client");
    let reader = stream.try_clone().expect("clone fake --api UDS stream");
    (Box::new(reader), Box::new(stream))
}

#[cfg(unix)]
fn cleanup_api_pipe(pipe_path: &str) {
    let _ = std::fs::remove_file(pipe_path);
}

#[cfg(windows)]
type ApiPipeListener = std::fs::File;

#[cfg(windows)]
fn bind_api_pipe() -> (String, ApiPipeListener) {
    use std::os::windows::io::FromRawHandle;
    use windows_sys::Win32::System::Pipes::{
        CreateNamedPipeW, PIPE_ACCESS_DUPLEX, PIPE_READMODE_BYTE, PIPE_TYPE_BYTE, PIPE_WAIT,
    };

    let name = format!(r"\\.\pipe\verter-fake-api-{}", std::process::id());
    let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
    let handle = unsafe {
        CreateNamedPipeW(
            wide.as_ptr(),
            PIPE_ACCESS_DUPLEX,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT,
            1,
            65536,
            65536,
            0,
            std::ptr::null(),
        )
    };
    assert!(
        handle != windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE,
        "CreateNamedPipeW failed"
    );
    // ConnectNamedPipe runs in `accept_api_pipe` so the client connect can
    // race the server creation (mirrors the real tsgo server).
    (name, unsafe { std::fs::File::from_raw_handle(handle as _) })
}

#[cfg(windows)]
fn accept_api_pipe(pipe: ApiPipeListener) -> PipeIo {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::System::Pipes::ConnectNamedPipe;

    let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle() as _, std::ptr::null_mut()) };
    if connected == 0 {
        let err = std::io::Error::last_os_error();
        // ERROR_PIPE_CONNECTED (535): the client connected between creation
        // and ConnectNamedPipe — the documented success race.
        const ERROR_PIPE_CONNECTED: i32 = 535;
        assert!(
            err.raw_os_error() == Some(ERROR_PIPE_CONNECTED),
            "ConnectNamedPipe failed: {err}"
        );
    }
    let reader = pipe.try_clone().expect("clone fake --api named pipe");
    let writer = unsafe { std::fs::File::from_raw_handle(pipe.as_raw_handle()) };
    std::mem::forget(pipe); // ownership moved into reader/writer
    (Box::new(reader), Box::new(writer))
}

#[cfg(windows)]
fn cleanup_api_pipe(_pipe_path: &str) {
    // Named pipes vanish when the last handle closes.
}

/// Serve ONE accepted `--api` pipe connection: `initialize` plus
/// `updateSnapshot` per the scenario, until EOF.
fn serve_api_connection(
    scenario: Scenario,
    reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
) {
    let mut reader = BufReader::new(reader);
    while let Some(body) = read_frame(&mut reader) {
        let Ok(message) = serde_json::from_slice::<serde_json::Value>(&body) else {
            continue;
        };
        let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = message.get("id").cloned();
        let Some(id) = id else { continue };
        match method {
            "initialize" => {
                write_frame(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "useCaseSensitiveFileNames": true, "currentDirectory": "/" }
                    }),
                );
            }
            "updateSnapshot" => {
                let config = message
                    .get("params")
                    .and_then(|p| p.get("openProjects"))
                    .and_then(|o| o.as_array())
                    .and_then(|a| a.first())
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string();
                let projects = scenario.snapshot_projects(&config);
                write_frame(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "result": { "snapshot": 1, "projects": projects }
                    }),
                );
            }
            _ => {
                write_frame(
                    &mut writer,
                    &serde_json::json!({
                        "jsonrpc": "2.0",
                        "id": id,
                        "error": { "code": -32601, "message": "fake engine: unknown method" }
                    }),
                );
            }
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
