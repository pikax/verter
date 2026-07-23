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

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::error::{TsgoApiError, TsgoApiResult};

/// The bound on the reap wait after a tree kill (SIGKILL / `TerminateJobObject`
/// is prompt; this only bounds an already-dead system's bookkeeping).
pub const REAP_BOUND: Duration = Duration::from_secs(2);

static ACTIVE_ENGINE_TREES: OnceLock<Mutex<HashMap<u64, u32>>> = OnceLock::new();
static NEXT_ENGINE_TREE_REGISTRATION: AtomicU64 = AtomicU64::new(1);

fn active_engine_trees() -> &'static Mutex<HashMap<u64, u32>> {
    ACTIVE_ENGINE_TREES.get_or_init(|| Mutex::new(HashMap::new()))
}

fn register_engine_tree(pid: u32) -> Option<u64> {
    if pid == 0 {
        return None;
    }
    let registration = NEXT_ENGINE_TREE_REGISTRATION.fetch_add(1, Ordering::Relaxed);
    active_engine_trees()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .insert(registration, pid);
    Some(registration)
}

fn unregister_engine_tree(registration: Option<u64>) {
    let Some(registration) = registration else {
        return;
    };
    active_engine_trees()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .remove(&registration);
}

/// Kill every engine process group currently owned by this process.
///
/// Normal provider teardown uses the individual [`TreeKill`] handles. The
/// process-lifetime monitor calls this first when the LSP client dies, while the
/// LSP runtime is still alive, so Unix descendants are killed before the LSP is
/// forcibly terminated and cannot outlive their direct engine parent.
pub fn terminate_registered_engine_trees() {
    let mut pids: Vec<u32> = active_engine_trees()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
        .values()
        .copied()
        .collect();
    pids.sort_unstable();
    pids.dedup();
    for pid in pids {
        kill_tree_by_pid(pid);
    }
}

/// Prepare a tokio command so the spawned child leads its own process group
/// (Unix) — the precondition for [`TreeKill`]'s group kill. On Windows this is
/// a no-op at the command level; the job object is armed from the pid in
/// [`TreeKill::arm`].
pub fn configure_tree_spawn(command: &mut tokio::process::Command) -> &mut tokio::process::Command {
    #[cfg(unix)]
    {
        command.process_group(0);
    }
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent_pid = std::process::id() as libc::pid_t;
        // SAFETY: the pre-exec closure calls only async-signal-safe libc
        // functions. The child arms its own parent-death signal before exec and
        // verifies that the spawning LSP did not die in the fork→prctl window.
        unsafe {
            command.as_std_mut().pre_exec(move || {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) != 0 {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent_pid {
                    libc::_exit(127);
                }
                Ok(())
            });
        }
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

/// Process-lifetime containment for an LSP launched by an editor client.
///
/// The standard LSP `InitializeParams.processId` is the authoritative witness
/// for every editor. A launcher may additionally pass `--client-pid` as an early
/// bootstrap witness; initialization replaces that monitor atomically. No OS
/// parent inference is used: a shell or short-lived launcher is not necessarily
/// the LSP client named by the protocol.
///
/// On Windows the guard also assigns the LSP itself to a kill-on-close Job
/// Object before providers may spawn. On Unix, active provider process groups
/// are registered by [`TreeKill`] and killed before the client-death monitor
/// terminates the LSP, so pipe-holding descendants cannot survive the engine.
#[derive(Debug)]
pub struct ClientProcessGuard {
    monitor: Arc<ClientMonitorState>,
    #[cfg(windows)]
    _job: JobHandle,
}

#[derive(Debug, Default)]
struct ClientMonitorState {
    generation: AtomicU64,
    client_pid: AtomicU32,
    bind_lock: Mutex<()>,
}

static CLIENT_MONITOR: OnceLock<Arc<ClientMonitorState>> = OnceLock::new();

impl ClientProcessGuard {
    /// Install process-wide containment and optionally bind an early launcher
    /// witness. `None` is valid: ordinary editors bind through the standard LSP
    /// initialize request instead of a Verter-specific command-line contract.
    pub fn arm(bootstrap_client_pid: Option<u32>) -> Result<Self, String> {
        let monitor = Arc::new(ClientMonitorState::default());
        CLIENT_MONITOR
            .set(Arc::clone(&monitor))
            .map_err(|_| "LSP client-process containment was already installed".to_string())?;

        #[cfg(windows)]
        let job = JobHandle::create_and_assign_current().ok_or_else(|| {
            format!(
                "failed to assign verter-lsp to a kill-on-close Job Object: {}",
                std::io::Error::last_os_error()
            )
        })?;

        let guard = Self {
            monitor,
            #[cfg(windows)]
            _job: job,
        };
        if let Some(client_pid) = bootstrap_client_pid {
            guard.bind_client(client_pid)?;
        }
        Ok(guard)
    }

    /// Replace the current witness with one stable OS-backed monitor.
    pub fn bind_client(&self, client_pid: u32) -> Result<(), String> {
        bind_client_monitor(&self.monitor, client_pid)
    }

    #[must_use]
    pub fn client_pid(&self) -> Option<u32> {
        match self.monitor.client_pid.load(Ordering::Acquire) {
            0 => None,
            pid => Some(pid),
        }
    }
}

/// Bind the standard LSP client-process witness after `initialize` is received.
///
/// Library embedders that did not install [`ClientProcessGuard`] are unaffected.
/// A `None` process id follows the LSP specification and leaves any explicit
/// bootstrap witness in place; otherwise stdio EOF remains the lifecycle rail.
pub fn bind_lsp_client_process(client_pid: Option<u32>) -> Result<(), String> {
    let Some(client_pid) = client_pid else {
        return Ok(());
    };
    let Some(monitor) = CLIENT_MONITOR.get() else {
        return Ok(());
    };
    bind_client_monitor(monitor, client_pid)
}

fn validate_client_pid(client_pid: u32) -> Result<(), String> {
    if client_pid == 0 || client_pid == std::process::id() {
        return Err(format!("invalid LSP client pid {client_pid}"));
    }
    #[cfg(unix)]
    if client_pid > libc::pid_t::MAX as u32 {
        return Err(format!(
            "LSP client pid {client_pid} is outside the positive pid_t range"
        ));
    }
    Ok(())
}

fn bind_client_monitor(state: &Arc<ClientMonitorState>, client_pid: u32) -> Result<(), String> {
    validate_client_pid(client_pid)?;
    let _bind = state
        .bind_lock
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let generation = state.generation.load(Ordering::Acquire).wrapping_add(1);
    spawn_client_monitor(client_pid, Arc::clone(state), generation)?;
    state.client_pid.store(client_pid, Ordering::Release);
    state.generation.store(generation, Ordering::Release);
    Ok(())
}

fn wait_until_monitor_is_current(state: &ClientMonitorState, generation: u64) -> bool {
    loop {
        match state.generation.load(Ordering::Acquire).cmp(&generation) {
            std::cmp::Ordering::Less => std::thread::yield_now(),
            std::cmp::Ordering::Equal => return true,
            std::cmp::Ordering::Greater => return false,
        }
    }
}

fn terminate_lsp_after_client_death(state: &ClientMonitorState, generation: u64) {
    if state.generation.load(Ordering::Acquire) != generation {
        return;
    }
    terminate_registered_engine_trees();
    #[cfg(windows)]
    unsafe {
        // ExitProcess closes the outer Job Object handle; KILL_ON_JOB_CLOSE
        // terminates every descendant even if the async runtime is wedged.
        windows_sys::Win32::System::Threading::ExitProcess(0);
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(std::process::id() as libc::pid_t, libc::SIGKILL);
    }
    #[cfg(not(any(unix, windows)))]
    std::process::exit(0);
}

#[cfg(windows)]
fn spawn_client_monitor(
    client_pid: u32,
    state: Arc<ClientMonitorState>,
    generation: u64,
) -> Result<(), String> {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE};

    let handle = unsafe { OpenProcess(PROCESS_SYNCHRONIZE, 0, client_pid) };
    if handle.is_null() {
        return Err(format!(
            "could not open LSP client process {client_pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let handle_value = handle as usize;
    std::thread::Builder::new()
        .name("verter-client-lifetime".into())
        .spawn(move || unsafe {
            use windows_sys::Win32::Foundation::CloseHandle;
            use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};

            let handle = handle_value as windows_sys::Win32::Foundation::HANDLE;
            if wait_until_monitor_is_current(&state, generation) {
                WaitForSingleObject(handle, INFINITE);
            }
            CloseHandle(handle);
            terminate_lsp_after_client_death(&state, generation);
        })
        .map_err(|error| {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(handle);
            }
            format!("spawn LSP client-lifetime monitor: {error}")
        })?;
    Ok(())
}

#[cfg(target_os = "linux")]
fn spawn_client_monitor(
    client_pid: u32,
    state: Arc<ClientMonitorState>,
    generation: u64,
) -> Result<(), String> {
    let pidfd = unsafe { libc::syscall(libc::SYS_pidfd_open, client_pid as libc::pid_t, 0) as i32 };
    if pidfd < 0 {
        return Err(format!(
            "could not open stable pidfd for LSP client {client_pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    std::thread::Builder::new()
        .name("verter-client-lifetime".into())
        .spawn(move || unsafe {
            if wait_until_monitor_is_current(&state, generation) {
                let mut pollfd = libc::pollfd {
                    fd: pidfd,
                    events: libc::POLLIN,
                    revents: 0,
                };
                loop {
                    let result = libc::poll(&mut pollfd, 1, -1);
                    if result >= 0
                        || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        break;
                    }
                }
            }
            libc::close(pidfd);
            terminate_lsp_after_client_death(&state, generation);
        })
        .map_err(|error| {
            unsafe {
                libc::close(pidfd);
            }
            format!("spawn LSP client-lifetime monitor: {error}")
        })?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn spawn_client_monitor(
    client_pid: u32,
    state: Arc<ClientMonitorState>,
    generation: u64,
) -> Result<(), String> {
    let queue = unsafe { libc::kqueue() };
    if queue < 0 {
        return Err(format!(
            "could not create kqueue for LSP client {client_pid}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let change = libc::kevent {
        ident: client_pid as libc::uintptr_t,
        filter: libc::EVFILT_PROC,
        flags: libc::EV_ADD | libc::EV_ENABLE | libc::EV_ONESHOT,
        fflags: libc::NOTE_EXIT,
        data: 0,
        udata: std::ptr::null_mut(),
    };
    unsafe {
        if libc::kevent(queue, &change, 1, std::ptr::null_mut(), 0, std::ptr::null()) != 0 {
            let error = std::io::Error::last_os_error();
            libc::close(queue);
            return Err(format!(
                "could not register stable kqueue witness for LSP client {client_pid}: {error}"
            ));
        }
    }
    std::thread::Builder::new()
        .name("verter-client-lifetime".into())
        .spawn(move || unsafe {
            if wait_until_monitor_is_current(&state, generation) {
                let mut event = std::mem::zeroed::<libc::kevent>();
                loop {
                    let result =
                        libc::kevent(queue, std::ptr::null(), 0, &mut event, 1, std::ptr::null());
                    if result >= 0
                        || std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted
                    {
                        break;
                    }
                }
            }
            libc::close(queue);
            terminate_lsp_after_client_death(&state, generation);
        })
        .map_err(|error| {
            unsafe {
                libc::close(queue);
            }
            format!("spawn LSP client-lifetime monitor: {error}")
        })?;
    Ok(())
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn spawn_client_monitor(
    client_pid: u32,
    state: Arc<ClientMonitorState>,
    generation: u64,
) -> Result<(), String> {
    if !process_alive(client_pid) {
        return Err(format!("LSP client process {client_pid} is not alive"));
    }
    std::thread::Builder::new()
        .name("verter-client-lifetime".into())
        .spawn(move || {
            if !wait_until_monitor_is_current(&state, generation) {
                return;
            }
            while process_alive(client_pid) {
                std::thread::sleep(Duration::from_millis(250));
            }
            terminate_lsp_after_client_death(&state, generation);
        })
        .map_err(|error| format!("spawn LSP client-lifetime monitor: {error}"))?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn spawn_client_monitor(
    client_pid: u32,
    state: Arc<ClientMonitorState>,
    generation: u64,
) -> Result<(), String> {
    if !process_alive(client_pid) {
        return Err(format!("LSP client process {client_pid} is not alive"));
    }
    std::thread::Builder::new()
        .name("verter-client-lifetime".into())
        .spawn(move || {
            if !wait_until_monitor_is_current(&state, generation) {
                return;
            }
            while process_alive(client_pid) {
                std::thread::sleep(Duration::from_millis(250));
            }
            terminate_lsp_after_client_death(&state, generation);
        })
        .map_err(|error| format!("spawn LSP client-lifetime monitor: {error}"))?;
    Ok(())
}

/// A process-tree kill handle armed against a spawned child's pid. Cheap and
/// idempotent: killing an already-exited tree is a no-op.
#[derive(Debug)]
pub struct TreeKill {
    pid: u32,
    registration: Option<u64>,
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
        let registration = register_engine_tree(pid);
        #[cfg(windows)]
        {
            Self {
                pid,
                registration,
                job: if pid == 0 {
                    None
                } else {
                    JobHandle::create_and_assign(pid)
                },
            }
        }
        #[cfg(not(windows))]
        {
            Self { pid, registration }
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

impl Drop for TreeKill {
    fn drop(&mut self) {
        self.kill_tree();
        unregister_engine_tree(self.registration);
    }
}

fn kill_tree_by_pid(pid: u32) {
    if pid == 0 {
        return;
    }
    #[cfg(unix)]
    unsafe {
        libc::kill(-(pid as libc::pid_t), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    terminate_process(pid);
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
    if !handle.is_null() {
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
    if !handle.is_null() {
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
    fn create_and_assign_current() -> Option<Self> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let job = Self::create()?;
        let assigned = unsafe { AssignProcessToJobObject(job.0, GetCurrentProcess()) };
        (assigned != 0).then_some(job)
    }

    fn create() -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::{
            CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        };

        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
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
            Some(Self(job))
        }
    }

    fn create_and_assign(pid: u32) -> Option<Self> {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };

        unsafe {
            let job = Self::create()?;
            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return None;
            }
            let assigned = AssignProcessToJobObject(job.0, process);
            CloseHandle(process);
            if assigned == 0 {
                return None;
            }
            Some(job)
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

    // @ai-generated - Client-death cleanup must use the active registry before
    // terminating the LSP, not rely on provider Drop running afterward.
    #[cfg(unix)]
    #[tokio::test]
    async fn active_registry_kills_engine_groups_before_lsp_exit() {
        let mut command = tokio::process::Command::new("/bin/sh");
        command
            .arg("-c")
            .arg("sleep 600 & wait")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        configure_tree_spawn(&mut command);
        let mut child = command.spawn().expect("spawn registered engine tree");
        let tree = TreeKill::arm(child.id().expect("engine pid"));

        terminate_registered_engine_trees();
        assert!(
            reap_child_bounded(&mut child, REAP_BOUND).await,
            "the registered engine group must die before the LSP exits"
        );
        drop(tree);
    }

    // @ai-generated - A provider Job Object must be armed while the direct
    // process is live so descendants created afterward join it automatically.
    #[cfg(windows)]
    #[tokio::test]
    async fn windows_tree_handle_kills_a_late_spawned_descendant() {
        use tokio::io::{AsyncBufReadExt, BufReader};

        let mut command = tokio::process::Command::new("powershell");
        command
            .args([
                "-NoProfile",
                "-Command",
                "Start-Sleep -Milliseconds 500; $child = Start-Process powershell -ArgumentList '-NoProfile','-Command','Start-Sleep -Seconds 300' -WindowStyle Hidden -PassThru; [Console]::Out.WriteLine($child.Id); [Console]::Out.Flush(); Wait-Process -Id $child.Id",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true);
        configure_tree_spawn(&mut command);
        let mut child = command.spawn().expect("spawn provider parent");
        let tree = TreeKill::arm(child.id().expect("provider pid"));
        let mut descendant_pid = String::new();
        tokio::time::timeout(
            Duration::from_secs(10),
            BufReader::new(child.stdout.take().expect("provider stdout"))
                .read_line(&mut descendant_pid),
        )
        .await
        .expect("descendant pid timeout")
        .expect("read descendant pid");
        let descendant_pid = descendant_pid
            .trim()
            .parse::<u32>()
            .expect("numeric descendant pid");

        tree.kill_tree();
        assert!(reap_child_bounded(&mut child, REAP_BOUND).await);
        for _ in 0..80 {
            if !process_alive(descendant_pid) {
                drop(tree);
                return;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        panic!("provider descendant {descendant_pid} survived its armed Job Object");
    }

    #[cfg(unix)]
    #[test]
    fn client_pid_must_fit_positive_pid_t() {
        assert!(validate_client_pid(i32::MAX as u32).is_ok());
        assert!(validate_client_pid(i32::MAX as u32 + 1).is_err());
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
