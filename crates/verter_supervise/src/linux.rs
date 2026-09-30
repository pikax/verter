//! Linux backend: a dedicated cgroup v2 the child joins before `exec`.
//!
//! The cgroup's `memory.max` is the cap for the whole tree (every descendant
//! is born inside it), `memory.swap.max=0` keeps the tree from spilling into
//! swap, and `memory.oom.group=1` makes the kernel's OOM kill take the whole
//! tree. Teardown writes `cgroup.kill`. A sentinel process outside the tree
//! kills the cgroup if the supervisor dies. The peak is the kernel's
//! `memory.peak` where the kernel has it (5.19+), otherwise the sampled
//! maximum of `memory.current`, named as such.
//!
//! Without a delegated cgroup v2 subtree that has the memory controller the
//! supervisor refuses to launch; a per-process `RLIMIT_AS` is not an
//! aggregate tree cap and is never substituted.

use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::report::{Containment, KilledBy, Report, Sample, SampleSeries};
use crate::unix::{child_environment, ms, try_reap, SentinelLink, Wakeups};
use crate::{Fault, Launch, MIDRUN_FAULT_OBSERVATION};

const CGROUP_ROOT: &str = "/sys/fs/cgroup";
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(50);
const TEARDOWN_LIMIT: Duration = Duration::from_secs(5);

/// The workload's cgroup.
struct Cgroup {
    dir: PathBuf,
    procs: File,
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
            let procs = OpenOptions::new()
                .write(true)
                .open(dir.join("cgroup.procs"))
                .map_err(|error| format!("cgroup.procs: {error}"))?;
            Ok(Cgroup {
                has_peak: dir.join("memory.peak").exists(),
                dir: dir.clone(),
                procs,
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

    fn members(&self) -> Vec<libc::pid_t> {
        std::fs::read_to_string(self.file("cgroup.procs"))
            .map(|procs| {
                procs
                    .lines()
                    .filter_map(|pid| pid.trim().parse().ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    fn kill_all(&self) {
        if write(&self.file("cgroup.kill"), "1").is_ok() {
            return;
        }
        crate::unix::kill_tree(0, self.members());
    }

    fn populated(&self) -> bool {
        std::fs::read_to_string(self.file("cgroup.events"))
            .map(|events| events.lines().any(|line| line == "populated 1"))
            .unwrap_or(false)
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

fn cpu_usage(cgroup: &Cgroup) -> Option<(f64, f64)> {
    let stat = std::fs::read_to_string(cgroup.file("cpu.stat")).ok()?;
    let field = |name: &str| -> Option<f64> {
        stat.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
            .and_then(|value| value.trim().parse::<f64>().ok())
    };
    Some((field("user_usec")? / 1000.0, field("system_usec")? / 1000.0))
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

    let wakeups = match Wakeups::install() {
        Ok(wakeups) => wakeups,
        Err(error) => {
            report
                .errors
                .push(format!("cannot install the cancellation handlers: {error}"));
            cgroup.remove();
            return;
        }
    };
    let mut sentinel = match SentinelLink::start()
        .and_then(|mut link| link.watch_cgroup(&cgroup.dir).map(|()| link))
    {
        Ok(link) => link,
        Err(error) => {
            report
                .errors
                .push(format!("cannot establish containment: {error}"));
            cgroup.remove();
            return;
        }
    };

    let procs_fd = cgroup.procs.as_raw_fd();
    let mut command = Command::new(&spec.program);
    command
        .args(&spec.args)
        .env_clear()
        .envs(child_environment(&spec.env))
        .stdin(Stdio::null())
        .stdout(Stdio::from(launch.outputs.stdout))
        .stderr(Stdio::from(launch.outputs.stderr))
        .process_group(0);
    if let Some(cwd) = &spec.cwd {
        command.current_dir(cwd);
    }
    // SAFETY: the closure only makes async-signal-safe system calls.
    unsafe {
        command.pre_exec(move || {
            // Join the workload cgroup before exec: the program's first
            // instruction already runs under the cap.
            if libc::write(procs_fd, b"0".as_ptr() as *const libc::c_void, 1) != 1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    if crate::unix::cancelled() {
        report.killed_by = Some(KilledBy::Cancel);
        sentinel.finish();
        cgroup.remove();
        return;
    }
    let started = Instant::now();
    let child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            report.errors.push(format!(
                "cannot spawn {}: {error}",
                spec.program.to_string_lossy()
            ));
            sentinel.finish();
            cgroup.kill_all();
            cgroup.remove();
            return;
        }
    };
    let pid = child.id() as libc::pid_t;
    // The child is reaped with waitpid below, not through `Child`.
    std::mem::forget(child);
    report.launched = true;
    let mut cause: Option<KilledBy> = None;
    let mut killed_at: Option<Instant> = None;
    let kill = |why: KilledBy, cause: &mut Option<KilledBy>, killed_at: &mut Option<Instant>| {
        if cause.is_none() {
            *cause = Some(why);
            *killed_at = Some(Instant::now());
        }
        cgroup.kill_all();
    };
    if let Err(error) = sentinel
        .watch_group(pid)
        .and_then(|()| sentinel.watch_pid(pid))
    {
        report.errors.push(error);
        kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
    }

    let interval = spec.sample_interval.unwrap_or(DEFAULT_SAMPLE_INTERVAL);
    report.sampling.interval_ms = ms(interval);
    let deadline = started + spec.timeout;
    let mut series = SampleSeries::default();
    let mut sampled_max = 0u64;
    let mut next_sample = started;
    let mut last_observation = started;
    let exit = loop {
        match try_reap(pid) {
            Ok(Some(exit)) => break Some(exit),
            Ok(None) => {}
            Err(error) => {
                report
                    .errors
                    .push(format!("cannot wait for the child: {error}"));
                kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
                break None;
            }
        }
        let now = Instant::now();
        if cause.is_none() && now >= deadline {
            kill(KilledBy::Timeout, &mut cause, &mut killed_at);
        }
        if crate::unix::cancelled() && cause.is_none() {
            kill(KilledBy::Cancel, &mut cause, &mut killed_at);
        }
        if let Some(at) = killed_at {
            if at.elapsed() > TEARDOWN_LIMIT {
                report
                    .errors
                    .push("the child did not exit after the tree was killed".to_owned());
                break None;
            }
        }
        if cause.is_none() && !sentinel.alive() {
            report.errors.push("the sentinel died mid-run".to_owned());
            kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
        }
        if cause.is_none() {
            if let Err(error) = sentinel.heartbeat() {
                report.errors.push(error);
                kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
            }
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
                    kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
                }
            }
            if cgroup.event("oom_kill").unwrap_or(0) > 0 {
                kill(KilledBy::Memory, &mut cause, &mut killed_at);
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
                fd: sentinel.fd(),
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
        if fds[1].revents != 0 && cause.is_none() {
            report.errors.push("the sentinel died mid-run".to_owned());
            kill(KilledBy::SupervisorError, &mut cause, &mut killed_at);
        }
    };

    let exited = Instant::now();
    if exit.is_some() {
        // Reaped: its pid may be reused from here on.
        sentinel.forget_pid(pid).ok();
    }
    report.wall_ms = Some(ms(exited.duration_since(started)));
    if let Some(at) = killed_at {
        report.termination_latency_ms = Some(ms(exited.saturating_duration_since(at)));
    }
    if let Some(exit) = exit {
        report.exit_code = exit.code;
        report.signal = exit.signal;
    }

    // Tear down whatever the child left behind, and wait for the cgroup to
    // empty.
    let left = cgroup.members();
    report.descendants_killed = Some(left.len() as u64);
    cgroup.kill_all();
    let teardown_deadline = Instant::now() + TEARDOWN_LIMIT;
    while cgroup.populated() {
        if Instant::now() > teardown_deadline {
            report.errors.push(format!(
                "{} processes were still alive {TEARDOWN_LIMIT:?} after teardown",
                cgroup.members().len()
            ));
            break;
        }
        cgroup.kill_all();
        std::thread::sleep(Duration::from_millis(2));
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
    sentinel.finish();
    cgroup.remove();
}
