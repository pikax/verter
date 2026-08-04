#![allow(clippy::missing_const_for_thread_local)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

struct ThreadCountingAllocator;

thread_local! {
    static COUNTING: Cell<bool> = Cell::new(false);
    static ALLOCATIONS: Cell<usize> = Cell::new(0);
    static ALLOCATED_BYTES: Cell<usize> = Cell::new(0);
}

unsafe impl GlobalAlloc for ThreadCountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|enabled| {
            if enabled.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
                ALLOCATED_BYTES.with(|bytes| bytes.set(bytes.get() + layout.size()));
            }
        });
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        COUNTING.with(|enabled| {
            if enabled.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
                ALLOCATED_BYTES.with(|bytes| bytes.set(bytes.get() + new_size));
            }
        });
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: ThreadCountingAllocator = ThreadCountingAllocator;

fn measure_allocations<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    ALLOCATIONS.with(|count| count.set(0));
    ALLOCATED_BYTES.with(|bytes| bytes.set(0));
    COUNTING.with(|enabled| enabled.set(true));
    let value = f();
    COUNTING.with(|enabled| enabled.set(false));
    let allocations = ALLOCATIONS.with(Cell::get);
    let bytes = ALLOCATED_BYTES.with(Cell::get);
    (value, allocations, bytes)
}

mod cases;
