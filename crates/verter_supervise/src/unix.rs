//! Shared Unix machinery: signal-driven wakeups, pipes, the sentinel client,
//! exit-status decoding and process-group teardown.

use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

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

/// The wakeup pipe: readable whenever a child changed state or the
/// supervisor was asked to stop.
pub(crate) struct Wakeups {
    read: OwnedFd,
    _write: OwnedFd,
}

impl Wakeups {
    /// Install the SIGINT/SIGTERM/SIGHUP/SIGCHLD handlers.
    pub(crate) fn install() -> std::io::Result<Wakeups> {
        let (read, write) = pipe()?;
        set_nonblocking(read.as_raw_fd())?;
        set_nonblocking(write.as_raw_fd())?;
        WAKE_FD.store(write.as_raw_fd(), Ordering::SeqCst);
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP, libc::SIGCHLD] {
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

/// Send SIGKILL to a process group and to every listed pid.
pub(crate) fn kill_tree(pgid: libc::pid_t, pids: impl IntoIterator<Item = libc::pid_t>) {
    // SAFETY: sending signals has no memory-safety preconditions.
    unsafe {
        if pgid > 1 {
            libc::kill(-pgid, libc::SIGKILL);
        }
        for pid in pids {
            if pid > 1 {
                libc::kill(pid, libc::SIGKILL);
            }
        }
    }
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

/// Reap `pid` if it has exited.
pub(crate) fn try_reap(pid: libc::pid_t) -> std::io::Result<Option<ExitInfo>> {
    let mut status = 0;
    // SAFETY: waitpid on our own child with a local out-parameter.
    let reaped = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    match reaped {
        0 => Ok(None),
        r if r == pid => Ok(Some(decode_status(status))),
        _ => Err(std::io::Error::last_os_error()),
    }
}

/// The child's environment: the supervisor's own, the `--env` overrides
/// applied, the fault hook removed.
pub(crate) fn child_environment(overrides: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let mut vars: std::collections::BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    for (name, value) in overrides {
        vars.insert(name.clone(), value.clone());
    }
    vars.remove(OsStr::new(FAULT_ENV));
    vars.into_iter().collect()
}

pub(crate) fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// How long the sentinel waits for a heartbeat before tearing the tree down.
pub(crate) const HEARTBEAT_EXPIRY: Duration = Duration::from_secs(2);
/// How often the supervisor sends a heartbeat.
pub(crate) const HEARTBEAT_EVERY: Duration = Duration::from_millis(250);

/// The supervisor's side of the sentinel: a separate process, outside the
/// workload's process group, that tears the tree down when the supervisor
/// dies (its control pipe reaches EOF) or stops responding (no heartbeat).
pub(crate) struct SentinelLink {
    child: Child,
    control: std::fs::File,
    replies: OwnedFd,
    last_heartbeat: Instant,
}

impl SentinelLink {
    /// Start the sentinel and wait for its ready handshake.
    pub(crate) fn start() -> Result<SentinelLink, String> {
        let exe = std::env::current_exe()
            .map_err(|error| format!("cannot locate the supervisor for its sentinel: {error}"))?;
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
            // sentinel (which must outlive it).
            command.process_group(0);
        }
        let mut child = command
            .spawn()
            .map_err(|error| format!("cannot start the sentinel: {error}"))?;
        let control = std::fs::File::from(OwnedFd::from(child.stdin.take().expect("piped stdin")));
        let replies: OwnedFd = child.stdout.take().expect("piped stdout").into();
        let mut link = SentinelLink {
            child,
            control,
            replies,
            last_heartbeat: Instant::now(),
        };
        if let Err(error) = link.await_ready() {
            link.child.kill().ok();
            link.child.wait().ok();
            return Err(error);
        }
        set_nonblocking(link.replies.as_raw_fd())
            .map_err(|error| format!("sentinel reply pipe: {error}"))?;
        Ok(link)
    }

    fn await_ready(&mut self) -> Result<(), String> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut seen = Vec::new();
        let mut reader = std::fs::File::from(
            self.replies
                .try_clone()
                .map_err(|error| format!("sentinel reply pipe: {error}"))?,
        );
        while !seen.ends_with(b"R\n") {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() || !poll_readable(self.replies.as_raw_fd(), remaining) {
                return Err("the sentinel did not complete its handshake".to_owned());
            }
            let mut byte = [0u8; 1];
            match reader.read(&mut byte) {
                Ok(1) => seen.push(byte[0]),
                _ => return Err("the sentinel exited during its handshake".to_owned()),
            }
        }
        Ok(())
    }

    fn send(&mut self, line: &str) -> Result<(), String> {
        self.control
            .write_all(line.as_bytes())
            .and_then(|()| self.control.flush())
            .map_err(|error| format!("the sentinel is unreachable: {error}"))
    }

    pub(crate) fn watch_group(&mut self, pgid: libc::pid_t) -> Result<(), String> {
        self.send(&format!("G {pgid}\n"))
    }

    pub(crate) fn watch_pid(&mut self, pid: libc::pid_t) -> Result<(), String> {
        self.send(&format!("P {pid}\n"))
    }

    /// Forget a tracked process that is gone, so a reused pid is never killed.
    pub(crate) fn forget_pid(&mut self, pid: libc::pid_t) -> Result<(), String> {
        self.send(&format!("X {pid}\n"))
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn watch_cgroup(&mut self, path: &std::path::Path) -> Result<(), String> {
        self.send(&format!("C {}\n", path.display()))
    }

    /// Send a heartbeat when one is due.
    pub(crate) fn heartbeat(&mut self) -> Result<(), String> {
        if self.last_heartbeat.elapsed() >= HEARTBEAT_EVERY {
            self.last_heartbeat = Instant::now();
            self.send("H\n")?;
        }
        Ok(())
    }

    /// The sentinel's reply pipe: readable (EOF) if the sentinel died.
    pub(crate) fn fd(&self) -> RawFd {
        self.replies.as_raw_fd()
    }

    /// Whether the sentinel is still alive.
    pub(crate) fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Disarm the sentinel after a completed teardown.
    pub(crate) fn finish(mut self) {
        self.send("D\n").ok();
        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            if !matches!(self.child.try_wait(), Ok(None)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        self.child.kill().ok();
        self.child.wait().ok();
    }
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
