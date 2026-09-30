//! The sentinel: an internal mode of the supervisor binary that outlives a
//! supervisor killed without warning (SIGKILL, a crash) and tears the
//! workload down.
//!
//! It runs in its own process group, reads a line protocol on stdin and
//! answers `R` once ready:
//!
//! ```text
//! G <pgid>   the workload's process group
//! P <pid>    a workload process (every descendant the supervisor discovers)
//! X <pid>    a workload process that is gone (its pid may be reused)
//! C <path>   the workload's cgroup directory (Linux)
//! H          heartbeat
//! D          done: the supervisor finished teardown; exit without killing
//! ```
//!
//! End of input (the supervisor died) or no heartbeat for
//! [`HEARTBEAT_EXPIRY`] (the supervisor stopped running) kills everything it
//! was told about. It can only kill what it was told about: a descendant the
//! supervisor never discovered is outside its reach, except inside a cgroup.

use std::io::Read;
use std::time::{Duration, Instant};

use crate::unix::{kill_tree, poll_readable, HEARTBEAT_EXPIRY};

/// The first argument that selects sentinel mode.
pub const SENTINEL_ARG: &str = "__sentinel";

pub(crate) fn run() -> i32 {
    // SAFETY: ignoring terminal and termination signals has no preconditions;
    // only end of input or a missed heartbeat ends the sentinel's watch.
    unsafe {
        libc::signal(libc::SIGINT, libc::SIG_IGN);
        libc::signal(libc::SIGHUP, libc::SIG_IGN);
        libc::signal(libc::SIGTERM, libc::SIG_IGN);
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }
    println!("R");
    use std::io::Write;
    std::io::stdout().flush().ok();

    let mut pgid: libc::pid_t = 0;
    let mut pids: Vec<libc::pid_t> = Vec::new();
    let mut cgroup: Option<String> = None;
    let mut pending = Vec::new();
    let mut stdin = std::io::stdin().lock();
    let mut last_heard = Instant::now();

    loop {
        let remaining = HEARTBEAT_EXPIRY.saturating_sub(last_heard.elapsed());
        if remaining.is_zero() {
            break;
        }
        if !poll_readable(0, remaining) {
            continue;
        }
        let mut buffer = [0u8; 4096];
        let read = match stdin.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(read) => read,
        };
        last_heard = Instant::now();
        pending.extend_from_slice(&buffer[..read]);
        while let Some(end) = pending.iter().position(|&byte| byte == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&line[..end]).into_owned();
            let (verb, rest) = line.split_once(' ').unwrap_or((line.as_str(), ""));
            match verb {
                "G" => pgid = rest.parse().unwrap_or(0),
                "P" => {
                    if let Ok(pid) = rest.parse() {
                        pids.push(pid);
                    }
                }
                "X" => {
                    if let Ok(pid) = rest.parse::<libc::pid_t>() {
                        pids.retain(|known| *known != pid);
                    }
                }
                "C" => cgroup = Some(rest.to_owned()),
                "D" => return 0,
                _ => {}
            }
        }
    }

    teardown(pgid, &pids, cgroup.as_deref());
    1
}

/// Kill everything the sentinel knows about, repeatedly for a short while so
/// a process forked during the first pass is caught by the next.
fn teardown(pgid: libc::pid_t, pids: &[libc::pid_t], cgroup: Option<&str>) {
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        if let Some(cgroup) = cgroup {
            kill_cgroup(cgroup);
        }
        kill_tree(pgid, pids.iter().copied());
        std::thread::sleep(Duration::from_millis(20));
    }
    if let Some(cgroup) = cgroup {
        // Empty now; an emptied cgroup can be removed.
        std::fs::remove_dir(cgroup).ok();
    }
}

#[cfg(target_os = "linux")]
fn kill_cgroup(path: &str) {
    let dir = std::path::Path::new(path);
    if std::fs::write(dir.join("cgroup.kill"), "1").is_ok() {
        return;
    }
    if let Ok(procs) = std::fs::read_to_string(dir.join("cgroup.procs")) {
        kill_tree(0, procs.lines().filter_map(|pid| pid.trim().parse().ok()));
    }
}

#[cfg(not(target_os = "linux"))]
fn kill_cgroup(_path: &str) {}
