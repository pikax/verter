//! The watchdog: an internal mode of the supervisor binary that owns the
//! workload on Unix. It spawns the workload, observes its exit, tears its
//! tree down and reaps it; the supervisor only decides when to kill.
//!
//! It runs in its own process group and speaks a line protocol: requests on
//! stdin, replies on stdout.
//!
//! ```text
//! -> R                 ready
//! <- Q <json>          spawn this workload (a `SpawnRequest`)
//! -> S <pid>           spawned (suspended on macOS)   | E <message>: refused
//! <- GO                release it
//! -> G <start_ns>      released at this monotonic time
//! <- K                 kill the tree
//! <- H                 heartbeat
//! -> X <code|-> <signal|-> <exit_ns> <descendants> <survivors>
//!                      the workload exited (observed at exit_ns), its tree
//!                      was torn down and it was reaped
//! <- D                 done; exit
//! ```
//!
//! End of input (the supervisor died) or no heartbeat for
//! [`HEARTBEAT_EXPIRY`] (the supervisor stopped running) kills the tree. The
//! kill never uses a cached process id: on Linux it is the workload cgroup's
//! `cgroup.kill`, opened before launch; on macOS it is the workload's process
//! group, whose leader the watchdog keeps unreaped (so the group id cannot be
//! reused) until the group is empty. Teardown stops once the tree is empty.

use std::io::{Read, Write};
use std::time::Instant;

#[cfg(target_os = "linux")]
use crate::linux::Workload;
#[cfg(target_os = "macos")]
use crate::macos::Workload;
use crate::unix::{
    decode_status, format_exit, monotonic_ns, set_nonblocking, Exited, SpawnRequest, Wakeups,
    HEARTBEAT_EXPIRY,
};

/// The first argument that selects watchdog mode.
pub const SENTINEL_ARG: &str = "__sentinel";

fn reply(line: &str) {
    let mut stdout = std::io::stdout().lock();
    stdout.write_all(line.as_bytes()).ok();
    if !line.ends_with('\n') {
        stdout.write_all(b"\n").ok();
    }
    stdout.flush().ok();
}

/// Non-blocking line reader over stdin.
struct Input {
    pending: Vec<u8>,
    open: bool,
}

impl Input {
    fn fill(&mut self) {
        let mut buffer = [0u8; 4096];
        let mut stdin = std::io::stdin().lock();
        loop {
            match stdin.read(&mut buffer) {
                Ok(0) => {
                    self.open = false;
                    return;
                }
                Ok(read) => self.pending.extend_from_slice(&buffer[..read]),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => return,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => {
                    self.open = false;
                    return;
                }
            }
        }
    }

    fn line(&mut self) -> Option<String> {
        let end = self.pending.iter().position(|&byte| byte == b'\n')?;
        let line: Vec<u8> = self.pending.drain(..=end).collect();
        Some(String::from_utf8_lossy(&line[..end]).into_owned())
    }
}

/// Wait for input or a child-state change, for at most `limit`.
fn wait(wakeups: &Wakeups, limit: std::time::Duration) {
    let mut fds = [
        libc::pollfd {
            fd: 0,
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: wakeups.fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let timeout = limit.as_millis().clamp(1, i32::MAX as u128) as libc::c_int;
    // SAFETY: polls two valid descriptors.
    unsafe {
        libc::poll(fds.as_mut_ptr(), 2, timeout);
    }
    wakeups.drain();
}

/// Whether `pid` has exited, observed without reaping it (`WNOWAIT`): the
/// unreaped process keeps its pid and group id reserved.
fn exited(pid: libc::pid_t) -> bool {
    // SAFETY: waitid with a zeroed local out-parameter.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let observed = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        observed == 0 && info.si_pid() == pid
    }
}

fn reap(pid: libc::pid_t) -> crate::unix::ExitInfo {
    let mut status = 0;
    // SAFETY: reaps our own exited child.
    unsafe {
        libc::waitpid(pid, &mut status, 0);
    }
    decode_status(status)
}

/// Tear the exited workload's tree down, reap it and report.
fn finish_workload(workload: &Workload, exit_ns: u64) -> Exited {
    let (descendants, survivors) = workload.teardown();
    Exited {
        exit: reap(workload.pid()),
        exit_ns,
        descendants,
        survivors,
    }
}

pub(crate) fn run() -> i32 {
    // SAFETY: ignoring terminal and termination signals has no
    // preconditions; only end of input or a missed heartbeat ends the watch.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    let wakeups = match Wakeups::install(&[libc::SIGCHLD]) {
        Ok(wakeups) => wakeups,
        Err(error) => {
            reply(&format!("E the watchdog cannot watch its child: {error}"));
            return 1;
        }
    };
    if let Err(error) = set_nonblocking(0) {
        reply(&format!(
            "E the watchdog cannot read its control pipe: {error}"
        ));
        return 1;
    }
    reply("R");

    let mut input = Input {
        pending: Vec::new(),
        open: true,
    };
    let started = Instant::now();
    let request = loop {
        input.fill();
        if let Some(line) = input.line() {
            break line;
        }
        if !input.open || started.elapsed() > HEARTBEAT_EXPIRY {
            return 1;
        }
        wait(&wakeups, HEARTBEAT_EXPIRY);
    };
    let request: SpawnRequest = match request
        .strip_prefix("Q ")
        .and_then(|json| serde_json::from_str(json).ok())
    {
        Some(request) => request,
        None => {
            reply("E the watchdog received a malformed spawn request");
            return 1;
        }
    };
    let workload = match Workload::spawn(&request) {
        Ok(workload) => workload,
        Err(error) => {
            reply(&format!("E {error}"));
            return 1;
        }
    };
    reply(&format!("S {}", workload.pid()));

    let mut reported = false;
    let mut last_heard = Instant::now();
    loop {
        if !reported && exited(workload.pid()) {
            let exit_ns = monotonic_ns();
            reply(&format_exit(&finish_workload(&workload, exit_ns)));
            reported = true;
        }
        let remaining = HEARTBEAT_EXPIRY.saturating_sub(last_heard.elapsed());
        if remaining.is_zero() {
            break;
        }
        wait(&wakeups, remaining);
        let before = input.pending.len();
        input.fill();
        if input.pending.len() != before {
            last_heard = Instant::now();
        }
        while let Some(line) = input.line() {
            match line.as_str() {
                "GO" => match workload.release() {
                    Ok(start_ns) => reply(&format!("G {start_ns}")),
                    Err(error) => {
                        reply(&format!("E {error}"));
                        workload.kill();
                    }
                },
                "K" => workload.kill(),
                "D" if reported => return 0,
                _ => {}
            }
        }
        if !input.open {
            break;
        }
    }

    // The supervisor is gone or stopped answering: kill the tree, wait for
    // the workload to exit, tear down what remains and reap it.
    if !reported {
        workload.kill();
        let deadline = Instant::now() + std::time::Duration::from_secs(5);
        while !exited(workload.pid()) && Instant::now() < deadline {
            wait(&wakeups, std::time::Duration::from_millis(20));
            workload.kill();
        }
        if exited(workload.pid()) {
            finish_workload(&workload, monotonic_ns());
        } else {
            workload.teardown();
        }
    }
    workload.abandoned();
    1
}
