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

/// The stable ASCII identity marker embedded in the shim binary so a packaging step can prove a
/// candidate file's BYTES are the Verter relay shim (not a renamed `tsgo` or an unrelated binary) by
/// scanning for the pinned `VERTER_RELAY_SHIM_IDENTITY:v1:` prefix. The prefix is a CLOSED contract
/// the packaging scanner greps for — it must not drift. The `--verter-shim-identity` handler prints
/// this string (a reachable reference that keeps the literal), and [`SHIM_IDENTITY_MARKER`] pins its
/// bytes in `.rodata` as a second retention.
const SHIM_IDENTITY: &str = concat!("VERTER_RELAY_SHIM_IDENTITY:v1:", env!("CARGO_PKG_VERSION"));

/// Belt-and-suspenders retention of [`SHIM_IDENTITY`]'s bytes in the shipped binary: `#[used]` keeps
/// this static — and hence the literal it references — through compiler + linker dead-code
/// elimination even if the print handler were ever removed, guaranteeing the marker is always
/// scannable in the emitted bytes.
#[used]
static SHIM_IDENTITY_MARKER: &[u8] = SHIM_IDENTITY.as_bytes();

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
    // Shut the runtime down BEFORE exiting: a `ShimExit::Signal` re-raises, and we want no tokio
    // signal machinery / background threads alive when we restore the default disposition and raise.
    drop(runtime);
    shim_exit.exit()
}

/// Whether the shim's OWN top-level args request the hidden identity probe
/// (`--verter-shim-identity`). The scan STOPS at the first `--` separator, so a real-tsgo arg
/// forwarded after `--` can NEVER trigger the probe — the reserved flag is recognized ONLY among the
/// shim's own args, never in the forwarded engine argv. Split out as a pure function so this narrow
/// contract is unit-testable without the process argv.
fn is_identity_probe(args: impl Iterator<Item = String>) -> bool {
    args.take_while(|a| a != "--")
        .any(|a| a == "--verter-shim-identity")
}

async fn run() -> ShimExit {
    // The hidden identity probe: print the embedded identity marker and exit 0. Recognized ONLY
    // among the shim's OWN top-level args ([`is_identity_probe`] stops the scan at the first `--`),
    // so a real-tsgo arg forwarded after `--` can never trigger it. Checked BEFORE arg parsing (it
    // needs no `--real-tsgo`) and BEFORE the relay / passthrough paths, so a packaging identity scan
    // never has to supply engine args.
    if is_identity_probe(std::env::args().skip(1)) {
        println!("{SHIM_IDENTITY}");
        return ShimExit::Code(0);
    }

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

/// The spawned real-tsgo child. The child is contained at spawn on EVERY platform — its OWN
/// session/process-group on Unix, the shim's job on Windows — so this is a thin wrapper that adds
/// only the cooperative contained-kill; it holds no per-child job handle.
///
/// OS-level containment is the PRIMARY "no orphaned tsgo" guarantee; the cooperative RAII
/// ([`ChildSetupGuard`]) + Unix signal-handler paths are the BACKSTOP for graceful teardown. The
/// hard case — the shim itself `SIGKILL`ed or hard-crashing so NEITHER `Drop` NOR a signal handler
/// can run — is closed by a kernel parent-death primitive INTRINSIC AT SPAWN on Linux and Windows
/// ONLY; macOS/BSD have NO such primitive, so the hard case there falls to the best-effort
/// cooperative backstop (third bullet):
///
/// - **Linux**: the child spawns with `PR_SET_PDEATHSIG = SIGKILL` (armed in `pre_exec`), so the
///   kernel kills it the instant the shim dies, even on an uncatchable `SIGKILL` of the shim.
/// - **Windows**: the shim assigns ITSELF to a `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` Job Object
///   BEFORE spawning tsgo (see [`create_kill_on_close_job_and_self_assign`]); on Win8+ a job
///   member's children join the job at CREATION (no breakaway limit is set), so tsgo is BORN into
///   the kill-on-close job with ZERO spawn→assign window. The shim's sole job handle is held for the
///   process lifetime and closed by the OS at exit — normal OR `TerminateProcess` — firing
///   `KILL_ON_JOB_CLOSE` and reaping the child.
/// - **Unix generally** (incl. macOS/BSD): the child spawns in its OWN session/process-group
///   (`setsid`, falling back to `setpgid`), so cooperative teardown group-kills the whole subtree.
///   macOS/BSD have NO parent-death primitive, so a HARD-killed shim there relies on the RAII /
///   signal path (best-effort) — there is no kernel reap of an orphan on those platforms. Closing
///   that window needs a watcher that OUTLIVES the shim — a surviving supervisor PROCESS, or the
///   child watching the parent via `kqueue`/`EVFILT_PROC`/`NOTE_EXIT` — because an in-shim thread
///   cannot act after the shim's own uncatchable `SIGKILL`.
struct OwnedChild {
    child: Child,
}

impl OwnedChild {
    /// Wrap a freshly spawned child. On BOTH platforms the child is ALREADY contained at spawn: on
    /// Unix its own session/process-group (and on Linux `PR_SET_PDEATHSIG`) armed in `pre_exec`; on
    /// Windows it is born into the shim's kill-on-close Job Object (the shim self-assigned to it
    /// BEFORE the spawn), so there is no post-spawn containment step that could fail.
    fn new(child: Child) -> Self {
        Self { child }
    }

    /// Start an uncatchable kill of the child. On Unix this ALSO group-kills the child's process
    /// group (its own session/group after `setsid`), reaping any tsgo grandchildren; `SIGKILL` is
    /// still delivered to the direct child, so a subsequent `wait()` observes a `SIGKILL`
    /// signal-exit exactly as a plain `start_kill()` would. On Windows this is the cooperative
    /// `TerminateProcess` (the Job Object is the hard-kill backstop, not the cooperative kill).
    fn start_contained_kill(&mut self) -> std::io::Result<()> {
        #[cfg(unix)]
        {
            if let Some(pid) = self.child.id() {
                // The child is its own process-group leader (setsid/setpgid in `pre_exec`), so its
                // pgid == pid; group-killing reaps the whole subtree.
                // SAFETY: killpg with the child's own pgid + SIGKILL is async-signal-safe and only
                // targets the child's own group.
                unsafe {
                    libc::killpg(pid as libc::pid_t, libc::SIGKILL);
                }
            }
        }
        // The guaranteed direct-child kill (and the sole kill on Windows).
        self.child.start_kill()
    }
}

impl std::ops::Deref for OwnedChild {
    type Target = Child;
    fn deref(&self) -> &Child {
        &self.child
    }
}

impl std::ops::DerefMut for OwnedChild {
    fn deref_mut(&mut self) -> &mut Child {
        &mut self.child
    }
}

/// An owning RAII wrapper for a Job Object handle (Windows), used ONLY while
/// [`create_kill_on_close_job_and_self_assign`] sets the job up. On the error paths there — BEFORE
/// the shim self-assigns — its `Drop` closes the handle, which is safe because the shim is not yet a
/// job member. On SUCCESS the handle is deliberately LEAKED (`std::mem::forget`), never reaching this
/// `Drop`: the shim IS a member of this `KILL_ON_JOB_CLOSE` job, so closing the last handle WHILE THE
/// SHIM IS ALIVE would fire the kill against the shim itself. The handle is instead held for the
/// whole process lifetime and closed by the OS at exit — normal OR `TerminateProcess` — which is
/// exactly when `KILL_ON_JOB_CLOSE` should reap any surviving child.
#[cfg(windows)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        // Reached ONLY on a setup error before the shim self-assigns (a success `mem::forget`s the
        // handle). SAFETY: `self.0` is a job handle we own from `CreateJobObjectW`; closing it once
        // is valid and releases the (memberless) job.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

/// Create the kill-on-close Job Object and assign THIS shim process to it — called BEFORE spawning
/// the real tsgo. On Win8+ a child created by a job member joins the member's job at CREATION (we set
/// no breakaway limit), so the tsgo spawned NEXT is BORN into this `KILL_ON_JOB_CLOSE` job with ZERO
/// spawn→assign window: a `TerminateProcess` of the shim can never leave an already-spawned tsgo
/// outside the job. `tokio::process::Child` is used unchanged (no raw `CreateProcessW`) — the
/// inheritance is automatic.
///
/// On SUCCESS the job handle is LEAKED (held for the process lifetime, closed by the OS at exit): the
/// shim is itself a member of this kill-on-close job, so closing the last handle while alive would
/// fire the kill against the shim. The OS closing it at exit — a normal exit OR a hard
/// `TerminateProcess` — is precisely when `KILL_ON_JOB_CLOSE` should reap any surviving child. On
/// FAILURE the handle is closed via the [`JobHandle`] RAII guard (the shim is not yet a member, so
/// closing is safe) and the error propagates so setup fails closed BEFORE any child is spawned.
#[cfg(windows)]
fn create_kill_on_close_job_and_self_assign() -> Result<(), String> {
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    // A NULL name + NULL attributes create an unnamed, NON-inheritable job: no child inherits the
    // HANDLE (only job MEMBERSHIP, by default association), so closing our SOLE handle is what
    // triggers KILL_ON_JOB_CLOSE.
    // SAFETY: FFI per the Win32 contract; a NULL return is the documented failure sentinel.
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(format!(
            "CreateJobObjectW failed: {}",
            std::io::Error::last_os_error()
        ));
    }
    // Own the handle NOW so every early return below closes it (the shim is not yet a member).
    let job = JobHandle(job);

    let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: `info` is a fully-initialized extended-limit struct; we pass its exact byte size.
    let set = unsafe {
        SetInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::addr_of!(info).cast(),
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    if set == 0 {
        return Err(format!(
            "SetInformationJobObject failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    // Assign the SHIM ITSELF to the job. On Win8+ the tsgo spawned NEXT joins this job at creation (a
    // job member's children are associated by default; no breakaway limit is set), closing the
    // spawn→assign window entirely.
    // SAFETY: GetCurrentProcess returns a pseudo-handle to this process; assigning it places THIS
    // process — and, by default association, its future children — under KILL_ON_JOB_CLOSE.
    let assigned = unsafe { AssignProcessToJobObject(job.0, GetCurrentProcess()) };
    if assigned == 0 {
        return Err(format!(
            "AssignProcessToJobObject(self) failed: {}",
            std::io::Error::last_os_error()
        ));
    }

    // Success: the shim is now a job member. LEAK the handle — it must stay open for the whole shim
    // lifetime and be closed by the OS at process exit; closing it while the shim runs would fire
    // KILL_ON_JOB_CLOSE against the shim itself.
    std::mem::forget(job);
    Ok(())
}

/// Contain a freshly-forked real-tsgo child before it `exec`s (Unix), bounding its lifecycle to the
/// shim's. Runs IN the forked child, so it MUST call ONLY async-signal-safe libc functions.
///
/// - `setsid()` puts the child in its OWN session + process-group (it becomes the group leader), so
///   cooperative teardown can `killpg` the whole subtree; on failure (already a group leader — not
///   possible for a fresh fork, but defensive) it falls back to `setpgid(0, 0)`. If BOTH fail the
///   child leads NO group of its own (its pgid stays inherited, not its own pid), so teardown's
///   `killpg(child_pid)` MISSES the child's subtree — no group carries that pgid, leaving only the
///   direct-child `start_kill`, so tsgo grandchildren could survive as orphans. The child
///   `_exit(127)`s rather than run a subtree teardown cannot reap.
/// - **Linux**: `PR_SET_PDEATHSIG = SIGKILL` makes the kernel kill the child when the shim dies —
///   the hard-kill orphan guarantee. It is then re-checked against the pre-fork parent pid: if the
///   parent ALREADY died in the fork→prctl window, `getppid()` no longer matches and the child
///   `_exit(127)`s rather than running orphaned.
#[cfg(unix)]
fn contain_child_unix(parent_pid: u32) -> std::io::Result<()> {
    // SAFETY: executed in the forked child before exec; every call here is async-signal-safe.
    unsafe {
        // Put the child in its OWN process group. `setsid` (new session + group leader) is the
        // primary path; if it fails, fall back to `setpgid(0, 0)`. If BOTH fail the child leads no
        // group of its own, so teardown's `killpg(child_pid)` MISSES the child's subtree (no group
        // carries that pgid) — only the direct-child kill lands, so tsgo grandchildren could survive
        // as orphans. Refuse to run rather than leave the subtree unreapable.
        if libc::setsid() == -1 && libc::setpgid(0, 0) == -1 {
            libc::_exit(127);
        }
        #[cfg(target_os = "linux")]
        {
            if libc::prctl(
                libc::PR_SET_PDEATHSIG,
                libc::SIGKILL as libc::c_ulong,
                0,
                0,
                0,
            ) == -1
            {
                libc::_exit(127);
            }
            // Close the fork→prctl race: if the parent already died, PDEATHSIG was armed against a
            // subreaper (or nothing), so refuse to run orphaned.
            if libc::getppid() != parent_pid as libc::pid_t {
                libc::_exit(127);
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            // Non-Linux Unix has no parent-death signal; the process-group containment above plus
            // the cooperative RAII / signal path are the (best-effort) guarantee. `parent_pid` is
            // consumed only by the Linux pdeathsig re-check.
            let _ = parent_pid;
        }
    }
    Ok(())
}

/// RAII ownership of the spawned real-tsgo [`OwnedChild`] DURING `--lsp` setup.
///
/// The intrinsic-at-spawn HARD-kill orphan guarantee is Linux (`PR_SET_PDEATHSIG`) + Windows (the
/// self-assigned Job Object) ONLY — there the kernel reaps the child even if this guard never runs.
/// On macOS/BSD the `pre_exec` `setsid` gives process-group CONTAINMENT only (no parent-death
/// primitive), so for the hard-kill case there THIS RAII guard, together with the Unix
/// signal-handler path, IS the (best-effort) orphan backstop — best-effort because it cannot run on
/// a `SIGKILL`/hard-crash of the shim. This guard is the cooperative-teardown BACKSTOP in every
/// teardown. The shim SPAWNED this child, so it owns its lifecycle. If any setup step AFTER the
/// spawn but BEFORE steady-state hand-off fails —
/// nonce minting, endpoint-path creation, control bind, advertisement write — the guard's `Drop`
/// kills + synchronously reaps the child so the real tsgo never lingers until the shim itself exits.
/// The guard is disarmed ([`into_inner`](Self::into_inner)) only once the accept loop is running and
/// steady-state teardown owns the child.
///
/// Kill semantics on setup failure are `start_contained_kill` (SIGKILL on Unix — group-killing the
/// child's subtree — / `TerminateProcess` on Windows) — never a graceful SIGTERM: setup never
/// reached steady state, so there is no clean-shutdown contract to honor. `tokio::process::Child`
/// has no async `Drop`, so the reap is a synchronous `try_wait` poll (the least-bad RAII answer — a
/// spawn-and-forget async reaper is unreliable once `run()` returns and the runtime shuts down).
struct ChildSetupGuard {
    child: Option<OwnedChild>,
}

impl ChildSetupGuard {
    fn new(child: OwnedChild) -> Self {
        Self { child: Some(child) }
    }

    /// Borrow the guarded child (e.g. to take its piped stdio) while ARMED.
    fn child_mut(&mut self) -> &mut OwnedChild {
        self.child
            .as_mut()
            .expect("child is present until the steady-state hand-off")
    }

    /// Disarm the guard and hand the child to the steady-state owner. After this the
    /// guard's `Drop` is a no-op — teardown owns the child's status from here.
    fn into_inner(mut self) -> OwnedChild {
        self.child
            .take()
            .expect("child is present until the steady-state hand-off")
    }

    /// Kill + asynchronously reap the guarded child, consuming the guard (disarming its `Drop`).
    /// Used when a shutdown signal wins the setup-window race: the child is torn down cooperatively
    /// here (an async `wait`, the runtime still alive) rather than through the synchronous `Drop`
    /// backstop.
    #[cfg(unix)]
    async fn kill_and_reap(mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.start_contained_kill();
            let _ = child.wait().await;
        }
    }
}

impl Drop for ChildSetupGuard {
    fn drop(&mut self) {
        let Some(mut child) = self.child.take() else {
            return; // disarmed: steady-state teardown owns the child.
        };
        // Setup failed after spawn. Kill (if not already gone) and synchronously reap so the child
        // does not linger until the shim exits. SIGKILL / TerminateProcess is uncatchable, so the
        // poll terminates; a bounded cap is a defensive safety valve against a wedged reap (leaking
        // is strictly better than hanging the shim's own teardown forever). On Windows the shim's
        // self-assigned kill-on-close job (closed by the OS if the shim itself dies) is the hard
        // backstop.
        if let Ok(None) = child.try_wait() {
            let _ = child.start_contained_kill();
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
    /// A normal exit with this FULL-WIDTH status code. Carried as `i32` (not `u8`) so a Windows
    /// NTSTATUS-shaped code survives intact; the terminal [`exit`](Self::exit) uses
    /// `std::process::exit` because `ExitCode::from` only accepts `u8` and would clamp it.
    Code(i32),
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
        // Preserve the FULL exit code — no `& 0xff` truncation. A Unix wait-status exit code is
        // already 8-bit by POSIX, but a Windows code can be a full 32-bit NTSTATUS-shaped value, so
        // narrowing here would corrupt it (e.g. 0xC0000005 → 5).
        ShimExit::Code(status.code().unwrap_or(1))
    }

    /// Terminate the process at the outermost [`main`] with the shim's faithful exit. A
    /// [`ShimExit::Code`] exits with the FULL-WIDTH code via [`std::process::exit`] (`ExitCode::from`
    /// only accepts `u8`, so it would clamp a Windows NTSTATUS-shaped code). A [`ShimExit::Signal`]
    /// restores the DEFAULT disposition, UNBLOCKS the signal, and re-raises so the shim terminates
    /// with the SAME signal (a signal-killed engine / shim must never report a clean exit); the
    /// trailing `_exit(128 + signo)` runs only if `raise` somehow returns.
    fn exit(self) -> ! {
        match self {
            ShimExit::Code(code) => std::process::exit(code),
            #[cfg(unix)]
            ShimExit::Signal(sig) => {
                // Restore the DEFAULT disposition via `sigaction` (not the weaker, less-portable
                // `signal`), UNBLOCK the signal so an inherited mask can never suppress it, then
                // re-raise so the shim dies from the SAME signal. Every call is async-signal-safe and
                // the async runtime is already shut down (no tokio signal machinery alive).
                // SAFETY: libc signal primitives on a valid signal number; the runtime is dropped.
                unsafe {
                    let mut act: libc::sigaction = std::mem::zeroed();
                    act.sa_sigaction = libc::SIG_DFL;
                    libc::sigemptyset(&mut act.sa_mask);
                    act.sa_flags = 0;
                    libc::sigaction(sig, &act, std::ptr::null_mut());

                    let mut unblock: libc::sigset_t = std::mem::zeroed();
                    libc::sigemptyset(&mut unblock);
                    libc::sigaddset(&mut unblock, sig);
                    libc::sigprocmask(libc::SIG_UNBLOCK, &unblock, std::ptr::null_mut());

                    libc::raise(sig);
                    // `raise` should not return for a default-fatal signal; if it does, `_exit` (NOT
                    // a normal return) avoids re-running any teardown and reports `128 + signo`.
                    libc::_exit(128 + (sig & 0xff));
                }
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

    /// Await a shutdown signal already captured during the synchronous setup body, bounded so the
    /// genuinely-no-signal case cannot block the shim's setup-error path. A signal delivered DURING
    /// setup is captured by tokio's OS signal handler — written to the signal self-pipe — at
    /// delivery; the awaited `recv()` here TURNS the reactor (a real waker-driven wakeup, not a poll
    /// gamble), so a buffered signal is drained DETERMINISTICALLY and returned immediately. The 50ms
    /// bound only limits the rare case where no signal was delivered (a plain setup error), after
    /// which this returns `None` and the setup error takes its faithful `Code(1)` path.
    async fn recv_pending_now(&mut self) -> Option<i32> {
        // `timeout` yields `Ok(signum)` on a drained signal and `Err(Elapsed)` on the 50ms bound;
        // `.ok()` collapses that to `Some(signum)` / `None`.
        tokio::time::timeout(std::time::Duration::from_millis(50), self.recv())
            .await
            .ok()
    }
}

/// The outcome of the [`run_relay`] setup-window race: EITHER a shutdown signal delivered while the
/// fallible post-spawn setup was still in flight, OR the setup's completion. Unix-only — Windows has
/// no shutdown signals, so its setup runs linearly with no race.
#[cfg(unix)]
enum SetupOutcome<T> {
    /// A shutdown signal was delivered during setup.
    Signalled(i32),
    /// The fallible setup finished — `Ok` with the steady-state handles, or a propagated `Err`.
    Done(Result<T, String>),
}

/// How the setup-window race resolves into the shim's next action.
#[cfg(unix)]
enum SetupResolution<T> {
    /// Re-raise this signal (after killing + reaping the guarded child).
    Signal(i32),
    /// Setup succeeded — proceed to steady state with these handles.
    Proceed(T),
    /// Setup errored with no signal — return this as the `Code(1)` path.
    Error(String),
}

/// Resolve the setup-window race. A delivered shutdown signal WINS: it is re-raised as
/// [`SetupResolution::Signal`] even when setup was interrupted mid-flight, because a shutdown signal
/// is the faithful process outcome and must NEVER be masked as the `Code(1)` setup-error path.
///
/// The setup body runs SYNCHRONOUSLY (its only `.await` is a detached accept task), so the
/// setup-window select polls its signal arm just ONCE; a signal delivered DURING setup that then
/// ERRORS is not seen by that select. `pending_signal_after_setup` carries a bounded, reactor-turning
/// re-check of the shutdown signals taken on the error path: when it is `Some`, the setup error is
/// superseded by the faithful signal re-raise. A clean setup maps to [`SetupResolution::Proceed`]
/// regardless (the steady-state biased select observes any still-pending signal); only a setup error
/// with NO pending signal maps to [`SetupResolution::Error`]. Kept a pure function so every ordering
/// is unit-testable deterministically (the microscopic real-timing race is not portably forceable).
#[cfg(unix)]
fn resolve_setup_race<T>(
    outcome: SetupOutcome<T>,
    pending_signal_after_setup: Option<i32>,
) -> SetupResolution<T> {
    match outcome {
        SetupOutcome::Signalled(signum) => SetupResolution::Signal(signum),
        SetupOutcome::Done(Ok(handles)) => SetupResolution::Proceed(handles),
        // A setup ERROR does NOT win over a shutdown signal that became pending during the
        // synchronous setup body: re-raise that signal faithfully instead of masking it as Code(1).
        SetupOutcome::Done(Err(message)) => match pending_signal_after_setup {
            Some(signum) => SetupResolution::Signal(signum),
            None => SetupResolution::Error(message),
        },
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
/// window WE own the kill, but the reaped status must still be inspected. We treat a bare `SIGKILL`
/// after our relay-stop kill as OUR kill — a clean editor-disconnect exit → `Code(0)` — while
/// ACKNOWLEDGING that we CANNOT DISTINGUISH it from a child self-`SIGKILL` or an OOM-kill racing the
/// teardown deadline: a wait status carries no origin, so this `SIGKILL` attribution gap is a known
/// RESIDUAL ambiguity, not a guarantee. ANY OTHER status means the child died on its OWN between the
/// grace deadline and our kill (a DISTINGUISHABLE self-signal or a non-zero self-exit) — that case is
/// propagated faithfully via [`ShimExit::from_status`]. (The `SIGKILL` attribution gap can only be
/// closed with process-origin telemetry — a pidfd / process-handle death record — which a wait
/// status does not carry.)
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
///
/// This passthrough child is INTENTIONALLY UNCONTAINED: unlike the `--lsp` relay child it gets NO OS
/// containment (no Job Object / `setsid` / `PR_SET_PDEATHSIG`) and NO shutdown-signal handlers. The
/// "no orphaned tsgo" containment contract covers the `--lsp` relay child ONLY — a passthrough is a
/// short-lived probe (`--version`, `--help`) run to completion with inherited stdio, so there is no
/// long-lived engine to orphan.
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

    // Windows: create the kill-on-close Job Object and assign THIS shim process to it BEFORE
    // spawning tsgo. On Win8+ a job member's children join the job at CREATION (no breakaway limit
    // is set), so the tsgo spawned next is BORN into the kill-on-close job with ZERO spawn→assign
    // window — a `TerminateProcess` of the shim can never orphan an already-spawned tsgo. A failure
    // here returns BEFORE any child is spawned, so it too can never orphan. (Unix containment is
    // intrinsic to the `pre_exec` spawn below.)
    #[cfg(windows)]
    create_kill_on_close_job_and_self_assign()
        .map_err(|e| format!("contain the relay shim in a job object: {e}"))?;

    // Spawn the real tsgo under OS-level containment — the PRIMARY "no orphaned tsgo" guarantee. Its
    // HARD-kill part (the kernel reaps the child even if neither Drop nor a signal handler runs) is
    // INTRINSIC to the spawn on Linux (PR_SET_PDEATHSIG) + Windows (the kill-on-close Job Object)
    // ONLY: Unix → its own session/process-group (+ Linux PR_SET_PDEATHSIG) armed in `pre_exec`;
    // Windows → born into the shim's kill-on-close Job Object (self-assigned just above). On
    // macOS/BSD the `setsid` process-group containment has NO parent-death primitive, so the RAII
    // guard / signal path is the best-effort hard-kill backstop there. The RAII guard is the
    // cooperative-teardown backstop generally. The signal install above precedes this spawn, so no
    // spawn→install window can orphan the child on a signal.
    #[cfg(unix)]
    let child = {
        use std::os::unix::process::CommandExt;
        // Captured BEFORE the fork so the forked child can detect a parent that already died in the
        // fork→pdeathsig window and refuse to run orphaned.
        let parent_pid = std::process::id();
        let mut std_cmd = std::process::Command::new(&args.real_tsgo);
        std_cmd
            .args(&args.forwarded)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        // SAFETY: the `pre_exec` closure runs in the forked child before exec and calls ONLY
        // async-signal-safe libc functions (see `contain_child_unix`).
        unsafe {
            std_cmd.pre_exec(move || contain_child_unix(parent_pid));
        }
        Command::from(std_cmd)
            .spawn()
            .map_err(|e| format!("spawn real tsgo {:?}: {e}", args.real_tsgo))?
    };
    #[cfg(not(unix))]
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
    let mut child_guard = ChildSetupGuard::new(OwnedChild::new(child));

    // The fallible post-spawn setup: take the child's piped stdio, start the relay, mint the
    // rendezvous witnesses, bind the control endpoint, publish the advertisement, and spawn the
    // control accept loop. On Unix this is RACED against a shutdown signal (below) with signal
    // priority, so a signal buffered DURING setup is re-raised faithfully rather than masked as the
    // `Code(1)` setup-error path.
    let setup = async {
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

        Ok::<(Arc<LspRelay>, PathBuf, tokio::task::JoinHandle<()>), String>((
            relay,
            advertisement_path,
            accept_task,
        ))
    };

    // Resolve the setup window. On Unix, RACE the fallible setup against a shutdown signal with
    // SIGNAL PRIORITY (`biased`): a signal delivered while setup is in flight is the faithful
    // process outcome — the still-ARMED guard kills + reaps the OWNED child (no orphan) and the
    // signal is re-raised, never masked as `Code(1)`. A setup error with no signal takes the error
    // path (the armed guard reaps as the `Err` unwinds). Windows has no shutdown signals, so its
    // setup runs linearly.
    #[cfg(unix)]
    let (relay, advertisement_path, accept_task) = {
        let outcome = tokio::select! {
            biased;
            signum = shutdown_signals.recv() => SetupOutcome::Signalled(signum),
            res = setup => SetupOutcome::Done(res),
        };
        // The setup body runs SYNCHRONOUSLY (its only `.await` is the detached accept task), so the
        // select above polled its signal arm just once. A shutdown signal delivered DURING setup that
        // then ERRORED would otherwise be masked as `Code(1)`. Recover it DETERMINISTICALLY on the
        // error path with a bounded await: the signal was captured by tokio's OS signal handler
        // (written to the signal self-pipe) at delivery, and `recv_pending_now` TURNS the reactor on
        // the awaited `recv()` — a real waker-driven wakeup, not a non-reactor poll — so a signal
        // buffered mid-setup is drained and re-raised faithfully instead of masked. The 50ms bound
        // only limits the genuinely-no-signal case (a plain setup error → the faithful `Code(1)`
        // path); a delivered signal returns immediately.
        let pending_signal_after_setup = match &outcome {
            SetupOutcome::Done(Err(_)) => shutdown_signals.recv_pending_now().await,
            _ => None,
        };
        match resolve_setup_race(outcome, pending_signal_after_setup) {
            SetupResolution::Signal(signum) => {
                child_guard.kill_and_reap().await;
                return Ok(ShimExit::Signal(signum));
            }
            SetupResolution::Error(message) => return Err(message),
            SetupResolution::Proceed(handles) => handles,
        }
    };
    #[cfg(not(unix))]
    let (relay, advertisement_path, accept_task) = setup.await?;

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
    // Await the aborted accept task so its control listener (the UDS socket file on Unix / the
    // named pipe on Windows) is DETERMINISTICALLY dropped before we return, rather than left to
    // runtime drop timing — a fresh shim on the same endpoint must not race a lingering listener.
    let _ = accept_task.await;
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
                let _ = child.start_contained_kill();
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
            let _ = child.start_contained_kill();
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
#[path = "main_tests.rs"]
mod tests;
