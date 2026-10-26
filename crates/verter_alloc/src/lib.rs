//! The global allocator of Verter's native executables and Node addon.
//!
//! Every shipped native entry point — `verter-lsp`, `verter-tsc`,
//! `verter-mcp` (and its `verter-mcp-server` twin) and the `@verter/native`
//! addon — installs [`Allocator`] as its `#[global_allocator]` and calls
//! [`configure`] before it does any work:
//!
//! ```ignore
//! #[global_allocator]
//! static GLOBAL: verter_alloc::Allocator = verter_alloc::ALLOCATOR;
//!
//! fn main() {
//!     verter_alloc::configure();
//!     // ...
//! }
//! ```
//!
//! ## What it selects
//!
//! - **Native targets:** mimalloc v3 (through `mimalloc-safe`), non-secure,
//!   built with `skip_collect_on_exit`, with a [`PURGE_DELAY_MS`] purge delay.
//! - **`wasm32`:** the target's default allocator (dlmalloc). Nothing here
//!   compiles C for wasm; [`Allocator`] is `std::alloc::System` there.
//! - **Feature `system-allocator`:** the platform heap on every target, for
//!   hardened builds that want its protections (Windows LFH randomisation and
//!   encoded headers, glibc safe-linking) back.
//!
//! ## Why mimalloc, and why this configuration
//!
//! Allocation is a large share of a cold semantic request (hundreds of
//! thousands to millions of allocations). Measured against the platform heap
//! with byte-identical answers: about 1.2x faster cold requests on Windows,
//! 1.1x on glibc, and 1.5x on musl, whose own allocator is the slowest of the
//! three.
//!
//! - **Purge delay.** v3 holds freed pages committed for 1000 ms by default,
//!   which in a long editing session keeps about a quarter more memory
//!   resident than the platform heap. At 10 ms resident memory matches the
//!   platform heap and long sessions stay faster; 100 ms measured no better
//!   on either axis. The value is a default: a purge delay mimalloc read
//!   from the environment at startup (`MIMALLOC_PURGE_DELAY`, or its legacy
//!   name `MIMALLOC_RESET_DELAY`) stays in force.
//! - **`skip_collect_on_exit`.** Verter's scheduler threads can still be
//!   running when `main` returns; collecting the heap under them at exit is
//!   the pattern behind exit-time crashes.
//! - **Not secure mode.** Guard pages, encrypted free lists and randomised
//!   allocation cost the whole speed gain on this workload.
//!
//! The retention account's byte ceilings (`verter_retention::RetentionLimits`)
//! are figures of its own allocation model, not of resident memory, so the
//! allocator choice changes nothing they decide. What changes is the real
//! memory behind an estimated byte: in a long editing session about 1.36
//! bytes of private commit per estimated byte under this configuration,
//! against 1.11 on the Windows heap, with resident memory about 1.25 on
//! both.
//!
//! ## In a Node addon
//!
//! A `#[global_allocator]` in a `cdylib` governs only the Rust allocations of
//! that image; Node, V8 and other addons keep their own allocators, and each
//! side frees what it allocated (napi-rs copies strings and buffers, and an
//! external buffer's finaliser runs back into the addon). Each image carries
//! its own static mimalloc copy. On macOS `mimalloc-safe` gives every copy
//! its own thread-local state, so two v3 copies in one process do not share
//! a heap. On Linux every image is compiled with the local-dynamic TLS model
//! a `dlopen`ed image needs; when the linker links an executable it relaxes
//! those accesses to local-exec, so the executables pay nothing for it.
//!
//! ## Configure before any other thread allocates
//!
//! mimalloc's option store is plain memory. [`configure`] must run before the
//! process starts any thread that allocates: first in a synchronous `main`,
//! before an async runtime is built, or in the addon's load-time constructor.

#[cfg(all(not(target_family = "wasm"), not(feature = "system-allocator")))]
mod selected {
    use core::ffi::c_long;

    pub type Allocator = mimalloc_safe::MiMalloc;
    pub const ALLOCATOR: Allocator = mimalloc_safe::MiMalloc;
    pub const KIND: super::AllocatorKind = super::AllocatorKind::Mimalloc;

    pub fn configure() {
        // SAFETY: a plain option store, made before any other thread
        // allocates (see the crate docs); mimalloc initialises itself on first
        // use. `set_default` leaves a value read from the environment in place.
        unsafe {
            libmimalloc_sys2::mi_option_set_default(
                libmimalloc_sys2::mi_option_purge_delay,
                c_long::from(super::PURGE_DELAY_MS),
            );
        }
    }

    pub fn purge_delay_ms() -> Option<i64> {
        // SAFETY: plain option read.
        Some(widen(unsafe {
            libmimalloc_sys2::mi_option_get(libmimalloc_sys2::mi_option_purge_delay)
        }))
    }

    /// `c_long` is 32 bits on Windows and on 32-bit targets, 64 bits elsewhere.
    #[cfg(any(windows, target_pointer_width = "32"))]
    fn widen(value: c_long) -> i64 {
        i64::from(value)
    }

    /// `c_long` is 32 bits on Windows and on 32-bit targets, 64 bits elsewhere.
    #[cfg(not(any(windows, target_pointer_width = "32")))]
    fn widen(value: c_long) -> i64 {
        value
    }

    pub fn linked_mimalloc_version() -> Option<i32> {
        // SAFETY: returns a compile-time constant of the linked C library.
        Some(unsafe { libmimalloc_sys2::mi_version() })
    }

    pub fn owns(ptr: *const u8) -> bool {
        // SAFETY: `mi_is_in_heap_region` only compares the address against
        // this copy's arenas; it never dereferences it.
        unsafe { libmimalloc_sys2::mi_is_in_heap_region(ptr.cast()) }
    }

    pub fn collect_idle_memory() {
        // SAFETY: `mi_collect` is callable from any thread at any time. It
        // only frees and purges pages that hold no live block; it never
        // touches a live allocation, and it takes mimalloc's own arena
        // locks, not a Rust one.
        unsafe { libmimalloc_sys2::mi_collect(true) }
    }
}

#[cfg(any(target_family = "wasm", feature = "system-allocator"))]
mod selected {
    pub type Allocator = std::alloc::System;
    pub const ALLOCATOR: Allocator = std::alloc::System;
    pub const KIND: super::AllocatorKind = super::AllocatorKind::System;

    pub fn configure() {}

    pub fn purge_delay_ms() -> Option<i64> {
        None
    }

    pub fn linked_mimalloc_version() -> Option<i32> {
        None
    }

    pub fn owns(_ptr: *const u8) -> bool {
        false
    }

    pub fn collect_idle_memory() {}
}

/// Which allocator a build installs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AllocatorKind {
    /// mimalloc v3, the native default.
    Mimalloc,
    /// The platform heap: `wasm32`, or a `system-allocator` build.
    System,
}

/// The type every native entry point installs as `#[global_allocator]`.
pub type Allocator = selected::Allocator;

/// The [`Allocator`] value for the `#[global_allocator]` static.
pub const ALLOCATOR: Allocator = selected::ALLOCATOR;

/// The allocator this build selects.
pub const SELECTED: AllocatorKind = selected::KIND;

/// mimalloc's purge delay, in milliseconds: how long freed pages stay
/// committed before they are returned to the OS.
pub const PURGE_DELAY_MS: i32 = 10;

/// The C allocator version the build links (`MI_MALLOC_VERSION`: major,
/// two digits of minor, two of patch). Bumping the vendored sources without
/// this, or this without the sources, fails its test.
pub const MIMALLOC_VERSION: i32 = 30503;

/// Applies the runtime allocator configuration. Call it first thing in
/// `main`, or when an addon loads; calling it again is harmless.
pub fn configure() {
    selected::configure();
}

/// The purge delay mimalloc is using, or `None` without mimalloc.
#[must_use]
pub fn purge_delay_ms() -> Option<i64> {
    selected::purge_delay_ms()
}

/// The version of the linked C allocator, or `None` without mimalloc.
#[must_use]
pub fn linked_mimalloc_version() -> Option<i32> {
    selected::linked_mimalloc_version()
}

/// Whether `ptr` lies in this image's own mimalloc heap. Always `false` when
/// mimalloc is not linked. Only compares the address; never dereferences it.
#[must_use]
pub fn mimalloc_owns(ptr: *const u8) -> bool {
    selected::owns(ptr)
}

/// Returns memory the allocator holds but no live allocation uses to the OS
/// (`mi_collect(true)`), for a long-running process that has gone idle.
///
/// mimalloc has no background purge thread: it purges only inside its own
/// allocator calls, so an idle process keeps the pages its last burst freed
/// committed. Calling this once when the process goes quiet brings idle
/// commit back toward the platform heap's level.
///
/// Safe from any thread at any time, and never moves or frees a live
/// allocation. It is not free: it walks every heap's pages, so call it off
/// the request path and once per idle period, never in a loop. Allocations
/// on other threads keep running during it and may briefly contend on the
/// arena locks it takes. Does nothing on `wasm32` and `system-allocator`
/// builds.
pub fn collect_idle_memory() {
    selected::collect_idle_memory();
}

/// Whether this image's mimalloc served a fresh global allocation: `true`
/// exactly when the image's `#[global_allocator]` is [`ALLOCATOR`] on a
/// mimalloc build. Always `false` when mimalloc is not linked.
#[must_use]
pub fn mimalloc_serves_global_allocations() -> bool {
    let probe = std::hint::black_box(Box::new(0_u64));
    selected::owns(std::ptr::from_ref::<u64>(&probe).cast())
}

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
