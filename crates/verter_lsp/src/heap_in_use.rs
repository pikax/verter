//! The bytes the process heap currently has allocated, read from the
//! platform allocator itself.
//!
//! A resident-set figure includes what the allocator keeps committed after a
//! free (fragmentation that settles over hundreds of edit cycles) and, for a
//! garbage-collected child, whatever its collector has not yet returned. The
//! allocator's own in-use count has neither: it moves only when live
//! allocations do, so a long-session retention measurement can tell a slow
//! leak from allocator settling. It is read on demand (one platform call per
//! `$/verter/getStatistics`), never sampled by a wrapper on the allocation
//! path, so it costs nothing while the server works.
//!
//! `None` where the platform offers no such figure; the endurance lane then
//! reports that metric unavailable rather than guessing.

/// Bytes currently allocated from the process heap the Rust `System`
/// allocator draws from, or `None` when the platform cannot say.
#[must_use]
pub fn heap_in_use_bytes() -> Option<u64> {
    platform::heap_in_use_bytes()
}

#[cfg(windows)]
mod platform {
    use std::ffi::c_void;

    /// `HEAP_SUMMARY` from `heapapi.h`.
    #[repr(C)]
    struct HeapSummary {
        cb: u32,
        allocated: usize,
        committed: usize,
        reserved: usize,
        max_reserve: usize,
    }

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetProcessHeap() -> *mut c_void;
        fn HeapSummary(heap: *mut c_void, flags: u32, summary: *mut HeapSummary) -> i32;
    }

    /// The Rust `System` allocator on Windows is `HeapAlloc` on the process
    /// heap, so that heap's allocated bytes are the process's live heap.
    pub(super) fn heap_in_use_bytes() -> Option<u64> {
        let mut summary = HeapSummary {
            cb: std::mem::size_of::<HeapSummary>() as u32,
            allocated: 0,
            committed: 0,
            reserved: 0,
            max_reserve: 0,
        };
        // SAFETY: the process heap handle is always valid, and `summary` is a
        // correctly sized, writable `HEAP_SUMMARY`.
        let ok = unsafe { HeapSummary(GetProcessHeap(), 0, &mut summary) };
        (ok != 0).then_some(summary.allocated as u64)
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
mod platform {
    use std::ffi::{c_char, c_void};
    use std::sync::OnceLock;

    /// `struct mallinfo2` from `malloc.h` (glibc 2.33+).
    #[repr(C)]
    struct Mallinfo2 {
        arena: usize,
        ordblks: usize,
        smblks: usize,
        hblks: usize,
        hblkhd: usize,
        usmblks: usize,
        fsmblks: usize,
        uordblks: usize,
        fordblks: usize,
        keepcost: usize,
    }

    #[link(name = "dl")]
    unsafe extern "C" {
        fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    }

    type Mallinfo2Fn = unsafe extern "C" fn() -> Mallinfo2;

    /// `mallinfo2`, looked up at run time rather than linked: it only exists
    /// from glibc 2.33, and the release cross-builds link against an older
    /// glibc (`aarch64-unknown-linux-gnu` through cargo-zigbuild), where a
    /// direct reference is an undefined symbol. On an older glibc the figure
    /// is unavailable, as on any platform without one.
    fn mallinfo2() -> Option<Mallinfo2Fn> {
        static RESOLVED: OnceLock<Option<Mallinfo2Fn>> = OnceLock::new();
        *RESOLVED.get_or_init(|| {
            // SAFETY: a null handle is glibc's `RTLD_DEFAULT` (the global
            // symbol scope), and the name is NUL-terminated.
            let symbol = unsafe { dlsym(std::ptr::null_mut(), c"mallinfo2".as_ptr()) };
            // SAFETY: a non-null `mallinfo2` is the glibc function of this
            // signature (`struct mallinfo2 mallinfo2(void)`).
            (!symbol.is_null())
                .then(|| unsafe { std::mem::transmute::<*mut c_void, Mallinfo2Fn>(symbol) })
        })
    }

    /// In-use bytes in the main arena plus mmapped allocations.
    pub(super) fn heap_in_use_bytes() -> Option<u64> {
        let mallinfo2 = mallinfo2()?;
        // SAFETY: `mallinfo2` takes no arguments and returns by value.
        let info = unsafe { mallinfo2() };
        Some((info.uordblks + info.hblkhd) as u64)
    }
}

#[cfg(target_os = "macos")]
mod platform {
    use std::ffi::c_void;

    /// `malloc_statistics_t` from `malloc/malloc.h`.
    #[repr(C)]
    struct MallocStatistics {
        blocks_in_use: u32,
        size_in_use: usize,
        max_size_in_use: usize,
        size_allocated: usize,
    }

    unsafe extern "C" {
        fn malloc_zone_statistics(zone: *mut c_void, stats: *mut MallocStatistics);
    }

    /// In-use bytes across every malloc zone (`zone == NULL`).
    pub(super) fn heap_in_use_bytes() -> Option<u64> {
        let mut stats = MallocStatistics {
            blocks_in_use: 0,
            size_in_use: 0,
            max_size_in_use: 0,
            size_allocated: 0,
        };
        // SAFETY: a null zone means every zone, and `stats` is a writable
        // `malloc_statistics_t`.
        unsafe { malloc_zone_statistics(std::ptr::null_mut(), &mut stats) };
        Some(stats.size_in_use as u64)
    }
}

#[cfg(not(any(
    windows,
    all(target_os = "linux", target_env = "gnu"),
    target_os = "macos"
)))]
mod platform {
    pub(super) fn heap_in_use_bytes() -> Option<u64> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::heap_in_use_bytes;

    /// The figure follows live allocations: a large allocation raises it and
    /// its release lowers it back, on every platform that reports one.
    #[test]
    fn follows_live_allocations() {
        let Some(before) = heap_in_use_bytes() else {
            return;
        };
        let held: Vec<u8> = vec![1u8; 64 << 20];
        let during = heap_in_use_bytes().expect("platform reports the figure");
        assert!(
            during >= before + (48 << 20),
            "a 64 MiB allocation raises the figure ({before} -> {during})"
        );
        drop(held);
        let after = heap_in_use_bytes().expect("platform reports the figure");
        assert!(
            after < during - (48 << 20),
            "releasing it lowers the figure ({during} -> {after})"
        );
    }
}
