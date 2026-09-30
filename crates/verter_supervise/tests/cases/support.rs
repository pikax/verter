//! Shared launch and observation helpers for the supervisor cases.

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

/// The backend this platform must report.
pub const EXPECTED_BACKEND: &str = if cfg!(windows) {
    "windows-job-object"
} else if cfg!(target_os = "linux") {
    "linux-cgroup-v2"
} else {
    "macos-phys-footprint"
};

/// The containment level this platform must report.
pub const EXPECTED_CONTAINMENT: &str = if cfg!(target_os = "macos") {
    "sampled"
} else {
    "hard"
};

/// The memory cap the allocation cases run under.
pub const CAP_MB: u64 = 256;
/// The fixture's allocation step.
pub const CHUNK_MB: u64 = 8;

pub fn supervisor() -> PathBuf {
    verter_test_support::cargo_test_binary_path!("verter-supervise")
}

pub fn fixture() -> PathBuf {
    verter_test_support::cargo_test_binary_path!("verter-supervise-fixture")
}

/// A fresh scratch directory for one case.
pub fn scratch(name: &str) -> PathBuf {
    let dir = verter_test_support::unique_temp_dir(&format!("verter-supervise-{name}"));
    std::fs::create_dir_all(&dir).expect("create scratch dir");
    dir
}

/// Flags every run needs on this platform besides the limits: a sampled
/// backend runs only with explicit consent.
pub fn platform_flags() -> Vec<String> {
    if cfg!(target_os = "macos") {
        vec!["--allow-sampled".to_owned()]
    } else {
        Vec::new()
    }
}

/// The supervisor command for `fixture_args`, writing `dir/result.json`.
pub fn command(dir: &Path, limits: &[&str], fixture_args: &[&str]) -> Command {
    let mut command = Command::new(supervisor());
    command
        .arg("run")
        .args(limits)
        .args(platform_flags())
        .arg("--out")
        .arg(dir.join("result.json"))
        .arg("--")
        .arg(fixture())
        .args(fixture_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command
}

/// A finished supervisor run.
pub struct Run {
    pub code: Option<i32>,
    pub stderr: String,
    pub result: Value,
    pub dir: PathBuf,
}

/// Run the supervisor to completion over `fixture_args`.
pub fn supervise(name: &str, limits: &[&str], fixture_args: &[&str]) -> Run {
    let dir = scratch(name);
    supervise_in(&dir, limits, fixture_args, &[])
}

/// Run the supervisor in `dir` with extra supervisor environment.
pub fn supervise_in(
    dir: &Path,
    limits: &[&str],
    fixture_args: &[&str],
    env: &[(&str, &str)],
) -> Run {
    let mut command = command(dir, limits, fixture_args);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command.output().expect("launch verter-supervise");
    finish(dir, output.status.code(), &output.stderr)
}

pub fn finish(dir: &Path, code: Option<i32>, stderr: &[u8]) -> Run {
    let stderr = String::from_utf8_lossy(stderr).into_owned();
    let text = std::fs::read_to_string(dir.join("result.json"))
        .unwrap_or_else(|error| panic!("result.json missing ({error}); stderr: {stderr}"));
    let result: Value = serde_json::from_str(&text).expect("result.json parses");
    Run {
        code,
        stderr,
        result,
        dir: dir.to_path_buf(),
    }
}

/// Wait for `child` to exit within `limit`, killing it and failing otherwise.
pub fn wait_with_limit(child: &mut Child, limit: Duration) -> Option<i32> {
    let deadline = Instant::now() + limit;
    loop {
        if let Some(status) = child.try_wait().expect("poll supervisor") {
            return status.code();
        }
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("the supervisor did not exit within {limit:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Read a pid the fixture published, waiting for it to appear.
pub fn published_pid(path: &Path) -> u32 {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if let Ok(text) = std::fs::read_to_string(path) {
            if let Ok(pid) = text.trim().parse() {
                return pid;
            }
        }
        assert!(
            Instant::now() < deadline,
            "the fixture never published {}",
            path.display()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Whether `pid` has exited within `limit`.
pub fn exits_within(pid: u32, limit: Duration) -> bool {
    let deadline = Instant::now() + limit;
    loop {
        if !alive(pid) {
            return true;
        }
        if Instant::now() > deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Kill a process the test itself found alive, so a failing case leaks
/// nothing.
pub fn kill_pid(pid: u32) {
    #[cfg(windows)]
    // SAFETY: plain Win32 calls on a handle this function owns.
    unsafe {
        use windows_sys::Win32::Foundation::CloseHandle;
        use windows_sys::Win32::System::Threading::{
            OpenProcess, TerminateProcess, PROCESS_TERMINATE,
        };
        let handle = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !handle.is_null() {
            TerminateProcess(handle, 1);
            CloseHandle(handle);
        }
    }
    #[cfg(unix)]
    // SAFETY: sending a signal has no memory-safety preconditions.
    unsafe {
        libc::kill(pid as libc::pid_t, libc::SIGKILL);
    }
}

#[cfg(windows)]
pub fn alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE,
    };
    // SAFETY: plain Win32 calls on a handle this function owns.
    unsafe {
        let handle = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            0,
            pid,
        );
        if handle.is_null() {
            return false;
        }
        let running = WaitForSingleObject(handle, 0) == WAIT_TIMEOUT;
        CloseHandle(handle);
        running
    }
}

#[cfg(unix)]
pub fn alive(pid: u32) -> bool {
    // SAFETY: signal 0 only probes for existence.
    if unsafe { libc::kill(pid as libc::pid_t, 0) } != 0 {
        return false;
    }
    // A zombie has exited; only its parent has not reaped it yet.
    #[cfg(target_os = "linux")]
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{pid}/stat")) {
        if let Some(state) = stat
            .rsplit(')')
            .next()
            .and_then(|rest| rest.split_whitespace().next())
        {
            return state != "Z";
        }
    }
    true
}

pub fn str_field<'a>(result: &'a Value, field: &str) -> Option<&'a str> {
    result.get(field).and_then(Value::as_str)
}

pub fn u64_field(result: &Value, field: &str) -> Option<u64> {
    result.get(field).and_then(Value::as_u64)
}
