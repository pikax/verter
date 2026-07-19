//! Process-tree-aware spawn and termination for engine subprocesses.
//!
//! A tsgo candidate (or a shim wrapping one) can spawn DESCENDANTS that
//! outlive the direct child — classically a grandchild that inherits the
//! stdout/stderr pipe handles and keeps them open after the candidate exits.
//! Killing only the direct child then leaves the pipes open (a "bounded"
//! probe hangs on its reader joins) and leaks the descendant. The utilities
//! here make engine spawns tree-addressable:
//!
//! - [`configure_tree_spawn`] / [`configure_tree_spawn_std`] put the child in
//!   its OWN process group on Unix (`process_group(0)`); on Windows the job
//!   object is armed post-spawn (see [`TreeKill`]).
//! - [`TreeKill`] kills the WHOLE tree: `killpg` on Unix (the child is the
//!   group leader, so every descendant still in the group dies), a Job Object
//!   (`KILL_ON_JOB_CLOSE` + `TerminateJobObject`) on Windows.
//!
//! [`TreeKill::arm`] must only be used on a child spawned through
//! [`configure_tree_spawn`] / [`configure_tree_spawn_std`]: on Unix the tree
//! kill targets the process GROUP whose id is the child's pid, which is only
//! meaningful when the child leads its own group.

use std::time::Duration;

use crate::error::{TsgoApiError, TsgoApiResult};

/// The bound on the reap wait after a tree kill (SIGKILL / `TerminateJobObject`
/// is prompt; this only bounds an already-dead system's bookkeeping).
pub const REAP_BOUND: Duration = Duration::from_secs(2);

/// Prepare a tokio command so the spawned child leads its own process group
/// (Unix) — the precondition for [`TreeKill`]'s group kill. On Windows this is
/// a no-op at the command level; the job object is armed from the pid in
/// [`TreeKill::arm`].
pub fn configure_tree_spawn(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    command
}

/// The `std::process` variant of [`configure_tree_spawn`] (e.g. verter-tsc's
/// declaration invocation, which drives a blocking child).
pub fn configure_tree_spawn_std(command: &mut std::process::Command) -> &mut std::process::Command {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    command
}

/// A process-tree kill handle armed against a spawned child's pid. Cheap and
/// idempotent: killing an already-exited tree is a no-op.
#[derive(Debug)]
pub struct TreeKill {
    pid: u32,
    #[cfg(windows)]
    job: Option<JobHandle>,
}

impl TreeKill {
    /// Arm the tree kill for `pid` (the child must have been spawned via
    /// [`configure_tree_spawn`] / [`configure_tree_spawn_std`]). On Windows
    /// this creates the `KILL_ON_JOB_CLOSE` job and assigns the child to it;
    /// a failure degrades to a direct-child-only kill (logged by the caller's
    /// context, never silent about the tree).
    ///
    /// `pid == 0` arms a NO-OP kill: `kill(-0, SIGKILL)` would target the
    /// CALLER'S OWN process group, so a caller handing over a pid it failed
    /// to read (e.g. an already-reaped child, whose `Child::id()` is `None`)
    /// must never become a self-kill.
    pub fn arm(pid: u32) -> Self {
        #[cfg(windows)]
        {
            Self {
                pid,
                job: if pid == 0 {
                    None
                } else {
                    JobHandle::create_and_assign(pid)
                },
            }
        }
        #[cfg(not(windows))]
        {
            Self { pid }
        }
    }

    /// Kill the entire process tree rooted at the armed child. Does not reap
    /// the direct child — the caller reaps (`child.wait()`,
    /// [`reap_child_bounded`]) so no zombie is left behind. A tree armed with
    /// pid 0 is a documented no-op (see [`TreeKill::arm`]).
    pub fn kill_tree(&self) {
        if self.pid == 0 {
            return;
        }
        #[cfg(unix)]
        {
            // The child leads the group (configure_tree_spawn), so the group
            // id IS its pid: SIGKILL the whole group — descendants that
            // inherited the pipes die too. An empty group (tree already gone)
            // is ESRCH, harmlessly ignored.
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(windows)]
        {
            if let Some(job) = &self.job {
                job.terminate();
            } else {
                // Degraded (job assignment failed): at least the direct child.
                terminate_process(self.pid);
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            terminate_process(self.pid);
        }
    }
}

/// Reap a tokio child after a kill, bounded: never hang on a stuck reaper.
/// Returns `true` when the child was reaped. On a reap timeout the caller
/// relies on tokio's `kill_on_drop` orphan reaping (the child handle is
/// dropped right after).
pub async fn reap_child_bounded(child: &mut tokio::process::Child, bound: Duration) -> bool {
    matches!(tokio::time::timeout(bound, child.wait()).await, Ok(Ok(_)))
}

/// Whether `pid` currently names a live process (zombie-free check: a reaped
/// pid reads dead).
#[cfg(unix)]
pub fn process_alive(pid: u32) -> bool {
    // kill(pid, 0): 0 = live; EPERM = live but owned by another user; ESRCH = dead.
    let rc = unsafe { libc::kill(pid as i32, 0) };
    if rc == 0 {
        return true;
    }
    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(code) if code == libc::EPERM
    )
}

/// Windows: a pid is alive when `OpenProcess` succeeds (or is denied, meaning
/// it exists but is inaccessible).
#[cfg(windows)]
pub fn process_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    const ERROR_ACCESS_DENIED: i32 = 5;
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle != std::ptr::null_mut() {
        unsafe {
            CloseHandle(handle);
        }
        return true;
    }
    matches!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(code) if code == ERROR_ACCESS_DENIED
    )
}

/// Fallback for non-Unix/non-Windows hosts: liveness cannot be portably
/// checked; report dead (the kill paths above also degrade to direct-child).
#[cfg(not(any(unix, windows)))]
pub fn process_alive(_pid: u32) -> bool {
    false
}

/// Forcibly terminate ONE process by pid (SIGKILL / `TerminateProcess`). Test
/// cleanup and the degraded no-job path use this; tree kills go through
/// [`TreeKill`].
#[cfg(unix)]
pub fn terminate_process(pid: u32) {
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
}

/// Windows: terminate ONE process by pid.
#[cfg(windows)]
pub fn terminate_process(pid: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, TerminateProcess, PROCESS_TERMINATE};

    let handle = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if handle != std::ptr::null_mut() {
        unsafe {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
}

/// Fallback for non-Unix/non-Windows hosts.
#[cfg(not(any(unix, windows)))]
pub fn terminate_process(_pid: u32) {}

/// The Windows job object backing [`TreeKill`]: created with
/// `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` (dropping the handle kills the tree as
/// a backstop) and terminated explicitly by [`TreeKill::kill_tree`].
#[cfg(windows)]
#[derive(Debug)]
struct JobHandle(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl JobHandle {
    fn create_and_assign(pid: u32) -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
            SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job == std::ptr::null_mut() {
                return None;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                CloseHandle(job);
                return None;
            }
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process == std::ptr::null_mut() {
                CloseHandle(job);
                return None;
            }
            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            if assigned == 0 {
                CloseHandle(job);
                return None;
            }
            Some(Self(job))
        }
    }

    fn terminate(&self) {
        use windows_sys::Win32::System::JobObjects::TerminateJobObject;
        unsafe {
            TerminateJobObject(self.0, 1);
        }
    }
}

#[cfg(windows)]
impl Drop for JobHandle {
    fn drop(&mut self) {
        // KILL_ON_JOB_CLOSE: closing our (last) handle kills any surviving
        // tree member — the backstop behind the explicit terminate.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[cfg(windows)]
unsafe impl Send for JobHandle {}

#[cfg(windows)]
unsafe impl Sync for JobHandle {}

/// Kill + bounded-reap convenience for a tokio child spawned via
/// [`configure_tree_spawn`]: kill the tree, then reap the direct child within
/// `bound`. Returns an error string when the reap could not complete (the
/// caller surfaces it; the child handle's `kill_on_drop` remains the backstop).
pub async fn kill_tree_and_reap(
    child: &mut tokio::process::Child,
    bound: Duration,
) -> TsgoApiResult<()> {
    let pid = child.id().unwrap_or(0);
    TreeKill::arm(pid).kill_tree();
    if reap_child_bounded(child, bound).await {
        Ok(())
    } else {
        Err(TsgoApiError::Transport(format!(
            "the engine child (pid {pid}) could not be reaped within {} ms after the tree kill",
            bound.as_millis()
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── DISCRIMINATING: a child in its own group whose grandchild inherits the
    //    pipes is tree-killed — the group kill reaches the descendant, and the
    //    direct child is reaped. Drives /bin/sh (unix-only; the Windows job
    //    object path mirrors it through the same TreeKill API). ────────────────
    #[cfg(unix)]
    #[tokio::test]
    async fn tree_kill_reaches_a_pipe_holding_descendant() {
        use tokio::io::AsyncReadExt;

        // sh -c 'echo ready; (exec sleep 600) & wait' — the grandchild sleep
        // inherits stdout/stderr; the direct child waits on it forever.
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("echo ready; sleep 600 & wait")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        configure_tree_spawn(&mut command);
        let mut child = command.spawn().expect("spawn sh");
        let pid = child.id().expect("child pid");

        // Let the tree establish, then kill it.
        tokio::time::sleep(Duration::from_millis(200)).await;
        TreeKill::arm(pid).kill_tree();
        assert!(
            reap_child_bounded(&mut child, REAP_BOUND).await,
            "the direct child must be reaped after the tree kill"
        );
        // The pipes must hit EOF once every writer is dead (the grandchild
        // held them): a read that returns 0 bytes proves the tree died.
        let mut out = child.stdout.take().expect("stdout");
        let mut buf = Vec::new();
        let read = tokio::time::timeout(REAP_BOUND, out.read_to_end(&mut buf)).await;
        assert!(
            read.is_ok(),
            "the pipes must drain to EOF once the tree is dead"
        );
    }

    // ── the liveness probe agrees with a real spawn/kill round-trip ──────────
    #[cfg(unix)]
    #[tokio::test]
    async fn process_alive_tracks_a_real_child() {
        let mut command = tokio::process::Command::new("/bin/sleep");
        command.arg("30").kill_on_drop(true);
        configure_tree_spawn(&mut command);
        let mut child = command.spawn().expect("spawn sleep");
        let pid = child.id().expect("pid");
        assert!(process_alive(pid));
        TreeKill::arm(pid).kill_tree();
        assert!(reap_child_bounded(&mut child, REAP_BOUND).await);
        for _ in 0..40 {
            if !process_alive(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(!process_alive(pid), "a reaped child must read dead");
    }

    // ── DISCRIMINATING: pid 0 arms a NO-OP kill — `kill(-0, SIGKILL)` would
    //    target the CALLER'S OWN process group (a caller that failed to read a
    //    pid must never become a self-kill). If this regressed, this very test
    //    binary would be SIGKILLed mid-run. ────────────────────────────────────
    #[test]
    fn tree_kill_armed_with_pid_zero_is_a_noop() {
        TreeKill::arm(0).kill_tree();
    }

    // ── kill_tree_and_reap surfaces an outcome, never hangs ──────────────────
    #[cfg(unix)]
    #[tokio::test]
    async fn kill_tree_and_reap_is_bounded() {
        let mut command = tokio::process::Command::new("/bin/sleep");
        command.arg("30").kill_on_drop(true);
        configure_tree_spawn(&mut command);
        let mut child = command.spawn().expect("spawn sleep");
        let start = std::time::Instant::now();
        kill_tree_and_reap(&mut child, REAP_BOUND)
            .await
            .expect("a healthy kill+reap succeeds");
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
