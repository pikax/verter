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
use std::process::{ExitCode, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::process::{Child, Command};

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
    let shim_exit = runtime.block_on(run());
    // Shut the runtime down BEFORE converting: a `ShimExit::Signal` re-raises, and we want
    // no tokio signal machinery / background threads alive when we restore the default
    // disposition and raise.
    drop(runtime);
    shim_exit.into_exit_code()
}

async fn run() -> ShimExit {
    let args = match parse_args() {
        Ok(args) => args,
        Err(message) => {
            eprintln!("verter-relay-shim: {message}");
            // Usage/argument error: a distinct non-zero code.
            return ShimExit::Code(2);
        }
    };

    // The relay contract is ONLY for `--lsp` stdio. A non-`--lsp` invocation is
    // passed through to the real tsgo unchanged (inherited stdio).
    if !args.forwarded.iter().any(|a| a == "--lsp") {
        return passthrough(&args).await;
    }

    match run_relay(args).await {
        Ok(exit) => exit,
        Err(message) => {
            eprintln!("verter-relay-shim: {message}");
            ShimExit::Code(1)
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

/// RAII ownership of the spawned real-tsgo child DURING `--lsp` setup.
///
/// The shim SPAWNED this child, so it owns its lifecycle. If any setup step AFTER the
/// spawn but BEFORE steady-state hand-off fails — nonce minting, endpoint-path creation,
/// control bind, advertisement write — the guard's `Drop` kills + synchronously reaps the
/// child so the real tsgo is NEVER orphaned. The guard is disarmed ([`into_inner`]) only
/// once the accept loop is running and steady-state teardown owns the child.
///
/// Kill semantics on setup failure are `start_kill` (SIGKILL on Unix, `TerminateProcess`
/// on Windows) — never a graceful SIGTERM: setup never reached steady state, so there is
/// no clean-shutdown contract to honor. `tokio::process::Child` has no async `Drop`, so
/// the reap is a synchronous `try_wait` poll (the least-bad RAII answer — a spawn-and-
/// forget async reaper is unreliable once `run()` returns and the runtime shuts down).
struct ChildSetupGuard {
    child: Option<Child>,
}

impl ChildSetupGuard {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }

    /// Borrow the guarded child (e.g. to take its piped stdio) while ARMED.
    fn child_mut(&mut self) -> &mut Child {
        self.child
            .as_mut()
            .expect("child is present until the steady-state hand-off")
    }

    /// Disarm the guard and hand the child to the steady-state owner. After this the
    /// guard's `Drop` is a no-op — teardown owns the child's status from here.
    fn into_inner(mut self) -> Child {
        self.child
            .take()
            .expect("child is present until the steady-state hand-off")
    }
}

impl Drop for ChildSetupGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return; // disarmed: steady-state teardown owns the child.
        };
        // Setup failed after spawn. Kill (if not already gone) and synchronously reap so
        // the child is never orphaned. SIGKILL / TerminateProcess is uncatchable, so the
        // poll terminates; a bounded cap is a defensive safety valve against a wedged reap
        // (leaking is strictly better than hanging the shim's own teardown forever).
        if let Ok(None) = child.try_wait() {
            let _ = child.start_kill();
        }
        for _ in 0..2000 {
            match child.try_wait() {
                Ok(Some(_)) | Err(_) => return,
                Ok(None) => std::thread::sleep(std::time::Duration::from_millis(1)),
            }
        }
    }
}

/// The shim's faithful process-exit outcome, converted to the OS exit at the outermost
/// [`main`]. On Unix a child (or the shim itself) terminated by a signal is reported AS
/// that signal — never masked as a clean exit.
#[derive(Debug)]
enum ShimExit {
    /// A normal exit with this status code.
    Code(u8),
    /// A Unix signal-termination, re-raised at [`main`] so the shim exits VIA the signal.
    #[cfg(unix)]
    Signal(i32),
}

impl ShimExit {
    /// Map a process [`ExitStatus`] to a faithful [`ShimExit`]: on Unix a signal-exit maps
    /// to [`ShimExit::Signal`] (via `ExitStatusExt::signal`), never a masked success.
    fn from_status(status: ExitStatus) -> Self {
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt;
            if let Some(sig) = status.signal() {
                return ShimExit::Signal(sig);
            }
        }
        ShimExit::Code(status.code().map(|code| (code & 0xff) as u8).unwrap_or(1))
    }

    /// Convert to the OS exit at the outermost [`main`]. A [`ShimExit::Signal`] restores
    /// the default disposition and re-raises, so the shim terminates with the SAME signal
    /// (a signal-killed engine / shim must not report a clean exit); the `128 + signo`
    /// fallback only applies if the re-raise somehow returns.
    fn into_exit_code(self) -> ExitCode {
        match self {
            ShimExit::Code(code) => ExitCode::from(code),
            #[cfg(unix)]
            ShimExit::Signal(sig) => {
                // SAFETY: restoring the default disposition (`signal`) and `raise` are
                // async-signal-safe libc calls, and the async runtime is already shut down.
                unsafe {
                    libc::signal(sig, libc::SIG_DFL);
                    libc::raise(sig);
                }
                ExitCode::from(128u8.wrapping_add((sig & 0xff) as u8))
            }
        }
    }
}

/// The FIRST teardown trigger observed by [`run_relay`]'s steady-state select.
enum Teardown {
    /// The editor disconnected (the relay stopped pumping).
    RelayStopped,
    /// The real tsgo child exited on its own.
    ChildExited(std::io::Result<ExitStatus>),
    /// The shim itself received a Unix shutdown signal.
    #[cfg(unix)]
    Signal(i32),
}

/// The Unix shutdown signals the shim handles for a faithful signal-exit teardown:
/// SIGINT/SIGTERM/SIGHUP/SIGQUIT. On any of them the shim tears down (advertisement +
/// listener + relay), kills + reaps its OWNED child, then re-raises the original signal.
#[cfg(unix)]
struct ShutdownSignals {
    streams: Vec<(i32, tokio::signal::unix::Signal)>,
}

#[cfg(unix)]
impl ShutdownSignals {
    /// Install the handlers. Must be called within a tokio runtime (each registers with
    /// the reactor).
    fn install() -> Result<Self, String> {
        use tokio::signal::unix::{signal, SignalKind};
        let specs = [
            (libc::SIGINT, SignalKind::interrupt()),
            (libc::SIGTERM, SignalKind::terminate()),
            (libc::SIGHUP, SignalKind::hangup()),
            (libc::SIGQUIT, SignalKind::quit()),
        ];
        let mut streams = Vec::with_capacity(specs.len());
        for (num, kind) in specs {
            let stream = signal(kind)
                .map_err(|e| format!("install signal handler for signal {num}: {e}"))?;
            streams.push((num, stream));
        }
        Ok(Self { streams })
    }

    /// Await the FIRST shutdown signal, returning its signal number.
    async fn recv(&mut self) -> i32 {
        std::future::poll_fn(|cx| {
            for (num, stream) in &mut self.streams {
                if stream.poll_recv(cx).is_ready() {
                    return std::task::Poll::Ready(*num);
                }
            }
            std::task::Poll::Pending
        })
        .await
    }
}

/// A SHORT grace check on editor disconnect: if the child has already exited (or exits
/// within the window), return its status so an engine crash is not masked as a clean
/// editor disconnect; otherwise `None` (the child is still alive and the shim owns the
/// kill).
async fn grace_check_child_exit(child: &mut Child) -> Option<ExitStatus> {
    match tokio::time::timeout(std::time::Duration::from_millis(200), child.wait()).await {
        Ok(Ok(status)) => Some(status),
        _ => None,
    }
}

/// Classify the child's FINAL status after a relay-stop kill. When the child outlived the grace
/// window WE own the kill, but the reaped status must still be inspected: a plain `SIGKILL` is OUR
/// kill (a clean editor-disconnect exit → `Code(0)`), whereas ANY other status means the child died
/// on its OWN in the race between the grace deadline and our kill (a self-signal or a non-zero
/// self-exit) — propagate THAT faithfully via [`ShimExit::from_status`] so an engine crash is never
/// masked as a clean disconnect. Blindly returning `Code(0)` here would hide exactly that crash.
fn shim_exit_after_relay_stop_kill(status: ExitStatus) -> ShimExit {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if status.signal() == Some(libc::SIGKILL) {
            ShimExit::Code(0)
        } else {
            ShimExit::from_status(status)
        }
    }
    #[cfg(not(unix))]
    {
        // On Windows `start_kill` is `TerminateProcess(1)`; a child exit code of 1 is
        // indistinguishable from a real crash exit, so a relay-stop reports a clean disconnect.
        let _ = status;
        ShimExit::Code(0)
    }
}

/// Pass a non-`--lsp` invocation straight through to the real tsgo with
/// inherited stdio, propagating its exit FAITHFULLY (a Unix signal-exit is re-raised at
/// [`main`], never masked as a code).
async fn passthrough(args: &ShimArgs) -> ShimExit {
    let status = Command::new(&args.real_tsgo)
        .args(&args.forwarded)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .await;
    match status {
        Ok(status) => ShimExit::from_status(status),
        Err(e) => {
            eprintln!(
                "verter-relay-shim: failed to spawn real tsgo {:?}: {e}",
                args.real_tsgo
            );
            ShimExit::Code(1)
        }
    }
}

/// Run the `--lsp` relay: spawn the real tsgo, wire the stdio relay, advertise +
/// serve the control endpoint, and tear down on the first shutdown trigger, returning the
/// faithful [`ShimExit`].
async fn run_relay(args: ShimArgs) -> Result<ShimExit, String> {
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

    // Install the Unix shutdown handlers BEFORE spawning the child, so from the INSTANT the child
    // exists a shutdown signal is caught (tokio buffers it) and drives guarded teardown instead of
    // killing the shim by the default (process-killing) disposition and orphaning the child.
    // Installing AFTER the spawn would leave a spawn→install window where a signal kills the shim
    // before the RAII guard can reap. An install failure returns BEFORE any child is spawned, so
    // it too can never orphan.
    #[cfg(unix)]
    let mut shutdown_signals = ShutdownSignals::install()?;

    let child = Command::new(&args.real_tsgo)
        .args(&args.forwarded)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| format!("spawn real tsgo {:?}: {e}", args.real_tsgo))?;

    // From here the child is owned by the RAII setup guard: any setup failure before the
    // steady-state hand-off below kills + reaps it, so the real tsgo is never orphaned. With the
    // handlers already installed, a signal delivered anywhere from the spawn through to the
    // post-accept disarm is buffered and observed by the steady-state select (which reaps the
    // child); any fallible setup step in between instead unwinds through this armed guard.
    let mut child_guard = ChildSetupGuard::new(child);

    let child_stdin = child_guard
        .child_mut()
        .stdin
        .take()
        .ok_or("real tsgo child stdin was not piped")?;
    let child_stdout = child_guard
        .child_mut()
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

    // Steady-state hand-off: the accept loop is running and every fallible setup step
    // (including the Unix signal-handler install above) has SUCCEEDED, so disarm the guard and
    // give the child to steady-state teardown. This disarm is the LAST step before the select
    // — NOTHING fallible follows it, so the child can never be dropped un-reaped.
    let mut child = child_guard.into_inner();

    // Tear down on the FIRST trigger: editor disconnect (relay stop), real tsgo exit, or a
    // Unix shutdown signal delivered to the shim. A Verter `verter/detach` NEVER triggers
    // teardown — it is non-destructive (retract overlays + drop the Verter control pipe). The
    // select is BIASED so a shutdown signal already delivered to the shim is re-raised
    // faithfully rather than losing to an also-ready relay/child arm, and a child that already
    // exited is observed AS a child exit (faithful status) rather than as a bare relay stop.
    let teardown = {
        #[cfg(unix)]
        {
            tokio::select! {
                biased;
                signum = shutdown_signals.recv() => Teardown::Signal(signum),
                status = child.wait() => Teardown::ChildExited(status),
                _ = relay.wait_stopped() => Teardown::RelayStopped,
            }
        }
        #[cfg(not(unix))]
        {
            tokio::select! {
                biased;
                status = child.wait() => Teardown::ChildExited(status),
                _ = relay.wait_stopped() => Teardown::RelayStopped,
            }
        }
    };

    // Teardown: stop accepting (dropping the listener — on Unix this removes the socket
    // file) and remove the advertisement, common to every teardown reason. The shim
    // SPAWNED this tsgo, so it owns THIS child's lifecycle; the editor's own relayed
    // `exit` already passed through transparently if the editor sent it.
    accept_task.abort();
    remove_advertisement(&advertisement_path);

    // SINGLE status owner: exactly one path reaps the child. Its faithful exit propagates
    // (a Unix signal-exit is re-raised at `main`) so an engine crash is never masked as a
    // clean exit or double-killed.
    let shim_exit = match teardown {
        // The child exited on its own — reap here, faithfully; do NOT kill/wait again.
        Teardown::ChildExited(status) => {
            let status = status.map_err(|e| format!("await real tsgo exit: {e}"))?;
            ShimExit::from_status(status)
        }
        // Editor disconnect: grace-check whether the child ALSO already exited (an engine
        // crash racing the disconnect); if so propagate ITS status, else WE own the kill —
        // a clean shim exit, not an engine failure.
        Teardown::RelayStopped => match grace_check_child_exit(&mut child).await {
            Some(status) => ShimExit::from_status(status),
            None => {
                // The child outlived the grace window, so WE own the kill. Reap the FINAL status
                // and classify it: a plain SIGKILL is OUR kill (a clean editor-disconnect exit), but
                // a child that died on its OWN in the race between the grace deadline and our kill
                // surfaces a different status — propagate THAT faithfully rather than masking an
                // engine crash as a clean Code(0) disconnect.
                let _ = child.start_kill();
                match child.wait().await {
                    Ok(status) => shim_exit_after_relay_stop_kill(status),
                    // Could not reap the final status; the clean editor-disconnect exit stands.
                    Err(_) => ShimExit::Code(0),
                }
            }
        },
        // The shim itself was signaled: kill + reap the OWNED child (no orphan), then
        // report the signal so `main` re-raises it (faithful signal-exit).
        #[cfg(unix)]
        Teardown::Signal(signum) => {
            let _ = child.start_kill();
            let _ = child.wait().await;
            ShimExit::Signal(signum)
        }
    };
    relay.shutdown().await;
    Ok(shim_exit)
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

    /// F1 — the Unix shutdown-signal install MUST precede BOTH the child spawn AND the guard
    /// disarm, and NOTHING fallible may sit between the disarm and the steady-state select. An
    /// install AFTER the spawn leaves a spawn→install window: a signal in that window kills the
    /// shim by the default disposition before the RAII guard can reap, orphaning the child. An
    /// install after the `into_inner` disarm leaves the same orphan window at the tail. The runtime
    /// orphan test cannot portably inject a signal into that microscopic window, so this is a
    /// source-structure guard on the ordering invariant. Anchored within `run_relay` so the
    /// passthrough `Command` (which uses `.status()`, not `.spawn()`) is never mistaken for the
    /// relay child spawn.
    #[test]
    fn shutdown_signal_install_precedes_spawn_and_disarm() {
        let src = include_str!("main.rs");
        let run_relay = src
            .find("async fn run_relay(")
            .expect("run_relay is present");
        let region = &src[run_relay..];
        let install = region
            .find("ShutdownSignals::install()")
            .expect("the shutdown-signal install call site is present in run_relay");
        let spawn = region
            .find(".spawn()")
            .expect("the real-tsgo child spawn is present in run_relay");
        let disarm = region
            .find("child_guard.into_inner()")
            .expect("the guard disarm hand-off is present in run_relay");
        // The handlers must install BEFORE the child spawn — from the instant the child exists a
        // signal must be caught, never able to kill the shim by default and orphan the child.
        assert!(
            install < spawn,
            "the Unix shutdown-signal install (byte {install}) must run BEFORE the real-tsgo child \
             spawn (byte {spawn}) so no spawn→install window can orphan the child on a signal"
        );
        // ...and before the guard disarm, so a failed install or a setup-window signal unwinds
        // through the ARMED guard and never orphans the child.
        assert!(
            install < disarm,
            "the shutdown-signal install (byte {install}) must run BEFORE the guard disarm \
             (byte {disarm})"
        );
        // No fallible `?` may sit between the disarm and the steady-state select — the disarm
        // is the last fallible-free hand-off. Strip line comments so a prose `?` never trips it.
        let after_disarm = &region[disarm..];
        let select_off = after_disarm
            .find("let teardown =")
            .expect("the steady-state teardown select follows the disarm");
        let between_code: String = after_disarm[..select_off]
            .lines()
            .map(|line| line.split("//").next().unwrap_or(""))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !between_code.contains('?'),
            "no fallible `?` may sit between the guard disarm and the steady-state select; \
             found one in: {between_code:?}"
        );
    }

    /// F7(a) — the Unix teardown select must be `biased` with the shutdown-signal arm FIRST,
    /// so a signal already delivered to the shim is re-raised faithfully rather than losing to
    /// an also-ready relay/child arm and exiting by code. An unbiased select (or a
    /// signal-arm-last ordering) can drop a pending SIGTERM. Source-structure guard on the
    /// tie-break ordering (a select! tie is not deterministically forceable at runtime).
    #[test]
    fn teardown_select_prioritizes_the_shutdown_signal() {
        let src = include_str!("main.rs");
        let signal_arm = src
            .find("signum = shutdown_signals.recv()")
            .expect("the unix teardown signal arm is present");
        let biased = src[..signal_arm]
            .rfind("biased;")
            .expect("the unix teardown select is `biased`");
        let child_arm = src[signal_arm..]
            .find("status = child.wait()")
            .map(|off| signal_arm + off)
            .expect("the child-exit arm follows the signal arm");
        assert!(
            biased < signal_arm,
            "the teardown select must be `biased` (byte {biased}) before the signal arm \
             (byte {signal_arm})"
        );
        assert!(
            signal_arm < child_arm,
            "the shutdown-signal arm (byte {signal_arm}) must be polled BEFORE the child/relay \
             arms (byte {child_arm}) — biased signal-priority"
        );
    }

    /// G5 — the relay-stop kill classifier returns a clean `Code(0)` ONLY for OUR SIGKILL; a child
    /// that died on its OWN (a self-signal or a non-zero self-exit in the race just after the grace
    /// deadline) is propagated faithfully, never masked as a clean disconnect. UNIX-ONLY (constructs
    /// raw wait statuses via `ExitStatusExt::from_raw`); the microscopic post-grace race is not
    /// portably inducible in a live test, so the decision function is exercised directly.
    ///
    /// Discriminating-by-construction: a classifier that blindly returns `Code(0)` after the
    /// relay-stop kill (the pre-fix branch) FAILS the SIGTERM + non-zero-exit cases below.
    #[cfg(unix)]
    #[test]
    fn relay_stop_kill_classifier_propagates_child_self_death_not_code_zero() {
        use std::os::unix::process::ExitStatusExt;

        // OUR SIGKILL of a still-alive child → a clean editor-disconnect exit.
        let killed = ExitStatus::from_raw(libc::SIGKILL);
        assert!(
            matches!(shim_exit_after_relay_stop_kill(killed), ShimExit::Code(0)),
            "our SIGKILL of a still-alive child is a clean Code(0) disconnect"
        );

        // The child self-signalled (an engine crash via SIGTERM) → re-raise the signal, not Code(0).
        let self_signalled = ExitStatus::from_raw(libc::SIGTERM);
        assert!(
            matches!(
                shim_exit_after_relay_stop_kill(self_signalled),
                ShimExit::Signal(sig) if sig == libc::SIGTERM
            ),
            "a child that died from its OWN SIGTERM must be re-raised, not masked as Code(0)"
        );

        // The child self-exited non-zero (a crash exit code) → propagate the code, not Code(0).
        // A raw wait status encodes the exit code in bits 8..16 (`code << 8`).
        let self_exited = ExitStatus::from_raw(42 << 8);
        assert!(
            matches!(
                shim_exit_after_relay_stop_kill(self_exited),
                ShimExit::Code(42)
            ),
            "a child that self-exited non-zero must propagate that code, not Code(0)"
        );
    }
}
