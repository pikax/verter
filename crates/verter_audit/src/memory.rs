#![deny(missing_docs)]
//! Process / cache memory snapshots carried on every audit record
//! envelope, plus the cross-platform [`current_process_rss`] helper.

use serde::{Deserialize, Serialize};

use crate::record::{i64_as_decimal_string, u64_as_decimal_string};

/// Memory snapshots — process RSS before/after, plus host and
/// workspace cache footprints. All values in bytes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestMemoryAudit {
    /// Process RSS before request start (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_before_bytes: u64,
    /// Process RSS after request completion (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_after_bytes: u64,
    /// Signed delta = after − before (bytes).
    #[serde(with = "i64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_delta_bytes: i64,
    /// Host cache memory footprint before the request (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_before_bytes: u64,
    /// Host cache memory footprint after the request (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_after_bytes: u64,
    /// Workspace memory footprint before the request (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_before_bytes: u64,
    /// Workspace memory footprint after the request (bytes).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_after_bytes: u64,
    /// Sum of `bytes_read` across every
    /// [`crate::files::FileAudit`] entry whose role is not
    /// [`crate::files::FileRole::NotLoaded`]. Always-on under
    /// `audit_enabled` — derived from the per-request file ledger so
    /// it adds no instrumentation cost. Defaults to `0` when no
    /// producer populated the file ledger.
    #[serde(default, with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub bytes_parsed: u64,
}

/// Get current process RSS in bytes. Returns 0 if unavailable.
///
/// Per-platform sources:
/// - Linux: `/proc/self/statm` field 1 (resident pages) × 4 KB.
/// - macOS: `getrusage(RUSAGE_SELF).ru_maxrss` (already in bytes on
///   macOS).
/// - Windows: `K32GetProcessMemoryInfo(GetCurrentProcess()).WorkingSetSize`.
/// - WASM (`wasm32`): no process memory accounting; returns `0`.
/// - Other targets: returns `0` (best-effort fallback).
#[must_use]
pub fn current_process_rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages.parse::<u64>() {
                    return pages * 4096;
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc_rusage>::uninit();
        // SAFETY: `getrusage` is a POSIX function that fills the
        // provided struct.
        let ret = unsafe {
            getrusage(0 /* RUSAGE_SELF */, usage.as_mut_ptr())
        };
        if ret == 0 {
            let usage = unsafe { usage.assume_init() };
            return usage.ru_maxrss as u64;
        }
    }
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let mut counters = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::uninit();
        // SAFETY: `K32GetProcessMemoryInfo` writes a `PROCESS_MEMORY_COUNTERS`
        // through `counters.as_mut_ptr()` when it returns non-zero, with the
        // size of the struct passed via `cb`. `GetCurrentProcess` returns a
        // pseudo-handle that does not need to be closed. We only read
        // `counters.assume_init()` on the success path.
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                counters.as_mut_ptr(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        if ok != 0 {
            // SAFETY: success path — `counters` was fully initialized by the
            // call above.
            let counters = unsafe { counters.assume_init() };
            return counters.WorkingSetSize as u64;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        // WASM has no process working-set accounting; the audit
        // substrate records `process_rss_*=0` on this target by
        // design.
    }
    0
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(non_camel_case_types)]
struct libc_rusage {
    ru_utime: [i64; 2],
    ru_stime: [i64; 2],
    ru_maxrss: i64,
    _pad: [i64; 13],
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut libc_rusage) -> i32;
}
