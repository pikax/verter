//! The unit-test binary's global allocator: the system allocator, with a
//! per-thread count of what a measured closure allocates, so a test can
//! bound an operation's allocation cost without seeing another test's.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct ThreadCountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
    static ALLOCATED_BYTES: Cell<usize> = const { Cell::new(0) };
}

fn note(bytes: usize) {
    // `try_with`: the allocator also runs during thread teardown.
    let counting = COUNTING.try_with(Cell::get).unwrap_or(false);
    if counting {
        let _ = ALLOCATIONS.try_with(|count| count.set(count.get() + 1));
        let _ = ALLOCATED_BYTES.try_with(|total| total.set(total.get() + bytes));
    }
}

// SAFETY: every call forwards to the system allocator unchanged; the
// counters only observe the sizes.
unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc(layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        note(layout.size());
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        note(new_size);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

/// Run `f`, returning its value with the number of allocations it made on
/// this thread and the bytes they requested.
pub(crate) fn measure_allocations<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.with(|count| count.set(0));
    ALLOCATED_BYTES.with(|total| total.set(0));
    COUNTING.with(|counting| counting.set(true));
    let value = f();
    COUNTING.with(|counting| counting.set(false));
    (
        value,
        ALLOCATIONS.with(Cell::get),
        ALLOCATED_BYTES.with(Cell::get),
    )
}
