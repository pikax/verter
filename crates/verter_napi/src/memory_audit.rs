//! Opt-in deep memory audit for the native binding.
//!
//! Compiled with the `memory_audit` cargo feature, this module installs a
//! counting [`std::alloc::GlobalAlloc`] wrapper over [`std::alloc::System`]
//! that tracks allocation/deallocation counts, total allocated bytes, live
//! bytes, and a resettable live-bytes high-water mark. Without the feature
//! the wrapper is not compiled at all (zero overhead) and the two NAPI
//! entry points degrade to `null` / `false` so JS callers can detect a
//! non-instrumented binary.
//!
//! The two NAPI functions are ALWAYS exported (feature on or off):
//! - `memoryAuditSnapshot(): MemoryAuditSnapshot | null` — `null` means the
//!   binary was built without `--features memory_audit`.
//! - `memoryAuditResetHighWater(): boolean` — `false` means uninstrumented.
//!
//! Instrumented build: `pnpm --filter @verter/native run build:memory-audit`
//! (a `napi build --release` with `--features memory_audit`). Timing
//! benchmark runs must use the regular non-instrumented build.

use napi_derive::napi;

/// Point-in-time counters from the counting global allocator.
///
/// All values are reported as `f64` for plain JS `number` interop; the
/// magnitudes involved (audit windows, bytes) stay far below 2^53.
#[napi(object)]
pub struct NapiMemoryAuditSnapshot {
    /// Total allocating calls (`alloc` / `alloc_zeroed` / `realloc`)
    /// observed since process start.
    pub allocCount: f64,
    /// Total deallocating calls (`dealloc` / `realloc`) observed since
    /// process start.
    pub deallocCount: f64,
    /// Total bytes requested by allocating calls since process start
    /// (monotonic; never decremented on free).
    pub allocatedBytesTotal: f64,
    /// Currently live heap bytes (allocated minus freed).
    pub liveBytes: f64,
    /// High-water mark of `liveBytes` since process start or the last
    /// `memoryAuditResetHighWater()` call.
    pub peakLiveBytes: f64,
}

#[cfg(feature = "memory_audit")]
mod counting {
    use std::alloc::{GlobalAlloc, Layout, System};
    use std::sync::atomic::{AtomicU64, Ordering};

    pub(super) static ALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
    pub(super) static DEALLOC_COUNT: AtomicU64 = AtomicU64::new(0);
    pub(super) static ALLOCATED_BYTES_TOTAL: AtomicU64 = AtomicU64::new(0);
    pub(super) static LIVE_BYTES: AtomicU64 = AtomicU64::new(0);
    pub(super) static PEAK_LIVE_BYTES: AtomicU64 = AtomicU64::new(0);

    /// Counting wrapper over the system allocator. Every successful
    /// allocating call bumps the counters and advances the live-bytes
    /// high-water mark via `fetch_max`; failed allocations (null return)
    /// are not recorded so `LIVE_BYTES` stays exact.
    struct CountingAllocator;

    #[inline]
    fn record_alloc(bytes: usize) {
        ALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        ALLOCATED_BYTES_TOTAL.fetch_add(bytes as u64, Ordering::Relaxed);
        let live = LIVE_BYTES.fetch_add(bytes as u64, Ordering::Relaxed) + bytes as u64;
        PEAK_LIVE_BYTES.fetch_max(live, Ordering::Relaxed);
    }

    #[inline]
    fn record_dealloc(bytes: usize) {
        DEALLOC_COUNT.fetch_add(1, Ordering::Relaxed);
        LIVE_BYTES.fetch_sub(bytes as u64, Ordering::Relaxed);
    }

    unsafe impl GlobalAlloc for CountingAllocator {
        unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc(layout) };
            if !ptr.is_null() {
                record_alloc(layout.size());
            }
            ptr
        }

        unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
            let ptr = unsafe { System.alloc_zeroed(layout) };
            if !ptr.is_null() {
                record_alloc(layout.size());
            }
            ptr
        }

        unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
            record_dealloc(layout.size());
            unsafe { System.dealloc(ptr, layout) }
        }

        unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
            let new_ptr = unsafe { System.realloc(ptr, layout, new_size) };
            if !new_ptr.is_null() {
                // Model a successful realloc as one alloc of the new size
                // plus one dealloc of the old size so `LIVE_BYTES` stays
                // exact and both event counters advance.
                record_alloc(new_size);
                record_dealloc(layout.size());
            }
            new_ptr
        }
    }

    #[global_allocator]
    static GLOBAL: CountingAllocator = CountingAllocator;

    pub(super) fn snapshot() -> super::NapiMemoryAuditSnapshot {
        super::NapiMemoryAuditSnapshot {
            allocCount: ALLOC_COUNT.load(Ordering::Relaxed) as f64,
            deallocCount: DEALLOC_COUNT.load(Ordering::Relaxed) as f64,
            allocatedBytesTotal: ALLOCATED_BYTES_TOTAL.load(Ordering::Relaxed) as f64,
            liveBytes: LIVE_BYTES.load(Ordering::Relaxed) as f64,
            peakLiveBytes: PEAK_LIVE_BYTES.load(Ordering::Relaxed) as f64,
        }
    }

    /// Re-arm the high-water mark at the current live-bytes level so the
    /// next window measures its own peak. Best-effort under concurrency
    /// (audit windows are driven by a single JS caller).
    pub(super) fn reset_high_water() {
        PEAK_LIVE_BYTES.store(LIVE_BYTES.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

/// Return the current allocator counters, or `null` when this binary was
/// built without the `memory_audit` feature (the non-instrumented,
/// zero-overhead production build).
#[napi]
pub fn memory_audit_snapshot() -> Option<NapiMemoryAuditSnapshot> {
    snapshot_impl()
}

/// Reset the live-bytes high-water mark to the current live-bytes level.
/// Returns `false` when this binary is not instrumented (feature off).
#[napi]
pub fn memory_audit_reset_high_water() -> bool {
    reset_high_water_impl()
}

#[cfg(feature = "memory_audit")]
fn snapshot_impl() -> Option<NapiMemoryAuditSnapshot> {
    Some(counting::snapshot())
}

#[cfg(not(feature = "memory_audit"))]
fn snapshot_impl() -> Option<NapiMemoryAuditSnapshot> {
    None
}

#[cfg(feature = "memory_audit")]
fn reset_high_water_impl() -> bool {
    counting::reset_high_water();
    true
}

#[cfg(not(feature = "memory_audit"))]
fn reset_high_water_impl() -> bool {
    false
}

/// Feature-off contract: the exports stay present but advertise the
/// non-instrumented binary (`null` snapshot, `false` reset).
#[cfg(all(test, not(feature = "memory_audit")))]
mod uninstrumented_tests {
    use super::*;

    #[test]
    fn snapshot_is_null_and_reset_reports_uninstrumented_without_feature() {
        assert!(
            memory_audit_snapshot().is_none(),
            "feature off: memoryAuditSnapshot() must return null so JS \
             callers can detect the non-instrumented binary"
        );
        assert!(
            !memory_audit_reset_high_water(),
            "feature off: memoryAuditResetHighWater() must return false"
        );
    }
}

#[cfg(all(test, feature = "memory_audit"))]
mod instrumented_tests {
    use std::hint::black_box;
    use std::sync::Mutex;

    use super::*;

    /// The counters are process-global and sibling lib tests allocate on
    /// their own threads; serialise this module's measurement windows and
    /// keep assertion margins large so unrelated churn cannot flip them.
    static AUDIT_TEST_SERIAL: Mutex<()> = Mutex::new(());

    fn serial_guard() -> std::sync::MutexGuard<'static, ()> {
        AUDIT_TEST_SERIAL
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    const PROBE_BYTES: usize = 128 * 1024 * 1024;

    #[test]
    fn allocations_and_deallocations_move_counters() {
        let _serial = serial_guard();

        let before = memory_audit_snapshot().expect("instrumented build must snapshot");

        let probe = black_box(vec![0u8; PROBE_BYTES]);
        let held = memory_audit_snapshot().expect("instrumented build must snapshot");
        assert!(
            held.allocCount > before.allocCount,
            "an allocation must advance allocCount ({} -> {})",
            before.allocCount,
            held.allocCount
        );
        assert!(
            held.allocatedBytesTotal >= before.allocatedBytesTotal + PROBE_BYTES as f64,
            "allocatedBytesTotal must grow by at least the probe size \
             ({} -> {}, probe {PROBE_BYTES})",
            before.allocatedBytesTotal,
            held.allocatedBytesTotal
        );
        assert!(
            held.liveBytes > before.liveBytes,
            "liveBytes must grow while the probe is held ({} -> {})",
            before.liveBytes,
            held.liveBytes
        );
        assert!(
            held.peakLiveBytes >= held.liveBytes,
            "peakLiveBytes is a high-water mark and can never trail liveBytes \
             (peak {}, live {})",
            held.peakLiveBytes,
            held.liveBytes
        );

        drop(black_box(probe));
        let after = memory_audit_snapshot().expect("instrumented build must snapshot");
        assert!(
            after.deallocCount > held.deallocCount,
            "dropping the probe must advance deallocCount ({} -> {})",
            held.deallocCount,
            after.deallocCount
        );
        assert!(
            after.liveBytes <= held.liveBytes - (PROBE_BYTES / 2) as f64,
            "dropping the {PROBE_BYTES}-byte probe must shrink liveBytes \
             substantially ({} -> {})",
            held.liveBytes,
            after.liveBytes
        );
        assert!(
            after.allocatedBytesTotal >= held.allocatedBytesTotal,
            "allocatedBytesTotal is monotonic ({} -> {})",
            held.allocatedBytesTotal,
            after.allocatedBytesTotal
        );
    }

    #[test]
    fn reset_high_water_drops_peak_to_current_live_and_rearms() {
        let _serial = serial_guard();

        // Raise the high-water mark far above steady-state live bytes,
        // then release the spike.
        let spike = black_box(vec![0u8; PROBE_BYTES]);
        drop(black_box(spike));

        let peaked = memory_audit_snapshot().expect("instrumented build must snapshot");
        assert!(
            peaked.peakLiveBytes >= peaked.liveBytes + (PROBE_BYTES / 2) as f64,
            "precondition: after the spike is freed, peak ({}) must sit far \
             above live ({})",
            peaked.peakLiveBytes,
            peaked.liveBytes
        );

        assert!(
            memory_audit_reset_high_water(),
            "instrumented build: memoryAuditResetHighWater() must return true"
        );

        let reset = memory_audit_snapshot().expect("instrumented build must snapshot");
        assert!(
            reset.peakLiveBytes < peaked.peakLiveBytes - (PROBE_BYTES / 2) as f64,
            "reset must drop the high-water mark from the pre-reset peak \
             ({} -> {})",
            peaked.peakLiveBytes,
            reset.peakLiveBytes
        );
        // Post-reset the mark tracks current live bytes (small slack for
        // sibling-thread churn between the reset and this snapshot).
        assert!(
            reset.peakLiveBytes <= reset.liveBytes + (16 * 1024 * 1024) as f64,
            "reset must pin the high-water mark near current live bytes \
             (peak {}, live {})",
            reset.peakLiveBytes,
            reset.liveBytes
        );

        // The mark re-arms: a fresh spike raises it again.
        let respike = black_box(vec![0u8; PROBE_BYTES]);
        let rearmed = memory_audit_snapshot().expect("instrumented build must snapshot");
        drop(black_box(respike));
        assert!(
            rearmed.peakLiveBytes >= reset.peakLiveBytes + (PROBE_BYTES / 2) as f64,
            "a post-reset spike must advance the high-water mark again \
             ({} -> {})",
            reset.peakLiveBytes,
            rearmed.peakLiveBytes
        );
    }
}
