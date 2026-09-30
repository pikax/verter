//! Operating-system process statistics for the semantic benchmark.
//!
//! One reader serves both arms: the Verter probe reads its own process and
//! the tsc driver asks this binary to read the tsc API server's process by
//! pid, so both tools' numbers come from the same OS counters through the
//! same code. Every figure is the operating system's own accounting — a
//! high-water mark the kernel maintains, not a polled sample — and a figure
//! the platform cannot supply is absent, never zero.
//!
//! | platform | `peak_bytes` (headline) | `current_bytes` | also |
//! |---|---|---|---|
//! | Windows | peak private commit (`PeakPagefileUsage`) | private commit (`PrivateUsage`) | peak / current working set |
//! | macOS | lifetime max physical footprint | physical footprint | resident size |
//! | Linux | `VmHWM` (peak resident) | `VmRSS` | — |

use serde::Serialize;

/// One reading of a process's memory and CPU accounting.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProcessStats {
    /// Process the reading describes.
    pub pid: u32,
    /// Name of the headline memory metric (`peak_bytes` / `current_bytes`).
    pub metric: &'static str,
    /// The OS high-water mark of the headline metric over the process's life.
    pub peak_bytes: Option<u64>,
    /// The headline metric right now.
    pub current_bytes: Option<u64>,
    /// Peak resident set / working set, when the headline is not already it.
    pub peak_resident_bytes: Option<u64>,
    /// Current resident set / working set.
    pub current_resident_bytes: Option<u64>,
    /// User plus system CPU time consumed so far, in microseconds.
    pub cpu_micros: Option<u64>,
}

/// Why a reading could not be taken.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsError(pub String);

impl std::fmt::Display for StatsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Statistics of the calling process.
pub fn current_process() -> Result<ProcessStats, StatsError> {
    for_pid(std::process::id())
}

#[cfg(windows)]
pub fn for_pid(pid: u32) -> Result<ProcessStats, StatsError> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::ProcessStatus::{
        K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
    };
    use windows_sys::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_READ,
    };

    // SAFETY: plain Win32 calls on a handle this function opens and closes;
    // every out-parameter is a properly sized, zero-initialised local.
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_READ, 0, pid);
        if handle.is_null() {
            return Err(StatsError(format!(
                "OpenProcess({pid}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        let mut counters: PROCESS_MEMORY_COUNTERS_EX = std::mem::zeroed();
        counters.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
        let memory_ok = K32GetProcessMemoryInfo(
            handle,
            (&mut counters as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            counters.cb,
        ) != 0;
        let memory_error = std::io::Error::last_os_error();
        let zero = FILETIME {
            dwLowDateTime: 0,
            dwHighDateTime: 0,
        };
        let (mut creation, mut exit, mut kernel, mut user) = (zero, zero, zero, zero);
        let times_ok =
            GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) != 0;
        CloseHandle(handle);
        if !memory_ok {
            return Err(StatsError(format!(
                "GetProcessMemoryInfo({pid}) failed: {memory_error}"
            )));
        }
        let hundred_ns =
            |t: FILETIME| (u64::from(t.dwHighDateTime) << 32) | u64::from(t.dwLowDateTime);
        Ok(ProcessStats {
            pid,
            metric: "private-commit",
            peak_bytes: Some(counters.PeakPagefileUsage as u64),
            current_bytes: Some(counters.PrivateUsage as u64),
            peak_resident_bytes: Some(counters.PeakWorkingSetSize as u64),
            current_resident_bytes: Some(counters.WorkingSetSize as u64),
            cpu_micros: times_ok.then(|| (hundred_ns(kernel) + hundred_ns(user)) / 10),
        })
    }
}

/// `mach_timebase_info_data_t`, bound directly: the `libc` binding is
/// deprecated in favour of a separate crate, and this is the one call needed.
#[cfg(target_os = "macos")]
#[repr(C)]
struct MachTimebaseInfo {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    /// libSystem's `mach_timebase_info`: the ratio converting mach
    /// absolute-time ticks to nanoseconds.
    fn mach_timebase_info(info: *mut MachTimebaseInfo) -> libc::c_int;
}

#[cfg(target_os = "macos")]
pub fn for_pid(pid: u32) -> Result<ProcessStats, StatsError> {
    // SAFETY: `proc_pid_rusage` fills the caller-owned, correctly sized
    // `rusage_info_v4`; `mach_timebase_info` fills a local struct.
    unsafe {
        let mut info: libc::rusage_info_v4 = std::mem::zeroed();
        let rc = libc::proc_pid_rusage(
            pid as libc::c_int,
            libc::RUSAGE_INFO_V4,
            (&mut info as *mut libc::rusage_info_v4).cast(),
        );
        if rc != 0 {
            return Err(StatsError(format!(
                "proc_pid_rusage({pid}) failed: {}",
                std::io::Error::last_os_error()
            )));
        }
        // CPU times are in mach absolute-time units.
        let mut timebase = MachTimebaseInfo { numer: 0, denom: 0 };
        let cpu_micros =
            (mach_timebase_info(&mut timebase) == 0 && timebase.denom != 0).then(|| {
                let ticks = u128::from(info.ri_user_time) + u128::from(info.ri_system_time);
                (ticks * u128::from(timebase.numer) / u128::from(timebase.denom) / 1_000) as u64
            });
        Ok(ProcessStats {
            pid,
            metric: "phys-footprint",
            peak_bytes: Some(info.ri_lifetime_max_phys_footprint),
            current_bytes: Some(info.ri_phys_footprint),
            peak_resident_bytes: None,
            current_resident_bytes: Some(info.ri_resident_size),
            cpu_micros,
        })
    }
}

#[cfg(target_os = "linux")]
pub fn for_pid(pid: u32) -> Result<ProcessStats, StatsError> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status"))
        .map_err(|err| StatsError(format!("read /proc/{pid}/status: {err}")))?;
    let kib = |key: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(key))
            .and_then(|rest| {
                rest.trim()
                    .trim_end_matches("kB")
                    .trim()
                    .parse::<u64>()
                    .ok()
            })
            .map(|value| value * 1024)
    };
    Ok(ProcessStats {
        pid,
        metric: "resident",
        peak_bytes: kib("VmHWM:"),
        current_bytes: kib("VmRSS:"),
        peak_resident_bytes: None,
        current_resident_bytes: None,
        cpu_micros: None,
    })
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
pub fn for_pid(pid: u32) -> Result<ProcessStats, StatsError> {
    Err(StatsError(format!(
        "process statistics are not implemented on this platform (pid {pid})"
    )))
}

/// The machine's native architecture (Rust's name for it), independent of
/// the architecture this process runs as: a process translated or emulated
/// on another architecture reports the hardware's, not its own.
#[cfg(windows)]
pub fn native_arch() -> Option<&'static str> {
    use windows_sys::Win32::System::SystemInformation::{
        IMAGE_FILE_MACHINE_AMD64, IMAGE_FILE_MACHINE_ARM64, IMAGE_FILE_MACHINE_I386,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, IsWow64Process2};
    let (mut process, mut native) = (0u16, 0u16);
    // SAFETY: both out-parameters are local and correctly typed; the
    // pseudo-handle of the current process needs no closing.
    let ok = unsafe { IsWow64Process2(GetCurrentProcess(), &mut process, &mut native) } != 0;
    match (ok, native) {
        (true, IMAGE_FILE_MACHINE_AMD64) => Some("x86_64"),
        (true, IMAGE_FILE_MACHINE_ARM64) => Some("aarch64"),
        (true, IMAGE_FILE_MACHINE_I386) => Some("x86"),
        _ => None,
    }
}

/// See the Windows variant.
#[cfg(target_os = "macos")]
pub fn native_arch() -> Option<&'static str> {
    let mut value: libc::c_int = 0;
    let mut size = std::mem::size_of::<libc::c_int>();
    // SAFETY: the name is NUL-terminated and the out-buffer is a local of the
    // size passed in.
    let rc = unsafe {
        libc::sysctlbyname(
            c"hw.optional.arm64".as_ptr(),
            (&mut value as *mut libc::c_int).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    // The key exists only on Apple silicon; its absence means an Intel Mac.
    Some(if rc == 0 && value == 1 {
        "aarch64"
    } else {
        "x86_64"
    })
}

/// See the Windows variant.
#[cfg(all(unix, not(target_os = "macos")))]
pub fn native_arch() -> Option<&'static str> {
    // SAFETY: `uname` fills the caller-owned struct.
    let mut name: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut name) } != 0 {
        return None;
    }
    let machine: Vec<u8> = name
        .machine
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    match machine.as_slice() {
        b"x86_64" => Some("x86_64"),
        b"aarch64" | b"arm64" => Some("aarch64"),
        _ => None,
    }
}

#[cfg(not(any(windows, unix)))]
pub fn native_arch() -> Option<&'static str> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_native_architecture_is_known_and_runs_this_build_natively_or_translated() {
        let native = native_arch().expect("the native architecture is readable");
        assert!(["x86_64", "aarch64", "x86"].contains(&native), "{native}");
    }

    #[test]
    fn the_current_process_reports_a_positive_peak_at_least_its_current_use() {
        let stats = current_process().expect("the current process is readable");
        let peak = stats.peak_bytes.expect("a peak");
        let current = stats.current_bytes.expect("a current figure");
        assert!(peak > 0 && current > 0);
        assert!(peak >= current, "peak {peak} below current {current}");
    }

    #[test]
    fn a_process_that_does_not_exist_is_an_error_not_zero() {
        // Pid 0 is the idle / swapper process everywhere: never readable as a
        // user process.
        assert!(for_pid(0).is_err());
    }
}
