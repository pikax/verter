//! CPU-time accounting for the process tree a [`super::TreeKill`] is armed
//! against.
//!
//! Wall-clock time is the wrong measure of whether a child is working: on a
//! loaded machine a healthy child spends most of its wall time waiting for a
//! core. The summed user+system CPU time of the whole tree only advances while
//! the tree actually runs, so a caller can tell "still computing" from "wedged"
//! and "runaway" without counting the time the scheduler kept it queued.
//!
//! The tree is every process descended from the armed root by parentage, plus
//! every process still in the root's process group. Parentage matters: an
//! engine the root starts may lead a process group of its own (the provider
//! transports spawn tsgo that way so they can tree-kill it), and counting only
//! the root's group would miss exactly the process doing the work. Group
//! membership catches descendants reparented away after their parent exited.
//! Time a member spent in children it already reaped is included through the
//! member's own child-time counters.

use std::time::Duration;

/// One process as the platform reports it: identity, parentage, group, and the
/// CPU time it has used (its own plus that of children it reaped), in the
/// platform's native unit.
#[cfg(any(target_os = "linux", target_os = "macos"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ProcRow {
    pid: u32,
    ppid: u32,
    pgrp: u32,
    cpu: u64,
}

/// The rows that belong to the tree rooted at `root`: the root, everything
/// descended from it by parentage, and everything in its process group.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn tree_members(root: u32, rows: &[ProcRow]) -> Vec<ProcRow> {
    use std::collections::{BTreeMap, BTreeSet};

    let mut children: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for row in rows {
        children.entry(row.ppid).or_default().push(row.pid);
    }
    let mut members: BTreeSet<u32> = rows
        .iter()
        .filter(|row| row.pid == root || row.pgrp == root)
        .map(|row| row.pid)
        .collect();
    let mut frontier: Vec<u32> = members.iter().copied().collect();
    while let Some(pid) = frontier.pop() {
        for &child in children.get(&pid).map(Vec::as_slice).unwrap_or_default() {
            // pid 0 is the kernel's own parent on macOS; never walk through it.
            if child != 0 && members.insert(child) {
                frontier.push(child);
            }
        }
    }
    rows.iter()
        .filter(|row| members.contains(&row.pid))
        .copied()
        .collect()
}

/// `/proc/<pid>/stat` → the row it describes. The command name is enclosed in
/// parentheses and may itself contain spaces or `)`, so the fixed fields are
/// read after the LAST `") "`.
#[cfg(target_os = "linux")]
fn parse_linux_stat(pid: u32, stat: &str) -> Option<ProcRow> {
    let (_, rest) = stat.rsplit_once(") ")?;
    // After the name: state(3) ppid(4) pgrp(5) … utime(14) stime(15)
    // cutime(16) cstime(17); index 0 here is field 3.
    let fields: Vec<&str> = rest.split_ascii_whitespace().collect();
    let field = |n: usize| fields.get(n - 3).and_then(|f| f.parse::<i64>().ok());
    let ticks = [14, 15, 16, 17]
        .iter()
        .map(|&n| field(n).map(|value| value.max(0) as u64))
        .sum::<Option<u64>>()?;
    Some(ProcRow {
        pid,
        ppid: u32::try_from(field(4)?).ok()?,
        pgrp: u32::try_from(field(5)?).ok()?,
        cpu: ticks,
    })
}

/// Linux: scan `/proc`, keep the tree's rows, convert clock ticks.
#[cfg(target_os = "linux")]
pub(super) fn descendant_cpu_time(root: u32) -> Option<Duration> {
    // SAFETY: sysconf has no preconditions.
    let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let ticks_per_second = u64::try_from(ticks_per_second).ok().filter(|t| *t > 0)?;
    let rows: Vec<ProcRow> = std::fs::read_dir("/proc")
        .ok()?
        .flatten()
        .filter_map(|entry| {
            let pid = entry.file_name().to_str()?.parse::<u32>().ok()?;
            // A process may exit between the listing and this read; it simply
            // drops out of this sample.
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            parse_linux_stat(pid, &stat)
        })
        .collect();
    let ticks: u64 = tree_members(root, &rows).iter().map(|row| row.cpu).sum();
    let nanos = u128::from(ticks) * 1_000_000_000 / u128::from(ticks_per_second);
    Some(Duration::from_nanos(
        u64::try_from(nanos).unwrap_or(u64::MAX),
    ))
}

/// macOS: list every pid, read each one's parent and group, then read the CPU
/// usage of the tree's members only.
#[cfg(target_os = "macos")]
pub(super) fn descendant_cpu_time(root: u32) -> Option<Duration> {
    use std::ffi::c_void;
    use std::mem::{size_of, MaybeUninit};

    // `mach_timebase_info` is declared here rather than taken from `libc`,
    // whose binding is deprecated in favour of a crate this workspace does not
    // use. The layout is the documented `struct mach_timebase_info`.
    #[repr(C)]
    struct MachTimebaseInfo {
        numer: u32,
        denom: u32,
    }
    extern "C" {
        fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
    }

    // SAFETY: a null buffer asks only for the current count.
    let estimate = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    let estimate = usize::try_from(estimate).ok().filter(|n| *n > 0)?;
    // Headroom for processes started between the two calls.
    let mut pids: Vec<libc::pid_t> = vec![0; estimate + 256];
    let buffer_bytes = libc::c_int::try_from(pids.len() * size_of::<libc::pid_t>()).ok()?;
    // SAFETY: the buffer is `buffer_bytes` long and properly aligned.
    let listed =
        unsafe { libc::proc_listallpids(pids.as_mut_ptr().cast::<c_void>(), buffer_bytes) };
    let listed = usize::try_from(listed).ok().filter(|n| *n > 0)?;
    pids.truncate(listed.min(pids.len()));

    let rows: Vec<ProcRow> = pids
        .iter()
        .filter_map(|&pid| {
            let pid_u32 = u32::try_from(pid).ok().filter(|p| *p > 0)?;
            let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
            let size = libc::c_int::try_from(size_of::<libc::proc_bsdinfo>()).ok()?;
            // SAFETY: `info` is a properly sized, writable `proc_bsdinfo`.
            let written = unsafe {
                libc::proc_pidinfo(
                    pid,
                    libc::PROC_PIDTBSDINFO,
                    0,
                    info.as_mut_ptr().cast::<c_void>(),
                    size,
                )
            };
            if written != size {
                // Exited since the listing, or not inspectable by this user —
                // neither can be a descendant still doing this tree's work.
                return None;
            }
            // SAFETY: the kernel filled all `size` bytes.
            let info = unsafe { info.assume_init() };
            Some(ProcRow {
                pid: pid_u32,
                ppid: info.pbi_ppid,
                pgrp: info.pbi_pgid,
                cpu: 0,
            })
        })
        .collect();

    let mut timebase = MachTimebaseInfo { numer: 0, denom: 0 };
    // SAFETY: `timebase` is a writable `mach_timebase_info`.
    if unsafe { mach_timebase_info(&mut timebase) } != 0 || timebase.denom == 0 {
        return None;
    }

    let mut ticks: u128 = 0;
    for member in tree_members(root, &rows) {
        let mut usage = MaybeUninit::<libc::rusage_info_v2>::zeroed();
        // SAFETY: `usage` is a writable `rusage_info_v2`, the struct the
        // `RUSAGE_INFO_V2` flavor fills.
        let rc = unsafe {
            libc::proc_pid_rusage(
                member.pid as libc::c_int,
                libc::RUSAGE_INFO_V2,
                usage.as_mut_ptr().cast::<libc::rusage_info_t>(),
            )
        };
        if rc != 0 {
            continue;
        }
        // SAFETY: a zero return means the kernel filled the struct.
        let usage = unsafe { usage.assume_init() };
        // The `ri_*_time` fields are in mach absolute-time units (nanoseconds
        // on Intel, ticks on Apple silicon); the timebase converts both.
        ticks += u128::from(usage.ri_user_time)
            + u128::from(usage.ri_system_time)
            + u128::from(usage.ri_child_user_time)
            + u128::from(usage.ri_child_system_time);
    }
    let nanos = ticks * u128::from(timebase.numer) / u128::from(timebase.denom);
    Some(Duration::from_nanos(
        u64::try_from(nanos).unwrap_or(u64::MAX),
    ))
}

/// Windows: the job object's own accounting, which covers every process ever
/// assigned to it (descendants inherit the job, nested jobs included) and keeps
/// the time of members that already exited.
#[cfg(windows)]
pub(super) fn job_cpu_time(job: windows_sys::Win32::Foundation::HANDLE) -> Option<Duration> {
    use windows_sys::Win32::System::JobObjects::{
        JobObjectBasicAccountingInformation, QueryInformationJobObject,
        JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    };

    // SAFETY: an all-zero accounting record is a valid value of this plain
    // data struct.
    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: `info` is a writable record of exactly the size passed.
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
            std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then(|| hundred_ns(info.TotalUserTime) + hundred_ns(info.TotalKernelTime))
}

/// Windows without a job (assignment failed, so the kill is degraded to the
/// direct child too): the direct child's own times. `None` once it is gone.
#[cfg(windows)]
pub(super) fn process_cpu_time(pid: u32) -> Option<Duration> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    // SAFETY: OpenProcess has no memory preconditions; a null return is checked.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle.is_null() {
        return None;
    }
    let zero = FILETIME {
        dwLowDateTime: 0,
        dwHighDateTime: 0,
    };
    let (mut created, mut exited, mut kernel, mut user) = (zero, zero, zero, zero);
    // SAFETY: `handle` is a live process handle and every out-pointer is a
    // writable FILETIME.
    let ok = unsafe { GetProcessTimes(handle, &mut created, &mut exited, &mut kernel, &mut user) };
    // SAFETY: `handle` came from OpenProcess and is closed exactly once.
    unsafe {
        CloseHandle(handle);
    }
    let filetime = |t: FILETIME| (i64::from(t.dwHighDateTime) << 32) | i64::from(t.dwLowDateTime);
    (ok != 0).then(|| hundred_ns(filetime(kernel)) + hundred_ns(filetime(user)))
}

#[cfg(windows)]
fn hundred_ns(value: i64) -> Duration {
    Duration::from_nanos(u64::try_from(value).unwrap_or(0).saturating_mul(100))
}

#[cfg(test)]
mod tests {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::time::{Duration, Instant};

    use super::super::{configure_tree_spawn_std, TreeKill};

    /// How long a wait below may take before the test calls it a hang. Only a
    /// broken implementation reaches it; a passing run returns on the first
    /// observation that satisfies the condition.
    const HANG_GUARD: Duration = Duration::from_secs(8);

    /// CPU a spinning tree must accumulate before the test treats it as
    /// "visibly growing" — well above every platform's accounting granularity
    /// (10ms clock ticks on Linux).
    const GROWTH: Duration = Duration::from_millis(100);

    struct Tree {
        child: Child,
        kill: TreeKill,
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            self.kill.kill_tree();
            let _ = self.child.wait();
        }
    }

    fn spawn_tree(mut command: Command) -> Tree {
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        configure_tree_spawn_std(&mut command);
        let child = command.spawn().expect("spawn the fixture tree");
        let kill = TreeKill::arm(child.id());
        Tree { child, kill }
    }

    /// A process that computes until it is killed.
    fn spinner() -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd");
            command.arg("/C").raw_arg("for /L %i in (0,0,1) do @rem");
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sh");
            command.args(["-c", "while :; do :; done"]);
            command
        }
    }

    /// A process that prints `ready` and then blocks reading a stdin nobody
    /// writes, using no CPU from then on.
    fn blocked_reader() -> Command {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            let mut command = Command::new("cmd");
            command.arg("/C").raw_arg("echo ready& set /p x=");
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sh");
            command.args(["-c", "echo ready; read x"]);
            command
        }
    }

    fn cpu(tree: &Tree) -> Duration {
        tree.kill
            .cpu_time()
            .expect("a live tree's CPU time must be measurable on this platform")
    }

    /// Wait for `tree` to have used `GROWTH` more CPU than `from`. The hang
    /// guard only stops a broken implementation from hanging the suite.
    fn wait_for_growth(tree: &Tree, from: Duration, what: &str) -> Duration {
        let deadline = Instant::now() + HANG_GUARD;
        loop {
            let now = cpu(tree);
            if now >= from + GROWTH {
                return now;
            }
            assert!(
                Instant::now() < deadline,
                "{what}: the tree's CPU time stayed at {now:?} (started at {from:?}) although \
                 it was computing the whole time"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    /// DISCRIMINATING: CPU time advances for a tree that computes and stays
    /// flat for one that is blocked, side by side and over the same interval.
    ///
    /// An implementation that returns a constant fails the spinning half; one
    /// that measures the wrong processes (the caller, the whole machine, or
    /// wall time) fails the blocked half.
    #[test]
    fn tree_cpu_time_grows_while_computing_and_stays_flat_while_blocked() {
        let spinning = spawn_tree(spinner());
        let mut blocked = spawn_tree(blocked_reader());

        let mut line = String::new();
        BufReader::new(blocked.child.stdout.take().expect("stdout was piped"))
            .read_line(&mut line)
            .expect("read the blocked fixture's readiness line");
        assert_eq!(line.trim(), "ready", "the blocked fixture must start");

        let spin_start = cpu(&spinning);
        // The first interval lets the blocked shell finish entering its read;
        // the second is the one measured.
        let spin_mid = wait_for_growth(&spinning, spin_start, "first interval");
        let blocked_before = cpu(&blocked);
        wait_for_growth(&spinning, spin_mid, "second interval");
        let blocked_after = cpu(&blocked);

        assert_eq!(
            blocked_before, blocked_after,
            "a blocked tree used no CPU while another tree computed for {GROWTH:?}, so its \
             CPU time must not move"
        );
    }

    /// DISCRIMINATING: a descendant that leads a process group of its own is
    /// still counted — the provider transports start tsgo exactly this way, and
    /// a group-only count would see an idle parent while the engine does all
    /// the work.
    #[cfg(unix)]
    #[test]
    fn tree_cpu_time_counts_a_descendant_in_its_own_process_group() {
        // The root shell only waits; the grandchild computes in a new group.
        // Linux shells refuse job control without a terminal, so `setsid` makes
        // the group there; macOS has no `setsid` but its `sh` honours `set -m`.
        // The grandchild reports its own pid, so the report comes after it is
        // already in its new group.
        let script = if cfg!(target_os = "linux") {
            "setsid sh -c 'echo $$; while :; do :; done' & wait"
        } else {
            "set -m; sh -c 'echo $$; while :; do :; done' & wait"
        };
        let mut command = Command::new("sh");
        command.args(["-c", script]);
        let mut tree = spawn_tree(command);

        let mut line = String::new();
        BufReader::new(tree.child.stdout.take().expect("stdout was piped"))
            .read_line(&mut line)
            .expect("read the grandchild's pid");
        let grandchild: libc::pid_t = line.trim().parse().expect("a numeric grandchild pid");

        // Precondition: the shape under test really holds.
        // SAFETY: getpgid has no memory preconditions.
        let group = unsafe { libc::getpgid(grandchild) };
        assert!(
            group > 0 && group != tree.child.id() as libc::pid_t,
            "the fixture's grandchild must lead a group other than the root's (got {group})"
        );

        let start = cpu(&tree);
        wait_for_growth(&tree, start, "descendant in its own group");
    }

    /// The tree membership walk: parentage, group, and nothing else.
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn tree_members_follow_parentage_and_the_root_group_only() {
        use super::{tree_members, ProcRow};

        let row = |pid, ppid, pgrp| ProcRow {
            pid,
            ppid,
            pgrp,
            cpu: u64::from(pid),
        };
        let rows = [
            row(1, 0, 1),    // init: not in the tree
            row(10, 1, 10),  // the root
            row(11, 10, 10), // its child
            row(12, 11, 12), // a grandchild leading its own group
            row(13, 12, 12), // that group's child
            row(20, 1, 10),  // reparented to init, still in the root's group
            row(30, 1, 30),  // unrelated
            row(31, 30, 30), // unrelated child
            row(40, 20, 40), // child of the reparented group member
        ];
        let mut members: Vec<u32> = tree_members(10, &rows).iter().map(|r| r.pid).collect();
        members.sort_unstable();
        assert_eq!(members, vec![10, 11, 12, 13, 20, 40]);
    }
}
