//! Shared Unix machinery: signal-driven wakeups, pipes, the monotonic clock
//! shared by the supervisor and its watchdog, the spawn request, and the
//! supervisor's side of the watchdog link.
//!
//! On Unix the watchdog (an internal mode of this binary, outside the
//! workload's process group) owns the workload: it spawns it, observes its
//! exit, tears the tree down and reaps it. The supervisor only decides
//! (limits, deadline, cancellation, telemetry) and asks the watchdog to kill.
//! Whatever way the supervisor ends, its control pipe closes and the watchdog
//! tears the tree down; nothing is ever killed by a cached process id.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::cli::RunSpec;
use crate::FAULT_ENV;

/// Set by SIGINT, SIGTERM or SIGHUP.
static CANCELLED: AtomicBool = AtomicBool::new(false);
/// The write end of the wakeup pipe the signal handler writes to.
static WAKE_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_signal(signal: libc::c_int) {
    if signal != libc::SIGCHLD {
        CANCELLED.store(true, Ordering::SeqCst);
    }
    let fd = WAKE_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let byte = signal as u8;
        // SAFETY: write(2) is async-signal-safe; the pipe is non-blocking so
        // a full pipe drops the byte (a wakeup is already pending).
        unsafe {
            libc::write(fd, &byte as *const u8 as *const libc::c_void, 1);
        }
    }
}

pub(crate) fn cancelled() -> bool {
    CANCELLED.load(Ordering::SeqCst)
}

/// A pipe that becomes readable whenever one of the handled signals arrives.
pub(crate) struct Wakeups {
    read: OwnedFd,
    _write: OwnedFd,
}

impl Wakeups {
    /// Install handlers for `signals`. SIGCHLD only wakes; any other signal
    /// also marks the supervisor cancelled.
    pub(crate) fn install(signals: &[libc::c_int]) -> std::io::Result<Wakeups> {
        let (read, write) = pipe()?;
        set_nonblocking(read.as_raw_fd())?;
        set_nonblocking(write.as_raw_fd())?;
        WAKE_FD.store(write.as_raw_fd(), Ordering::SeqCst);
        for &signal in signals {
            // SAFETY: installs a handler that only touches atomics and write(2).
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = on_signal as *const () as libc::sighandler_t;
                action.sa_flags = libc::SA_RESTART;
                libc::sigemptyset(&mut action.sa_mask);
                if libc::sigaction(signal, &action, std::ptr::null_mut()) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
        }
        Ok(Wakeups {
            read,
            _write: write,
        })
    }

    pub(crate) fn fd(&self) -> RawFd {
        self.read.as_raw_fd()
    }

    /// Drain pending wakeups.
    pub(crate) fn drain(&self) {
        let mut buffer = [0u8; 64];
        // SAFETY: reads into a local buffer from a non-blocking pipe.
        while unsafe {
            libc::read(
                self.read.as_raw_fd(),
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len(),
            )
        } > 0
        {}
    }
}

impl Drop for Wakeups {
    fn drop(&mut self) {
        WAKE_FD.store(-1, Ordering::SeqCst);
    }
}

/// A close-on-exec pipe.
pub(crate) fn pipe() -> std::io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0 as libc::c_int; 2];
    // SAFETY: `fds` is a valid two-element out-array.
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: pipe(2) returned two fresh descriptors this function now owns.
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    set_cloexec(read.as_raw_fd())?;
    set_cloexec(write.as_raw_fd())?;
    Ok((read, write))
}

fn set_cloexec(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: fcntl on a descriptor the caller owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

pub(crate) fn set_nonblocking(fd: RawFd) -> std::io::Result<()> {
    // SAFETY: fcntl on a descriptor the caller owns.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

/// Nanoseconds on the system-wide monotonic clock, comparable between the
/// supervisor and its watchdog.
pub(crate) fn monotonic_ns() -> u64 {
    // SAFETY: clock_gettime with a local out-parameter.
    unsafe {
        let mut now: libc::timespec = std::mem::zeroed();
        libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut now);
        now.tv_sec as u64 * 1_000_000_000 + now.tv_nsec as u64
    }
}

pub(crate) fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

pub(crate) fn ns_to_ms(ns: u64) -> f64 {
    ns as f64 / 1_000_000.0
}

/// How the child ended.
pub(crate) struct ExitInfo {
    pub code: Option<i64>,
    pub signal: Option<i32>,
}

pub(crate) fn decode_status(status: libc::c_int) -> ExitInfo {
    if libc::WIFEXITED(status) {
        ExitInfo {
            code: Some(i64::from(libc::WEXITSTATUS(status))),
            signal: None,
        }
    } else if libc::WIFSIGNALED(status) {
        ExitInfo {
            code: None,
            signal: Some(libc::WTERMSIG(status)),
        }
    } else {
        ExitInfo {
            code: None,
            signal: None,
        }
    }
}

/// What the watchdog needs to spawn the workload. Byte strings, so any
/// Unix path, argument or environment entry survives the trip.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct SpawnRequest {
    pub program: Vec<u8>,
    pub args: Vec<Vec<u8>>,
    pub env: Vec<(Vec<u8>, Vec<u8>)>,
    pub cwd: Option<Vec<u8>>,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// The workload's cgroup directory (Linux).
    pub cgroup: Option<Vec<u8>>,
}

impl SpawnRequest {
    pub(crate) fn new(spec: &RunSpec, cgroup: Option<&std::path::Path>) -> SpawnRequest {
        let bytes = |text: &OsStr| text.as_bytes().to_vec();
        SpawnRequest {
            program: bytes(&spec.program),
            args: spec.args.iter().map(|arg| bytes(arg)).collect(),
            env: child_environment(&spec.env)
                .into_iter()
                .map(|(name, value)| (name.into_vec(), value.into_vec()))
                .collect(),
            cwd: spec.cwd.as_ref().map(|cwd| bytes(cwd.as_os_str())),
            stdout: bytes(spec.stdout_path().as_os_str()),
            stderr: bytes(spec.stderr_path().as_os_str()),
            cgroup: cgroup.map(|dir| bytes(dir.as_os_str())),
        }
    }
}

pub(crate) fn os(bytes: &[u8]) -> &OsStr {
    OsStr::from_bytes(bytes)
}

/// The child's environment: the supervisor's own, the `--env` overrides
/// applied, the fault hook removed.
fn child_environment(overrides: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut vars: std::collections::BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (name, value) in overrides {
        vars.insert(name.clone(), value.clone());
    }
    vars.remove(OsStr::new(FAULT_ENV));
    vars.into_iter().collect()
}

/// How long the watchdog waits for a heartbeat before tearing the tree down.
pub(crate) const HEARTBEAT_EXPIRY: Duration = Duration::from_secs(2);
/// How often the supervisor sends a heartbeat.
pub(crate) const HEARTBEAT_EVERY: Duration = Duration::from_millis(250);
/// How long a handshake step with the watchdog may take.
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(5);

/// The watchdog's report that the workload exited and its tree is gone.
pub(crate) struct Exited {
    pub exit: ExitInfo,
    /// Monotonic time the watchdog observed the exit.
    pub exit_ns: u64,
    /// Descendants still alive when the child exited, killed by teardown.
    pub descendants: u64,
    /// Processes teardown could not remove.
    pub survivors: u64,
}

/// What the watchdog link produced since the last look.
pub(crate) enum LinkEvent {
    Exited(Exited),
    /// The watchdog died: the tree is no longer owned by anyone.
    Lost,
}

/// The supervisor's side of the watchdog: a control pipe the watchdog reads
/// (its end of input means the supervisor is gone) and a reply pipe.
pub(crate) struct WatchdogLink {
    child: Child,
    control: std::fs::File,
    replies: OwnedFd,
    pending: Vec<u8>,
    last_heartbeat: Instant,
}

impl WatchdogLink {
    /// Start the watchdog and wait for its ready handshake.
    pub(crate) fn start() -> Result<WatchdogLink, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot locate the supervisor for its watchdog: {error}"))?;
        let mut command = Command::new(exe);
        command
            .arg(crate::sentinel::SENTINEL_ARG)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .env_remove(FAULT_ENV);
        {
            use std::os::unix::process::CommandExt;
            // Outside the terminal's foreground group, so an interactive
            // Ctrl-C reaches the supervisor (which cancels) but not the
            // watchdog (which must outlive it).
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start the watchdog: {error}"))?;
        let control = std::fs::File::from(OwnedFd::from(child.stdin.take().expect("piped stdin")));
        let replies = OwnedFd::from(child.stdout.take().expect("piped stdout"));
        let mut link = WatchdogLink {
            child,
            control,
            replies,
            pending: Vec::new(),
            last_heartbeat: Instant::now(),
        };
        let ready = set_nonblocking(link.replies.as_raw_fd())
            .map_err(|error| format!("watchdog reply pipe: {error}"))
            .and_then(|()| link.expect_line(HANDSHAKE_LIMIT))
            .and_then(|line| {
                if line == "R" {
                    Ok(())
                } else {
                    Err(format!("the watchdog answered {line:?} to its handshake"))
                }
            });
        if let Err(error) = ready {
            link.child.kill().ok();
            link.child.wait().ok();
            return Err(error);
        }
        Ok(link)
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        self.control
            .write_all(line.as_bytes())
            .and_then(|()| self.control.flush())
            .map_err(|error| format!("the watchdog is unreachable: {error}"))
    }

    /// Read available reply bytes; `false` at end of input.
    fn fill(&mut self) -> bool {
        let mut buffer = [0u8; 4096];
        let mut reader = std::fs::File::from(match self.replies.try_clone() {
            Ok(fd) => fd,
            Err(_) => return false,
        });
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => return false,
                Ok(read) => self.pending.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return true,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return false,
            }
        }
    }

    fn next_line(&mut self) -> Option<String> {
        let end = self.pending.iter().position(|&byte| byte == b'\n')?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        Some(String::from_utf8_lossy(&line[..end]).into_owned())
    }

    /// Wait for the next reply line. `E <message>` becomes an error.
    fn expect_line(&mut self, limit: Duration) -> Result<String, String> {
        let deadline = Instant::now() + limit;
        loop {
            if let Some(line) = self.next_line() {
                return match line.strip_prefix("E ") {
                    Some(message) => Err(message.to_owned()),
                    None => Ok(line),
                };
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err("the watchdog did not answer in time".to_owned());
            }
            poll_readable(self.replies.as_raw_fd(), remaining);
            if !self.fill() && !self.pending.contains(&b'\n') {
                return Err("the watchdog exited".to_owned());
            }
        }
    }

    /// Ask the watchdog to spawn the workload; returns its pid. On macOS the
    /// workload is suspended until [`WatchdogLink::release`].
    pub(crate) fn spawn(&mut self, request: &SpawnRequest) -> Result<libc::pid_t, String> {
        let json = serde_json::to_string(request).map_err(|error| error.to_string())?;
        self.send(&format!("Q {json}\n"))?;
        let line = self.expect_line(HANDSHAKE_LIMIT)?;
        line.strip_prefix("S ")
            .and_then(|pid| pid.parse().ok())
            .ok_or_else(|| format!("the watchdog answered {line:?} to the spawn request"))
    }

    /// Release the workload; returns the monotonic time it was released.
    pub(crate) fn release(&mut self) -> Result<u64, String> {
        self.send("GO\n")?;
        let line = self.expect_line(HANDSHAKE_LIMIT)?;
        line.strip_prefix("G ")
            .and_then(|at| at.parse().ok())
            .ok_or_else(|| format!("the watchdog answered {line:?} to the release"))
    }

    /// Ask the watchdog to kill the workload's tree.
    pub(crate) fn kill(&mut self) -> Result<(), String> {
        self.send("K\n")
    }

    /// Send a heartbeat when one is due.
    pub(crate) fn heartbeat(&mut self) -> Result<(), String> {
        if self.last_heartbeat.elapsed() >= HEARTBEAT_EVERY {
            self.last_heartbeat = Instant::now();
            self.send("H\n")?;
        }
        Ok(())
    }

    /// The reply pipe, for polling.
    pub(crate) fn fd(&self) -> RawFd {
        self.replies.as_raw_fd()
    }

    /// Collect what the watchdog reported.
    pub(crate) fn event(&mut self) -> Option<LinkEvent> {
        let open = self.fill();
        while let Some(line) = self.next_line() {
            if let Some(exited) = line.strip_prefix("X ").and_then(parse_exit) {
                return Some(LinkEvent::Exited(exited));
            }
        }
        if !open || !matches!(self.child.try_wait(), Ok(None)) {
            return Some(LinkEvent::Lost);
        }
        None
    }

    /// Dismiss the watchdog after it reported the exit.
    pub(crate) fn finish(mut self) {
        self.send("D\n").ok();
        self.reap();
    }

    /// Abandon the run: closing the control pipe makes the watchdog tear
    /// the tree down (if it spawned one) and exit.
    pub(crate) fn abandon(self) {
        let WatchdogLink {
            mut child, control, ..
        } = self;
        drop(control);
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if !matches!(child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    fn reap(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

/// `X <code|-> <signal|-> <exit_ns> <descendants> <survivors>`.
fn parse_exit(text: &str) -> Option<Exited> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [code, signal, exit_ns, descendants, survivors] = fields[..] else {
        return None;
    };
    Some(Exited {
        exit: ExitInfo {
            code: code.parse().ok(),
            signal: signal.parse().ok(),
        },
        exit_ns: exit_ns.parse().ok()?,
        descendants: descendants.parse().ok()?,
        survivors: survivors.parse().ok()?,
    })
}

pub(crate) fn format_exit(exited: &Exited) -> String {
    let field = |value: Option<String>| value.unwrap_or_else(|| "-".to_owned());
    format!(
        "X {} {} {} {} {}\n",
        field(exited.exit.code.map(|code| code.to_string())),
        field(exited.exit.signal.map(|signal| signal.to_string())),
        exited.exit_ns,
        exited.descendants,
        exited.survivors
    )
}

/// Wait until `fd` is readable (or hung up) for at most `limit`.
pub(crate) fn poll_readable(fd: RawFd, limit: Duration) -> bool {
    let mut entry = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    let timeout = limit.as_millis().min(i32::MAX as u128) as libc::c_int;
    // SAFETY: polls one valid descriptor.
    unsafe { libc::poll(&mut entry, 1, timeout) > 0 }
}

/// Die at once, as a crash would: no destructor, handler or cleanup runs.
pub(crate) fn crash_now() -> ! {
    // SAFETY: SIGKILL to ourselves; nothing runs after it.
    unsafe {
        libc::kill(libc::getpid(), libc::SIGKILL);
    }
    std::process::abort()
}
