//! The Verter tsgo relay shim.
//!
//! The editor is pointed (via `typescript.native-preview.tsdk`) at this shim as
//! its `tsgo`. The editor spawns the shim; the shim spawns the REAL `tsgo` and
//! relays the editor↔tsgo `--lsp` stdio, owning the carrier egress taint. A
//! SEPARATE `verter_lsp` process — which holds the compiled carrier overlays —
//! drives carrier injection over a versioned CONTROL endpoint the shim exposes,
//! never a raw wire.
//!
//! The shim stays DUMB by contract: relay + egress + control + injection ONLY —
//! NO Vue/Svelte parsing, NO prop walker, NO source mapping, NO semantic TS
//! service. Those belong to `verter_lsp` (which owns `--api` queries + source
//! mapping) and to the shared resolver.
//!
//! ## CLI
//!
//! ```text
//! verter-relay-shim --real-tsgo <path> --control-dir <dir> --session-key <key> -- <tsgo --lsp args...>
//! ```
//!
//! - `--real-tsgo` may also come from `VERTER_RELAY_REAL_TSGO`, so a `tsdk`
//!   wrapper can supply it from config.
//! - Everything after `--` is forwarded to the real tsgo verbatim.
//! - A non-`--lsp` invocation (e.g. `--version`) is passed through to the real
//!   tsgo unchanged (inherited stdio) — the relay contract is only for `--lsp`
//!   stdio.
//!
//! ## Lifecycle
//!
//! On `--lsp` startup the shim spawns `<real-tsgo> <forwarded args>`, wires the
//! stdio relay, mints a rendezvous nonce + editor-session generation, binds a
//! local control endpoint, writes an advertisement into `--control-dir`, and
//! serves the control protocol. It tears down on the FIRST of: the editor
//! disconnecting (relay stop) or the real tsgo exiting. The shim SPAWNED this
//! tsgo, so it owns THIS child's lifecycle and kills it on teardown; it never
//! ORIGINATES `exit`/`shutdown` toward an editor-owned engine (the editor's own
//! relayed `exit` passes through transparently). A Verter `verter/detach` is
//! NON-DESTRUCTIVE: it retracts Verter's overlays and closes the Verter control
//! connection ONLY, leaving the editor↔tsgo relay AND the tsgo child ALIVE — it
//! never tears the shim (or its child) down.

use std::path::PathBuf;
use std::process::{ExitCode, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::process::Command;

use verter_tsgo_api::control::messages::PROTOCOL_VERSION;
use verter_tsgo_api::control::{
    control_endpoint_path, remove_advertisement, stable_hash_str, Advertisement, ControlListener,
    ControlServer, ADVERTISEMENT_VERSION,
};
use verter_tsgo_api::proto::schema_manifest::PINNED;
use verter_tsgo_api::relay::LspRelay;

/// The env var a `tsdk` wrapper can use to supply the real tsgo path instead of
/// `--real-tsgo`.
const REAL_TSGO_ENV: &str = "VERTER_RELAY_REAL_TSGO";

/// The parsed shim CLI. `control_dir` / `session_key` are the CONTROL rendezvous args
/// required ONLY by the `--lsp` relay path; a non-`--lsp` passthrough invocation (a
/// probe such as `--version`) does not require them, so they are optional here and
/// enforced at the `--lsp` branch in [`run_relay`].
#[derive(Debug)]
struct ShimArgs {
    real_tsgo: PathBuf,
    control_dir: Option<PathBuf>,
    session_key: Option<String>,
    /// The args forwarded to the real tsgo verbatim (everything after `--`).
    forwarded: Vec<String>,
}

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("verter-relay-shim: failed to start async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    runtime.block_on(run())
}

async fn run() -> ExitCode {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("verter-relay-shim: {message}");
            // Usage/argument error: a distinct non-zero code.
            return ExitCode::from(2);
        }
    };

    // The relay contract is ONLY for `--lsp` stdio. A non-`--lsp` invocation is
    // passed through to the real tsgo unchanged (inherited stdio).
    if !args.forwarded.iter().any(|a| a == "--lsp") {
        return passthrough(&args).await;
    }

    match run_relay(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("verter-relay-shim: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Parse the shim CLI, falling back to [`REAL_TSGO_ENV`] for `--real-tsgo`.
fn parse_args() -> Result<ShimArgs, String> {
    parse_args_from(std::env::args().skip(1))
}

/// Parse the shim CLI from an explicit token stream (the args after the program name).
/// Split out from [`parse_args`] so the CLI contract is unit-testable without the
/// process argv. The CONTROL rendezvous args are NOT required here — only the `--lsp`
/// relay path enforces them ([`run_relay`]) — so a non-`--lsp` passthrough probe
/// (`--real-tsgo <path> -- --version`) parses cleanly instead of erroring.
fn parse_args_from(mut args: impl Iterator<Item = String>) -> Result<ShimArgs, String> {
    let mut real_tsgo: Option<String> = None;
    let mut control_dir: Option<String> = None;
    let mut session_key: Option<String> = None;
    let mut forwarded: Vec<String> = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--real-tsgo" => real_tsgo = Some(expect_value(&mut args, "--real-tsgo")?),
            "--control-dir" => control_dir = Some(expect_value(&mut args, "--control-dir")?),
            "--session-key" => session_key = Some(expect_value(&mut args, "--session-key")?),
            "--" => {
                forwarded.extend(args.by_ref());
                break;
            }
            other => return Err(format!("unknown argument {other:?}")),
        }
    }

    // `--real-tsgo` is required for BOTH paths (passthrough spawns it too); the CONTROL
    // rendezvous args are validated later, only on the `--lsp` relay path.
    let real_tsgo = real_tsgo
        .or_else(|| std::env::var(REAL_TSGO_ENV).ok())
        .ok_or_else(|| format!("missing --real-tsgo (or {REAL_TSGO_ENV})"))?;

    Ok(ShimArgs {
        real_tsgo: PathBuf::from(real_tsgo),
        control_dir: control_dir.map(PathBuf::from),
        session_key,
        forwarded,
    })
}

/// Read the value following a flag, erroring if the flag is the last token.
fn expect_value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<String, String> {
    args.next()
        .ok_or_else(|| format!("{flag} requires a value"))
}

/// Pass a non-`--lsp` invocation straight through to the real tsgo with
/// inherited stdio, propagating its exit code.
async fn passthrough(args: &ShimArgs) -> ExitCode {
    let status = Command::new(&args.real_tsgo)
        .args(&args.forwarded)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await;
    match status {
        Ok(status) => {
            let code = status.code().unwrap_or(0);
            ExitCode::from(u8::try_from(code & 0xff).unwrap_or(1))
        }
        Err(e) => {
            eprintln!(
                "verter-relay-shim: failed to spawn real tsgo {:?}: {e}",
                args.real_tsgo
            );
            ExitCode::FAILURE
        }
    }
}

/// Run the `--lsp` relay: spawn the real tsgo, wire the stdio relay, advertise +
/// serve the control endpoint, and tear down on the first shutdown trigger.
async fn run_relay(args: ShimArgs) -> Result<(), String> {
    // The `--lsp` relay path REQUIRES the CONTROL rendezvous args (a non-`--lsp`
    // passthrough probe does not — that branch is taken in `run` before here).
    let control_dir = args
        .control_dir
        .as_deref()
        .ok_or("missing --control-dir (required for the --lsp relay)")?;
    let session_key = args
        .session_key
        .as_deref()
        .ok_or("missing --session-key (required for the --lsp relay)")?;

    let mut child = Command::new(&args.real_tsgo)
        .args(&args.forwarded)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn real tsgo {:?}: {e}", args.real_tsgo))?;

    let child_stdin = child
        .stdin
        .take()
        .ok_or("real tsgo child stdin was not piped")?;
    let child_stdout = child
        .stdout
        .take()
        .ok_or("real tsgo child stdout was not piped")?;

    // The relay: editor side = this process's stdio; server side = the child.
    let relay = Arc::new(LspRelay::start(
        tokio::io::stdin(),
        tokio::io::stdout(),
        child_stdout,
        child_stdin,
    ));

    // Rendezvous witnesses.
    let pid = std::process::id();
    let nonce = mint_nonce()?;
    let editor_session_generation = mint_generation();
    let wire_pin = PINNED.wire_fingerprint();
    let disambiguator = format!("{:016x}", stable_hash_str(&nonce));

    // Bind the control endpoint and record its actual path in the advertisement.
    let endpoint = control_endpoint_path(control_dir, session_key, pid, &disambiguator);
    let mut listener =
        ControlListener::bind(&endpoint).map_err(|e| format!("bind control endpoint: {e}"))?;
    let endpoint = listener.endpoint().to_string();

    let real_tsgo_str = args.real_tsgo.to_string_lossy().into_owned();
    let advertisement = Advertisement {
        advertisement_version: ADVERTISEMENT_VERSION,
        protocol: PROTOCOL_VERSION,
        endpoint,
        nonce: nonce.clone(),
        pid,
        session_key: session_key.to_string(),
        real_tsgo: real_tsgo_str.clone(),
        real_tsgo_hash: stable_hash_str(&real_tsgo_str),
        wire_pin,
        editor_session_generation,
    };
    let advertisement_path = advertisement
        .write(control_dir)
        .map_err(|e| format!("write advertisement: {e}"))?;

    // The control accept loop: a fresh control server per accepted connection,
    // all sharing the ONE relay. A `verter/detach` closes ONLY its own control
    // connection (non-destructive); the shim's teardown is owned by the editor /
    // real-tsgo lifecycle below, never by a Verter control message.
    let relay_for_accept = Arc::clone(&relay);
    let accept_task = tokio::spawn(async move {
        let session_counter = AtomicU64::new(0);
        // Accept until the listener stops (a listener error ends the loop).
        while let Ok((read, write)) = listener.accept().await {
            let n = session_counter.fetch_add(1, Ordering::Relaxed);
            let server = ControlServer::new(
                Arc::clone(&relay_for_accept),
                nonce.clone(),
                editor_session_generation,
                wire_pin,
                format!("ctl-{pid}-{n}"),
            );
            tokio::spawn(server.serve(read, write));
        }
    });

    // Tear down on the FIRST trigger: editor disconnect (relay stop) or real tsgo
    // exit. A Verter `verter/detach` NEVER triggers teardown — it is non-destructive
    // (retract overlays + drop the Verter control pipe only).
    tokio::select! {
        _ = relay.wait_stopped() => {}
        status = child.wait() => { let _ = status; }
    }

    // Teardown: stop accepting (dropping the listener — on Unix this removes the
    // socket file), remove the advertisement, and kill the child. The shim
    // SPAWNED this tsgo, so it owns THIS child's lifecycle; the editor's own
    // relayed `exit` already passed through transparently if the editor sent it.
    accept_task.abort();
    remove_advertisement(&advertisement_path);
    let _ = child.start_kill();
    let _ = child.wait().await;
    relay.shutdown().await;
    Ok(())
}

/// Mint the rendezvous nonce from 32 bytes of OS CSPRNG entropy (256-bit,
/// hex-encoded). The nonce prevents stale/accidental cross-attach on same-user local
/// IPC; CSPRNG entropy makes it unguessable rather than merely unique. Fails CLOSED
/// (the shim refuses to start) if the OS entropy source is unavailable — never falls
/// back to a weak nonce.
fn mint_nonce() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|e| format!("OS CSPRNG unavailable for the rendezvous nonce: {e}"))?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// Mint the editor-session generation: a process-local monotone-ish rendezvous witness
/// mixing wall-clock nanoseconds with the pid, unique per shim start so a reconnect
/// (a fresh shim) advertises a distinct generation.
fn mint_generation() -> u64 {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    nanos ^ (u64::from(std::process::id()) << 32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens(args: &[&str]) -> std::vec::IntoIter<String> {
        args.iter()
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
            .into_iter()
    }

    fn is_lsp(args: &ShimArgs) -> bool {
        args.forwarded.iter().any(|a| a == "--lsp")
    }

    /// A non-`--lsp` passthrough probe parses WITHOUT requiring the control rendezvous
    /// args and routes to passthrough — not the relay, and NOT an arg error (the
    /// advertised transparent-`tsc`-for-probes behaviour). Pre-fix this errored on the
    /// missing `--control-dir`/`--session-key`.
    #[test]
    fn non_lsp_probe_parses_and_routes_to_passthrough() {
        let args = parse_args_from(tokens(&["--real-tsgo", "/opt/tsgo", "--", "--version"]))
            .expect("a `--real-tsgo X -- --version` probe must parse WITHOUT control args");
        assert_eq!(args.real_tsgo, PathBuf::from("/opt/tsgo"));
        assert!(
            args.control_dir.is_none(),
            "a probe requires no --control-dir"
        );
        assert!(
            args.session_key.is_none(),
            "a probe requires no --session-key"
        );
        assert_eq!(args.forwarded, vec!["--version".to_string()]);
        assert!(!is_lsp(&args), "no --lsp ⇒ the passthrough route is taken");
    }

    /// The `--lsp` relay path parses the control rendezvous args and is detected as the
    /// relay route (the control args are enforced in `run_relay`).
    #[test]
    fn lsp_relay_parses_with_control_args() {
        let args = parse_args_from(tokens(&[
            "--real-tsgo",
            "/opt/tsgo",
            "--control-dir",
            "/tmp/ctl",
            "--session-key",
            "sess-1",
            "--",
            "--lsp",
            "--stdio",
        ]))
        .expect("the full --lsp relay invocation must parse");
        assert_eq!(args.control_dir, Some(PathBuf::from("/tmp/ctl")));
        assert_eq!(args.session_key.as_deref(), Some("sess-1"));
        assert_eq!(
            args.forwarded,
            vec!["--lsp".to_string(), "--stdio".to_string()]
        );
        assert!(is_lsp(&args), "--lsp ⇒ the relay route is taken");
    }

    /// The CLI contract shape is preserved: an unknown flag BEFORE `--` is still an
    /// arg error (only the control-arg requirement moved to the `--lsp` path).
    #[test]
    fn unknown_flag_before_dashdash_is_still_an_error() {
        let err = parse_args_from(tokens(&["--real-tsgo", "/opt/tsgo", "--bogus"]))
            .expect_err("an unknown flag before -- must still error");
        assert!(
            err.contains("bogus"),
            "the error must name the unknown arg; got {err:?}"
        );
    }
}
