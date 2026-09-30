//! macOS backend: a sampled cap over the summed `phys_footprint` of every
//! tracked descendant.
//!
//! macOS offers an unprivileged, unentitled process no kernel-enforced
//! process-tree memory ceiling (no job objects, no cgroups; `RLIMIT_AS` and
//! `RLIMIT_RSS` are not enforced as a footprint cap, and the memorystatus
//! limit setters need root or an entitlement). So this backend is labelled
//! `sampled` and runs only with `--allow-sampled`:
//!
//! - Before the child runs: consent, a cap within physical memory minus the
//!   host reserve, normal host memory pressure, a readable footprint, a
//!   sampling cadence that meets its age budget, and a ready watchdog. Any
//!   failure refuses the launch.
//! - The watchdog owns the workload: it `posix_spawn`s the child suspended
//!   (`POSIX_SPAWN_START_SUSPENDED`) into its own process group with only its
//!   three standard descriptors, releases it with `SIGCONT` on the
//!   supervisor's word, observes its exit with `waitid(WNOWAIT)`, kills the
//!   group while the unreaped leader keeps the group id reserved, and only
//!   then reaps it. A supervisor that dies at any point, even between the
//!   spawn and the release, leaves the watchdog to kill the group.
//! - The supervisor samples: every sweep enumerates the process group and the
//!   children of every tracked process, reads each one's `ri_phys_footprint`,
//!   and asks the watchdog to kill the group when the sum reaches the kill
//!   trigger (the cap less a headroom of 1/16). A fork wakes a sweep at once.
//! - Fail closed mid-run: an unreadable live process, a descendant that left
//!   the process group, an observation older than twice the sampling
//!   interval (at least 50 ms: macOS sometimes wakes a sleep that late), a
//!   dead watchdog or host memory pressure kills the group and invalidates
//!   the run. Nothing is ever killed by a cached process id. A descendant
//!   that left the group dies with the tree only when its pid is pinned: the
//!   kill stops the group first, and a stopped parent cannot reap its child,
//!   so the child it still parents keeps its pid. One whose parent already
//!   exited belongs to launchd and is only reported.
//!
//! Sampling proves no overshoot bound: allocation between two sweeps, and
//! kill latency, are not bounded by anything the supervisor controls. The
//! result says so (`overshootBoundBytes: null`) and reports the observed
//! overshoot, sweep age and termination latency instead.

use std::collections::BTreeSet;
use std::ffi::{c_char, c_int, CString};
use std::os::fd::AsRawFd;
use std::time::{Duration, Instant};

use crate::disk;

use crate::report::{Containment, KilledBy, Report, Sample, SampleSeries};
use crate::unix::{
    crash_now, monotonic_ns, ms, ns_to_ms, os, Exited, LinkEvent, SpawnRequest, Wakeups,
    WatchdogLink,
};
use crate::{Fault, Launch, MIDRUN_FAULT_OBSERVATION};

const METRIC: &str = "sampled-tree-phys-footprint-sum";
const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_millis(10);
const TEARDOWN_LIMIT: Duration = Duration::from_secs(5);
const MIN_HOST_RESERVE: u64 = 2 * 1024 * 1024 * 1024;
/// `kern.memorystatus_vm_pressure_level`: 1 normal, 2 warning, 4 critical.
const PRESSURE_NORMAL: i32 = 1;

extern "C" {
    fn posix_spawn_file_actions_addchdir_np(
        actions: *mut libc::posix_spawn_file_actions_t,
        path: *const c_char,
    ) -> c_int;
}

pub(crate) fn supervise(launch: Launch<'_>, report: &mut Report) {
    let spec = launch.spec;
    if !spec.allow_sampled {
        report.errors.push(
            "refusing to launch: macOS gives an unprivileged process no kernel-enforced \
             process-tree memory cap, so this backend enforces the cap by sampling, with no \
             guaranteed overshoot bound; pass --allow-sampled to accept a sampled cap"
                .to_owned(),
        );
        return;
    }
    if launch.fault == Some(Fault::Containment) {
        report
            .errors
            .push("cannot establish containment: injected fault".to_owned());
        return;
    }
    let interval = spec.sample_interval.unwrap_or(DEFAULT_SAMPLE_INTERVAL);
    let max_age = sample_age_budget(interval);
    let cap = spec.mem_limit_bytes;
    let trigger = cap - cap / 16;
    report.kill_trigger_bytes = Some(trigger);
    report.sampling.interval_ms = ms(interval);

    if let Err(error) = preflight(spec, interval, interval * 2, launch.fault) {
        report.errors.push(error);
        return;
    }
    report.containment = Some(Containment::Sampled);
    report.peak_metric = Some(METRIC);
    report.sample_metric = Some(METRIC);

    let wakeups = match Wakeups::install(&[libc::SIGINT, libc::SIGTERM, libc::SIGHUP]) {
        Ok(wakeups) => wakeups,
        Err(error) => {
            report
                .errors
                .push(format!("cannot install the cancellation handlers: {error}"));
            return;
        }
    };
    let queue = match Queue::new() {
        Ok(queue) => queue,
        Err(error) => {
            report
                .errors
                .push(format!("cannot establish containment: kqueue: {error}"));
            return;
        }
    };
    let mut link = match WatchdogLink::start() {
        Ok(link) => link,
        Err(error) => {
            report
                .errors
                .push(format!("cannot establish containment: {error}"));
            return;
        }
    };
    let pid = match link.spawn(&SpawnRequest::new(spec, None)) {
        Ok(pid) => pid,
        Err(error) => {
            report.errors.push(error);
            link.abandon();
            return;
        }
    };
    if launch.fault == Some(Fault::DieAfterSpawn) {
        eprintln!("verter-supervise: fault: dying after spawning pid {pid}");
        crash_now();
    }
    let pgid = pid;
    let mut known: BTreeSet<libc::pid_t> = BTreeSet::from([pid]);
    let armed = queue
        .watch_process(pid)
        .map_err(|error| format!("cannot establish containment: kqueue on the child: {error}"))
        .and_then(|()| {
            queue
                .watch_fd(wakeups.fd())
                .map_err(|error| format!("kqueue: {error}"))
        })
        .and_then(|()| {
            queue
                .watch_fd(link.fd())
                .map_err(|error| format!("kqueue: {error}"))
        })
        .and_then(|()| {
            footprint(pid).map(|_| ()).map_err(|error| {
                format!("cannot establish telemetry: the child's footprint is unreadable: {error}")
            })
        });
    if let Err(error) = armed {
        report.errors.push(error);
        link.abandon();
        return;
    }
    if crate::unix::cancelled() {
        report.killed_by = Some(KilledBy::Cancel);
        link.abandon();
        return;
    }
    let start_ns = match link.release() {
        Ok(start_ns) => start_ns,
        Err(error) => {
            report
                .errors
                .push(format!("cannot release the contained child: {error}"));
            link.abandon();
            return;
        }
    };
    report.launched = true;
    let started = Instant::now();

    let deadline = started + spec.timeout;
    let mut cause: Option<KilledBy> = None;
    let mut kill_ns: Option<u64> = None;
    let mut killed_at: Option<Instant> = None;
    let mut series = SampleSeries::default();
    let mut peak = 0u64;
    let mut next_sample = started;
    let mut last_observation = started;
    let mut sweep_now = true;
    let mut tree_ever: BTreeSet<libc::pid_t> = known.clone();

    let exited: Option<Exited> = loop {
        let mut kill = |why: KilledBy, link: &mut WatchdogLink, cause: &mut Option<KilledBy>| {
            if cause.is_none() {
                *cause = Some(why);
                kill_ns = Some(monotonic_ns());
            }
            link.kill().ok();
        };
        match link.event() {
            Some(LinkEvent::Exited(exited)) => break Some(exited),
            Some(LinkEvent::Lost) => {
                report.errors.push(
                    "the watchdog died mid-run; the process group is no longer owned".to_owned(),
                );
                if cause.is_none() {
                    cause = Some(KilledBy::SupervisorError);
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
        if cause.is_none() && (sweep_now || now >= next_sample) {
            sweep_now = false;
            report.sampling.count += 1;
            let injected = launch.fault == Some(Fault::TelemetryMidrun)
                && report.sampling.count == MIDRUN_FAULT_OBSERVATION;
            match sweep(pgid, &mut known, injected) {
                Ok(outcome) => {
                    let done = Instant::now();
                    for gone in &outcome.gone {
                        known.remove(gone);
                    }
                    for &new in &outcome.discovered {
                        tree_ever.insert(new);
                        // Registration only speeds up fork wakeups; a pid
                        // that is already gone needs none.
                        queue.watch_process(new).ok();
                    }
                    peak = peak.max(outcome.footprint);
                    series.push(Sample {
                        t_ms: ms(done.duration_since(started)),
                        bytes: outcome.footprint,
                    });
                    let age = done.duration_since(last_observation);
                    let sampling = &mut report.sampling;
                    sampling.max_sample_age_ms = sampling.max_sample_age_ms.max(ms(age));
                    sampling.max_sweep_ms = sampling.max_sweep_ms.max(ms(done.duration_since(now)));
                    last_observation = done;
                    if let Some(escapee) = outcome.escaped {
                        report.errors.push(format!(
                            "descendant {escapee} left the process group; the tree is no longer \
                             fully observable"
                        ));
                        kill(KilledBy::SupervisorError, &mut link, &mut cause);
                    } else if outcome.footprint >= trigger {
                        kill(KilledBy::Memory, &mut link, &mut cause);
                    } else if age > max_age {
                        report.errors.push(format!(
                            "telemetry lost mid-run: an observation was {:.1} ms old, over the \
                             {:.1} ms budget",
                            ms(age),
                            ms(max_age)
                        ));
                        kill(KilledBy::SupervisorError, &mut link, &mut cause);
                    } else {
                        match pressure_level() {
                            Ok(PRESSURE_NORMAL) => {}
                            Ok(level) => {
                                report.errors.push(format!(
                                    "host memory pressure rose to level {level} mid-run"
                                ));
                                kill(KilledBy::Pressure, &mut link, &mut cause);
                            }
                            Err(error) => {
                                report
                                    .errors
                                    .push(format!("telemetry lost mid-run: {error}"));
                                kill(KilledBy::SupervisorError, &mut link, &mut cause);
                            }
                        }
                    }
                }
                Err(error) => {
                    report
                        .errors
                        .push(format!("telemetry lost mid-run: {error}"));
                    kill(KilledBy::SupervisorError, &mut link, &mut cause);
                }
            }
            next_sample = next_sweep_due(Instant::now(), interval);
        }

        let now = Instant::now();
        let until = if cause.is_some() {
            now + Duration::from_millis(5)
        } else {
            deadline
                .min(next_sample)
                .min(now + crate::unix::HEARTBEAT_EVERY)
        };
        match queue.wait(until.saturating_duration_since(now)) {
            Ok(events) => {
                if events.forked {
                    sweep_now = true;
                }
            }
            Err(error) => {
                report.errors.push(format!("kevent: {error}"));
                kill(KilledBy::SupervisorError, &mut link, &mut cause);
            }
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
            report.errors.push(format!(
                "{} processes of the group survived teardown",
                exited.survivors
            ));
        }
    }
    report.killed_by = cause;
    report.peak_bytes = Some(peak);
    report.process_count = Some(tree_ever.len() as u64);
    report.samples = series.into_vec();
    if exited.is_some() {
        link.finish();
    } else {
        link.abandon();
    }
}

/// The workload, as its watchdog owns it: a child spawned suspended into a
/// process group of its own, which it leads. The watchdog reaps the leader
/// only after the group is empty, so the group id is never reused while it
/// is being killed.
pub(crate) struct Workload {
    pid: libc::pid_t,
}

impl Workload {
    pub(crate) fn spawn(request: &SpawnRequest) -> Result<Workload, String> {
        Ok(Workload {
            pid: spawn_suspended(request)?,
        })
    }

    pub(crate) fn pid(&self) -> libc::pid_t {
        self.pid
    }

    pub(crate) fn release(&self) -> Result<u64, String> {
        let start_ns = monotonic_ns();
        // SAFETY: continues our own suspended child.
        if unsafe { libc::kill(self.pid, libc::SIGCONT) } != 0 {
            return Err(format!(
                "cannot release the child: {}",
                std::io::Error::last_os_error()
            ));
        }
        Ok(start_ns)
    }

    /// Kill the group and every descendant that left it while its pid is
    /// still pinned (see [`pinned_escapees`]).
    pub(crate) fn kill(&self) {
        // SAFETY: the group's leader is our unreaped child, so the group id
        // still names this workload's group.
        unsafe {
            libc::kill(-self.pid, libc::SIGSTOP);
        }
        for escapee in pinned_escapees(self.pid) {
            // SAFETY: the pid is pinned: its parent, or the escapee itself,
            // is stopped and has not been reaped.
            unsafe {
                libc::kill(escapee, libc::SIGKILL);
            }
        }
        // SAFETY: as above.
        unsafe {
            libc::kill(-self.pid, libc::SIGKILL);
        }
    }

    /// Kill the group until no live member is left, after the leader exited
    /// (unreaped). Returns the members that were still alive and how many
    /// survived.
    pub(crate) fn teardown(&self) -> (u64, u64) {
        let live = || {
            group_members(self.pid)
                .into_iter()
                .filter(|&member| member != self.pid && alive_not_zombie(member))
                .count() as u64
        };
        let descendants = live();
        let deadline = Instant::now() + TEARDOWN_LIMIT;
        let mut remaining = descendants;
        while remaining > 0 && Instant::now() < deadline {
            self.kill();
            std::thread::sleep(Duration::from_millis(2));
            remaining = live();
        }
        (descendants, remaining)
    }

    /// Nothing outlives the group on macOS.
    pub(crate) fn abandoned(&self) {}
}

/// Everything that must hold before the child is created.
fn preflight(
    spec: &crate::cli::RunSpec,
    interval: Duration,
    sweep_budget: Duration,
    fault: Option<Fault>,
) -> Result<(), String> {
    let memsize = sysctl_u64("hw.memsize")
        .map_err(|error| format!("cannot establish containment: {error}"))?;
    let reserve = spec
        .host_reserve_bytes
        .unwrap_or_else(|| (memsize / 4).max(MIN_HOST_RESERVE));
    if spec.mem_limit_bytes.saturating_add(reserve) > memsize {
        return Err(format!(
            "cannot establish containment: the cap of {} bytes plus the host reserve of \
             {reserve} bytes exceeds the host's {memsize} bytes of physical memory",
            spec.mem_limit_bytes
        ));
    }
    match pressure_level() {
        Ok(PRESSURE_NORMAL) => {}
        Ok(level) => {
            return Err(format!(
                "cannot establish containment: host memory pressure is at level {level}, not normal"
            ))
        }
        Err(error) => return Err(format!("cannot establish telemetry: {error}")),
    }
    if fault == Some(Fault::Telemetry) {
        return Err("cannot establish telemetry: injected fault".to_owned());
    }
    // The cadence check: sweeps of this process must fit the age budget.
    // SAFETY: getpid has no preconditions.
    let own = unsafe { libc::getpid() };
    // SAFETY: getpgrp has no preconditions.
    let own_group = unsafe { libc::getpgrp() };
    for _ in 0..20 {
        let started = Instant::now();
        footprint(own).map_err(|error| format!("cannot establish telemetry: {error}"))?;
        let _ = group_members(own_group);
        let _ = children(own);
        let took = started.elapsed();
        if took > sweep_budget {
            return Err(format!(
                "cannot establish telemetry: a sampling sweep took {:.1} ms, over the {:.1} ms \
                 budget for a {:.1} ms interval",
                ms(took),
                ms(sweep_budget),
                ms(interval)
            ));
        }
    }
    Ok(())
}

/// When the sweep after one that finished at `observed` is due. Keyed to
/// the observation, not the previous slot: a fork wakes a sweep early, and
/// advancing the old slot instead would let the next observation age to
/// almost twice the interval.
fn next_sweep_due(observed: Instant, interval: Duration) -> Instant {
    observed + interval
}

/// The floor of the mid-run observation-age budget.
const MIN_SAMPLE_AGE_BUDGET: Duration = Duration::from_millis(50);

/// How old a mid-run observation may get before the run fails closed:
/// twice the interval, never under [`MIN_SAMPLE_AGE_BUDGET`]. macOS now
/// and then wakes a timed sleep tens of milliseconds late, whatever the
/// thread's priority or timer class; a stalled sampler still fails the run.
fn sample_age_budget(interval: Duration) -> Duration {
    (interval * 2).max(MIN_SAMPLE_AGE_BUDGET)
}

/// The result of one sweep over the tree.
struct SweepOutcome {
    footprint: u64,
    discovered: Vec<libc::pid_t>,
    /// Tracked processes confirmed gone; forgotten so a reused pid is never
    /// mistaken for a member.
    gone: Vec<libc::pid_t>,
    escaped: Option<libc::pid_t>,
}

/// Enumerate the tree (the process group plus the children of every tracked
/// process), check that every member is still in the group, and sum the
/// members' current footprints.
fn sweep(
    pgid: libc::pid_t,
    known: &mut BTreeSet<libc::pid_t>,
    injected_failure: bool,
) -> Result<SweepOutcome, String> {
    if injected_failure {
        return Err("injected fault".to_owned());
    }
    let mut discovered = Vec::new();
    let mut frontier: Vec<libc::pid_t> = group_members(pgid);
    frontier.extend(known.iter().copied());
    let mut seen: BTreeSet<libc::pid_t> = BTreeSet::new();
    while let Some(member) = frontier.pop() {
        if !seen.insert(member) {
            continue;
        }
        if known.insert(member) {
            discovered.push(member);
        }
        frontier.extend(children(member));
    }
    let mut total = 0u64;
    let mut escaped = None;
    let mut gone = Vec::new();
    for &member in &seen {
        let info = match bsd_info(member) {
            Ok(info) => info,
            Err(_) if !exists(member) => {
                gone.push(member);
                continue;
            }
            // An exited, unreaped member (the leader the watchdog holds
            // until the group is empty) has nothing left to read.
            Err(error) if is_zombie_error(&error) => continue,
            Err(error) => return Err(format!("process {member} is unreadable: {error}")),
        };
        if info.pbi_status == libc::SZOMB {
            continue;
        }
        if info.pbi_pgid as libc::pid_t != pgid {
            escaped = Some(member);
        }
        match footprint(member) {
            Ok(bytes) => total += bytes,
            Err(_) if !alive_not_zombie(member) => {}
            Err(error) => {
                return Err(format!(
                    "process {member}'s footprint is unreadable: {error}"
                ))
            }
        }
    }
    Ok(SweepOutcome {
        footprint: total,
        discovered,
        gone,
        escaped,
    })
}

fn footprint(pid: libc::pid_t) -> Result<u64, std::io::Error> {
    // SAFETY: the out-parameter is the V4 structure the flavor names.
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        if libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            &mut info as *mut _ as *mut libc::rusage_info_t,
        ) != 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(info.ri_phys_footprint)
    }
}

fn bsd_info(pid: libc::pid_t) -> Result<libc::proc_bsdinfo, std::io::Error> {
    // SAFETY: the out-parameter is the structure the flavor names.
    unsafe {
        let mut info: libc::proc_bsdinfo = std::mem::zeroed();
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as c_int;
        let read = libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        );
        if read != size {
            return Err(std::io::Error::last_os_error());
        }
        Ok(info)
    }
}

/// Whether `pid` exists (a zombie counts: it has not been reaped).
fn exists(pid: libc::pid_t) -> bool {
    // SAFETY: signal 0 only probes for existence.
    unsafe { libc::kill(pid, 0) == 0 }
}

fn c_string(text: &std::ffi::OsStr) -> Result<CString, String> {
    use std::os::unix::ffi::OsStrExt;
    CString::new(text.as_bytes()).map_err(|_| format!("{text:?} contains a NUL byte"))
}

/// Whether a process-information failure for a pid that still exists means
/// it is a zombie: the kernel answers `ESRCH` for an exited, unreaped
/// process, which `kill(pid, 0)` still finds.
fn is_zombie_error(error: &std::io::Error) -> bool {
    error.raw_os_error() == Some(libc::ESRCH)
}

/// How long one kill waits for a process to report itself stopped.
const FREEZE_LIMIT: Duration = Duration::from_millis(100);

/// Wait until `pid` is stopped; `false` if it is gone, a zombie, or still
/// running at `deadline`.
fn stopped(pid: libc::pid_t, deadline: Instant) -> bool {
    loop {
        match bsd_info(pid) {
            Ok(info) if info.pbi_status == libc::SSTOP => return true,
            Ok(info) if info.pbi_status != libc::SZOMB && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_micros(200));
            }
            _ => return false,
        }
    }
}

/// The descendants outside group `pgid` whose pids cannot have been reused,
/// once the group has been sent `SIGSTOP`. A child of a stopped process is
/// pinned: its parent cannot reap it. So each group member is confirmed
/// stopped before its children are read, and each escapee is stopped and
/// confirmed before its own children are. An escapee whose parent exited
/// belongs to launchd and is never found here.
fn pinned_escapees(pgid: libc::pid_t) -> Vec<libc::pid_t> {
    let deadline = Instant::now() + FREEZE_LIMIT;
    let mut frontier = Vec::new();
    for member in group_members(pgid) {
        if stopped(member, deadline) {
            frontier.extend(children(member).into_iter().filter(|&child| {
                bsd_info(child).is_ok_and(|info| info.pbi_pgid as libc::pid_t != pgid)
            }));
        }
    }
    let mut pinned = Vec::new();
    let mut seen = BTreeSet::new();
    while let Some(escapee) = frontier.pop() {
        if !seen.insert(escapee) {
            continue;
        }
        pinned.push(escapee);
        // SAFETY: `escapee` is pinned by its stopped parent.
        unsafe {
            libc::kill(escapee, libc::SIGSTOP);
        }
        if stopped(escapee, deadline) {
            frontier.extend(children(escapee));
        }
    }
    pinned
}

fn alive_not_zombie(pid: libc::pid_t) -> bool {
    exists(pid) && bsd_info(pid).is_ok_and(|info| info.pbi_status != libc::SZOMB)
}

/// `fill` is one of libproc's pid listings, which take the buffer size in
/// bytes and return the number of pids written.
fn pid_list(fill: impl Fn(*mut libc::c_void, c_int) -> c_int) -> Vec<libc::pid_t> {
    let mut capacity = 64usize;
    loop {
        let mut buffer = vec![0 as libc::pid_t; capacity];
        let bytes = (capacity * std::mem::size_of::<libc::pid_t>()) as c_int;
        let returned = fill(buffer.as_mut_ptr() as *mut libc::c_void, bytes);
        if returned <= 0 {
            return Vec::new();
        }
        let count = returned as usize;
        if count < capacity {
            buffer.truncate(count);
            buffer.retain(|&pid| pid > 0);
            return buffer;
        }
        capacity *= 2;
    }
}

fn group_members(pgid: libc::pid_t) -> Vec<libc::pid_t> {
    // SAFETY: the buffer and its size come from `pid_list`.
    pid_list(|buffer, size| unsafe { libc::proc_listpgrppids(pgid, buffer, size) })
}

fn children(pid: libc::pid_t) -> Vec<libc::pid_t> {
    // SAFETY: the buffer and its size come from `pid_list`.
    pid_list(|buffer, size| unsafe { libc::proc_listchildpids(pid, buffer, size) })
}

fn sysctl_u64(name: &str) -> Result<u64, String> {
    let key = CString::new(name).expect("static sysctl name");
    let mut value = 0u64;
    let mut size = std::mem::size_of::<u64>();
    // SAFETY: the out-parameter and its size match.
    let read = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            &mut value as *mut u64 as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return Err(format!(
            "sysctl {name}: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(value)
}

fn pressure_level() -> Result<i32, String> {
    let key = CString::new("kern.memorystatus_vm_pressure_level").expect("static sysctl name");
    let mut value: c_int = 0;
    let mut size = std::mem::size_of::<c_int>();
    // SAFETY: the out-parameter and its size match.
    let read = unsafe {
        libc::sysctlbyname(
            key.as_ptr(),
            &mut value as *mut c_int as *mut libc::c_void,
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return Err(format!(
            "host memory pressure is unreadable: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(value)
}

/// Spawn the child suspended, in a new process group, with only its three
/// standard descriptors and default signal dispositions.
fn spawn_suspended(request: &SpawnRequest) -> Result<libc::pid_t, String> {
    let program_text = os(&request.program).to_string_lossy().into_owned();
    let refuse = |why: String| format!("cannot spawn {program_text}: {why}");
    let program = c_string(os(&request.program)).map_err(refuse)?;
    let mut argv_owned = vec![program.clone()];
    for arg in &request.args {
        argv_owned.push(c_string(os(arg)).map_err(refuse)?);
    }
    let mut envp_owned = Vec::new();
    for (name, value) in &request.env {
        let mut entry = name.clone();
        entry.push(b'=');
        entry.extend_from_slice(value);
        envp_owned.push(c_string(os(&entry)).map_err(refuse)?);
    }
    let cwd = match &request.cwd {
        Some(cwd) => Some(c_string(os(cwd)).map_err(refuse)?),
        None => None,
    };
    let stdin = disk::open("/dev/null")
        .map_err(|error| refuse(format!("cannot open /dev/null: {error}")))?;
    let output = |path: &[u8]| {
        disk::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(os(path))
            .map_err(|error| refuse(format!("cannot open {:?}: {error}", os(path))))
    };
    let stdout = output(&request.stdout)?;
    let stderr = output(&request.stderr)?;
    let mut argv: Vec<*mut c_char> = argv_owned
        .iter()
        .map(|arg| arg.as_ptr() as *mut c_char)
        .collect();
    argv.push(std::ptr::null_mut());
    let mut envp: Vec<*mut c_char> = envp_owned
        .iter()
        .map(|entry| entry.as_ptr() as *mut c_char)
        .collect();
    envp.push(std::ptr::null_mut());

    // SAFETY: the attribute and file-action objects are initialised before
    // use and destroyed on every path; every pointer handed to posix_spawn
    // outlives the call.
    unsafe {
        let mut actions: libc::posix_spawn_file_actions_t = std::ptr::null_mut();
        let mut attr: libc::posix_spawnattr_t = std::ptr::null_mut();
        if libc::posix_spawn_file_actions_init(&mut actions) != 0 {
            return Err(refuse("posix_spawn_file_actions_init failed".to_owned()));
        }
        if libc::posix_spawnattr_init(&mut attr) != 0 {
            libc::posix_spawn_file_actions_destroy(&mut actions);
            return Err(refuse("posix_spawnattr_init failed".to_owned()));
        }
        let mut all_signals: libc::sigset_t = std::mem::zeroed();
        let mut no_signals: libc::sigset_t = std::mem::zeroed();
        libc::sigfillset(&mut all_signals);
        libc::sigemptyset(&mut no_signals);
        let flags = libc::POSIX_SPAWN_SETPGROUP
            | libc::POSIX_SPAWN_START_SUSPENDED
            | libc::POSIX_SPAWN_CLOEXEC_DEFAULT
            | libc::POSIX_SPAWN_SETSIGDEF
            | libc::POSIX_SPAWN_SETSIGMASK;
        let mut failed = libc::posix_spawnattr_setflags(&mut attr, flags as libc::c_short) != 0
            || libc::posix_spawnattr_setpgroup(&mut attr, 0) != 0
            || libc::posix_spawnattr_setsigdefault(&mut attr, &all_signals) != 0
            || libc::posix_spawnattr_setsigmask(&mut attr, &no_signals) != 0
            || libc::posix_spawn_file_actions_adddup2(&mut actions, stdin.as_raw_fd(), 0) != 0
            || libc::posix_spawn_file_actions_adddup2(&mut actions, stdout.as_raw_fd(), 1) != 0
            || libc::posix_spawn_file_actions_adddup2(&mut actions, stderr.as_raw_fd(), 2) != 0;
        if let Some(cwd) = &cwd {
            failed =
                failed || posix_spawn_file_actions_addchdir_np(&mut actions, cwd.as_ptr()) != 0;
        }
        let mut pid: libc::pid_t = 0;
        let spawned = if failed {
            Err(refuse("cannot prepare the spawn attributes".to_owned()))
        } else {
            let status = if program.as_bytes().contains(&b'/') {
                libc::posix_spawn(
                    &mut pid,
                    program.as_ptr(),
                    &actions,
                    &attr,
                    argv.as_ptr(),
                    envp.as_ptr(),
                )
            } else {
                libc::posix_spawnp(
                    &mut pid,
                    program.as_ptr(),
                    &actions,
                    &attr,
                    argv.as_ptr(),
                    envp.as_ptr(),
                )
            };
            if status == 0 {
                Ok(pid)
            } else {
                Err(refuse(
                    std::io::Error::from_raw_os_error(status).to_string(),
                ))
            }
        };
        libc::posix_spawnattr_destroy(&mut attr);
        libc::posix_spawn_file_actions_destroy(&mut actions);
        spawned
    }
}

/// What woke the supervisor.
struct Events {
    forked: bool,
}

/// A kqueue watching the tree's processes and the wakeup descriptors.
struct Queue {
    fd: std::os::fd::OwnedFd,
}

impl Queue {
    fn new() -> std::io::Result<Queue> {
        use std::os::fd::FromRawFd;
        // SAFETY: kqueue returns a fresh descriptor or -1.
        let fd = unsafe { libc::kqueue() };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: the descriptor is fresh and owned from here on.
        Ok(Queue {
            fd: unsafe { std::os::fd::OwnedFd::from_raw_fd(fd) },
        })
    }

    fn register(&self, ident: usize, filter: i16, fflags: u32) -> std::io::Result<()> {
        let change = libc::kevent {
            ident,
            filter,
            flags: libc::EV_ADD | libc::EV_ENABLE,
            fflags,
            data: 0,
            udata: std::ptr::null_mut(),
        };
        // SAFETY: registers one change; no events are read.
        let result = unsafe {
            libc::kevent(
                self.fd.as_raw_fd(),
                &change,
                1,
                std::ptr::null_mut(),
                0,
                std::ptr::null(),
            )
        };
        if result < 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }

    fn watch_process(&self, pid: libc::pid_t) -> std::io::Result<()> {
        self.register(
            pid as usize,
            libc::EVFILT_PROC,
            libc::NOTE_EXIT | libc::NOTE_FORK | libc::NOTE_EXEC,
        )
    }

    fn watch_fd(&self, fd: i32) -> std::io::Result<()> {
        self.register(fd as usize, libc::EVFILT_READ, 0)
    }

    fn wait(&self, limit: Duration) -> std::io::Result<Events> {
        let timeout = libc::timespec {
            tv_sec: limit.as_secs() as libc::time_t,
            tv_nsec: limit.subsec_nanos() as libc::c_long,
        };
        // SAFETY: an all-zero kevent array is valid storage for results.
        let mut events: [libc::kevent; 16] = unsafe { std::mem::zeroed() };
        // SAFETY: reads into a local array of the stated length.
        let count = unsafe {
            libc::kevent(
                self.fd.as_raw_fd(),
                std::ptr::null(),
                0,
                events.as_mut_ptr(),
                events.len() as c_int,
                &timeout,
            )
        };
        if count < 0 {
            let error = std::io::Error::last_os_error();
            if error.kind() == std::io::ErrorKind::Interrupted {
                return Ok(Events { forked: false });
            }
            return Err(error);
        }
        let mut result = Events { forked: false };
        for event in &events[..count as usize] {
            if event.filter == libc::EVFILT_PROC && event.fflags & libc::NOTE_FORK != 0 {
                result.forked = true;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    fn spawn_in_own_group(program: &str, args: &[&str]) -> std::process::Child {
        Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn")
    }

    /// Block until `pid` is a zombie, without reaping it.
    fn wait_for_zombie(pid: libc::pid_t) {
        // SAFETY: waitid with a zeroed local out-parameter; WNOWAIT leaves
        // the child unreaped.
        unsafe {
            let mut info: libc::siginfo_t = std::mem::zeroed();
            libc::waitid(
                libc::P_PID,
                pid as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOWAIT,
            );
        }
    }

    /// A fork wakes a sweep before its slot; the one after it is still due
    /// an interval later, never an interval past the skipped slot, so no
    /// observation ages to twice the interval.
    #[test]
    fn an_early_sweep_does_not_skip_the_next_slot() {
        let interval = Duration::from_millis(10);
        let base = Instant::now();
        let early = base + Duration::from_millis(1);
        let due = next_sweep_due(early, interval);
        assert!(
            due.duration_since(early) <= interval,
            "due {:?} after the last observation",
            due.duration_since(early)
        );
    }

    /// A late wakeup of the size macOS produces now and then (tens of
    /// milliseconds) must not fail a run at the default cadence; a slow
    /// interval still gets twice itself.
    #[test]
    fn the_age_budget_absorbs_a_late_wakeup_but_scales_with_the_interval() {
        assert!(sample_age_budget(DEFAULT_SAMPLE_INTERVAL) >= Duration::from_millis(50));
        assert_eq!(
            sample_age_budget(Duration::from_millis(100)),
            Duration::from_millis(200)
        );
    }

    #[test]
    fn the_group_and_child_listings_name_every_member() {
        let mut child = spawn_in_own_group("/bin/sleep", &["30"]);
        let pid = child.id() as libc::pid_t;
        let members = group_members(pid);
        // SAFETY: getpid has no preconditions.
        let own_children = children(unsafe { libc::getpid() });
        child.kill().ok();
        child.wait().ok();
        assert_eq!(members, vec![pid]);
        assert!(
            own_children.contains(&pid),
            "{pid} missing from {own_children:?}"
        );
    }

    #[test]
    fn a_sweep_over_an_unreaped_exited_leader_is_not_a_telemetry_loss() {
        let mut child = spawn_in_own_group("/usr/bin/true", &[]);
        let pid = child.id() as libc::pid_t;
        wait_for_zombie(pid);
        let mut known = BTreeSet::from([pid]);
        let outcome = sweep(pid, &mut known, false);
        let still_alive = alive_not_zombie(pid);
        child.wait().ok();
        let outcome = outcome.expect("a zombie leader is exited, not unreadable");
        assert_eq!(outcome.footprint, 0);
        assert!(outcome.escaped.is_none());
        assert!(!still_alive);
    }
}
