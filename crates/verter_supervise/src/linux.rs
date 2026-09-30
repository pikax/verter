//! Linux backend: a dedicated cgroup v2 the child joins before `exec`.
//!
//! The cgroup's `memory.max` is the cap for the whole tree (every descendant
//! is born inside it), `memory.swap.max=0` keeps the tree from spilling into
//! swap, and `memory.oom.group=1` makes the kernel's OOM kill take the whole
//! tree. The watchdog owns the workload: before launch it opens the cgroup's
//! `cgroup.kill` (the only way the tree is ever killed; without it the
//! supervisor refuses), spawns the child into the cgroup, observes its exit
//! and empties the cgroup. The peak is the kernel's `memory.peak` where the
//! kernel has it (5.19+), otherwise the sampled maximum of `memory.current`,
//! named as such.
//!
//! Without a delegated cgroup v2 subtree that has the memory controller the
//! supervisor refuses to launch; a per-process `RLIMIT_AS` is not an
//! aggregate tree cap and is never substituted.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::report::{Containment, KilledBy, Report, Sample, SampleSeries};
use crate::unix::{
    crash_now, monotonic_ns, ms, ns_to_ms, os, Exited, LinkEvent, SpawnRequest, Wakeups,
    WatchdogLink,
};
use crate::{Fault, Launch, MIDRUN_FAULT_OBSERVATION};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
const TEARDOWN_LIMIT: Duration = Duration::from_secs(5);

/// The workload's cgroup, as the supervisor sees it.
struct Cgroup {
    dir: PathBuf,
    has_peak: bool,
}

impl Cgroup {
    fn establish(mem_limit_bytes: u64, fault: Option<Fault>) -> Result<Cgroup, String> {
        let refuse = |why: String| {
            format!(
                "cannot establish containment: no delegated cgroup v2 subtree with the memory \
                 controller ({why}); run under `systemd-run --user --scope -p Delegate=yes`"
            )
        };
        if fault == Some(Fault::Containment) {
            return Err("cannot establish containment: injected fault".to_owned());
        }
        let root = Path::new(CGROUP_ROOT);
        if !root.join("cgroup.controllers").exists() {
            return Err(refuse(format!(
                "{CGROUP_ROOT} is not a cgroup v2 unified hierarchy"
            )));
        }
        let own = std::fs::read_to_string("/proc/self/cgroup")
            .map_err(|error| refuse(format!("/proc/self/cgroup: {error}")))?;
        let own = own
            .lines()
            .find_map(|line| line.strip_prefix("0::"))
            .ok_or_else(|| refuse("the supervisor is not in a cgroup v2 hierarchy".to_owned()))?;
        let own = root.join(own.trim().trim_start_matches('/'));
        let parent = workload_parent(&own).map_err(refuse)?;

        let unique = format!(
            "verter-supervise-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        let dir = parent.join(unique);
        std::fs::create_dir(&dir)
            .map_err(|error| refuse(format!("cannot create {}: {error}", dir.display())))?;
        let configure = || -> Result<Cgroup, String> {
            write(&dir.join("memory.max"), &mem_limit_bytes.to_string())?;
            let swap = dir.join("memory.swap.max");
            if swap.exists() {
                write(&swap, "0")?;
            } else if swap_active() {
                return Err(format!(
                    "{} is missing while swap is active, so the tree could exceed the cap in swap",
                    swap.display()
                ));
            }
            write(&dir.join("memory.oom.group"), "1")?;
            let applied = read_trimmed(&dir.join("memory.max"))?;
            if applied != mem_limit_bytes.to_string() {
                return Err(format!(
                    "memory.max reads back {applied:?}, not {mem_limit_bytes}"
                ));
            }
            Ok(Cgroup {
                has_peak: dir.join("memory.peak").exists(),
                dir: dir.clone(),
            })
        };
        configure().map_err(|error| {
            std::fs::remove_dir(&dir).ok();
            format!("cannot establish containment: {error}")
        })
    }

    fn file(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    fn current(&self) -> Result<u64, String> {
        read_u64(&self.file("memory.current"))
    }

    fn event(&self, name: &str) -> Result<u64, String> {
        let events = std::fs::read_to_string(self.file("memory.events"))
            .map_err(|error| format!("memory.events: {error}"))?;
        Ok(events
            .lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
            .and_then(|count| count.trim().parse().ok())
            .unwrap_or(0))
    }

    /// Empty the cgroup when its watchdog is gone. Only `cgroup.kill`: never
    /// a process id.
    fn kill_orphaned(&self) -> Result<(), String> {
        let kill = OpenOptions::new()
            .write(true)
            .open(self.file("cgroup.kill"))
            .map_err(|error| format!("cgroup.kill: {error}"))?;
        let (_, survivors) = empty_cgroup(&kill, &self.file("cgroup.events"), TEARDOWN_LIMIT);
        if survivors {
            return Err("the cgroup did not empty".to_owned());
        }
        Ok(())
    }

    fn remove(&self) {
        std::fs::remove_dir(&self.dir).ok();
    }
}

/// Choose the cgroup the workload's cgroup is created under, in order of
/// preference:
///
/// 1. the supervisor's own cgroup, when it already passes the memory
///    controller to its children;
/// 2. the supervisor's own cgroup, when the supervisor is alone in it: the
///    supervisor moves into a leaf of its own so the cgroup can pass the
///    memory controller on (the `systemd-run --scope -p Delegate=yes` case);
/// 3. the parent of the supervisor's cgroup, when the parent already passes
///    the memory controller down (creating the cgroup there still needs
///    write access to it).
fn workload_parent(own: &Path) -> Result<PathBuf, String> {
    let has_memory = |file: &Path| {
        read_trimmed(file).is_ok_and(|text| text.split_whitespace().any(|c| c == "memory"))
    };
    let control = own.join("cgroup.subtree_control");
    if has_memory(&control) {
        return Ok(own.to_path_buf());
    }
    let pid = std::process::id().to_string();
    let members = read_trimmed(&own.join("cgroup.procs"))?;
    if members.lines().all(|member| member.trim() == pid) {
        let leaf = own.join(format!("verter-supervise-{pid}-self"));
        match std::fs::create_dir(&leaf) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(format!("cannot create {}: {error}", leaf.display())),
        }
        write(&leaf.join("cgroup.procs"), &pid)?;
        write(&control, "+memory")?;
        return Ok(own.to_path_buf());
    }
    match own.parent() {
        Some(parent)
            if parent.starts_with(CGROUP_ROOT) && has_memory(&own.join("cgroup.controllers")) =>
        {
            Ok(parent.to_path_buf())
        }
        _ => Err(format!(
            "{} holds other processes and does not pass the memory controller to its children",
            own.display()
        )),
    }
}

fn swap_active() -> bool {
    std::fs::read_to_string("/proc/swaps")
        .map(|swaps| swaps.lines().count() > 1)
        .unwrap_or(true)
}

fn write(path: &Path, value: &str) -> Result<(), String> {
    std::fs::write(path, value).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_trimmed(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path)
        .map(|text| text.trim().to_owned())
        .map_err(|error| format!("{}: {error}", path.display()))
}

fn read_u64(path: &Path) -> Result<u64, String> {
    let text = read_trimmed(path)?;
    text.parse()
        .map_err(|_| format!("{}: unreadable value {text:?}", path.display()))
}

fn populated(events: &Path) -> bool {
    std::fs::read_to_string(events)
        .map(|events| events.lines().any(|line| line == "populated 1"))
        .unwrap_or(true)
}

/// Kill everything in a cgroup through its `cgroup.kill` and wait until it is
/// empty. The kill is repeated only while the cgroup is still populated, and
/// nothing is signalled once it is empty. Returns whether the cgroup was
/// populated at the start and whether anything survived `limit`.
fn empty_cgroup(kill: &File, events: &Path, limit: Duration) -> (bool, bool) {
    let was_populated = populated(events);
    let deadline = Instant::now() + limit;
    let mut last_kill: Option<Instant> = None;
    while populated(events) {
        if Instant::now() > deadline {
            return (was_populated, true);
        }
        if last_kill.is_none_or(|at| at.elapsed() >= Duration::from_millis(100)) {
            let mut kill = kill;
            kill.write_all(b"1").ok();
            last_kill = Some(Instant::now());
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    (was_populated, false)
}

fn cpu_usage(cgroup: &Cgroup) -> Option<(f64, f64)> {
    let stat = std::fs::read_to_string(cgroup.file("cpu.stat")).ok()?;
    let field = |name: &str| -> Option<f64> {
        stat.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
            .and_then(|value| value.trim().parse::<f64>().ok())
    };
    Some((field("user_usec")? / 1000.0, field("system_usec")? / 1000.0))
}

/// The workload, as its watchdog owns it: the child and the cgroup's kill
/// switch, opened before the child exists.
pub(crate) struct Workload {
    pid: libc::pid_t,
    kill: File,
    dir: PathBuf,
    start_ns: u64,
}

impl Workload {
    pub(crate) fn spawn(request: &SpawnRequest) -> Result<Workload, String> {
        let dir = PathBuf::from(os(request
            .cgroup
            .as_deref()
            .ok_or("cannot establish containment: the spawn request names no cgroup")?));
        let kill = OpenOptions::new()
            .write(true)
            .open(dir.join("cgroup.kill"))
            .map_err(|error| {
                format!(
                    "cannot establish containment: the cgroup's kill switch {} is unavailable \
                     ({error}); Linux 5.14 or later is required",
                    dir.join("cgroup.kill").display()
                )
            })?;
        let procs = OpenOptions::new()
            .write(true)
            .open(dir.join("cgroup.procs"))
            .map_err(|error| format!("cannot establish containment: cgroup.procs: {error}"))?;
        let program = os(&request.program);
        let refuse = |why: String| format!("cannot spawn {}: {why}", program.to_string_lossy());
        let output = |path: &[u8]| {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(os(path))
                .map_err(|error| refuse(format!("cannot open {:?}: {error}", os(path))))
        };
        let stdout = output(&request.stdout)?;
        let stderr = output(&request.stderr)?;
        let mut command = Command::new(program);
        command
            .args(request.args.iter().map(|arg| os(arg)))
            .env_clear()
            .envs(
                request
                    .env
                    .iter()
                    .map(|(name, value)| (os(name), os(value))),
            )
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr)
            .process_group(0);
        if let Some(cwd) = &request.cwd {
            command.current_dir(os(cwd));
        }
        let procs_fd = procs.as_raw_fd();
        // SAFETY: the closure only makes async-signal-safe system calls.
        unsafe {
            command.pre_exec(move || {
                // Join the workload cgroup before exec: the program's first
                // instruction already runs under the cap.
                if libc::write(procs_fd, b"0".as_ptr() as *const libc::c_void, 1) != 1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let start_ns = monotonic_ns();
        let child = command.spawn().map_err(|error| refuse(error.to_string()))?;
        let pid = child.id() as libc::pid_t;
        // The watchdog observes and reaps the child with waitid/waitpid.
        std::mem::forget(child);
        Ok(Workload {
            pid,
            kill,
            dir,
            start_ns,
        })
    }

    pub(crate) fn pid(&self) -> libc::pid_t {
        self.pid
    }

    /// The child runs from `exec`, already inside the cgroup; its start is
    /// the moment it was spawned.
    pub(crate) fn release(&self) -> Result<u64, String> {
        Ok(self.start_ns)
    }

    pub(crate) fn kill(&self) {
        let mut kill = &self.kill;
        kill.write_all(b"1").ok();
    }

    /// Empty the cgroup after the child exited. Returns the descendants that
    /// were still alive and whether any survived.
    pub(crate) fn teardown(&self) -> (u64, u64) {
        let members = |dir: &Path| {
            std::fs::read_to_string(dir.join("cgroup.procs"))
                .map(|procs| procs.lines().count() as u64)
                .unwrap_or(0)
        };
        let descendants = members(&self.dir);
        let (_, survived) =
            empty_cgroup(&self.kill, &self.dir.join("cgroup.events"), TEARDOWN_LIMIT);
        (
            descendants,
            if survived {
                members(&self.dir).max(1)
            } else {
                0
            },
        )
    }

    /// Remove the emptied cgroup when the supervisor is gone and cannot.
    pub(crate) fn abandoned(&self) {
        std::fs::remove_dir(&self.dir).ok();
    }
}

pub(crate) fn supervise(launch: Launch<'_>, report: &mut Report) {
    let spec = launch.spec;
    report.kill_trigger_bytes = Some(spec.mem_limit_bytes);
    let cgroup = match Cgroup::establish(spec.mem_limit_bytes, launch.fault) {
        Ok(cgroup) => cgroup,
        Err(error) => {
            report.errors.push(error);
            return;
        }
    };
    let probe = if launch.fault == Some(Fault::Telemetry) {
        Err("injected fault".to_owned())
    } else {
        cgroup
            .current()
            .and_then(|_| cgroup.event("oom_kill"))
            .map(|_| ())
    };
    if let Err(error) = probe {
        report
            .errors
            .push(format!("cannot establish telemetry: {error}"));
        cgroup.remove();
        return;
    }
    report.containment = Some(Containment::Hard);
    report.overshoot_bound_bytes = Some(0);
    report.sample_metric = Some("cgroup-memory.current");
    report.peak_metric = Some(if cgroup.has_peak {
        "cgroup-memory.peak"
    } else {
        "cgroup-memory.current-sampled-max"
    });

    let wakeups = match Wakeups::install(&[libc::SIGINT, libc::SIGTERM, libc::SIGHUP]) {
        Ok(wakeups) => wakeups,
        Err(error) => {
            report
                .errors
                .push(format!("cannot install the cancellation handlers: {error}"));
            cgroup.remove();
            return;
        }
    };
    let mut link = match WatchdogLink::start() {
        Ok(link) => link,
        Err(error) => {
            report
                .errors
                .push(format!("cannot establish containment: {error}"));
            cgroup.remove();
            return;
        }
    };
    let pid = match link.spawn(&SpawnRequest::new(spec, Some(&cgroup.dir))) {
        Ok(pid) => pid,
        Err(error) => {
            report.errors.push(error);
            link.abandon();
            cgroup.remove();
            return;
        }
    };
    if launch.fault == Some(Fault::DieAfterSpawn) {
        eprintln!("verter-supervise: fault: dying after spawning pid {pid}");
        crash_now();
    }
    if crate::unix::cancelled() {
        report.killed_by = Some(KilledBy::Cancel);
        link.abandon();
        cgroup.remove();
        return;
    }
    let start_ns = match link.release() {
        Ok(start_ns) => start_ns,
        Err(error) => {
            report
                .errors
                .push(format!("cannot release the contained child: {error}"));
            link.abandon();
            cgroup.remove();
            return;
        }
    };
    report.launched = true;
    let started = Instant::now();

    let mut cause: Option<KilledBy> = None;
    let mut kill_ns: Option<u64> = None;
    let mut kill = |why: KilledBy, link: &mut WatchdogLink, cause: &mut Option<KilledBy>| {
        if cause.is_none() {
            *cause = Some(why);
            kill_ns = Some(monotonic_ns());
        }
        link.kill().ok();
    };

    let interval = spec.sample_interval.unwrap_or(DEFAULT_SAMPLE_INTERVAL);
    report.sampling.interval_ms = ms(interval);
    let deadline = started + spec.timeout;
    let mut series = SampleSeries::default();
    let mut sampled_max = 0u64;
    let mut next_sample = started;
    let mut last_observation = started;
    let mut killed_at: Option<Instant> = None;
    let exited: Option<Exited> = loop {
        match link.event() {
            Some(LinkEvent::Exited(exited)) => break Some(exited),
            Some(LinkEvent::Lost) => {
                report
                    .errors
                    .push("the watchdog died mid-run; the cgroup was killed".to_owned());
                if cause.is_none() {
                    cause = Some(KilledBy::SupervisorError);
                }
                if let Err(error) = cgroup.kill_orphaned() {
                    report.errors.push(error);
                }
                break None;
            }
            None => {}
        }
        let now = Instant::now();
        if cause.is_none() && now >= deadline {
            kill(KilledBy::Timeout, &mut link, &mut cause);
        }
        if cause.is_none() && crate::unix::cancelled() {
            kill(KilledBy::Cancel, &mut link, &mut cause);
        }
        if cause.is_some() {
            let at = *killed_at.get_or_insert(now);
            if at.elapsed() > TEARDOWN_LIMIT * 2 {
                report
                    .errors
                    .push("the watchdog did not report the tree gone after the kill".to_owned());
                break None;
            }
        }
        if let Err(error) = link.heartbeat() {
            report.errors.push(error);
            kill(KilledBy::SupervisorError, &mut link, &mut cause);
        }
        if cause.is_none() && now >= next_sample {
            report.sampling.count += 1;
            let observed = if launch.fault == Some(Fault::TelemetryMidrun)
                && report.sampling.count == MIDRUN_FAULT_OBSERVATION
            {
                Err("injected fault".to_owned())
            } else {
                cgroup.current()
            };
            match observed {
                Ok(bytes) => {
                    let done = Instant::now();
                    sampled_max = sampled_max.max(bytes);
                    series.push(Sample {
                        t_ms: ms(done.duration_since(started)),
                        bytes,
                    });
                    let sampling = &mut report.sampling;
                    sampling.max_sample_age_ms = sampling
                        .max_sample_age_ms
                        .max(ms(done.duration_since(last_observation)));
                    sampling.max_sweep_ms = sampling.max_sweep_ms.max(ms(done.duration_since(now)));
                    last_observation = done;
                }
                Err(error) => {
                    report
                        .errors
                        .push(format!("telemetry lost mid-run: {error}"));
                    kill(KilledBy::SupervisorError, &mut link, &mut cause);
                }
            }
            if cgroup.event("oom_kill").unwrap_or(0) > 0 {
                kill(KilledBy::Memory, &mut link, &mut cause);
            }
            next_sample = (next_sample + interval).max(Instant::now());
        }
        let now = Instant::now();
        let until = if cause.is_some() {
            now + Duration::from_millis(20)
        } else {
            deadline.min(next_sample)
        };
        let mut fds = [
            libc::pollfd {
                fd: wakeups.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
            libc::pollfd {
                fd: link.fd(),
                events: libc::POLLIN,
                revents: 0,
            },
        ];
        let timeout = until
            .saturating_duration_since(now)
            .min(crate::unix::HEARTBEAT_EVERY)
            .as_millis() as libc::c_int;
        // SAFETY: polls two valid descriptors.
        unsafe {
            libc::poll(fds.as_mut_ptr(), 2, timeout);
        }
        wakeups.drain();
    };

    if let Some(exited) = &exited {
        report.wall_ms = Some(ns_to_ms(exited.exit_ns.saturating_sub(start_ns)));
        report.termination_latency_ms =
            kill_ns.map(|at| ns_to_ms(exited.exit_ns.saturating_sub(at)));
        report.exit_code = exited.exit.code;
        report.signal = exited.exit.signal;
        report.descendants_killed = Some(exited.descendants);
        if exited.survivors > 0 {
            report
                .errors
                .push("processes survived the cgroup's teardown".to_owned());
        }
    }
    match cgroup.event("oom_kill") {
        Ok(kills) if kills > 0 && cause.is_none() => cause = Some(KilledBy::Memory),
        Ok(_) => {}
        Err(error) => report.errors.push(format!("telemetry: {error}")),
    }
    report.killed_by = cause;
    let peak = if cgroup.has_peak {
        read_u64(&cgroup.file("memory.peak"))
    } else {
        Ok(sampled_max)
    };
    match peak {
        Ok(peak) => report.peak_bytes = Some(peak),
        Err(error) => report
            .errors
            .push(format!("telemetry: cannot read the peak: {error}")),
    }
    if let Some((user, system)) = cpu_usage(&cgroup) {
        report.cpu_user_ms = Some(user);
        report.cpu_kernel_ms = Some(system);
    }
    report.samples = series.into_vec();
    if exited.is_some() {
        link.finish();
    } else {
        link.abandon();
    }
    cgroup.remove();
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Once the cgroup reads empty, teardown writes `cgroup.kill` no more:
    /// nothing is signalled after a successful cleanup.
    #[test]
    fn teardown_stops_signalling_once_the_cgroup_is_empty() {
        let dir =
            std::env::temp_dir().join(format!("verter-supervise-teardown-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let events = dir.join("cgroup.events");
        let kill_path = dir.join("cgroup.kill");
        std::fs::write(&kill_path, "").unwrap();
        let kill = OpenOptions::new().append(true).open(&kill_path).unwrap();

        std::fs::write(&events, "populated 0\nfrozen 0\n").unwrap();
        assert_eq!(
            empty_cgroup(&kill, &events, Duration::from_millis(300)),
            (false, false)
        );
        assert_eq!(std::fs::read_to_string(&kill_path).unwrap(), "");

        // Populated until the first kill lands, then empty.
        std::fs::write(&events, "populated 1\n").unwrap();
        let flip = {
            let events = events.clone();
            let kill_path = kill_path.clone();
            std::thread::spawn(move || {
                while std::fs::read_to_string(&kill_path).unwrap().is_empty() {
                    std::thread::sleep(Duration::from_millis(1));
                }
                std::fs::write(&events, "populated 0\n").unwrap();
            })
        };
        assert_eq!(
            empty_cgroup(&kill, &events, Duration::from_secs(5)),
            (true, false)
        );
        flip.join().unwrap();
        std::thread::sleep(Duration::from_millis(250));
        assert_eq!(
            std::fs::read_to_string(&kill_path).unwrap(),
            "1",
            "exactly one kill, none after the cgroup emptied"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
