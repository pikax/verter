//! Heap attribution: a `GlobalAlloc` wrapper that charges every
//! allocation to the innermost open [`super::scope::ScopeGuard`].
//!
//! COMPILED ONLY under the `attribution` feature.
//!
//! This is what makes "allocations, copies and retained bytes by
//! logical identity" measurable without hand-annotating every `Vec`:
//! any region already wrapped in a scope guard reports its own heap
//! traffic for free.
//!
//! It is a MEASUREMENT HARNESS component, not a production one. A
//! process installs it by writing
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: verter_audit::attribution::AttributingAllocator<std::alloc::System> =
//!     verter_audit::attribution::AttributingAllocator::new(std::alloc::System);
//! ```
//!
//! in a binary it owns. No library in the workspace installs it, and
//! nothing in the workspace can: `#[global_allocator]` is a whole-program
//! choice that only the final binary makes.
//!
//! ## Attribution rules
//!
//! - An allocation is charged to the innermost open scope on the
//!   ALLOCATING thread, and a release to the innermost open scope on the
//!   RELEASING thread. Those can differ; that is the honest reading of
//!   "who was running when the bytes moved", and it is why a site's
//!   `alloc_bytes - dealloc_bytes` is a contribution to retention rather
//!   than a claim about ownership.
//! - Traffic with no scope open lands on
//!   [`WorkSite::UnattributedAllocation`], so totals stay closed rather
//!   than silently dropping the remainder.
//! - The wrapper never allocates: it reads a `const`-init thread-local
//!   and touches `Relaxed` atomics only.

use std::alloc::{GlobalAlloc, Layout};
use std::sync::atomic::Ordering;

use super::schema::WorkSite;
use super::scope::current_site_index;
use super::table::{cell, CELLS};

/// Wraps another global allocator and attributes its traffic to the
/// innermost open scope.
pub struct AttributingAllocator<A> {
    inner: A,
}

impl<A> AttributingAllocator<A> {
    /// Wrap `inner`.
    pub const fn new(inner: A) -> Self {
        Self { inner }
    }

    /// The wrapped allocator.
    pub const fn inner(&self) -> &A {
        &self.inner
    }
}

#[inline]
fn charged_cell() -> &'static super::table::SiteCell {
    match current_site_index() {
        Some(index) => &CELLS[index],
        None => cell(WorkSite::UnattributedAllocation),
    }
}

#[inline]
fn charge_alloc(size: usize) {
    let target = charged_cell();
    target.alloc_count.fetch_add(1, Ordering::Relaxed);
    target.alloc_bytes.fetch_add(size as u64, Ordering::Relaxed);
}

#[inline]
fn charge_dealloc(size: usize) {
    charged_cell()
        .dealloc_bytes
        .fetch_add(size as u64, Ordering::Relaxed);
}

// SAFETY: every method forwards to `self.inner`, which is required by
// `GlobalAlloc`'s own contract to be a correct allocator, and returns
// its pointer unchanged. The added work is bookkeeping over `Relaxed`
// atomics and a `const`-init thread-local read, neither of which
// allocates, so the wrapper introduces no re-entrancy into the
// allocation path.
unsafe impl<A: GlobalAlloc> GlobalAlloc for AttributingAllocator<A> {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc(layout) };
        if !ptr.is_null() {
            charge_alloc(layout.size());
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        charge_dealloc(layout.size());
        unsafe { self.inner.dealloc(ptr, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { self.inner.alloc_zeroed(layout) };
        if !ptr.is_null() {
            charge_alloc(layout.size());
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let out = unsafe { self.inner.realloc(ptr, layout, new_size) };
        if !out.is_null() {
            // A realloc is one release of the old block and one
            // acquisition of the new one; recording it as both keeps
            // `alloc_bytes - dealloc_bytes` equal to live bytes.
            charge_dealloc(layout.size());
            charge_alloc(new_size);
        }
        out
    }
}
