//! The INSTRUMENTED twin of `semantic_perf_probe`: identical, except that a
//! counting global allocator records every allocation, so a job's record
//! carries per-request allocation counts. Its timings include the counter's
//! cost; the benchmark reports its runs separately, labelled instrumented,
//! and never compares them with the tsc arm. The counter wraps the allocator
//! the shipped binaries install (`verter_alloc`).

use std::alloc::{GlobalAlloc, Layout};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};

use verter_bench::semantic_perf::AllocHooks;

static ALLOCATIONS: AtomicU64 = AtomicU64::new(0);
static ALLOCATED_BYTES: AtomicU64 = AtomicU64::new(0);

struct CountingAllocator;

/// The allocator every call forwards to.
const INNER: verter_alloc::Allocator = verter_alloc::ALLOCATOR;

fn count(bytes: usize) {
    ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
    ALLOCATED_BYTES.fetch_add(bytes as u64, Ordering::Relaxed);
}

// SAFETY: every method forwards to `INNER` with the caller's arguments
// unchanged; the counters are side effects only.
unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { INNER.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { INNER.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        count(layout.size());
        unsafe { INNER.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        count(new_size);
        unsafe { INNER.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static GLOBAL: CountingAllocator = CountingAllocator;

fn reset() {
    ALLOCATIONS.store(0, Ordering::Relaxed);
    ALLOCATED_BYTES.store(0, Ordering::Relaxed);
}

fn read() -> (u64, u64) {
    (
        ALLOCATIONS.load(Ordering::Relaxed),
        ALLOCATED_BYTES.load(Ordering::Relaxed),
    )
}

fn main() -> ExitCode {
    verter_alloc::configure();
    verter_bench::semantic_perf::cli::main(Some(AllocHooks { reset, read }))
}

#[cfg(test)]
mod tests {
    /// The counter forwards to the shipped allocator, not to the platform heap.
    #[test]
    fn the_shipped_allocator_serves_this_probe() {
        assert_eq!(
            verter_alloc::mimalloc_serves_global_allocations(),
            verter_alloc::SELECTED == verter_alloc::AllocatorKind::Mimalloc
        );
    }
}
